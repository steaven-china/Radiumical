//! Conversation context — zstd-compressed JSONL-backed message history.
//!
//! Each line = one Message as JSON, stored in `conversation.jsonl.zst`.
//! Backward-compatible: reads plain `.jsonl` if `.zst` doesn't exist.
//!
//! Flush is async: `push()` writes to memory and queues a JSONL line.
//! A background task drains the queue every 500ms and appends to the zstd file.

use crate::types::{Message, MessageContent, Role, ToolCall, ToolResult};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Manages the full conversation history with JSONL persistence.
pub struct Conversation {
    messages: Vec<Message>,
    system_prompt: String,
    jsonl_path: Option<PathBuf>,
    seen_files: HashMap<String, u64>,
    /// Queued JSONL lines waiting to be flushed.
    pending: Arc<Mutex<Vec<String>>>,
    /// Dirty counter — incremented on each push, drained by flush task.
    dirty: Arc<AtomicU32>,
    /// Set when a full rewrite is needed (reset/compress).
    needs_rewrite: Arc<AtomicBool>,
}

impl Conversation {
    pub fn new(system_prompt: String, jsonl_path: Option<PathBuf>) -> Self {
        Self {
            messages: Vec::new(),
            system_prompt,
            jsonl_path,
            seen_files: HashMap::new(),
            pending: Arc::new(Mutex::new(Vec::new())),
            dirty: Arc::new(AtomicU32::new(0)),
            needs_rewrite: Arc::new(AtomicBool::new(false)),
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
        self.seen_files.insert(path.to_string(), 0);
    }

    fn changed_seen_files(&self, workspace: &Path) -> Vec<String> {
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

    pub fn push_tool_result(
        &mut self,
        call: &ToolCall,
        result: &ToolResult,
        _workspace: Option<&Path>,
    ) {
        // Warn if files the model read earlier were modified.
        let mut extra = String::new();
        if let Some(ws) = _workspace {
            let changed = self.changed_seen_files(ws);
            if !changed.is_empty() {
                extra = format!(
                    "\n\n⚠️ Files changed since you last read them: {}. \
                     Re-read before editing if unsure.",
                    changed.join(", ")
                );
            }
        }

        // Tool result only — the assistant message with tool_calls is pushed
        // by the caller (harness) before calling this method.
        let content = if extra.is_empty() {
            result.content.clone()
        } else {
            format!("{}{}", result.content, extra)
        };
        self.push(Message {
            role: Role::Tool,
            content: MessageContent::Text(content),
            tool_calls: None,
            tool_call_id: Some(call.id.clone()),
            name: Some(call.function.name.clone()),
            reasoning_content: None,
        });
    }

    // ── Accessors ──

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Replace the in-memory messages and rewrite the JSONL backing file.
    pub fn reset_messages(&mut self, messages: Vec<Message>) {
        self.messages = messages;
        self.drain_pending(); // discard any queued writes
        self.full_rewrite();
    }

    /// Sanitize messages in-place: remove orphaned tool_calls (assistant
    /// messages with tool_calls but no matching tool result) and orphan tool
    /// results (tool messages with no matching call). Must be called before
    /// sending to providers that strictly enforce tool_calls → tool_results
    /// pairing (e.g. DeepSeek).
    pub fn sanitize(&mut self) {
        crate::types::sanitize_tool_messages(&mut self.messages);
    }

    /// Read-only access to messages.
    /// Replace messages[1..split_at] with a single summary message.
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
        self.drain_pending();
        self.full_rewrite();
    }

    /// Set or clear the JSONL persistence path.
    pub fn set_jsonl_path(&mut self, path: Option<PathBuf>) {
        self.jsonl_path = path;
    }

    // ── Token estimation ──

    /// Rough token count (1 token ≈ 4 chars), including reasoning and tool calls.
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

    /// Truncate old messages to fit within `max_tokens`.
    /// Keeps system prompt and the most recent messages.
    #[allow(dead_code)]
    pub fn truncate_to_tokens(&mut self, max_tokens: usize) {
        let total = self.estimate_tokens();
        if total <= max_tokens || self.messages.len() <= 2 {
            return;
        }
        // Keep system message + recent messages that fit in budget.
        let mut kept = vec![self.messages[0].clone()];
        let mut tokens = 0;
        for msg in self.messages.iter().skip(1).rev() {
            let msg_tokens = match &msg.content {
                MessageContent::Text(s) => s.chars().count() / 4,
                _ => 0,
            };
            if tokens + msg_tokens > max_tokens {
                break;
            }
            tokens += msg_tokens;
            kept.push(msg.clone());
        }
        kept.reverse();
        self.messages = kept;
    }

    // ── Build context for LLM ──

    pub fn build_context(&self, task: &str, _workspace: Option<&Path>) -> Vec<Message> {
        let mut msgs = Vec::new();

        // System prompt
        msgs.push(Message {
            role: Role::System,
            content: MessageContent::Text(self.system_prompt.clone()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        });

        // Historical messages
        msgs.extend(self.messages.iter().cloned());

        // Current task
        if !task.is_empty() {
            msgs.push(Message {
                role: Role::User,
                content: MessageContent::Text(task.to_string()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
                reasoning_content: None,
            });
        }

        msgs
    }

    /// Build a truncated context with a preview of conversation history.
    /// For long conversations, shows head + tail to stay within token limits.
    #[allow(dead_code)]
    pub fn build_context_with_preview(
        &self,
        task: &str,
        workspace: Option<&Path>,
        max_chars: usize,
    ) -> Vec<Message> {
        let msgs = self.build_context(task, workspace);
        if msgs.len() <= 6 {
            return msgs;
        }

        // Estimate total chars
        let total_chars: usize = msgs
            .iter()
            .map(|m| match &m.content {
                MessageContent::Text(s) => s.len(),
                _ => 0,
            })
            .sum();

        if total_chars <= max_chars {
            return msgs;
        }

        // Keep: system + first 2 + ... + last 2 + current task
        let mut result = vec![
            msgs[0].clone(), // system
            msgs[1].clone(), // workspace outline
            msgs[2].clone(), // first user
            msgs[3].clone(), // first assistant
        ];

        let skipped = msgs.len() - 6;
        result.push(Message {
            role: Role::System,
            content: MessageContent::Text(format!(
                "[... {skipped} messages omitted for brevity ...]"
            )),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        });

        for msg in &msgs[msgs.len() - 3..] {
            result.push(msg.clone());
        }

        result
    }

    /// Produce a compact text preview of a conversation message.
    #[allow(dead_code)]
    pub fn preview_message(msg: &Message) -> String {
        let role = match msg.role {
            Role::System => "sys",
            Role::User => "user",
            Role::Assistant => "asst",
            Role::Tool => "tool",
        };
        let text = match &msg.content {
            MessageContent::Text(s) => s.as_str(),
            _ => "",
        };
        let preview = Self::truncate_str(text, 200);
        format!("[{role}] {preview}")
    }

    /// Truncate a string to max_chars, adding "..." if truncated.
    #[allow(dead_code)]
    fn truncate_str(s: &str, max_chars: usize) -> String {
        if s.chars().count() <= max_chars {
            return s.to_string();
        }
        let head: String = s.chars().take(max_chars / 2).collect();
        let tail: String = s
            .chars()
            .skip(s.chars().count() - max_chars / 2)
            .take(max_chars / 4)
            .collect();
        format!(
            "{head}\n\n... [truncated {} chars → {} kept] ...\n\n{tail}",
            s.chars().count(),
            max_chars,
        )
    }

    // ── Async flush ──

    /// Internal: push a message, queue its JSONL line, increment dirty counter.
    fn push(&mut self, msg: Message) {
        if let Ok(json) = serde_json::to_string(&msg) {
            if let Ok(mut pending) = self.pending.lock() {
                pending.push(json);
            }
        }
        self.messages.push(msg);
        self.dirty.fetch_add(1, Ordering::Relaxed);
    }

    /// Drain pending queue, returning the queued JSONL lines.
    fn drain_pending(&self) -> Vec<String> {
        if let Ok(mut pending) = self.pending.lock() {
            let drained: Vec<String> = pending.drain(..).collect();
            self.dirty.store(0, Ordering::Relaxed);
            drained
        } else {
            Vec::new()
        }
    }

    /// Spawn a background flush task. Call once at startup.
    /// Returns a JoinHandle that can be aborted on shutdown.
    pub fn spawn_flush_task(&self) -> tokio::task::JoinHandle<()> {
        let pending = Arc::clone(&self.pending);
        let dirty = Arc::clone(&self.dirty);
        let needs_rewrite = Arc::clone(&self.needs_rewrite);
        let jsonl_path = self.jsonl_path.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                // Check if a full rewrite was requested.
                if needs_rewrite.swap(false, Ordering::Relaxed) {
                    // Full rewrite is handled synchronously by the caller.
                    // We just skip this tick.
                    continue;
                }

                // Drain pending lines.
                let lines: Vec<String> = {
                    let Ok(mut guard) = pending.lock() else {
                        continue;
                    };
                    if guard.is_empty() {
                        continue;
                    }
                    dirty.store(0, Ordering::Relaxed);
                    guard.drain(..).collect()
                };

                if lines.is_empty() {
                    continue;
                }

                // Append to zstd file.
                if let Some(ref path) = jsonl_path {
                    let zst_path = zst_path(path);
                    let data = lines.join("\n") + "\n";
                    if let Ok(compressed) = zstd::encode_all(data.as_bytes(), 3) {
                        if let Ok(mut f) =
                            OpenOptions::new().create(true).append(true).open(&zst_path)
                        {
                            if let Err(e) = f.write_all(&compressed) {
                                tracing::error!(error = %e, "failed to write conversation JSONL (append)");
                            }
                        }
                    }
                }
            }
        })
    }

    /// Synchronous full rewrite of the zstd file (used for reset/compress).
    fn full_rewrite(&self) {
        if let Some(ref path) = self.jsonl_path {
            let zst_path = zst_path(path);
            let mut buf = Vec::new();
            for msg in &self.messages {
                if let Ok(json) = serde_json::to_string(msg) {
                    buf.extend_from_slice(json.as_bytes());
                    buf.push(b'\n');
                }
            }
            if let Ok(compressed) = zstd::encode_all(buf.as_slice(), 3) {
                if let Ok(mut f) = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&zst_path)
                {
                    if let Err(e) = f.write_all(&compressed) {
                        tracing::error!(error = %e, "failed to write conversation JSONL (rewrite)");
                    }
                }
            }
        }
    }

    // ── JSONL loading ──

    #[allow(dead_code)]
    pub fn load_jsonl(path: &Path) -> Option<Vec<Message>> {
        let zst = zst_path(path);
        if let Some(msgs) = Self::load_zst(&zst) {
            return Some(msgs);
        }
        Self::load_plain(path)
    }

    fn load_plain(path: &Path) -> Option<Vec<Message>> {
        let file = File::open(path).ok()?;
        let reader = BufReader::new(file);
        let mut msgs = Vec::new();
        for line in reader.lines().map_while(Result::ok) {
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

    fn load_zst(path: &Path) -> Option<Vec<Message>> {
        let compressed = std::fs::read(path).ok()?;
        let decompressed = zstd::decode_all(compressed.as_slice()).ok()?;
        let text = String::from_utf8(decompressed).ok()?;
        let mut msgs = Vec::new();
        for line in text.lines() {
            if let Ok(msg) = serde_json::from_str::<Message>(line) {
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

/// Derive `.jsonl.zst` path from a `.jsonl` path.
fn zst_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".zst");
    PathBuf::from(s)
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

    fn make_tool_result(id: &str, content: &str) -> ToolResult {
        ToolResult {
            tool_call_id: id.to_string(),
            content: content.to_string(),
            is_error: false,
        }
    }

    #[test]
    fn test_new_empty() {
        let conv = Conversation::new("sys".to_string(), None);
        assert!(conv.messages().is_empty());
    }

    #[test]
    fn test_push_user_assistant() {
        let mut conv = Conversation::new("sys".to_string(), None);
        conv.push_user("hello");
        conv.push_assistant("hi", None, None);
        assert_eq!(conv.messages().len(), 2);
        assert_eq!(conv.messages()[0].role, Role::User);
        assert_eq!(conv.messages()[1].role, Role::Assistant);
    }

    #[test]
    fn test_push_tool_result() {
        let mut conv = Conversation::new("sys".to_string(), None);
        let call = make_tool_call("1", "read_file", "{}");
        let result = make_tool_result("1", "file content");
        // Simulate harness flow: push assistant first, then tool result
        conv.push_assistant("", Some(vec![call.clone()]), None);
        conv.push_tool_result(&call, &result, None);
        assert_eq!(conv.messages().len(), 2); // assistant with tool_calls + tool result
        assert!(conv.messages()[0].tool_calls.is_some());
        assert_eq!(conv.messages()[0].role, Role::Assistant);
        assert_eq!(conv.messages()[1].role, Role::Tool);
    }

    #[test]
    fn test_build_context() {
        let mut conv = Conversation::new("system".to_string(), None);
        conv.push_user("hi");
        conv.push_assistant("hello", None, None);
        let ctx = conv.build_context("do stuff", None);
        // system + 2 history + task
        assert_eq!(ctx.len(), 4);
        assert_eq!(ctx[0].role, Role::System);
        assert_eq!(ctx[1].role, Role::User);
        assert_eq!(ctx[2].role, Role::Assistant);
        assert_eq!(ctx[3].role, Role::User);
    }

    #[test]
    fn test_build_context_empty_history() {
        let conv = Conversation::new("sys".to_string(), None);
        let ctx = conv.build_context("task", None);
        assert_eq!(ctx.len(), 2); // system + task
    }

    #[test]
    fn test_estimated_tokens() {
        let mut conv = Conversation::new("sys".to_string(), None);
        conv.push_user("hello world");
        let tokens = conv.estimate_tokens();
        assert!(tokens > 0);
    }

    #[test]
    fn test_truncate_to_tokens() {
        let mut conv = Conversation::new("sys".to_string(), None);
        for i in 0..100 {
            conv.push_user(&format!("message {i}"));
            conv.push_assistant(&format!("reply {i}"), None, None);
        }
        let before = conv.messages().len();
        conv.truncate_to_tokens(50);
        let after = conv.messages().len();
        assert!(after < before);
    }

    #[test]
    fn test_preview_message() {
        let msg = Message {
            role: Role::User,
            content: MessageContent::Text("hello world".to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        };
        let preview = Conversation::preview_message(&msg);
        assert!(preview.contains("user"));
        assert!(preview.contains("hello"));
    }

    #[test]
    fn test_truncate_str_short() {
        let result = Conversation::truncate_str("short", 100);
        assert_eq!(result, "short");
    }

    #[test]
    fn test_truncate_str_long() {
        let long = "a".repeat(1000);
        let result = Conversation::truncate_str(&long, 100);
        assert!(result.contains("truncated"));
        assert!(result.len() < long.len());
    }

    #[test]
    fn test_pending_queue() {
        let mut conv = Conversation::new("sys".to_string(), None);
        conv.push_user("hello");
        conv.push_assistant("hi", None, None);
        // Pending should have 2 entries
        let pending = conv.drain_pending();
        assert_eq!(pending.len(), 2);
        // After drain, dirty should be 0
        assert_eq!(conv.dirty.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_compress_range() {
        let mut conv = Conversation::new("sys".to_string(), None);
        conv.push_user("a");
        conv.push_assistant("b", None, None);
        conv.push_user("c");
        conv.push_assistant("d", None, None);
        conv.push_user("e");
        conv.push_assistant("f", None, None);
        let before = conv.messages().len();
        conv.compress_range(3, "summary".to_string());
        // Should have: system + summary + messages[3..]
        assert!(conv.messages().len() < before);
        assert!(matches!(&conv.messages()[1].content, MessageContent::Text(s) if s == "summary"));
    }

    #[test]
    fn test_build_context_with_preview() {
        let mut conv = Conversation::new("system prompt".to_string(), None);
        for i in 0..20 {
            conv.push_user(&format!(
                "user message {i} with some extra padding to increase length"
            ));
            conv.push_assistant(
                &format!("assistant reply {i} with some extra padding too"),
                None,
                None,
            );
        }
        let ctx = conv.build_context_with_preview("task", None, 200);
        // Should be truncated since total chars far exceed 200
        assert!(ctx.len() < 42); // 20 messages * 2 + system + task = 42, should be less
        let has_omission = ctx.iter().any(|m| match &m.content {
            MessageContent::Text(s) => s.contains("omitted for brevity"),
            _ => false,
        });
        assert!(has_omission, "should contain omission message");
    }

    #[test]
    fn test_mark_file_seen_and_changed() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "original").unwrap();

        let mut conv = Conversation::new("sys".to_string(), None);
        conv.mark_file_seen(dir.path(), "test.txt");

        // Sleep to ensure filesystem timestamp advances (mtime granularity)
        std::thread::sleep(std::time::Duration::from_millis(1500));

        // Modify the file
        std::fs::write(&file_path, "modified content").unwrap();

        let changed = conv.changed_seen_files(dir.path());
        assert!(changed.contains(&"test.txt".to_string()));
    }

    #[test]
    fn test_reset_messages_clears_pending() {
        let mut conv = Conversation::new("sys".to_string(), None);
        conv.push_user("hello");
        conv.push_assistant("hi", None, None);
        conv.push_user("again");
        assert_eq!(conv.messages().len(), 3);

        conv.reset_messages(vec![]);
        assert!(conv.messages().is_empty());
        assert_eq!(conv.dirty.load(Ordering::Relaxed), 0);
        let pending = conv.drain_pending();
        assert!(pending.is_empty());
    }

    #[test]
    fn test_estimate_tokens_with_reasoning() {
        let mut conv = Conversation::new("sys".to_string(), None);
        conv.push_assistant(
            "answer",
            None,
            Some("this is my chain of thought reasoning"),
        );
        let tokens = conv.estimate_tokens();
        // "answer" = 6 chars, reasoning = 39 chars, total = 45 / 4 = 11
        assert!(tokens > 0, "tokens should include reasoning content");
        // With only "answer" (6 chars / 4 = 1), tokens would be 1.
        // With reasoning (45 chars / 4 = 11), tokens should be > 1.
        assert!(tokens > 1, "reasoning should increase token count");
    }

    #[test]
    fn test_estimate_tokens_with_tool_calls() {
        let mut conv = Conversation::new("sys".to_string(), None);
        let call = make_tool_call("1", "read_file", "{\"path\":\"src/main.rs\"}");
        conv.push_assistant("", Some(vec![call]), None);
        let tokens = conv.estimate_tokens();
        // function name "read_file" = 9, arguments = 24, total = 33 / 4 = 8
        assert!(tokens > 0, "tokens should include tool call content");
    }

    #[test]
    fn test_zstd_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let jsonl_path = dir.path().join("conversation.jsonl");

        let mut conv = Conversation::new("sys".to_string(), Some(jsonl_path.clone()));
        conv.push_user("hello");
        conv.push_assistant("world", None, None);
        conv.push_user("how are you?");
        conv.push_assistant("fine", None, None);

        // Manually trigger a full rewrite so data is written to disk
        conv.full_rewrite();

        // Load back and verify round-trip
        let loaded = Conversation::load_jsonl(&jsonl_path).expect("should load messages");
        assert_eq!(loaded.len(), 4);
        assert!(matches!(&loaded[0].content, MessageContent::Text(s) if s == "hello"));
        assert!(matches!(&loaded[1].content, MessageContent::Text(s) if s == "world"));
        assert!(matches!(&loaded[2].content, MessageContent::Text(s) if s == "how are you?"));
        assert!(matches!(&loaded[3].content, MessageContent::Text(s) if s == "fine"));
    }

    #[test]
    fn test_pending_queue_multiple_pushes() {
        let mut conv = Conversation::new("sys".to_string(), None);
        for i in 0..10 {
            conv.push_user(&format!("msg {i}"));
        }
        assert_eq!(conv.dirty.load(Ordering::Relaxed), 10);
        let drained = conv.drain_pending();
        assert_eq!(drained.len(), 10);
        assert_eq!(conv.dirty.load(Ordering::Relaxed), 0);
    }
}
