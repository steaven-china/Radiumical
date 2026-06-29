pub mod agent;
pub mod agent_pool;
pub mod commands;
pub mod config;
pub mod conversation;
pub mod harness;
pub mod highlight;
pub mod hooks;
pub mod llm_cache;
pub mod lsp;
pub mod memory;
pub mod orchestrator;
pub mod outline;
pub mod perf;
pub mod pipeline;
pub mod plugins;
pub mod provider;
pub mod providers;
pub mod session;
pub mod subagent;
pub mod systools;
pub mod tools;
pub mod types;

// ═══ Stable public API re-exports ═══

pub use agent::Agent;
pub use agent_pool::{AgentDef, AgentRoleMode};
pub use config::Config;
pub use harness::{Harness, ToolHook};
pub use pipeline::PipelineRunner;
pub use provider::{create_provider, Provider};
pub use providers::{ProviderRegistry, ProviderSource};
pub use types::{AgentMode, BackendCmd, Message, SessionConfig, UiEvent};
