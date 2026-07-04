//! Agent harness — generic LLM → tool → LLM loop runtime.
//!
//! The harness is execution-policy-agnostic: it runs whatever [`Agent`] and
//! [`Provider`] it is given. All orchestration, conversation context, tool
//! dispatch, and cancellation handling lives here.

use crate::agent::Agent;
use crate::conversation::Conversation;
use crate::hooks::crlf::CRLFNormalizer;
use crate::memory::Memory;
use crate::orchestrator::Orchestrator;
use crate::plugins::source::{RegexLinter, SourcePluginRegistry};
use crate::provider::Provider;
use crate::session::{items_to_messages, SessionItem};
use crate::tools::{all_tools, Tool, ToolContext};
use crate::types::{
    AgentMode, Message, MessageContent, ProviderEvent, Role, SessionConfig, ToolCall,
    ToolDefinition, ToolResult, UiEvent,
};
use crate::{orchestrator};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Hook that can inspect or transform tool results before they are sent back
/// to the LLM.
#[async_trait::async_trait]
pub trait ToolHook: Send + Sync {
    fn after(&self,
        _call: &ToolCall,
        _result: ToolResult,
        _workspace: &PathBuf,
    ) -> ToolResult {
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
            Some(PathBuf::from("conversation.jsonl")),
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

    /// Replace the provider at runtime.
    pub fn set_provider(&mut self, provider: Arc<dyn Provider>) {
        self.provider = provider;
    }

    /// Update the model name in config.
    pub fn set_model(&mut self, model: String) {
        self.config.model = model;
    }

    /// Update the agent operating mode.
    pub fn set_mode(&mut self, mode: AgentMode) {
        self.config.mode = mode;
    }

    /// Reset the conversation to an empty history, truncating the JSONL file.
    pub fn reset_conversation(&mut self) {
        self.conversation.reset_messages(Vec::new());
    }

    /// Load session items into the conversation history.
    pub fn load_session_items(&mut self, items: &[SessionItem]) {
        let messages = items_to_messages(items);
        self.conversation.reset_messages(messages);
    }

    /// Access the source plugin registry.
    pub fn source_plugins(&mut self) -> &mut SourcePluginRegistry {
        &mut self.source_plugins
    }

    /// Compress conversation history if it exceeds the token budget.
    ///
    /// Keeps system prompt (first message) and the last `keep_recent` messages,
    /// summarises everything in between via a single LLM call, and replaces the
    /// compressed range with the summary.
    ///
    /// Returns the number of messages that were compressed (0 if no compression
    /// was needed).
    pub async fn compress_context(
        &mut self,
        ui_tx: &tokio::sync::mpsc::Sender<UiEvent>,
    ) -> usize {
        const KEEP_RECENT: usize = 6;

        let max_tokens = self.config.max_context_tokens;
        let threshold = (max_tokens as f64 * self.config.context_compress_ratio) as usize;
        let est = self.conversation.estimate_tokens();

        if est <= threshold || self.conversation.messages().len() <= KEEP_RECENT + 2 {
            return 0;
        }

        let total = self.conversation.messages().len();
        let split_at = total.saturating_sub(KEEP_RECENT).max(2);

        // Extract the range to compress as plain text.
        let range_text = self.conversation.messages()[1..split_at]
            .iter()
            .filter_map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };
                let text = match &m.content {
                    MessageContent::Text(s) => s.as_str(),
                    _ => "",
                };
                if text.is_empty() {
                    return None;
                }
                Some(format!("[{role}] {text}"))
            })
            .collect::<Vec<_>>()
            .join("\n");

        if range_text.trim().is_empty() {
            // Nothing to compress — just drop empty messages.
            self.conversation
                .compress_range(split_at, "[Context compressed: empty messages dropped]".into());
            return split_at - 1;
        }

        let _ = ui_tx.send(UiEvent::LlmChunk(
            "\n[Compressing context…]\n".into(),
        )).await;

        // Build summarisation prompt.
        let compress_messages = vec![
            Message {
                role: Role::System,
                content: MessageContent::Text(
                    "You are a conversation compressor. Given a conversation history, produce a \
                     concise summary (≤400 words) that preserves:\n\
                     1. What files were read/edited and key content found.\n\
                     2. What changes were made (edits, writes, commands run).\n\
                     3. Current task state and next steps.\n\
                     4. Any errors encountered and how they were resolved.\n\
                     5. Important decisions or constraints.\n\
                     Output ONLY the summary, no preamble.".into(),
                ),
                tool_calls: None,
                tool_call_id: None,
                name: None,
                reasoning_content: None,
            },
            Message {
                role: Role::User,
                content: MessageContent::Text(range_text),
                tool_calls: None,
                tool_call_id: None,
                name: None,
                reasoning_content: None,
            },
        ];

        // Call LLM for summarisation (no tools, simple chat).
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        let provider = Arc::clone(&self.provider);
        let handle =
            tokio::spawn(async move { provider.chat(&compress_messages, &[], tx).await });

        let mut summary = String::new();
        while let Some(event) = rx.recv().await {
            match event {
                ProviderEvent::Text(chunk) => summary.push_str(&chunk),
                ProviderEvent::Done => break,
                ProviderEvent::Error(e) => {
                    summary = format!("[Context compression failed: {e}]");
                    break;
                }
                _ => {}
            }
        }
        let _ = handle.await;

        let summary = summary.trim().to_string();
        let compressed_count = split_at - 1;
        self.conversation.compress_range(
            split_at,
            format!(
                "[Context compressed: {compressed_count} older messages summarised]\n\n{summary}"
            ),
        );

        let _ = ui_tx.send(UiEvent::LlmChunk(format!(
            "[Context compressed: {compressed_count} messages → summary]\n"
        ))).await;

        compressed_count
    }

    /// Run one agent task to completion.
    pub async fn run(
        &mut self,
        task: String,
        workspace: PathBuf,
        agent: &Agent,
        extra_tools: &[Box<dyn Tool>],
        _hb_cancel: Option<tokio::sync::mpsc::Sender<()>>,
        ui_tx: tokio::sync::mpsc::Sender<UiEvent>,
        mut cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        // Spawn background flush task for conversation persistence.
        let _flush_handle = self.conversation.spawn_flush_task();

        let llm_timeout = Duration::from_secs(self.config.llm_timeout_secs);
        let tool_timeout = Duration::from_secs(self.config.tool_timeout_secs);

        let mut messages = self.conversation.build_context(&task, Some(&workspace));
        if let Some(ctx) = orchestrator::get_context_for_workspace(&workspace.display().to_string()) {
            messages.push(user_msg(&ctx));
        }

        // If the agent has its own system prompt, prepend it.
        let mut insert_idx = 0;
        if !agent.system_prompt.is_empty()
            && agent.system_prompt != self.config.system_prompt
        {
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

        // Inject memory context as system messages.
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
        // Merge extra tools (e.g. MCP).
        let mut all_tool_defs = tool_defs.clone();
        // Merge extra tools (e.g. MCP).
        let extra_defs: Vec<ToolDefinition> = extra_tools.iter().map(|t| t.definition()).collect();
        all_tool_defs.extend(extra_defs);

        let allowed_names: std::collections::HashSet<String> = if agent.allowed_tools.is_empty() {
            all_tool_defs.iter().map(|d| d.function.name.clone()).collect()
        } else {
            agent.allowed_tools.iter().cloned().collect()
        };

        // Sanitize conversation history before first use.
        // Removes orphaned tool_calls that have no matching tool result,
        // which causes DeepSeek (and similar providers) to return 400.
        self.conversation.sanitize();

        for iteration in 0..self.config.max_iterations {
            // ── 0. Context compression ──
            if iteration > 0 {
                let compressed = self.compress_context(&ui_tx).await;
                if compressed > 0 {
                    // Rebuild local messages from the compressed conversation.
                    messages = self.conversation.build_context(&task, Some(&workspace));
                    if let Some(ctx) =
                        orchestrator::get_context_for_workspace(&workspace.display().to_string())
                    {
                        messages.push(user_msg(&ctx));
                    }
                    if !agent.system_prompt.is_empty()
                        && agent.system_prompt != self.config.system_prompt
                    {
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
                    }
                    let mut rebuild_idx = if !agent.system_prompt.is_empty()
                        && agent.system_prompt != self.config.system_prompt
                    {
                        1
                    } else {
                        0
                    };
                    let rebuild_core = memory.core_context();
                    if !rebuild_core.is_empty() {
                        messages.insert(
                            rebuild_idx,
                            Message {
                                role: Role::System,
                                content: MessageContent::Text(rebuild_core),
                                tool_calls: None,
                                tool_call_id: None,
                                name: None,
                                reasoning_content: None,
                            },
                        );
                        rebuild_idx += 1;
                    }
                    let rebuild_ctx = memory.context();
                    if !rebuild_ctx.is_empty() {
                        messages.insert(
                            rebuild_idx,
                            Message {
                                role: Role::System,
                                content: MessageContent::Text(rebuild_ctx),
                                tool_calls: None,
                                tool_call_id: None,
                                name: None,
                                reasoning_content: None,
                            },
                        );
                    }
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
                        let _ = ui_tx.send(UiEvent::LlmChunk(chunk.clone())).await;
                        full_text.push_str(&chunk);
                    }
                    Some(ProviderEvent::Reasoning(rc)) => {
                        let _ = ui_tx.send(UiEvent::LlmReasoning(rc.clone())).await;
                        full_reasoning.push_str(&rc);
                    }
                    Some(ProviderEvent::ToolCalls(calls)) => {
                        last_tool_calls = Some(calls);
                    }
                    Some(ProviderEvent::Done) => break,
                    Some(ProviderEvent::Error(e)) => {
                        let _ = ui_tx.send(UiEvent::Error(e)).await;
                        break;
                    }
                    None => break,
                }
            }

            let _ = ui_tx.send(UiEvent::LlmDone).await;

            if timed_out {
                chat_handle.abort();
                let explain = format!(
                    "⚠️ LLM request timed out after {}s (iteration {}).",
                    self.config.llm_timeout_secs,
                    iteration + 1
                );
                let _ = ui_tx.send(UiEvent::Error(explain.clone())).await;
                messages.push(user_msg(&explain));
                continue;
            }

            match chat_handle.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    let _ = ui_tx.send(UiEvent::Error(e.to_string())).await;
                    return Err(e);
                }
                Err(join_err) => {
                    if join_err.is_cancelled() {
                        continue;
                    }
                    let msg = format!("Provider panicked: {join_err}");
                    let _ = ui_tx.send(UiEvent::Error(msg.clone())).await;
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
                    // Filter by agent tool allowlist + mode allowlist.
                    if !allowed_names.contains(&tc.function.name) {
                        let err = format!("Tool '{}' is not allowed for this agent/mode", tc.function.name);
                        let _ = ui_tx.send(UiEvent::Error(err.clone())).await;
                        let tr = ToolResult {
                            tool_call_id: tc.id.clone(),
                            content: err,
                            is_error: true,
                        };
                            self.conversation
                                .push_tool_result(tc, &tr, Some(&workspace));
                        messages.push(tool_result_msg(tc, tr));
                        continue;
                    }

                    let tool = self
                        .tools
                        .iter()
                        .find(|t| t.definition().function.name == tc.function.name)
                        .or_else(|| {
                            extra_tools.iter().find(|t| t.definition().function.name == tc.function.name)
                        });

                    match tool {
                        Some(tool) => {
                            let _ = ui_tx.send(UiEvent::ToolStart {
                                name: tc.function.name.clone(),
                                index: i,
                                total,
                                args: tc.function.arguments.clone(),
                            }).await;
                            let ws = workspace.clone();
                            let args = tc.function.arguments.clone();
                            let ctx = ToolContext {
                                ui_tx: ui_tx.clone(),
                                source_plugins: Some(Arc::new(self.source_plugins.clone())),
                            };
                            let result =
                                exec_with_timeout(tool.as_ref(), &ws, &args, tool_timeout, &ctx)
                                    .await;

                            let mut final_result = result;
                            for hook in &self.tool_hooks {
                                final_result = hook.after(tc, final_result, &workspace);
                            }

                            let _ = ui_tx.send(UiEvent::ToolDone).await;
                            let _ = ui_tx.send(UiEvent::ToolResult {
                                content: final_result.content.trim_end().to_string(),
                            }).await;
                            self.conversation
                                .push_tool_result(tc, &final_result, Some(&workspace));
                            messages.push(tool_result_msg(tc, final_result));
                        }
                        None => {
                            let _ = ui_tx.send(UiEvent::ToolStart {
                                name: tc.function.name.clone(),
                                index: i,
                                total,
                                args: String::new(),
                            }).await;
                            let err = format!("Unknown tool: {}", tc.function.name);
                            let _ = ui_tx.send(UiEvent::Error(err.clone())).await;
                            let tr = ToolResult {
                                tool_call_id: tc.id.clone(),
                                content: err,
                                is_error: true,
                            };
                            self.conversation
                                .push_tool_result(tc, &tr, Some(&workspace));
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

            // ── 4. Session-level orchestration: auto-continue if plan has ready tasks ──
            if self.config.auto_continue {
                let orch = Orchestrator::new(Some(&workspace.to_string_lossy()));
                let ready = orch.get_ready_tasks();
                if !ready.is_empty() {
                    let next = ready[0];
                    let next_id = next.id;
                    let next_title = next.title.clone();
                    let next_agent = next.agent.clone();
                    let _ = ui_tx.send(UiEvent::LlmChunk(format!(
                        "\n\n▶ Auto-continuing plan: #{} {}\n\n",
                        next_id, next_title
                    ))).await;
                    // Inject the next task as a new user message
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
                    // Mark the task as active
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
                            let _ = title_tx.send(UiEvent::TitleGenerated(title)).await;
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
        let _ = ui_tx.send(UiEvent::Error(msg)).await;
        Ok(())
    }
}

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
