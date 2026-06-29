use std::borrow::Cow;
use serde::{Deserialize, Serialize};

use crate::providers::ProviderSource;
use crate::session::SessionItem;

// ── Chat types ──

/// Threshold for lz4 transparent compression (1 KB).
const COMPRESS_THRESHOLD: usize = 1024;

/// Magic prefix indicating lz4-compressed text content.
const LZ4_PREFIX: &str = "\x00lz4:";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl MessageContent {
    pub fn text(&self) -> Cow<'_, str> {
        match self {
            MessageContent::Text(s) => {
                if s.starts_with(LZ4_PREFIX) {
                    match decompress_text(s) {
                        Some(decompressed) => Cow::Owned(decompressed),
                        None => Cow::Borrowed(""),
                    }
                } else {
                    Cow::Borrowed(s.as_str())
                }
            }
            MessageContent::Parts(_) => Cow::Borrowed(""),
        }
    }

    /// Get raw text without decompression (for callers that handle compression themselves).
    pub fn raw_str(&self) -> &str {
        match self {
            MessageContent::Text(s) => s.as_str(),
            MessageContent::Parts(_) => "",
        }
    }

    /// Create Text, compressing with lz4 if > 1KB.
    pub fn from_text(text: String) -> Self {
        if text.len() > COMPRESS_THRESHOLD {
            if let Some(compressed) = compress_text(&text) {
                return MessageContent::Text(compressed);
            }
        }
        MessageContent::Text(text)
    }

    /// Return raw text without decompression (for serialization).
    pub fn raw_text(&self) -> &str {
        match self {
            MessageContent::Text(s) => s.as_str(),
            MessageContent::Parts(_) => "",
        }
    }

    /// Return true if this content is lz4-compressed.
    pub fn is_compressed(&self) -> bool {
        matches!(self, MessageContent::Text(s) if s.starts_with(LZ4_PREFIX))
    }
}

/// Compress text with lz4, returning prefixed string. Returns None on failure.
pub fn compress_text(text: &str) -> Option<String> {
    let compressed = lz4_flex::compress_prepend_size(text.as_bytes());
    let encoded = base64_encode(&compressed);
    Some(format!("{LZ4_PREFIX}{encoded}"))
}

/// Decompress lz4-prefixed text. Returns None on failure.
pub fn decompress_text(s: &str) -> Option<String> {
    let encoded = s.strip_prefix(LZ4_PREFIX)?;
    let compressed = base64_decode(encoded)?;
    let bytes = lz4_flex::decompress_size_prepended(&compressed).ok()?;
    String::from_utf8(bytes).ok()
}

/// Minimal base64 encode (no external dep needed for this use case).
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() * 4 / 3) + 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 {
            return None;
        }
        let a = val(chunk[0])? as u32;
        let b = val(chunk[1])? as u32;
        let c = if chunk.len() > 2 && chunk[2] != b'=' {
            val(chunk[2])? as u32
        } else {
            0
        };
        let d = if chunk.len() > 3 && chunk[3] != b'=' {
            val(chunk[3])? as u32
        } else {
            0
        };
        let triple = (a << 18) | (b << 12) | (c << 6) | d;
        out.push((triple >> 16) as u8);
        if chunk.len() > 2 && chunk[2] != b'=' {
            out.push((triple >> 8) as u8);
        }
        if chunk.len() > 3 && chunk[3] != b'=' {
            out.push(triple as u8);
        }
    }
    Some(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
}

// ── Tool types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
    pub is_error: bool,
}

// ── Provider response ──

#[derive(Debug, Clone)]
pub enum ProviderEvent {
    Text(String),
    Reasoning(String),
    ToolCalls(Vec<ToolCall>),
    Done,
    #[allow(dead_code)]
    Error(String),
}

// ── Session config ──

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub provider: ProviderKind,
    pub model: String,
    pub api_key: String,
    pub api_base: Option<String>,
    pub max_iterations: usize,
    pub system_prompt: String,
    /// Timeout per LLM request (seconds)
    pub llm_timeout_secs: u64,
    /// Timeout per tool execution (seconds)
    pub tool_timeout_secs: u64,
    /// Heartbeat interval during long ops (seconds, 0 = disabled)
    pub heartbeat_interval_secs: u64,
    /// Max parallel tool executions
    #[allow(dead_code)]
    pub concurrency: usize,
    /// Render markdown in terminal output
    #[allow(dead_code)]
    pub use_markdown: bool,
    /// Agent operating mode
    pub mode: AgentMode,
    /// Max context tokens before compression kicks in (default 120k)
    pub max_context_tokens: usize,
    /// Compress when context exceeds this ratio of max_context_tokens (default 0.8)
    pub context_compress_ratio: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentMode {
    /// All tools available (default)
    Auto,
    /// Read-only: only read_file, search_code, find_files
    Plan,
    /// Write-enabled: all tools including edit_file, write_file, run_command
    Exec,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderKind {
    OpenAI,
    Anthropic,
    Ollama,
}

impl ProviderKind {
    pub fn name(&self) -> &str {
        match self {
            ProviderKind::OpenAI => "openai",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Ollama => "ollama",
        }
    }

    pub fn default_base(&self) -> &str {
        match self {
            ProviderKind::OpenAI => "https://api.openai.com/v1",
            ProviderKind::Anthropic => "https://api.anthropic.com/v1",
            ProviderKind::Ollama => "http://localhost:11434/v1",
        }
    }

    pub fn default_model(&self) -> &str {
        match self {
            ProviderKind::OpenAI => "gpt-4o",
            ProviderKind::Anthropic => "claude-sonnet-4-20250514",
            ProviderKind::Ollama => "codellama",
        }
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::OpenAI,
            model: ProviderKind::OpenAI.default_model().into(),
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            api_base: None,
            max_iterations: 32,
            system_prompt: default_system_prompt(),
            llm_timeout_secs: 120,
            tool_timeout_secs: 300,
            heartbeat_interval_secs: 10,
            concurrency: 8,
            use_markdown: true,
            mode: AgentMode::Auto,
            max_context_tokens: 1_000_000,
            context_compress_ratio: 0.8,
        }
    }
}

pub fn default_system_prompt() -> String {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".into());
    let os = std::env::consts::OS;
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "?".into());
    let env_info = format!("\n\n## Environment\n- OS: {os}\n- Shell: {shell}\n- Workspace: {cwd}\n- Line endings: LF (use \\n, not \\r\\n)");

    // Inject core memory into prompt
    let mem = crate::memory::Memory::load().unwrap_or_default();
    let core_ctx = mem.core_context();

    if let Ok(content) = std::fs::read_to_string("system_prompt.md") {
        if !content.trim().is_empty() {
            return content + &env_info + &core_ctx;
        }
    }
    r#"You are Radiumical, a lean CLI coding agent. Your job is to help the user with software engineering tasks."#.to_string() + &env_info + &core_ctx + "\n\n" + r#"

## How you work
1. Read and understand the codebase using tools before making changes.
2. Use `search_code` (regex grep) and `find_files` (glob) to locate relevant code.
3. Use `read_file` to examine files. NEVER assume file contents.
4. Use `write_file` to create or overwrite files. Use `edit_file` for targeted changes.
5. Use `run_command` to execute build, test, or diagnostic commands.
6. Always validate your changes by running tests or builds when possible.

## Rules
- Be precise. Make minimal, focused edits. Do not change unrelated code.
- Match the existing code style and conventions.
- Explain your reasoning concisely before making changes.
- If you're uncertain, inspect the code first. Never guess.
- Report what you changed and why when done.
"#.into()
}
#[derive(Debug)]
pub enum UiEvent {
    LlmChunk(String),
    LlmReasoning(String),
    ThinkingTick,
    LlmDone,
    ToolStart {
        name: String,
        index: usize,
        total: usize,
        args: String,
    },
    ToolDone,
    ToolResult {
        content: String,
    },
    Choice {
        id: String,
        mode: String,
        options: Vec<String>,
    },
    Error(String),
    ThinkingDone,
    ProvidersLoaded(Vec<ProviderSource>),
    ModelsLoaded(Vec<String>),
    Toast {
        message: String,
        level: String,
        duration_secs: u64,
    },
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum BackendCmd {
    RunTask(String),
    Cancel,
    ChoiceResponse { id: String, value: String },
    SetModel(String),
    SetMode(AgentMode),
    SetThinkingEffort(String),
    RefreshModels,
    FetchProviders,
    FetchModels(ProviderSource),
    /// Reset the backend conversation history (e.g. /new).
    ResetConversation,
    /// Load a saved session into the backend conversation.
    LoadSession(Vec<SessionItem>),
}

// ═══ Slash hints ═══

// copied
