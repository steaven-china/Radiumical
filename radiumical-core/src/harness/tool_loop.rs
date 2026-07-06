//! Tool execution loop — dispatches tool calls from the LLM response and
//! collects results back into the conversation.

use super::helpers::{exec_with_timeout, tool_result_msg};
use crate::conversation::Conversation;
use crate::plugins::source::SourcePluginRegistry;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolCall, ToolResult, UiEvent};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn execute_tool_calls(
    calls: &[ToolCall],
    full_text: &str,
    full_reasoning: &str,
    tools: &[Box<dyn Tool>],
    extra_tools: &[Box<dyn Tool>],
    tool_hooks: &[Box<dyn crate::harness::ToolHook>],
    conversation: &mut Conversation,
    source_plugins: &SourcePluginRegistry,
    messages: &mut Vec<crate::types::Message>,
    workspace: &Path,
    ui_tx: &tokio::sync::mpsc::Sender<UiEvent>,
    tool_timeout: Duration,
    allowed_names: &HashSet<String>,
    config: &mut crate::types::SessionConfig,
    cancel_rx: &tokio::sync::watch::Receiver<bool>,
) -> bool {
    use crate::harness::helpers::assistant_msg;

    conversation.push_assistant(full_text, Some(calls.to_vec()), Some(full_reasoning));
    messages.push(assistant_msg(
        full_text,
        Some(calls.to_vec()),
        full_reasoning,
    ));

    // ── Checkpoint before mutating tool batches ──
    let has_mutating = calls.iter().any(|tc| {
        matches!(
            tc.function.name.as_str(),
            "write_file" | "edit_file" | "writeFile" | "editFile"
        )
    });
    if has_mutating {
        let summary = summarize_for_checkpoint(full_text, full_reasoning);
        match crate::checkpoint::create_checkpoint(workspace, &config.session_id, &summary).await {
            Ok(Some(cp)) => {
                if let Err(e) = ui_tx.send(UiEvent::CheckpointCreated(cp)).await {
                    tracing::warn!(error = %e, "failed to send CheckpointCreated to UI");
                }
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(error = %e, "failed to create checkpoint");
            }
        }
    }

    let total = calls.len();
    for (i, tc) in calls.iter().enumerate() {
        // ── Cancel check before each tool call ──
        if *cancel_rx.borrow() {
            // Fill in "cancelled" results for remaining tool calls so the
            // conversation stays well-formed (every tool_call needs a result).
            for remaining in &calls[i..] {
                let tr = ToolResult {
                    tool_call_id: remaining.id.clone(),
                    content: "Cancelled by user.".into(),
                    is_error: true,
                };
                conversation.push_tool_result(remaining, &tr, Some(workspace));
                messages.push(tool_result_msg(remaining, tr));
            }
            return true;
        }
        if !allowed_names.contains(&tc.function.name) {
            let err = format!(
                "Tool '{}' is not allowed for this agent/mode",
                tc.function.name
            );
            if let Err(e) = ui_tx.send(UiEvent::Error(err.clone())).await {
                tracing::warn!(error = %e, "failed to send tool-not-allowed error to UI");
            }
            let tr = ToolResult {
                tool_call_id: tc.id.clone(),
                content: err,
                is_error: true,
            };
            conversation.push_tool_result(tc, &tr, Some(workspace));
            messages.push(tool_result_msg(tc, tr));
            continue;
        }

        let tool = tools
            .iter()
            .find(|t| t.definition().function.name == tc.function.name)
            .or_else(|| {
                extra_tools
                    .iter()
                    .find(|t| t.definition().function.name == tc.function.name)
            });

        match tool {
            Some(tool) => {
                if let Err(e) = ui_tx
                    .send(UiEvent::ToolStart {
                        name: tc.function.name.clone(),
                        index: i,
                        total,
                        args: tc.function.arguments.clone(),
                    })
                    .await
                {
                    tracing::warn!(error = %e, "failed to send ToolStart to UI");
                }
                let ws = workspace.to_path_buf();
                let args = tc.function.arguments.clone();
                let ctx = ToolContext {
                    ui_tx: ui_tx.clone(),
                    source_plugins: Some(Arc::new(source_plugins.clone())),
                };
                let result = exec_with_timeout(tool.as_ref(), &ws, &args, tool_timeout, &ctx).await;

                let mut final_result = result;
                for hook in tool_hooks {
                    final_result = hook.after(tc, final_result, workspace);
                }

                // Intercept settings tool results — apply config changes in-place
                if tc.function.name == "settings" {
                    final_result = intercept_settings(final_result, config, ui_tx).await;
                }

                if let Err(e) = ui_tx.send(UiEvent::ToolDone).await {
                    tracing::warn!(error = %e, "failed to send ToolDone to UI");
                }
                if let Err(e) = ui_tx
                    .send(UiEvent::ToolResult {
                        content: final_result.content.trim_end().to_string(),
                    })
                    .await
                {
                    tracing::warn!(error = %e, "failed to send ToolResult to UI");
                }
                conversation.push_tool_result(tc, &final_result, Some(workspace));
                messages.push(tool_result_msg(tc, final_result));
            }
            None => {
                if let Err(e) = ui_tx
                    .send(UiEvent::ToolStart {
                        name: tc.function.name.clone(),
                        index: i,
                        total,
                        args: String::new(),
                    })
                    .await
                {
                    tracing::warn!(error = %e, "failed to send ToolStart to UI");
                }
                let err = format!("Unknown tool: {}", tc.function.name);
                if let Err(e) = ui_tx.send(UiEvent::Error(err.clone())).await {
                    tracing::warn!(error = %e, "failed to send unknown-tool error to UI");
                }
                let tr = ToolResult {
                    tool_call_id: tc.id.clone(),
                    content: err,
                    is_error: true,
                };
                conversation.push_tool_result(tc, &tr, Some(workspace));
                messages.push(tool_result_msg(tc, tr));
            }
        }
    }
    false
}

/// Intercept settings tool markers and apply config changes.
async fn intercept_settings(
    result: crate::types::ToolResult,
    config: &mut crate::types::SessionConfig,
    ui_tx: &tokio::sync::mpsc::Sender<UiEvent>,
) -> crate::types::ToolResult {
    let content = result.content.trim();

    // ── get ──
    if let Some(key) = content.strip_prefix("__settings_get__:") {
        let value = crate::tools::settings::read_setting(config, key);
        return match value {
            Ok(v) => crate::types::ToolResult {
                tool_call_id: result.tool_call_id,
                content: format!("{key} = {v}"),
                is_error: false,
            },
            Err(e) if e.starts_with("__read_tui__:") => {
                // TUI-only setting — can't read from harness, return hint
                crate::types::ToolResult {
                    tool_call_id: result.tool_call_id,
                    content: format!("{key} is managed by the TUI. Use /{key} command."),
                    is_error: false,
                }
            }
            Err(e) => crate::types::ToolResult {
                tool_call_id: result.tool_call_id,
                content: e,
                is_error: true,
            },
        };
    }

    // ── set ──
    if let Some(kv) = content.strip_prefix("__settings_set__:") {
        let (key, value) = match kv.split_once('=') {
            Some((k, v)) => (k, v),
            None => {
                return crate::types::ToolResult {
                    tool_call_id: result.tool_call_id,
                    content: format!("Invalid settings format: {kv}"),
                    is_error: true,
                };
            }
        };
        let apply_result = crate::tools::settings::apply_setting(config, key, value);
        return match apply_result {
            Ok(msg) => {
                let _ = ui_tx
                    .send(UiEvent::Toast {
                        message: msg.clone(),
                        level: "info".into(),
                        duration_secs: 3,
                    })
                    .await;
                crate::types::ToolResult {
                    tool_call_id: result.tool_call_id,
                    content: msg,
                    is_error: false,
                }
            }
            Err(e) if e.starts_with("__forward_effort__:") => {
                let effort = e.strip_prefix("__forward_effort__:").unwrap_or("max");
                let _ = ui_tx
                    .send(UiEvent::Toast {
                        message: format!("Thinking effort → {effort}"),
                        level: "info".into(),
                        duration_secs: 3,
                    })
                    .await;
                crate::types::ToolResult {
                    tool_call_id: result.tool_call_id,
                    content: format!(
                        "Thinking effort set to: {effort} (will take effect on next request)"
                    ),
                    is_error: false,
                }
            }
            Err(e) if e.starts_with("__forward_cod__:") => {
                let val = e.strip_prefix("__forward_cod__:").unwrap_or("off");
                crate::types::ToolResult {
                    tool_call_id: result.tool_call_id,
                    content: format!(
                        "Chain of Draft set to: {val} (will take effect on next request)"
                    ),
                    is_error: false,
                }
            }
            Err(e) => crate::types::ToolResult {
                tool_call_id: result.tool_call_id,
                content: e,
                is_error: true,
            },
        };
    }

    // ── save ──
    if content == "__settings_save__" {
        let mut cfg = crate::config::Config::load().unwrap_or(crate::config::Config {
            model: None,
            provider: None,
            api_key: None,
            api_base: None,
            heartbeat_secs: None,
            llm_timeout_secs: None,
            max_iterations: None,
            reasoning_effort: None,
            mode: None,
            max_context_tokens: None,
            context_compress_ratio: None,
            auto_resume_last_task: None,
        });
        cfg.model = Some(config.model.clone());
        cfg.mode = Some(format!("{:?}", config.mode).to_lowercase());
        cfg.max_iterations = Some(config.max_iterations);
        cfg.llm_timeout_secs = Some(config.llm_timeout_secs);
        cfg.max_context_tokens = Some(config.max_context_tokens);
        cfg.context_compress_ratio = Some(config.context_compress_ratio);
        return match cfg.save() {
            Ok(()) => {
                let _ = ui_tx
                    .send(UiEvent::Toast {
                        message: "Settings saved to ~/.radi/config.toml".into(),
                        level: "info".into(),
                        duration_secs: 3,
                    })
                    .await;
                crate::types::ToolResult {
                    tool_call_id: result.tool_call_id,
                    content: "Settings persisted to ~/.radi/config.toml".into(),
                    is_error: false,
                }
            }
            Err(e) => crate::types::ToolResult {
                tool_call_id: result.tool_call_id,
                content: format!("Failed to save: {e}"),
                is_error: true,
            },
        };
    }

    // Not a settings marker — pass through
    result
}

fn summarize_for_checkpoint(full_text: &str, full_reasoning: &str) -> String {
    let source = if full_reasoning.trim().is_empty() {
        full_text
    } else {
        full_reasoning
    };
    let first = source
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("agent step");
    let mut s = first.to_string();
    s = s
        .trim_start_matches("#")
        .trim_start_matches("*")
        .trim_start_matches("-")
        .trim()
        .to_string();
    if s.len() > 60 {
        s.truncate(60);
        s.push('…');
    }
    s
}
