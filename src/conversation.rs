//! Conversation context — JSONL-backed message history, reused across turns.
//! Each line = one Message as JSON. Debug with `cat conversation.jsonl`.
use crate::types::{Message, MessageContent, Role, ToolCall, ToolResult};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// Manages the full conversation history with JSONL persistence.
pub struct Conversation {
    messages: Vec<Message>,
    system_prompt: String,
    jsonl_path: Option<PathBuf>,
}

impl Conversation {
    pub fn new(system_prompt: String, _jsonl_path: Option<PathBuf>) -> Self {
        Self { messages: Vec::new(), system_prompt, jsonl_path: _jsonl_path }
    }

    // ── Mutation ──

    #[allow(dead_code)]
    pub fn push_system(&mut self, content: &str) {
        self.push(Message {
            role: Role::System,
            content: MessageContent::Text(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        });
    }

    #[allow(dead_code)]
    pub fn push_user(&mut self, content: &str) {
        self.push(Message {
            role: Role::User,
            content: MessageContent::Text(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        });
    }

    pub fn push_assistant(&mut self, content: &str, tool_calls: Option<Vec<ToolCall>>, reasoning: Option<&str>) {
        self.push(Message {
            role: Role::Assistant,
            content: MessageContent::Text(content.to_string()),
            tool_calls,
            tool_call_id: None,
            name: None,
            reasoning_content: reasoning.map(|s| s.to_string()),
        });
    }

    /// Max characters to keep from a tool result. Output beyond this is truncated
    /// to prevent bloating the conversation file and wasting LLM context.
    const MAX_TOOL_RESULT_CHARS: usize = 8000;

    pub fn push_tool_result(&mut self, call: &ToolCall, result: &ToolResult) {
        let call_id = if call.id.is_empty() {
            format!("call_{}", call.function.name)
        } else {
            call.id.clone()
        };
        let content = Self::truncate_tool_content(&result.content, Self::MAX_TOOL_RESULT_CHARS);
        self.push(Message {
            role: Role::Tool,
            content: MessageContent::Text(content),
            tool_calls: None,
            tool_call_id: Some(call_id),
            name: Some(call.function.name.clone()),
            reasoning_content: None,
        });
    }

    pub fn push(&mut self, msg: Message) {
        self.messages.push(msg);
        self.flush();
    }

    // ── Context assembly ──

    /// Build the full message array for an LLM request: [system, ...history, new_user_msg].
    /// Sanitizes: strips orphaned tool_calls without matching tool results.
    pub fn build_context(&self, user_task: &str) -> Vec<Message> {
        let mut ctx = vec![Message {
            role: Role::System,
            content: MessageContent::Text(self.system_prompt.clone()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        }];

        // Sanitize history: drop any assistant tool_calls that lack matching tool results
        let mut sanitized: Vec<Message> = Vec::new();
        let mut pending_tool_ids: Vec<String> = Vec::new();
        for msg in &self.messages {
            match msg.role {
                Role::Assistant if msg.tool_calls.is_some() => {
                    // Collect tool call IDs that need matching results
                    if let Some(ref calls) = msg.tool_calls {
                        for tc in calls {
                            if !tc.id.is_empty() {
                                pending_tool_ids.push(tc.id.clone());
                            }
                        }
                    }
                    sanitized.push(msg.clone());
                }
                Role::Tool => {
                    // This tool result matches a pending tool call
                    if let Some(ref call_id) = msg.tool_call_id {
                        pending_tool_ids.retain(|id| id != call_id);
                    }
                    sanitized.push(msg.clone());
                }
                _ => {
                    // If there are unresolved tool calls when we hit a user/system message,
                    // drop the orphaned assistant messages
                    if !pending_tool_ids.is_empty() && msg.role == Role::User {
                        // Remove the last assistant message(s) that have no tool results
                        while let Some(last) = sanitized.last() {
                            if last.role == Role::Assistant && last.tool_calls.is_some() {
                                sanitized.pop();
                            } else {
                                break;
                            }
                        }
                        pending_tool_ids.clear();
                    }
                    sanitized.push(msg.clone());
                }
            }
        }
        // Final cleanup: strip any trailing unresolved tool calls
        while let Some(last) = sanitized.last() {
            if last.role == Role::Assistant && last.tool_calls.is_some() {
                sanitized.pop();
            } else {
                break;
            }
        }

        ctx.extend(sanitized);
        ctx.push(Message {
            role: Role::User,
            content: MessageContent::Text(user_task.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        });
        ctx
    }

    #[allow(dead_code)]
    pub fn history_len(&self) -> usize {
        self.messages.len()
    }

    /// Rough token count (1 token ≈ 4 chars).
    #[allow(dead_code)]
    pub fn estimated_tokens(&self) -> usize {
        self.messages.iter().map(|m| match &m.content {
            MessageContent::Text(s) => s.chars().count(),
            _ => 0,
        }).sum::<usize>() / 4
    }

    /// Truncate context to keep the last `max_tokens` worth of messages.
    /// Always keeps the system prompt (first message if it's a system message).
    #[allow(dead_code)]
    pub fn truncate_to_tokens(&mut self, max_tokens: usize) {
        let mut total = 0usize;
        let mut keep_from = 0usize;
        // Always keep system message at index 0
        for (i, msg) in self.messages.iter().enumerate().rev() {
            let chars = match &msg.content {
                MessageContent::Text(s) => s.chars().count(),
                _ => 0,
            };
            if total + chars / 4 > max_tokens && i > 0 {
                keep_from = i + 1;
                break;
            }
            total += chars / 4;
        }
        if keep_from > 1 {
            self.messages.drain(1..keep_from);
        }
    }

    /// Clear all messages (keep system prompt in memory but not in messages).
    #[allow(dead_code)]
    pub fn clear_history(&mut self) {
        self.messages.clear();
    }

    // ── JSONL persistence ──

    fn flush(&self) {
        if let Some(ref path) = self.jsonl_path {
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
                if let Some(last) = self.messages.last() {
                    if let Ok(json) = serde_json::to_string(last) {
                        let _ = writeln!(f, "{json}");
                    }
                }
            }
        }
    }

    fn truncate_tool_content(s: &str, max_chars: usize) -> String {
        if s.chars().count() <= max_chars {
            return s.to_string();
        }
        let head: String = s.chars().take(max_chars / 2).collect();
        let tail: String = s.chars().rev().take(max_chars / 4).collect::<String>()
            .chars().rev().collect();
        format!(
            "{head}\n\n... [truncated {} chars → {} kept] ...\n\n{tail}",
            s.chars().count(),
            max_chars,
        )
    }

    #[allow(dead_code)]
    fn load_jsonl(path: &PathBuf) -> Option<Vec<Message>> {
        let file = File::open(path).ok()?;
        let reader = BufReader::new(file);
        let mut msgs = Vec::new();
        for line in reader.lines().flatten() {
            if let Ok(msg) = serde_json::from_str::<Message>(&line) {
                msgs.push(msg);
            }
        }
        if msgs.is_empty() { None } else { Some(msgs) }
    }
}
