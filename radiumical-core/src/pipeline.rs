//! Agent pipeline — LLM → tool → LLM loop, with persistent conversation context.
use crate::conversation::Conversation;
use crate::hooks::crlf::CRLFNormalizer;
use crate::tools::{all_tools, Tool, ToolContext};
use crate::types::{
    Message, MessageContent, ProviderEvent, Role, SessionConfig, ToolCall, ToolDefinition,
    ToolResult,
};
use crate::{orchestrator, provider::Provider, types::UiEvent};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Duration;

// ── Tool hook trait ──

#[async_trait::async_trait]
pub trait ToolHook: Send + Sync {
    fn after(&self, _call: &ToolCall, _result: ToolResult, _workspace: &PathBuf) -> ToolResult {
        _result
    }
}

// ── Pipeline runner ──

pub struct PipelineRunner {
    config: SessionConfig,
    provider: Arc<dyn Provider>,
    tools: Vec<Box<dyn Tool>>,
    tool_defs: Vec<ToolDefinition>,
    tool_hooks: Vec<Box<dyn ToolHook>>,
    pub conversation: Conversation,
}

impl PipelineRunner {
    pub fn new(config: SessionConfig, provider: Arc<dyn Provider>) -> Self {
        let tools = all_tools();
        let tool_defs = tools.iter().map(|t| t.definition()).collect();
        let conversation = Conversation::new(
            config.system_prompt.clone(),
            Some(std::path::PathBuf::from("conversation.jsonl")),
        );
        Self {
            config,
            provider,
            tools,
            tool_defs,
            tool_hooks: vec![Box::new(CRLFNormalizer::new())],
            conversation,
        }
    }

    pub fn set_model(&mut self, model: String) {
        self.config.model = model;
    }

    pub fn set_mode(&mut self, mode: crate::types::AgentMode) {
        self.config.mode = mode;
    }

    pub async fn run(
        &mut self,
        task: String,
        workspace: PathBuf,
        _hb_cancel: Option<tokio::sync::mpsc::UnboundedSender<()>>,
        ui_tx: mpsc::Sender<UiEvent>,
        mut cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let llm_timeout = Duration::from_secs(self.config.llm_timeout_secs);
        let tool_timeout = Duration::from_secs(self.config.tool_timeout_secs);

        let mut messages = self.conversation.build_context(&task);

        // Inject orchestrator plan context before first LLM call
        let ws_key = workspace.display().to_string();
        if let Some(ctx) = orchestrator::get_context_for_workspace(&ws_key) {
            messages.push(user_msg(&ctx));
        }

        for iteration in 0..self.config.max_iterations {
            // ── 1. LLM call ──
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let provider = Arc::clone(&self.provider);
            let msgs = messages.clone();
            let defs = self.tool_defs.clone();

            let chat_handle = tokio::spawn(async move { provider.chat(&msgs, &defs, tx).await });

            let mut full_text = String::new();
            let mut full_reasoning = String::new();
            let mut last_tool_calls: Option<Vec<ToolCall>> = None;
            let mut timed_out = false;

            loop {
                let event = if !timed_out {
                    tokio::select! {
                        e = rx.recv() => e,
                        _ = tokio::time::sleep(llm_timeout) => { timed_out = true; None }
                        _ = cancel_rx.changed() => { if *cancel_rx.borrow() { return Ok(()); } else { continue; } }
                    }
                } else {
                    tokio::select! {
                        e = rx.recv() => e,
                        _ = cancel_rx.changed() => { if *cancel_rx.borrow() { return Ok(()); } else { continue; } }
                    }
                };

                match event {
                    Some(ProviderEvent::Text(chunk)) => {
                        let _ = ui_tx.send(UiEvent::LlmChunk(chunk.clone()));
                        full_text.push_str(&chunk);
                    }
                    Some(ProviderEvent::Reasoning(rc)) => {
                        let _ = ui_tx.send(UiEvent::LlmReasoning(rc.clone()));
                        full_reasoning.push_str(&rc);
                    }
                    Some(ProviderEvent::ToolCalls(calls)) => {
                        last_tool_calls = Some(calls);
                    }
                    Some(ProviderEvent::Done) => break,
                    Some(ProviderEvent::Error(e)) => {
                        let _ = ui_tx.send(UiEvent::Error(e));
                        break;
                    }
                    None => break,
                }
            }

            let _ = ui_tx.send(UiEvent::LlmDone);

            if timed_out {
                // Abort the stuck provider task instead of awaiting it
                chat_handle.abort();
                let explain = format!(
                    "⚠️ LLM request timed out after {}s (iteration {}).",
                    self.config.llm_timeout_secs,
                    iteration + 1
                );
                let _ = ui_tx.send(UiEvent::Error(explain.clone()));
                messages.push(user_msg(&explain));
                continue;
            }

            match chat_handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    let _ = ui_tx.send(UiEvent::Error(e.to_string()));
                    return Err(e);
                }
                Err(join_err) => {
                    // If aborted, we already handled it above; this arm is for panics
                    if join_err.is_cancelled() {
                        continue;
                    }
                    let msg = format!("Provider panicked: {join_err}");
                    let _ = ui_tx.send(UiEvent::Error(msg.clone()));
                    return Err(anyhow::anyhow!("{msg}"));
                }
            }

            // ── 2. Tool execution ──
            if let Some(ref calls) = last_tool_calls {
                self.conversation.push_assistant(
                    &full_text,
                    Some(calls.clone()),
                    Some(&full_reasoning),
                );
                messages.push(assistant_msg(
                    &full_text,
                    Some(calls.clone()),
                    &full_reasoning,
                ));

                let total = calls.len();
                for (i, tc) in calls.iter().enumerate() {
                    let tool = self
                        .tools
                        .iter()
                        .find(|t| t.definition().function.name == tc.function.name);

                    match tool {
                        Some(tool) => {
                            let _ = ui_tx.send(UiEvent::ToolStart {
                                name: tc.function.name.clone(),
                                index: i,
                                total,
                                args: tc.function.arguments.clone(),
                            });
                            let ws = workspace.clone();
                            let args = tc.function.arguments.clone();
                            let ctx = ToolContext {
                                ui_tx: ui_tx.clone(),
                            };
                            let result =
                                exec_with_timeout(tool.as_ref(), &ws, &args, tool_timeout, &ctx)
                                    .await;

                            let mut final_result = result;
                            for hook in &self.tool_hooks {
                                final_result = hook.after(tc, final_result, &workspace);
                            }
                            let _ = ui_tx.send(UiEvent::ToolDone);
                            let _ = ui_tx.send(UiEvent::ToolResult {
                                content: final_result.content.trim_end().to_string(),
                            });
                            self.conversation.push_tool_result(tc, &final_result);
                            messages.push(tool_result_msg(tc, final_result));
                        }
                        None => {
                            let _ = ui_tx.send(UiEvent::ToolStart {
                                name: tc.function.name.clone(),
                                index: i,
                                total,
                                args: String::new(),
                            });
                            let err = format!("Unknown tool: {}", tc.function.name);
                            let _ = ui_tx.send(UiEvent::Error(err.clone()));
                            let tr = ToolResult {
                                tool_call_id: tc.id.clone(),
                                content: err,
                                is_error: true,
                            };
                            self.conversation.push_tool_result(tc, &tr);
                            messages.push(tool_result_msg(tc, tr));
                        }
                    }
                }
                continue;
            }

            // ── 3. Final response ──
            self.conversation
                .push_assistant(&full_text, None, Some(&full_reasoning));
            messages.push(assistant_msg(&full_text, None, &full_reasoning));
            return Ok(());
        }

        let msg = format!(
            "⚠️  Reached max iterations ({}) without completing.",
            self.config.max_iterations
        );
        let _ = ui_tx.send(UiEvent::Error(msg));
        Ok(())
    }
}

// ── Helpers ──

fn user_msg(text: &str) -> Message {
    Message {
        role: Role::User,
        content: MessageContent::Text(text.to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        reasoning_content: None,
    }
}

fn assistant_msg(text: &str, calls: Option<Vec<ToolCall>>, reasoning: &str) -> Message {
    Message {
        role: Role::Assistant,
        content: MessageContent::Text(text.to_string()),
        tool_calls: calls,
        tool_call_id: None,
        name: None,
        reasoning_content: if reasoning.is_empty() {
            None
        } else {
            Some(reasoning.to_string())
        },
    }
}

fn tool_result_msg(tc: &ToolCall, result: ToolResult) -> Message {
    let call_id = if tc.id.is_empty() {
        format!("call_{}", tc.function.name)
    } else {
        tc.id.clone()
    };
    Message {
        role: Role::Tool,
        content: MessageContent::Text(result.content),
        tool_calls: None,
        tool_call_id: Some(call_id),
        name: Some(tc.function.name.clone()),
        reasoning_content: None,
    }
}

async fn exec_with_timeout(
    tool: &dyn Tool,
    workspace: &PathBuf,
    arguments: &str,
    timeout: Duration,
    ctx: &ToolContext,
) -> ToolResult {
    let name = tool.definition().function.name.clone();
    let ws = workspace.clone();
    let args = arguments.to_string();
    match tokio::time::timeout(timeout, tool.execute_with_context(&ws, &args, ctx)).await {
        Ok(r) => r,
        Err(_) => ToolResult {
            tool_call_id: String::new(),
            content: format!("Tool '{name}' timed out after {}s.", timeout.as_secs()),
            is_error: true,
        },
    }
}
