use crate::conversation::Conversation;
use crate::plugins::source::SourcePluginRegistry;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolCall, ToolResult, UiEvent};
use super::helpers::{exec_with_timeout, tool_result_msg};
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
) {
    use crate::harness::helpers::assistant_msg;

    conversation.push_assistant(full_text, Some(calls.to_vec()), Some(full_reasoning));
    messages.push(assistant_msg(full_text, Some(calls.to_vec()), full_reasoning));

    let total = calls.len();
    for (i, tc) in calls.iter().enumerate() {
        if !allowed_names.contains(&tc.function.name) {
            let err = format!("Tool '{}' is not allowed for this agent/mode", tc.function.name);
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
                extra_tools.iter().find(|t| t.definition().function.name == tc.function.name)
            });

        match tool {
            Some(tool) => {
                if let Err(e) = ui_tx.send(UiEvent::ToolStart {
                    name: tc.function.name.clone(),
                    index: i,
                    total,
                    args: tc.function.arguments.clone(),
                }).await {
                    tracing::warn!(error = %e, "failed to send ToolStart to UI");
                }
                let ws = workspace.to_path_buf();
                let args = tc.function.arguments.clone();
                let ctx = ToolContext {
                    ui_tx: ui_tx.clone(),
                    source_plugins: Some(Arc::new(source_plugins.clone())),
                };
                let result =
                    exec_with_timeout(tool.as_ref(), &ws, &args, tool_timeout, &ctx).await;

                let mut final_result = result;
                for hook in tool_hooks {
                    final_result = hook.after(tc, final_result, workspace);
                }

                if let Err(e) = ui_tx.send(UiEvent::ToolDone).await {
                    tracing::warn!(error = %e, "failed to send ToolDone to UI");
                }
                if let Err(e) = ui_tx.send(UiEvent::ToolResult {
                    content: final_result.content.trim_end().to_string(),
                }).await {
                    tracing::warn!(error = %e, "failed to send ToolResult to UI");
                }
                conversation.push_tool_result(tc, &final_result, Some(workspace));
                messages.push(tool_result_msg(tc, final_result));
            }
            None => {
                if let Err(e) = ui_tx.send(UiEvent::ToolStart {
                    name: tc.function.name.clone(),
                    index: i,
                    total,
                    args: String::new(),
                }).await {
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
}
