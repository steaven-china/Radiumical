use serde::{Deserialize, Serialize};

// ── Chat types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>, // for tool results, who produced it
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
    #[allow(dead_code)]
    pub fn text(&self) -> &str {
        match self {
            MessageContent::Text(s) => s.as_str(),
            MessageContent::Parts(_) => "",
        }
    }
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
    pub arguments: String, // JSON string
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
    pub parameters: serde_json::Value, // JSON Schema
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
    ToolResult { content: String },
    Error(String),
    ThinkingDone,
}

#[derive(Debug, Clone)]
pub enum BackendCmd {
    RunTask(String),
    Cancel,
}

// ═══ Slash hints ═══

// copied
