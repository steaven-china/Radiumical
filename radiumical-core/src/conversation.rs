//! Conversation context — JSONL-backed message history, reused across turns.
//! Each line = one Message as JSON. Debug with `cat conversation.jsonl`.
use crate::types::{Message, MessageContent, Role, ToolCall, ToolResult};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Manages the full conversation history with JSONL persistence.
pub struct Conversation {
    messages: Vec<Message>,
    system_prompt: String,
    jsonl_path: Option<PathBuf>,
    /// Files the model has already read, mapped to the mtime when last seen.
    seen_files: HashMap<String, u64>,
}

impl Conversation {
    pub fn new(system_prompt: String, _jsonl_path: Option<PathBuf>) -> Self {
        Self {
            messages: Vec::new(),
            system_prompt,
            jsonl_path: _jsonl_path,
            seen_files: HashMap::new(),
        }
    }

    /// Record that the model has read `path` (workspace-relative) at the
    /// file's current modification time.
    pub fn mark_file_seen(&mut self, workspace: &Path, path: &str) {
        let full = workspace.join(path);
        if let Ok(meta) = std::fs::metadata(&full) {
            if let Ok(modified) = meta.modified() {
                if let Ok(elapsed) = modified.duration_since(SystemTime::UNIX_EPOCH) {
                    self.seen_files.insert(path.to_string(), elapsed.as_secs());
                    return;
                }
            }
        }
        // If we can't read metadata, still track it with 0 so we can detect
        // future changes.
        self.seen_files.insert(path.to_string(), 0);
    }

    /// Return workspace-relative paths of seen files that have been modified
    /// since the model last looked at them.
    fn changed_seen_files(&self,
        workspace: &Path,
    ) -> Vec<String> {
        let mut changed = Vec::new();
        for (path, seen_mtime) in &self.seen_files {
            let full = workspace.join(path);
            let current = std::fs::metadata(&full)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(*seen_mtime);
            if current > *seen_mtime {
                changed.push(path.clone());
            }
        }
        changed
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

    pub fn push_assistant(
        &mut self,
        content: &str,
        tool_calls: Option<Vec<ToolCall>>,
        reasoning: Option<&str>,
    ) {
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

    pub fn push_tool_result(
        &mut self,
        call: &ToolCall,
        result: &ToolResult,
        workspace: Option<&Path>,
    ) {
        let call_id = if call.id.is_empty() {
            format!("call_{}", call.function.name)
        } else {
            call.id.clone()
        };
        let content = Self::truncate_tool_content(&result.content, Self::MAX_TOOL_RESULT_CHARS);

        // Track files the model has read so we can later warn about external
        // modifications.
        if let Some(ws) = workspace {
            if matches!(
                call.function.name.as_str(),
                "read_file" | "edit_file" | "write_file"
            ) {
                if let Ok(args) = serde_json::from_str::<serde_json::Value>(
                    &call.function.arguments
                ) {
                    if let Some(path) = args["path"].as_str() {
                        self.mark_file_seen(ws, path);
                    }
                }
            }
        }

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

    /// Build the full message array for an LLM request: [system, outline, ...history, new_user_msg].
    /// Sanitizes: strips orphaned tool_calls without matching tool results.
    pub fn build_context(&self, user_task: &str, workspace: Option<&Path>) -> Vec<Message> {
        let mut ctx = vec![Message {
            role: Role::System,
            content: MessageContent::Text(self.system_prompt.clone()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        }];

        // Inject workspace outline if available
        if let Some(ws) = workspace {
            let outline_text = crate::outline::formatted_outline(ws);
            if !outline_text.is_empty() {
                ctx.push(Message {
                    role: Role::System,
                    content: MessageContent::Text(outline_text),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                    reasoning_content: None,
                });
            }

            // Notify the model about files it has already read that changed
            // underneath it.
            let changed = self.changed_seen_files(ws);
            if !changed.is_empty() {
                let notice = format!(
                    "## Changed files since you last read them\n\n\
                    The following files you have already looked at were modified externally \
                    or by your own edits. Re-read them if you need up-to-date contents:\n\n{}\n",
                    changed.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n")
                );
                ctx.push(Message {
                    role: Role::System,
                    content: MessageContent::Text(notice),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                    reasoning_content: None,
                });
            }
        }

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
    pub fn estimate_tokens(&self) -> usize {
        self.messages
            .iter()
            .map(|m| {
                let text = match &m.content {
                    MessageContent::Text(s) => s.chars().count(),
                    _ => 0,
                };
                let reasoning = m
                    .reasoning_content
                    .as_ref()
                    .map(|s| s.chars().count())
                    .unwrap_or(0);
                let tool_calls = m
                    .tool_calls
                    .as_ref()
                    .map(|calls| {
                        calls
                            .iter()
                            .map(|c| c.function.name.len() + c.function.arguments.len())
                            .sum::<usize>()
                    })
                    .unwrap_or(0);
                (text + reasoning + tool_calls) / 4
            })
            .sum()
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

    /// Replace the in-memory messages and rewrite the JSONL backing file.
    pub fn reset_messages(&mut self, messages: Vec<Message>) {
        self.messages = messages;
        self.rewrite_jsonl();
    }

    /// Read-only access to messages.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Replace messages[1..split_at] with a single summary message.
    /// Keeps messages[0] (system) and messages[split_at..] (recent) intact.
    pub fn compress_range(&mut self, split_at: usize, summary: String) {
        if split_at <= 1 || split_at >= self.messages.len() {
            return;
        }
        let mut new_msgs = Vec::with_capacity(split_at + 2);
        new_msgs.push(self.messages[0].clone());
        new_msgs.push(Message {
            role: Role::System,
            content: MessageContent::Text(summary),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        });
        new_msgs.extend_from_slice(&self.messages[split_at..]);
        self.messages = new_msgs;
        self.rewrite_jsonl();
    }

    /// Set or clear the JSONL persistence path.
    pub fn set_jsonl_path(&mut self, path: Option<PathBuf>) {
        self.jsonl_path = path;
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

    fn rewrite_jsonl(&self) {
        if let Some(ref path) = self.jsonl_path {
            if let Ok(mut f) = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(path)
            {
                for msg in &self.messages {
                    if let Ok(json) = serde_json::to_string(msg) {
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
        let tail: String = s
            .chars()
            .rev()
            .take(max_chars / 4)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
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
        if msgs.is_empty() {
            None
        } else {
            Some(msgs)
        }
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionCall, ToolCall, ToolResult};

    fn make_tool_call(id: &str, name: &str, args: &str) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: name.to_string(),
                arguments: args.to_string(),
            },
        }
    }

    #[test]
    fn test_truncate_tool_content_below_limit() {
        let short = "hello";
        let result = Conversation::truncate_tool_content(short, 8000);
        assert_eq!(result, short);
    }

    #[test]
    fn test_truncate_tool_content_above_limit() {
        let long = "x".repeat(10_000);
        let result = Conversation::truncate_tool_content(&long, 100);
        assert!(result.len() <= 200); // generous upper bound due to padding text
        assert!(result.contains("truncated"));
        // Should contain both head and tail
        assert!(result.starts_with("xxx"));
        let tail_start = result.rfind("xxx").unwrap();
        assert!(tail_start > 50); // tail is near the end
    }

    #[test]
    fn test_truncate_tool_content_exact_limit() {
        let exact = "x".repeat(8000);
        let result = Conversation::truncate_tool_content(&exact, 8000);
        assert_eq!(result.len(), 8000);
        assert!(!result.contains("truncated"));
    }

    #[test]
    fn test_estimated_tokens() {
        let mut conv = Conversation::new("You are helpful.".into(), None);
        conv.push_user("Hello, how are you?");
        conv.push_assistant("I'm fine, thank you!", None, None);
        // 26 + 21 = 47 chars / 4 ≈ 11 tokens
        let tokens = conv.estimate_tokens();
        assert!(tokens > 0);
    }

    #[test]
    fn test_clear_history() {
        let mut conv = Conversation::new("System prompt".into(), None);
        conv.push_user("Hello");
        assert_eq!(conv.messages.len(), 1);
        conv.clear_history();
        assert_eq!(conv.messages.len(), 0);
    }

    #[test]
    fn test_build_context_basic() {
        let mut conv = Conversation::new("You are Radium.".into(), None);
        conv.push_user("Previous question");
        conv.push_assistant("Previous answer", None, None);

        let ctx = conv.build_context("New task", None);
        assert_eq!(ctx.len(), 4); // system + user + assistant + new user
        assert!(matches!(ctx[0].role, Role::System));
        assert!(matches!(ctx[1].role, Role::User));
        assert!(matches!(ctx[2].role, Role::Assistant));
        assert!(matches!(ctx[3].role, Role::User));
        assert_eq!(
            match &ctx[3].content {
                MessageContent::Text(s) => s.as_str(),
                _ => "",
            },
            "New task"
        );
    }

    #[test]
    fn test_build_context_strips_orphan_tool_calls() {
        let mut conv = Conversation::new("System".into(), None);
        conv.push_user("Do something");
        // Assistant with tool calls but NO matching tool results
        conv.push(Message {
            role: Role::Assistant,
            content: MessageContent::Text("Calling tool...".into()),
            tool_calls: Some(vec![make_tool_call(
                "call_1",
                "read_file",
                r#"{"path":"x"}"#,
            )]),
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        });

        let ctx = conv.build_context("Next task", None);
        let assistant_msgs: Vec<_> = ctx
            .iter()
            .filter(|m| matches!(m.role, Role::Assistant))
            .collect();
        assert!(
            assistant_msgs.is_empty(),
            "orphan tool call should be removed"
        );
    }

    #[test]
    fn test_build_context_keeps_resolved_tool_calls() {
        let mut conv = Conversation::new("System".into(), None);
        conv.push_user("Read a file");
        let tc = make_tool_call("call_1", "read_file", r#"{"path":"x"}"#);
        conv.push(Message {
            role: Role::Assistant,
            content: MessageContent::Text("Reading...".into()),
            tool_calls: Some(vec![tc.clone()]),
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        });
        conv.push_tool_result(
            &tc,
            &ToolResult {
                tool_call_id: "call_1".into(),
                content: "file contents here".into(),
                is_error: false,
            },
            None,
        );

        let ctx = conv.build_context("Next task", None);
        let tool_msgs: Vec<_> = ctx
            .iter()
            .filter(|m| matches!(m.role, Role::Tool))
            .collect();
        assert_eq!(tool_msgs.len(), 1, "resolved tool result should remain");
    }

    #[test]
    fn test_truncate_to_tokens() {
        let mut conv = Conversation::new("System".into(), None);
        // Add lots of messages
        for i in 0..100 {
            conv.push_user(&format!("Message number {}", i));
        }
        let before = conv.messages.len();
        conv.truncate_to_tokens(50); // ~50 tokens = ~200 chars
        let after = conv.messages.len();
        assert!(after < before, "should have truncated some messages");
        assert!(after >= 1, "should keep at least one message");
    }

    #[test]
    fn test_truncate_tool_content_unicode() {
        let unicode_str = "你好世界".repeat(3000);
        let result = Conversation::truncate_tool_content(&unicode_str, 100);
        // Should not panic and truncate correctly
        assert!(result.chars().count() <= 200); // head + tail + padding
        assert!(result.contains("truncated"));
    }

    #[test]
    fn test_history_len() {
        let mut conv = Conversation::new("S".into(), None);
        assert_eq!(conv.history_len(), 0);
        conv.push_user("a");
        assert_eq!(conv.history_len(), 1);
        conv.push_assistant("b", None, None);
        assert_eq!(conv.history_len(), 2);
    }
}
