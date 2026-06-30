//! Session management — stored in `~/.radi/sessions/{workspace_hash}/` as semantic JSONL.
//!
//! Each workspace gets its own session directory, so sessions from different
//! projects never collide.  Each line is a typed record: meta / user / assistant
//! / reasoning / tool / raw.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::types::{AgentMode, Message, MessageContent, Role, ToolCall, ToolResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub name: String,
    pub created: String,
    pub updated: String,
    pub model: String,
    pub provider: String,
    #[serde(default)]
    pub mode: SessionMode,
    #[serde(default)]
    pub thinking_effort: String,
    pub description: String,
    pub message_count: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SessionMode {
    #[default]
    Auto,
    Plan,
    Exec,
}

impl From<AgentMode> for SessionMode {
    fn from(m: AgentMode) -> Self {
        match m {
            AgentMode::Auto => SessionMode::Auto,
            AgentMode::Plan => SessionMode::Plan,
            AgentMode::Exec => SessionMode::Exec,
        }
    }
}

impl From<SessionMode> for AgentMode {
    fn from(m: SessionMode) -> Self {
        match m {
            SessionMode::Auto => AgentMode::Auto,
            SessionMode::Plan => AgentMode::Plan,
            SessionMode::Exec => AgentMode::Exec,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SessionItem {
    #[serde(rename = "meta")]
    Meta {
        name: String,
        created: String,
        updated: String,
        model: String,
        #[serde(default)]
        provider: String,
        #[serde(default)]
        mode: SessionMode,
        #[serde(default)]
        thinking_effort: String,
        description: String,
        message_count: usize,
    },
    #[serde(rename = "user")]
    User { content: String },
    #[serde(rename = "assistant")]
    Assistant { content: String },
    #[serde(rename = "reasoning")]
    Reasoning { content: String },
    #[serde(rename = "tool")]
    Tool {
        id: String,
        name: String,
        args: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<String>,
    },
    #[serde(rename = "raw")]
    Raw { lines: Vec<String> },
}

impl SessionItem {
    pub fn as_user(&self) -> Option<&str> {
        match self {
            SessionItem::User { content } => Some(content),
            _ => None,
        }
    }
}

/// Rebuild LLM conversation messages from saved session items.
///
/// - User items become user messages.
/// - Assistant items become assistant messages.
/// - Reasoning items are attached as `reasoning_content` to the preceding
///   assistant message when possible.
/// - Tool items become assistant `tool_calls` followed by tool result messages.
/// - Raw/error lines are dropped from the conversation history.
pub fn items_to_messages(items: &[SessionItem]) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();
    let mut pending_reasoning: Option<String> = None;

    for item in items {
        match item {
            SessionItem::Meta { .. } => {}
            SessionItem::User { content } => {
                out.push(Message {
                    role: Role::User,
                    content: MessageContent::Text(content.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                    reasoning_content: None,
                });
            }
            SessionItem::Assistant { content } => {
                out.push(Message {
                    role: Role::Assistant,
                    content: MessageContent::Text(content.clone()),
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                    reasoning_content: pending_reasoning.take(),
                });
            }
            SessionItem::Reasoning { content } => {
                if let Some(existing) = &mut pending_reasoning {
                    existing.push_str(content);
                } else {
                    pending_reasoning = Some(content.clone());
                }
            }
            SessionItem::Tool {
                id,
                name,
                args,
                result,
            } => {
                // Merge the tool call into the most recent assistant message
                // when possible, matching the natural LLM response structure.
                match out.last_mut() {
                    Some(Message {
                        role: Role::Assistant,
                        tool_calls: Some(calls),
                        ..
                    }) => {
                        calls.push(ToolCall {
                            id: id.clone(),
                            call_type: "function".into(),
                            function: crate::types::FunctionCall {
                                name: name.clone(),
                                arguments: args.clone(),
                            },
                        });
                    }
                    Some(Message {
                        role: Role::Assistant,
                        tool_calls: None,
                        ..
                    }) => {
                        if let Some(last) = out.last_mut() {
                            last.tool_calls = Some(vec![ToolCall {
                                id: id.clone(),
                                call_type: "function".into(),
                                function: crate::types::FunctionCall {
                                    name: name.clone(),
                                    arguments: args.clone(),
                                },
                            }]);
                        }
                    }
                    _ => {
                        out.push(Message {
                            role: Role::Assistant,
                            content: MessageContent::Text(String::new()),
                            tool_calls: Some(vec![ToolCall {
                                id: id.clone(),
                                call_type: "function".into(),
                                function: crate::types::FunctionCall {
                                    name: name.clone(),
                                    arguments: args.clone(),
                                },
                            }]),
                            tool_call_id: None,
                            name: None,
                            reasoning_content: pending_reasoning.take(),
                        });
                    }
                }

                if let Some(result) = result {
                    out.push(Message {
                        role: Role::Tool,
                        content: MessageContent::Text(result.clone()),
                        tool_calls: None,
                        tool_call_id: Some(id.clone()),
                        name: Some(name.clone()),
                        reasoning_content: None,
                    });
                }
            }
            SessionItem::Raw { .. } => {}
        }
    }

    if let Some(reasoning) = pending_reasoning.take() {
        if let Some(last) = out.last_mut() {
            if last.role == Role::Assistant && last.reasoning_content.is_none() {
                last.reasoning_content = Some(reasoning);
            }
        }
    }

    out
}

/// Convert a tool result message back to the short `ToolResult` shape used by
/// the conversation layer.
#[allow(dead_code)]
pub fn tool_result_from_message(msg: &Message) -> Option<ToolResult> {
    if msg.role != Role::Tool {
        return None;
    }
    let content = match &msg.content {
        MessageContent::Text(s) => s.clone(),
        _ => String::new(),
    };
    Some(ToolResult {
        tool_call_id: msg.tool_call_id.clone().unwrap_or_default(),
        content,
        is_error: false,
    })
}

fn hash_name(name: &str) -> String {
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    format!("{:x}", h.finish())
}

/// Derive a short, stable hash from a workspace path for directory naming.
fn workspace_hash(workspace: &str) -> String {
    let canonical = std::fs::canonicalize(workspace)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| workspace.to_string());
    let mut h = DefaultHasher::new();
    canonical.to_lowercase().hash(&mut h);
    format!("{:x}", h.finish())
}

// ---------------------------------------------------------------------------
// Core session I/O — all operations are parameterized by `dir`.
// ---------------------------------------------------------------------------

fn list_dir(dir: &Path) -> Result<Vec<SessionMeta>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut metas = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "jsonl") {
            if let Ok(data) = fs::read_to_string(&path) {
                if let Some(first) = data.lines().next() {
                    if let Ok(SessionItem::Meta {
                        name,
                        created,
                        updated,
                        model,
                        provider,
                        mode,
                        thinking_effort,
                        description,
                        message_count,
                    }) = serde_json::from_str(first)
                    {
                        metas.push(SessionMeta {
                            name,
                            created,
                            updated,
                            model,
                            provider,
                            mode,
                            thinking_effort,
                            description,
                            message_count,
                        });
                    }
                }
            }
        }
    }
    metas.sort_by(|a, b| b.updated.cmp(&a.updated));
    Ok(metas)
}

#[allow(clippy::too_many_arguments)]
fn save_dir(
    dir: &Path,
    name: &str,
    items: &[SessionItem],
    model: &str,
    provider: &str,
    mode: SessionMode,
    thinking_effort: &str,
    description: Option<&str>,
) -> Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.jsonl", hash_name(name)));
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    let description = description.unwrap_or("").to_string();
    let message_count = items
        .iter()
        .filter(|i| !matches!(i, SessionItem::Meta { .. }))
        .count();

    let mut records: Vec<SessionItem> = vec![SessionItem::Meta {
        name: name.to_string(),
        created: now.clone(),
        updated: now,
        model: model.to_string(),
        provider: provider.to_string(),
        mode,
        thinking_effort: thinking_effort.to_string(),
        description,
        message_count,
    }];
    records.extend_from_slice(items);

    let lines: Vec<String> = records
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()?;
    fs::write(&path, lines.join("\n"))?;
    Ok(())
}

fn load_dir(dir: &Path, name: &str) -> Result<Option<(SessionMeta, Vec<SessionItem>)>> {
    let path = dir.join(format!("{}.jsonl", hash_name(name)));
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read_to_string(&path)?;
    let mut lines = data.lines();
    let first = lines.next().context("session file is empty")?.to_string();
    let meta = match serde_json::from_str::<SessionItem>(&first)? {
        SessionItem::Meta {
            name,
            created,
            updated,
            model,
            provider,
            mode,
            thinking_effort,
            description,
            message_count,
        } => SessionMeta {
            name,
            created,
            updated,
            model,
            provider,
            mode,
            thinking_effort,
            description,
            message_count,
        },
        _ => anyhow::bail!("first record is not meta"),
    };
    let mut items = Vec::new();
    for line in lines {
        match serde_json::from_str::<SessionItem>(line)? {
            SessionItem::Meta { .. } => {}
            item => items.push(item),
        }
    }
    Ok(Some((meta, items)))
}

fn delete_dir(dir: &Path, name: &str) -> Result<bool> {
    let path = dir.join(format!("{}.jsonl", hash_name(name)));
    if path.exists() {
        fs::remove_file(&path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// SessionPool — workspace-scoped session manager.
//
// Sessions live under `~/.radi/sessions/{workspace_hash}/` so different
// projects never collide.  The pool provides a unified interface to list,
// load, save, and delete sessions within the current workspace.
// ---------------------------------------------------------------------------

pub struct SessionPool {
    dir: PathBuf,
}

impl SessionPool {
    /// Create a pool for a specific directory.
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
        }
    }

    /// Create a pool scoped to the given workspace path.
    ///
    /// Sessions are stored under `~/.radi/sessions/{hash}/`.
    pub fn for_workspace(workspace: &str) -> Self {
        let dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".radi")
            .join("sessions")
            .join(workspace_hash(workspace));
        Self::new(dir)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn list(&self) -> Result<Vec<SessionMeta>> {
        list_dir(&self.dir)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save(
        &self,
        name: &str,
        items: &[SessionItem],
        model: &str,
        provider: &str,
        mode: SessionMode,
        thinking_effort: &str,
        description: Option<&str>,
    ) -> Result<()> {
        save_dir(
            &self.dir,
            name,
            items,
            model,
            provider,
            mode,
            thinking_effort,
            description,
        )
    }

    pub fn load(&self, name: &str) -> Result<Option<(SessionMeta, Vec<SessionItem>)>> {
        load_dir(&self.dir, name)
    }

    pub fn delete(&self, name: &str) -> Result<bool> {
        delete_dir(&self.dir, name)
    }
}

// ---------------------------------------------------------------------------
// Backward compat: static `Session` methods delegate to the legacy dir.
// New code should use `SessionPool::for_workspace()` instead.
// ---------------------------------------------------------------------------

pub struct Session;

impl Session {
    pub fn dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".radi")
            .join("session")
    }

    pub fn list() -> Result<Vec<SessionMeta>> {
        list_dir(&Self::dir())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save(
        name: &str,
        items: &[SessionItem],
        model: &str,
        provider: &str,
        mode: SessionMode,
        thinking_effort: &str,
        description: Option<&str>,
    ) -> Result<()> {
        save_dir(
            &Self::dir(),
            name,
            items,
            model,
            provider,
            mode,
            thinking_effort,
            description,
        )
    }

    pub fn load(name: &str) -> Result<Option<(SessionMeta, Vec<SessionItem>)>> {
        load_dir(&Self::dir(), name)
    }

    pub fn delete(name: &str) -> Result<bool> {
        delete_dir(&Self::dir(), name)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_name_deterministic() {
        let a = hash_name("test");
        let b = hash_name("test");
        assert_eq!(a, b);
    }

    #[test]
    fn test_hash_name_different() {
        let a = hash_name("hello");
        let b = hash_name("world");
        assert_ne!(a, b);
    }

    #[test]
    fn test_workspace_hash_deterministic() {
        let a = workspace_hash("/home/user/project");
        let b = workspace_hash("/home/user/project");
        assert_eq!(a, b);
    }

    #[test]
    fn test_workspace_hash_different() {
        let a = workspace_hash("/home/user/project-a");
        let b = workspace_hash("/home/user/project-b");
        assert_ne!(a, b);
    }

    #[test]
    fn test_list_empty() {
        let result = Session::list();
        assert!(result.is_ok());
    }

    #[test]
    fn test_save_delete_cycle() {
        let items = vec![
            SessionItem::User {
                content: "hello".into(),
            },
            SessionItem::Assistant {
                content: "hi".into(),
            },
            SessionItem::Tool {
                id: "call_1".into(),
                name: "read_file".into(),
                args: "{\"path\":\"x\"}".into(),
                result: Some("content".into()),
            },
        ];
        let result = Session::save(
            "_test_session",
            &items,
            "test-model",
            "openai",
            SessionMode::Auto,
            "max",
            Some("test desc"),
        );
        assert!(result.is_ok());
        let loaded = Session::load("_test_session").unwrap();
        assert!(loaded.is_some());
        let (meta, loaded_items) = loaded.unwrap();
        assert_eq!(meta.name, "_test_session");
        assert_eq!(meta.model, "test-model");
        assert_eq!(meta.provider, "openai");
        assert_eq!(meta.mode, SessionMode::Auto);
        assert_eq!(meta.thinking_effort, "max");
        assert_eq!(meta.description, "test desc");
        assert_eq!(meta.message_count, 3);
        assert_eq!(loaded_items.len(), 3);
        match &loaded_items[2] {
            SessionItem::Tool { result, .. } => assert_eq!(result.as_deref(), Some("content")),
            _ => panic!("expected tool item"),
        }
        let deleted = Session::delete("_test_session").unwrap();
        assert!(deleted);
        let gone = Session::load("_test_session").unwrap();
        assert!(gone.is_none());
    }

    #[test]
    fn test_pool_for_workspace_isolation() {
        let pool_a = SessionPool::for_workspace("/tmp/workspace-a");
        let pool_b = SessionPool::for_workspace("/tmp/workspace-b");
        assert_ne!(pool_a.dir(), pool_b.dir());
        // Both should be under ~/.radi/sessions/
        assert!(pool_a.dir().to_string_lossy().contains("sessions"));
        assert!(pool_b.dir().to_string_lossy().contains("sessions"));
    }

    #[test]
    fn test_items_to_messages_rebuilds_history() {
        let items = vec![
            SessionItem::User {
                content: "hi".into(),
            },
            SessionItem::Reasoning {
                content: "let me think".into(),
            },
            SessionItem::Assistant {
                content: "hello".into(),
            },
            SessionItem::Tool {
                id: "call_1".into(),
                name: "read_file".into(),
                args: "{\"path\":\"x\"}".into(),
                result: Some("data".into()),
            },
            SessionItem::User {
                content: "thanks".into(),
            },
        ];
        let msgs = items_to_messages(&items);
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[0].role, Role::User);
        assert_eq!(msgs[1].role, Role::Assistant);
        assert_eq!(msgs[1].reasoning_content.as_deref(), Some("let me think"));
        assert!(msgs[1].tool_calls.is_some());
        assert_eq!(msgs[1].tool_calls.as_ref().unwrap().len(), 1);
        assert_eq!(msgs[2].role, Role::Tool);
        assert_eq!(msgs[3].role, Role::User);
    }

    #[test]
    fn test_items_to_messages_skips_meta_and_raw() {
        let items = vec![
            SessionItem::Meta {
                name: "x".into(),
                created: "now".into(),
                updated: "now".into(),
                model: "m".into(),
                provider: "p".into(),
                mode: SessionMode::Auto,
                thinking_effort: "".into(),
                description: "".into(),
                message_count: 0,
            },
            SessionItem::Raw {
                lines: vec!["error".into()],
            },
            SessionItem::User {
                content: "u".into(),
            },
        ];
        let msgs = items_to_messages(&items);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, Role::User);
    }

    #[test]
    fn test_pool_save_load_delete() {
        let dir = tempfile::tempdir().unwrap();
        let pool = SessionPool::new(dir.path());
        let items = vec![
            SessionItem::User {
                content: "hello".into(),
            },
            SessionItem::Assistant {
                content: "world".into(),
            },
        ];
        pool.save(
            "my-session",
            &items,
            "gpt-4",
            "openai",
            SessionMode::Auto,
            "medium",
            Some("desc"),
        )
        .unwrap();

        let (meta, loaded_items) = pool.load("my-session").unwrap().unwrap();
        assert_eq!(meta.name, "my-session");
        assert_eq!(meta.model, "gpt-4");
        assert_eq!(meta.provider, "openai");
        assert_eq!(meta.description, "desc");
        assert_eq!(loaded_items.len(), 2);
        assert!(matches!(&loaded_items[0], SessionItem::User { content } if content == "hello"));
        assert!(matches!(&loaded_items[1], SessionItem::Assistant { content } if content == "world"));

        let deleted = pool.delete("my-session").unwrap();
        assert!(deleted);
        assert!(pool.load("my-session").unwrap().is_none());
    }

    #[test]
    fn test_pool_list_sorted_by_updated() {
        let dir = tempfile::tempdir().unwrap();
        let pool = SessionPool::new(dir.path());

        let names_and_dates = [
            ("alpha", "2025-01-01 10:00"),
            ("beta", "2025-01-02 10:00"),
            ("gamma", "2025-01-03 10:00"),
        ];
        for (name, date) in &names_and_dates {
            let meta = SessionItem::Meta {
                name: name.to_string(),
                created: date.to_string(),
                updated: date.to_string(),
                model: "m".into(),
                provider: "p".into(),
                mode: SessionMode::Auto,
                thinking_effort: "".into(),
                description: "".into(),
                message_count: 0,
            };
            let line = serde_json::to_string(&meta).unwrap();
            let path = dir.path().join(format!("{}.jsonl", hash_name(name)));
            fs::write(&path, line).unwrap();
        }

        let list = pool.list().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "gamma");
        assert_eq!(list[1].name, "beta");
        assert_eq!(list[2].name, "alpha");
    }

    #[test]
    fn test_pool_for_workspace_different_paths() {
        let pool_a = SessionPool::for_workspace("/tmp/ws-x");
        let pool_b = SessionPool::for_workspace("/tmp/ws-y");
        assert_ne!(pool_a.dir(), pool_b.dir());
        assert!(pool_a.dir().to_string_lossy().contains("sessions"));
        assert!(pool_b.dir().to_string_lossy().contains("sessions"));
    }

    #[test]
    fn test_save_overwrites_same_name() {
        let dir = tempfile::tempdir().unwrap();
        let pool = SessionPool::new(dir.path());

        let items1 = vec![SessionItem::User {
            content: "first".into(),
        }];
        pool.save("dup", &items1, "m", "p", SessionMode::Auto, "", None)
            .unwrap();

        let items2 = vec![
            SessionItem::User {
                content: "second".into(),
            },
            SessionItem::Assistant {
                content: "reply".into(),
            },
        ];
        pool.save("dup", &items2, "m", "p", SessionMode::Auto, "", None)
            .unwrap();

        let (_, loaded) = pool.load("dup").unwrap().unwrap();
        assert_eq!(loaded.len(), 2);
        assert!(matches!(&loaded[0], SessionItem::User { content } if content == "second"));
        assert!(matches!(&loaded[1], SessionItem::Assistant { content } if content == "reply"));
    }

    #[test]
    fn test_load_nonexistent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let pool = SessionPool::new(dir.path());
        let result = pool.load("does-not-exist").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_delete_nonexistent_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let pool = SessionPool::new(dir.path());
        let result = pool.delete("does-not-exist").unwrap();
        assert!(!result);
    }

    #[test]
    fn test_message_count_excludes_meta() {
        let dir = tempfile::tempdir().unwrap();
        let pool = SessionPool::new(dir.path());
        let items = vec![
            SessionItem::User {
                content: "q".into(),
            },
            SessionItem::Assistant {
                content: "a".into(),
            },
            SessionItem::Tool {
                id: "t1".into(),
                name: "fn".into(),
                args: "{}".into(),
                result: Some("r".into()),
            },
        ];
        pool.save("cnt", &items, "m", "p", SessionMode::Auto, "", None)
            .unwrap();
        let (meta, _) = pool.load("cnt").unwrap().unwrap();
        assert_eq!(meta.message_count, 3);
    }
}
