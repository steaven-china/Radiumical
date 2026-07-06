//! Session management — stored in `~/.radi/sessions/{workspace_hash}/` as semantic JSONL.
//!
//! Each workspace gets its own session directory, so sessions from different
//! projects never collide.  Each line is a typed record: meta / user / assistant
//! / reasoning / tool / raw.
use crate::types::{AgentMode, Message, MessageContent, Role, ToolCall, ToolResult};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
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
pub fn workspace_hash(workspace: &str) -> String {
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
// Workspace Registry — maps human-readable names to session directories.
//
// Stored at `~/.radi/workspaces.json`.  Each workspace has a name, path,
// hash, optional tags, and an optional per-workspace config override
// (`workspace.toml` inside the session directory).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEntry {
    pub name: String,
    pub path: String,
    pub hash: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub last_active: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm_timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_compress_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_continue: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_resume_last_task: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceRegistry {
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub workspaces: Vec<WorkspaceEntry>,
}

fn registry_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".radi")
        .join("workspaces.json")
}

fn sessions_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".radi")
        .join("sessions")
}

impl WorkspaceRegistry {
    pub fn load() -> Self {
        let path = registry_path();
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = registry_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(&path, json)?;
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&WorkspaceEntry> {
        self.workspaces.iter().find(|w| w.name == name)
    }

    pub fn get_by_hash(&self, hash: &str) -> Option<&WorkspaceEntry> {
        self.workspaces.iter().find(|w| w.hash == hash)
    }

    pub fn active_entry(&self) -> Option<&WorkspaceEntry> {
        self.active.as_deref().and_then(|name| self.get(name))
    }

    /// Register a new workspace. If name is None, derive from path.
    pub fn register(&mut self, path: &str, name: Option<&str>) -> Result<String> {
        let abs = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string());
        let hash = workspace_hash(&abs);

        // Already registered?
        if let Some(existing) = self.get_by_hash(&hash) {
            return Ok(existing.name.clone());
        }

        let ws_name = name.map(|s| s.to_string()).unwrap_or_else(|| {
            std::path::Path::new(&abs)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("ws-{}", &hash[..8]))
        });

        let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        self.workspaces.push(WorkspaceEntry {
            name: ws_name.clone(),
            path: abs,
            hash,
            tags: Vec::new(),
            pinned: false,
            last_active: now,
        });
        self.save()?;
        Ok(ws_name)
    }

    /// Switch the active workspace by name.
    pub fn switch(&mut self, name: &str) -> Result<()> {
        if self.get(name).is_none() {
            anyhow::bail!("Workspace '{name}' not found");
        }
        self.active = Some(name.to_string());
        // Update last_active
        let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        if let Some(ws) = self.workspaces.iter_mut().find(|w| w.name == name) {
            ws.last_active = now;
        }
        self.save()
    }

    pub fn remove(&mut self, name: &str) -> Result<bool> {
        let before = self.workspaces.len();
        self.workspaces.retain(|w| w.name != name);
        if self.workspaces.len() < before {
            if self.active.as_deref() == Some(name) {
                self.active = None;
            }
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn add_tag(&mut self, name: &str, tag: &str) -> Result<()> {
        let ws = self
            .workspaces
            .iter_mut()
            .find(|w| w.name == name)
            .ok_or_else(|| anyhow::anyhow!("Workspace '{name}' not found"))?;
        if !ws.tags.iter().any(|t| t == tag) {
            ws.tags.push(tag.to_string());
        }
        self.save()
    }

    pub fn remove_tag(&mut self, name: &str, tag: &str) -> Result<()> {
        let ws = self
            .workspaces
            .iter_mut()
            .find(|w| w.name == name)
            .ok_or_else(|| anyhow::anyhow!("Workspace '{name}' not found"))?;
        ws.tags.retain(|t| t != tag);
        self.save()
    }

    pub fn set_pinned(&mut self, name: &str, pinned: bool) -> Result<()> {
        let ws = self
            .workspaces
            .iter_mut()
            .find(|w| w.name == name)
            .ok_or_else(|| anyhow::anyhow!("Workspace '{name}' not found"))?;
        ws.pinned = pinned;
        self.save()
    }

    /// Auto-discover unregistered session directories and register them.
    pub fn discover(&mut self) {
        let dir = sessions_dir();
        if !dir.exists() {
            return;
        }
        let known_hashes: std::collections::HashSet<String> =
            self.workspaces.iter().map(|w| w.hash.clone()).collect();

        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let hash = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if known_hashes.contains(&hash) {
                    continue;
                }

                // Try to infer name from the latest session meta
                let name = infer_name_from_sessions(&path, &hash);

                let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
                self.workspaces.push(WorkspaceEntry {
                    name,
                    path: String::new(), // unknown for auto-discovered
                    hash,
                    tags: Vec::new(),
                    pinned: false,
                    last_active: now,
                });
            }
        }
        let _ = self.save();
    }
}

/// Try to derive a human-readable name from session files in a directory.
fn infer_name_from_sessions(dir: &Path, hash: &str) -> String {
    // Look for the newest .jsonl and read its meta
    let mut newest: Option<(std::time::SystemTime, String)> = None;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "jsonl") {
                if let Ok(meta) = fs::metadata(&p) {
                    if let Ok(modified) = meta.modified() {
                        if newest.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
                            newest = Some((modified, p.to_string_lossy().to_string()));
                        }
                    }
                }
            }
        }
    }

    if let Some((_, path)) = newest {
        if let Ok(data) = fs::read_to_string(&path) {
            if let Some(first) = data.lines().next() {
                if let Ok(SessionItem::Meta { name, .. }) = serde_json::from_str(first) {
                    if !name.is_empty() && !name.starts_with("auto-") {
                        return name;
                    }
                }
            }
        }
    }

    format!("ws-{}", &hash[..8.min(hash.len())])
}

/// Load workspace-level settings from `workspace.toml` inside the session dir.
pub fn load_workspace_settings(hash: &str) -> WorkspaceSettings {
    let path = sessions_dir().join(hash).join("workspace.toml");
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

/// Save workspace-level settings to `workspace.toml` inside the session dir.
pub fn save_workspace_settings(hash: &str, settings: &WorkspaceSettings) -> Result<()> {
    let dir = sessions_dir().join(hash);
    fs::create_dir_all(&dir)?;
    let path = dir.join("workspace.toml");
    let data = toml::to_string_pretty(settings)?;
    fs::write(&path, data)?;
    Ok(())
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

/// Session filtering, sorting, and retrieval utilities backed by a [`SessionPool`].
pub struct SessionTools {
    pool: SessionPool,
}

/// Strategy for picking which session to load.
pub enum UsageChoices {
    /// Most recently updated session.
    Newest,
    /// Least recently updated session.
    Oldest,
    /// Load a specific session by name.
    Customize(String),
}

/// Content-level metrics computed from a session file.
///
/// Each field is populated by scanning every [`SessionItem`] in the file
/// (excluding the leading [`SessionItem::Meta`]).
#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    /// Number of [`SessionItem::User`] records.
    pub user_messages: usize,
    /// Number of [`SessionItem::Assistant`] records.
    pub assistant_messages: usize,
    /// Number of [`SessionItem::Tool`] records that represent a tool call.
    pub tool_calls: usize,
    /// Number of [`SessionItem::Tool`] records that carry a result.
    pub tool_results: usize,
    /// Sum of `.len()` across all `content` / `args` / `result` strings.
    pub total_content_length: usize,
}

/// Sort key for listing sessions.
pub enum SortBy {
    /// Sort by the `updated` timestamp.
    Updated,
    /// Sort by the `created` timestamp.
    Created,
    /// Sort by session name.
    Name,
    /// Sort by message count (from meta — fast, no file read).
    MessageCount,
    /// Sort by model name.
    Model,
    /// Sort by provider name.
    Provider,
    /// Sort by session mode.
    Mode,
    /// Sort by thinking effort.
    ThinkingEffort,
    /// Sort by description text.
    Description,
    // ── content-based sorts (read every session file) ──
    /// Sort by number of user messages (requires reading the file).
    UserMessages,
    /// Sort by number of assistant messages (requires reading the file).
    AssistantMessages,
    /// Sort by number of tool calls issued (requires reading the file).
    ToolCalls,
    /// Sort by number of tool results received (requires reading the file).
    ToolResults,
    /// Sort by total character count of all content (requires reading the file).
    TotalContentLength,
}

/// Sort direction.
pub enum SortOrder {
    /// Ascending order.
    Asc,
    /// Descending order.
    Desc,
}

/// Composable session filter.  All fields default to `None` (no-op).
pub struct SessionFilter {
    /// Keep sessions whose name contains this substring (case-insensitive).
    pub name_contains: Option<String>,
    /// Keep sessions that match this model exactly.
    pub model: Option<String>,
    /// Keep sessions that match this mode exactly.
    pub mode: Option<SessionMode>,
    /// Keep sessions that match this provider exactly.
    pub provider: Option<String>,
}

impl SessionTools {
    /// Build from an existing [`SessionPool`].
    pub fn new(pool: SessionPool) -> Self {
        Self { pool }
    }

    /// Convenience constructor that delegates to [`SessionPool::for_workspace`].
    pub fn for_workspace(workspace: &str) -> Self {
        Self {
            pool: SessionPool::for_workspace(workspace),
        }
    }

    /// List all sessions with optional combined filtering and sorting.
    ///
    /// When `filter` is `None` all sessions are returned; otherwise non-`None`
    /// fields are AND-ed together.  Results are sorted in ascending order by the
    /// chosen key, then reversed when `order` is [`SortOrder::Desc`].
    pub fn list_filtered(
        &self,
        filter: Option<&SessionFilter>,
        sort_by: SortBy,
        order: SortOrder,
    ) -> Result<Vec<SessionMeta>> {
        let mut sessions = self.pool.list()?;

        if let Some(f) = filter {
            sessions.retain(|s| {
                f.name_contains
                    .as_ref()
                    .map_or(true, |n| s.name.to_lowercase().contains(&n.to_lowercase()))
                    && f.model.as_ref().map_or(true, |m| s.model == *m)
                    && f.mode.map_or(true, |m| s.mode == m)
                    && f.provider.as_ref().map_or(true, |p| s.provider == *p)
            });
        }

        match sort_by {
            SortBy::Updated => sessions.sort_by(|a, b| a.updated.cmp(&b.updated)),
            SortBy::Created => sessions.sort_by(|a, b| a.created.cmp(&b.created)),
            SortBy::Name => sessions.sort_by(|a, b| a.name.cmp(&b.name)),
            SortBy::MessageCount => sessions.sort_by_key(|s| s.message_count),
            SortBy::Model => sessions.sort_by(|a, b| a.model.cmp(&b.model)),
            SortBy::Provider => sessions.sort_by(|a, b| a.provider.cmp(&b.provider)),
            SortBy::Mode => sessions.sort_by_key(|s| s.mode),
            SortBy::ThinkingEffort => {
                sessions.sort_by(|a, b| a.thinking_effort.cmp(&b.thinking_effort))
            }
            SortBy::Description => sessions.sort_by(|a, b| a.description.cmp(&b.description)),
            // Content-based sorts: load each session file and compute stats.
            SortBy::UserMessages => {
                let stats: Vec<usize> = sessions
                    .iter()
                    .map(|m| self.content_stats(&m.name).map_or(0, |s| s.user_messages))
                    .collect();
                let mut indexed: Vec<_> = sessions.into_iter().zip(stats).collect();
                indexed.sort_by_key(|(_, count)| *count);
                sessions = indexed.into_iter().map(|(m, _)| m).collect();
            }
            SortBy::AssistantMessages => {
                let stats: Vec<usize> = sessions
                    .iter()
                    .map(|m| {
                        self.content_stats(&m.name)
                            .map_or(0, |s| s.assistant_messages)
                    })
                    .collect();
                let mut indexed: Vec<_> = sessions.into_iter().zip(stats).collect();
                indexed.sort_by_key(|(_, count)| *count);
                sessions = indexed.into_iter().map(|(m, _)| m).collect();
            }
            SortBy::ToolCalls => {
                let stats: Vec<usize> = sessions
                    .iter()
                    .map(|m| self.content_stats(&m.name).map_or(0, |s| s.tool_calls))
                    .collect();
                let mut indexed: Vec<_> = sessions.into_iter().zip(stats).collect();
                indexed.sort_by_key(|(_, count)| *count);
                sessions = indexed.into_iter().map(|(m, _)| m).collect();
            }
            SortBy::ToolResults => {
                let stats: Vec<usize> = sessions
                    .iter()
                    .map(|m| self.content_stats(&m.name).map_or(0, |s| s.tool_results))
                    .collect();
                let mut indexed: Vec<_> = sessions.into_iter().zip(stats).collect();
                indexed.sort_by_key(|(_, count)| *count);
                sessions = indexed.into_iter().map(|(m, _)| m).collect();
            }
            SortBy::TotalContentLength => {
                let stats: Vec<usize> = sessions
                    .iter()
                    .map(|m| {
                        self.content_stats(&m.name)
                            .map_or(0, |s| s.total_content_length)
                    })
                    .collect();
                let mut indexed: Vec<_> = sessions.into_iter().zip(stats).collect();
                indexed.sort_by_key(|(_, count)| *count);
                sessions = indexed.into_iter().map(|(m, _)| m).collect();
            }
        }

        if matches!(order, SortOrder::Desc) {
            sessions.reverse();
        }

        Ok(sessions)
    }

    /// Read a session file and compute content-level [`SessionStats`].
    ///
    /// This is intentionally separate from [`list_filtered`] so callers can
    /// inspect stats without re-scanning the file.  Returns `None` when the
    /// session file cannot be read or parsed.
    pub fn content_stats(&self, name: &str) -> Option<SessionStats> {
        let path = self.pool.dir().join(format!("{}.jsonl", hash_name(name)));
        let data = fs::read_to_string(&path).ok()?;
        let mut stats = SessionStats::default();
        for line in data.lines().skip(1) {
            let item: SessionItem = serde_json::from_str(line).ok()?;
            match item {
                SessionItem::User { ref content } => {
                    stats.user_messages += 1;
                    stats.total_content_length += content.len();
                }
                SessionItem::Assistant { ref content } => {
                    stats.assistant_messages += 1;
                    stats.total_content_length += content.len();
                }
                SessionItem::Reasoning { ref content } => {
                    stats.total_content_length += content.len();
                }
                SessionItem::Tool {
                    ref args,
                    ref result,
                    ..
                } => {
                    stats.tool_calls += 1;
                    stats.total_content_length += args.len();
                    if result.is_some() {
                        stats.tool_results += 1;
                        stats.total_content_length += result.as_ref().unwrap().len();
                    }
                }
                SessionItem::Meta { .. } | SessionItem::Raw { .. } => {}
            }
        }
        Some(stats)
    }

    /// Load a single session's full data according to the given [`UsageChoices`].
    ///
    /// - [`UsageChoices::Newest`] / [`UsageChoices::Oldest`]: picks the most /
    ///   least recently updated session.
    /// - [`UsageChoices::Customize`]: loads the named session directly.
    ///
    /// Returns `Ok(None)` when no sessions exist.
    pub fn get_session(
        &self,
        choice: UsageChoices,
    ) -> Result<Option<(SessionMeta, Vec<SessionItem>)>> {
        match choice {
            UsageChoices::Newest | UsageChoices::Oldest => {
                let mut list = self.pool.list()?;
                list.sort_by(|a, b| a.updated.cmp(&b.updated));
                if matches!(choice, UsageChoices::Oldest) {
                    list.reverse();
                }
                if let Some(first) = list.first() {
                    self.pool.load(&first.name)
                } else {
                    Ok(None)
                }
            }
            UsageChoices::Customize(name) => self.pool.load(&name),
        }
    }
}

// ---------------------------------------------------------------------------
// WorkspaceTools — workspace listing, filtering, and sorting
// ---------------------------------------------------------------------------

/// Sort key for listing workspaces.
pub enum WorkspaceSortBy {
    /// Sort by workspace name.
    Name,
    /// Sort by workspace path.
    Path,
    /// Sort by last-active timestamp.
    LastActive,
    /// Sort by pinned status (pinned first when descending).
    Pinned,
    /// Sort by number of tags.
    TagCount,
    /// Sort by number of sessions in the workspace.
    SessionCount,
}

/// Composable workspace filter.  All fields default to `None` (no-op).
pub struct WorkspaceFilter {
    /// Keep workspaces whose name contains this substring (case-insensitive).
    pub name_contains: Option<String>,
    /// Keep workspaces whose path contains this substring (case-insensitive).
    pub path_contains: Option<String>,
    /// Keep only pinned / unpinned workspaces.
    pub pinned: Option<bool>,
    /// Keep workspaces that have this tag.
    pub has_tag: Option<String>,
}

/// Workspace listing, filtering, and sorting utilities.
///
/// Operates on a shared [`WorkspaceRegistry`] and the sessions directory to
/// compute per-workspace session counts.
pub struct WorkspaceTools;

impl WorkspaceTools {
    /// List workspaces with optional combined filtering and sorting.
    ///
    /// Session counts are computed by reading each workspace's session
    /// directory.  Workspaces with no session directory get a count of 0.
    ///
    /// When `filter` is `None` all workspaces are returned; otherwise non-`None`
    /// fields are AND-ed together.  Results are sorted in ascending order by the
    /// chosen key, then reversed when `order` is [`SortOrder::Desc`].
    pub fn list_filtered(
        registry: &WorkspaceRegistry,
        filter: Option<&WorkspaceFilter>,
        sort_by: WorkspaceSortBy,
        order: SortOrder,
    ) -> Vec<WorkspaceEntry> {
        let mut entries: Vec<WorkspaceEntry> = registry.workspaces.clone();

        if let Some(f) = filter {
            entries.retain(|w| {
                f.name_contains
                    .as_ref()
                    .map_or(true, |n| w.name.to_lowercase().contains(&n.to_lowercase()))
                    && f.path_contains
                        .as_ref()
                        .map_or(true, |p| w.path.to_lowercase().contains(&p.to_lowercase()))
                    && f.pinned.map_or(true, |p| w.pinned == p)
                    && f.has_tag
                        .as_ref()
                        .map_or(true, |t| w.tags.iter().any(|tag| tag == t))
            });
        }

        match sort_by {
            WorkspaceSortBy::Name => entries.sort_by(|a, b| a.name.cmp(&b.name)),
            WorkspaceSortBy::Path => entries.sort_by(|a, b| a.path.cmp(&b.path)),
            WorkspaceSortBy::LastActive => {
                entries.sort_by(|a, b| a.last_active.cmp(&b.last_active))
            }
            WorkspaceSortBy::Pinned => entries.sort_by_key(|w| w.pinned),
            WorkspaceSortBy::TagCount => entries.sort_by_key(|w| w.tags.len()),
            WorkspaceSortBy::SessionCount => entries.sort_by_key(|w| Self::count_sessions(&w.hash)),
        }

        if matches!(order, SortOrder::Desc) {
            entries.reverse();
        }

        entries
    }

    /// Count the number of `.jsonl` session files in a workspace's directory.
    pub fn count_sessions(hash: &str) -> usize {
        let dir = sessions_dir().join(hash);
        if !dir.exists() {
            return 0;
        }
        fs::read_dir(&dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.path().extension().is_some_and(|ext| ext == "jsonl"))
                    .count()
            })
            .unwrap_or(0)
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
        assert!(
            matches!(&loaded_items[1], SessionItem::Assistant { content } if content == "world")
        );

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
