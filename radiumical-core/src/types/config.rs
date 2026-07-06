//! Session configuration and agent operating modes.

/// Operating mode that controls which tools the agent can use.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentMode {
    /// All tools available (default)
    Auto,
    /// Read-only: only read_file, search_code, find_files
    Plan,
    /// Write-enabled: all tools including edit_file, write_file, run_command
    Exec,
}

/// Configuration for a single agent session, including provider, model,
/// timeouts, context limits, and operating mode.
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
    /// Whether the harness should auto-continue when the orchestrator has ready tasks.
    /// When false, the harness stops after each LLM turn and waits for the user.
    pub auto_continue: bool,
    /// Whether to auto-resume the most recent session on startup.
    pub auto_resume_last_task: bool,
    /// Stable identifier for this TUI/backend session. Used for checkpoint branches.
    pub session_id: String,
    /// Default reasoning / thinking effort level for the provider (e.g. "low", "medium", "high", "max").
    /// `None` means use the provider's built-in default.
    pub thinking_effort: Option<String>,
}

/// Supported LLM provider backends.
///
/// Built-in variants cover the most common protocols. The [`ProviderKind::Custom`]
/// variant lets the registry-driven provider list extend support without code
/// changes: it carries the provider name and its `api_type` (e.g. `openai-chat`).
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderKind {
    OpenAI,
    Anthropic,
    Ollama,
    /// Registry-derived or otherwise unrecorded provider.
    /// Fields: `(name, api_type)`.
    Custom(String, String),
}

impl ProviderKind {
    pub fn name(&self) -> &str {
        match self {
            ProviderKind::OpenAI => "openai",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Ollama => "ollama",
            ProviderKind::Custom(name, _) => name.as_str(),
        }
    }

    /// API protocol/format used to pick the right adapter.
    pub fn api_type(&self) -> &str {
        match self {
            ProviderKind::OpenAI => "openai-chat",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Ollama => "ollama",
            ProviderKind::Custom(_, api_type) => api_type.as_str(),
        }
    }

    pub fn default_base(&self) -> &str {
        match self {
            ProviderKind::OpenAI => "https://api.openai.com/v1",
            ProviderKind::Anthropic => "https://api.anthropic.com/v1",
            ProviderKind::Ollama => "http://localhost:11434/v1",
            ProviderKind::Custom(_, _) => "",
        }
    }

    pub fn default_model(&self) -> &str {
        match self {
            ProviderKind::OpenAI => "gpt-4o",
            ProviderKind::Anthropic => "claude-sonnet-4-20250514",
            ProviderKind::Ollama => "codellama",
            ProviderKind::Custom(_, _) => "",
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
            auto_continue: true,
            auto_resume_last_task: false,
            session_id: "default".into(),
            thinking_effort: None,
        }
    }
}

/// Build the default system prompt, including environment info and core memory.
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
"#
}
