//! Agent harness — generic LLM → tool → LLM loop runtime.
//!
//! The harness is execution-policy-agnostic: it runs whatever [`Agent`] and
//! [`Provider`] it is given. All orchestration, conversation context, tool
//! dispatch, and cancellation handling lives here.

mod compress;
pub(crate) mod helpers;
mod tool_loop;

use crate::agent::Agent;
use crate::conversation::Conversation;
use crate::hooks::crlf::CRLFNormalizer;
use crate::memory::Memory;
use crate::orchestrator;
use crate::orchestrator::Orchestrator;
use crate::plugins::source::{RegexLinter, SourcePluginRegistry};
use crate::provider::Provider;
use crate::session::{items_to_messages, SessionItem};
use crate::tools::{all_tools, Tool};
use crate::types::{
    AgentMode, Message, MessageContent, ProviderEvent, Role, SessionConfig, ToolCall,
    ToolDefinition, UiEvent,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use helpers::{assistant_msg, user_msg};

/// Hook that can inspect or transform tool results before they are sent back
/// to the LLM.
#[async_trait::async_trait]
pub trait ToolHook: Send + Sync {
    fn after(
        &self,
        _call: &ToolCall,
        _result: crate::types::ToolResult,
        _workspace: &Path,
    ) -> crate::types::ToolResult {
        _result
    }
}

/// The agent harness. Owns the runtime state needed to run an agent loop.
pub struct Harness {
    config: SessionConfig,
    provider: Arc<dyn Provider>,
    tools: Vec<Box<dyn Tool>>,
    tool_defs: Vec<ToolDefinition>,
    tool_hooks: Vec<Box<dyn ToolHook>>,
    conversation: Conversation,
    source_plugins: SourcePluginRegistry,
}

impl Harness {
    pub fn new(config: SessionConfig, provider: Arc<dyn Provider>) -> Self {
        let tools = all_tools();
        let tool_defs = tools.iter().map(|t| t.definition()).collect();
        let conversation = Conversation::new(
            config.system_prompt.clone(),
            None,
        );
        Self {
            config,
            provider,
            tools,
            tool_defs,
            tool_hooks: vec![Box::new(CRLFNormalizer::new())],
            conversation,
            source_plugins: {
                let mut reg = SourcePluginRegistry::new();
                reg.register(Box::new(RegexLinter));
                reg
            },
        }
    }

    pub fn set_provider(&mut self, provider: Arc<dyn Provider>) {
        self.provider = provider;
    }

    pub fn set_model(&mut self, model: String) {
        self.config.model = model;
    }

    pub fn set_mode(&mut self, mode: AgentMode) {
        self.config.mode = mode;
    }

    pub fn reset_conversation(&mut self) {
        self.conversation.reset_messages(Vec::new());
    }

    pub fn load_session_items(&mut self, items: &[SessionItem]) {
        let messages = items_to_messages(items);
        self.conversation.reset_messages(messages);
    }

    pub fn source_plugins(&mut self) -> &mut SourcePluginRegistry {
        &mut self.source_plugins
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &mut self,
        task: String,
        workspace: PathBuf,
        agent: &Agent,
        extra_tools: &[Box<dyn Tool>],
        _hb_cancel: Option<tokio::sync::mpsc::Sender<()>>,
        ui_tx: tokio::sync::mpsc::Sender<UiEvent>,
        cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        self.run_with_images(
            task,
            Vec::new(),
            workspace,
            agent,
            extra_tools,
            _hb_cancel,
            ui_tx,
            cancel_rx,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn run_with_images(
        &mut self,
        task: String,
        images: Vec<std::path::PathBuf>,
        workspace: PathBuf,
        agent: &Agent,
        extra_tools: &[Box<dyn Tool>],
        _hb_cancel: Option<tokio::sync::mpsc::Sender<()>>,
        ui_tx: tokio::sync::mpsc::Sender<UiEvent>,
        mut cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let _flush_handle = self.conversation.spawn_flush_task();

        let llm_timeout = Duration::from_secs(self.config.llm_timeout_secs);
        let tool_timeout = Duration::from_secs(self.config.tool_timeout_secs);

        let mut messages = self.conversation.build_context(&task, Some(&workspace));
        if !images.is_empty() {
            // Replace the text-only task message with a multipart text+image message.
            if messages
                .last()
                .map(|m| m.role == Role::User)
                .unwrap_or(false)
            {
                messages.pop();
            }
            let content =
                crate::image::build_multipart_content(&task, &images).unwrap_or_else(|e| {
                    MessageContent::Text(format!("{task}\n\n[image load error: {e}]"))
                });
            messages.push(Message {
                role: Role::User,
                content,
                tool_calls: None,
                tool_call_id: None,
                name: None,
                reasoning_content: None,
            });
        }
        if let Some(ctx) = orchestrator::get_context_for_workspace(&workspace.display().to_string())
        {
            messages.push(user_msg(&ctx));
        }

        let mut insert_idx = 0;
        if !agent.system_prompt.is_empty() && agent.system_prompt != self.config.system_prompt {
            messages.insert(
                0,
                Message {
                    role: Role::System,
                    content: MessageContent::Text(agent.system_prompt.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                    reasoning_content: None,
                },
            );
            insert_idx = 1;
        }

        let memory = Memory::for_workspace(&workspace.to_string_lossy());
        let core_ctx = memory.core_context();
        if !core_ctx.is_empty() {
            messages.insert(
                insert_idx,
                Message {
                    role: Role::System,
                    content: MessageContent::Text(core_ctx),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                    reasoning_content: None,
                },
            );
            insert_idx += 1;
        }
        let mem_ctx = memory.context();
        if !mem_ctx.is_empty() {
            messages.insert(
                insert_idx,
                Message {
                    role: Role::System,
                    content: MessageContent::Text(mem_ctx),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                    reasoning_content: None,
                },
            );
        }

        let tool_defs = agent.filter_tools(&self.tool_defs);
        let mut all_tool_defs = tool_defs.clone();
        let extra_defs: Vec<ToolDefinition> = extra_tools.iter().map(|t| t.definition()).collect();
        all_tool_defs.extend(extra_defs);

        let allowed_names: HashSet<String> = if agent.allowed_tools.is_empty() {
            all_tool_defs
                .iter()
                .map(|d| d.function.name.clone())
                .collect()
        } else {
            agent.allowed_tools.iter().cloned().collect()
        };

        self.conversation.sanitize();

        for iteration in 0..self.config.max_iterations {
            // ── 0. Context compression ──
            if iteration > 0 {
                let compressed = compress::compress_context(
                    &self.config,
                    &self.provider,
                    &mut self.conversation,
                    &ui_tx,
                )
                .await;
                if compressed > 0 {
                    rebuild_messages(
                        &self.conversation,
                        &self.config.system_prompt,
                        &task,
                        &workspace,
                        agent,
                        &memory,
                        &mut messages,
                    );
                }
            }

            // ── 1. LLM call ──
            let msgs = messages.clone();
            let (tx, mut rx) = tokio::sync::mpsc::channel(256);
            let provider = Arc::clone(&self.provider);
            let defs = all_tool_defs.clone();

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
                        if let Err(e) = ui_tx.send(UiEvent::LlmChunk(chunk.clone())).await {
                            tracing::warn!(error = %e, "failed to send LlmChunk to UI");
                        }
                        full_text.push_str(&chunk);
                    }
                    Some(ProviderEvent::Reasoning(rc)) => {
                        if let Err(e) = ui_tx.send(UiEvent::LlmReasoning(rc.clone())).await {
                            tracing::warn!(error = %e, "failed to send LlmReasoning to UI");
                        }
                        full_reasoning.push_str(&rc);
                    }
                    Some(ProviderEvent::ToolCalls(calls)) => {
                        last_tool_calls = Some(calls);
                    }
                    Some(ProviderEvent::Done) => break,
                    Some(ProviderEvent::Error(e)) => {
                        if let Err(send_err) = ui_tx.send(UiEvent::Error(e)).await {
                            tracing::warn!(error = %send_err, "failed to send provider error to UI");
                        }
                        break;
                    }
                    None => break,
                }
            }

            if let Err(e) = ui_tx.send(UiEvent::LlmDone).await {
                tracing::warn!(error = %e, "failed to send LlmDone to UI");
            }

            if timed_out {
                chat_handle.abort();
                let explain = format!(
                    "⚠️ LLM request timed out after {}s (iteration {}).",
                    self.config.llm_timeout_secs,
                    iteration + 1
                );
                if let Err(e) = ui_tx.send(UiEvent::Error(explain.clone())).await {
                    tracing::warn!(error = %e, "failed to send timeout error to UI");
                }
                messages.push(user_msg(&explain));
                continue;
            }

            match chat_handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    if let Err(send_err) = ui_tx.send(UiEvent::Error(e.to_string())).await {
                        tracing::warn!(error = %send_err, "failed to send chat error to UI");
                    }
                    return Err(e);
                }
                Err(join_err) => {
                    if join_err.is_cancelled() {
                        continue;
                    }
                    let msg = format!("Provider panicked: {join_err}");
                    if let Err(send_err) = ui_tx.send(UiEvent::Error(msg.clone())).await {
                        tracing::warn!(error = %send_err, "failed to send panic error to UI");
                    }
                    return Err(anyhow::anyhow!("{msg}"));
                }
            }

            // ── 2. Tool execution ──
            if let Some(ref calls) = last_tool_calls {
                tool_loop::execute_tool_calls(
                    calls,
                    &full_text,
                    &full_reasoning,
                    &self.tools,
                    extra_tools,
                    &self.tool_hooks,
                    &mut self.conversation,
                    &self.source_plugins,
                    &mut messages,
                    &workspace,
                    &ui_tx,
                    tool_timeout,
                    &allowed_names,
                    &mut self.config,
                )
                .await;
                continue;
            }

            // ── 3. Final response ──
            self.conversation
                .push_assistant(&full_text, None, Some(&full_reasoning));
            messages.push(assistant_msg(&full_text, None, &full_reasoning));

            // ── 4. Session-level orchestration: auto-continue if plan has ready tasks ──
            if self.config.auto_continue {
                let orch = Orchestrator::new(Some(&workspace.to_string_lossy()));
                let ready = orch.get_ready_tasks();
                if !ready.is_empty() {
                    let next = ready[0];
                    let next_id = next.id;
                    let next_title = next.title.clone();
                    let next_agent = next.agent.clone();
                    if let Err(e) = ui_tx
                        .send(UiEvent::LlmChunk(format!(
                            "\n\n▶ Auto-continuing plan: #{} {}\n\n",
                            next_id, next_title
                        )))
                        .await
                    {
                        tracing::warn!(error = %e, "failed to send auto-continue notice to UI");
                    }
                    let agent_hint = next_agent
                        .as_deref()
                        .map(|a| format!(" (use agent role: {a})"))
                        .unwrap_or_default();
                    let prompt = format!(
                        "Continue the plan. Execute task #{}: {}{}",
                        next_id, next_title, agent_hint
                    );
                    self.conversation.push_user(&prompt);
                    messages.push(user_msg(&prompt));
                    let mut orch_mut = Orchestrator::new(Some(&workspace.to_string_lossy()));
                    let _ = orch_mut.start(next_id);
                    continue;
                }
            }

            // ── 5. Auto-generate session title on first turn ──
            if iteration == 0 {
                let provider = Arc::clone(&self.provider);
                let first_user = task.clone();
                let first_reply: String = full_text.chars().take(200).collect();
                let title_tx = ui_tx.clone();
                tokio::spawn(async move {
                    let prompt_msgs = vec![
                        Message {
                            role: Role::System,
                            content: MessageContent::Text(
                                "Generate a short session title (3-8 words) for this conversation. \
                                 Output ONLY the title, no quotes, no punctuation at the end."
                                    .into(),
                            ),
                            tool_calls: None,
                            tool_call_id: None,
                            name: None,
                            reasoning_content: None,
                        },
                        Message {
                            role: Role::User,
                            content: MessageContent::Text(format!(
                                "User: {first_user}\nAssistant: {first_reply}"
                            )),
                            tool_calls: None,
                            tool_call_id: None,
                            name: None,
                            reasoning_content: None,
                        },
                    ];
                    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
                    if provider.chat(&prompt_msgs, &[], tx).await.is_ok() {
                        let mut title = String::new();
                        while let Some(ev) = rx.recv().await {
                            match ev {
                                ProviderEvent::Text(t) => title.push_str(&t),
                                ProviderEvent::Done => break,
                                _ => {}
                            }
                        }
                        let title = title.trim().to_string();
                        if !title.is_empty() && title.len() < 80 {
                            if let Err(e) = title_tx.send(UiEvent::TitleGenerated(title)).await {
                                tracing::warn!(error = %e, "failed to send TitleGenerated to UI");
                            }
                        }
                    }
                });
            }

            return Ok(());
        }

        let msg = format!(
            "⚠️  Reached max iterations ({}) without completing.",
            self.config.max_iterations
        );
        if let Err(e) = ui_tx.send(UiEvent::Error(msg)).await {
            tracing::warn!(error = %e, "failed to send max-iterations error to UI");
        }
        Ok(())
    }
}

fn rebuild_messages(
    conversation: &Conversation,
    config_system_prompt: &str,
    task: &str,
    workspace: &Path,
    agent: &Agent,
    memory: &Memory,
    messages: &mut Vec<Message>,
) {
    messages.clear();
    messages.extend(conversation.build_context(task, Some(workspace)));

    if let Some(ctx) = orchestrator::get_context_for_workspace(&workspace.display().to_string()) {
        messages.push(user_msg(&ctx));
    }

    let mut idx = 0;
    if !agent.system_prompt.is_empty() && agent.system_prompt != config_system_prompt {
        messages.insert(
            0,
            Message {
                role: Role::System,
                content: MessageContent::Text(agent.system_prompt.clone()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
                reasoning_content: None,
            },
        );
        idx = 1;
    }

    let core_ctx = memory.core_context();
    if !core_ctx.is_empty() {
        messages.insert(
            idx,
            Message {
                role: Role::System,
                content: MessageContent::Text(core_ctx),
                tool_calls: None,
                tool_call_id: None,
                name: None,
                reasoning_content: None,
            },
        );
        idx += 1;
    }
    let mem_ctx = memory.context();
    if !mem_ctx.is_empty() {
        messages.insert(
            idx,
            Message {
                role: Role::System,
                content: MessageContent::Text(mem_ctx),
                tool_calls: None,
                tool_call_id: None,
                name: None,
                reasoning_content: None,
            },
        );
    }
}
