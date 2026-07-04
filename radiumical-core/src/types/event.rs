//! Event types exchanged between the LLM provider, the harness, and the UI layer.

use crate::checkpoint::Checkpoint;
use crate::orchestrator::TaskStatus;
use crate::providers::ProviderSource;
use crate::session::SessionItem;

use super::config::AgentMode;
use super::message::ToolCall;

// ── Provider response ──

/// A single streaming event from the LLM provider.
#[derive(Debug, Clone)]
pub enum ProviderEvent {
    Text(String),
    Reasoning(String),
    ToolCalls(Vec<ToolCall>),
    Done,
    #[allow(dead_code)]
    Error(String),
}

/// Events sent from the backend to the UI for rendering.
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
    CheckpointCreated(Checkpoint),
}

/// A lightweight task update emitted when the orchestration plan changes.
#[derive(Debug, Clone)]
pub struct PlanTaskUpdate {
    pub id: u32,
    pub title: String,
    pub status: TaskStatus,
}

/// Commands sent from the UI to the backend agent loop.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum BackendCmd {
    RunTask(String),
    RunTaskWithImages {
        task: String,
        images: Vec<std::path::PathBuf>,
    },
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
