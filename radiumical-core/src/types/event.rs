use crate::orchestrator::TaskStatus;
use crate::providers::ProviderSource;
use crate::session::SessionItem;

use super::config::AgentMode;
use super::message::ToolCall;

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
    TitleGenerated(String),
    SubAgentDone {
        id: String,
        success: bool,
    },
    McpStatus {
        name: String,
        alive: bool,
        tool_count: usize,
    },
    PlanUpdated {
        title: String,
        tasks: Vec<PlanTaskUpdate>,
    },
}

#[derive(Debug, Clone)]
pub struct PlanTaskUpdate {
    pub id: u32,
    pub title: String,
    pub status: TaskStatus,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum BackendCmd {
    RunTask(String),
    Cancel,
    ChoiceResponse {
        id: String,
        value: String,
    },
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
    /// Toggle an MCP server on/off by name.
    ToggleMcpServer {
        name: String,
    },
}
