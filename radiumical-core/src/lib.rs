//! Core library for the Radiumical CLI coding agent.
//!
//! This crate provides the runtime primitives that power the agent:
//! - **LLM provider abstraction** (`provider`, `providers`)
//! - **Tool system** (`tools`) with file, command, search, orchestration, and more
//! - **Harness** (`harness`) — the generic LLM-loop runtime
//! - **Orchestrators** — linear (`orchestrator`) and dynamic/conditional (`dynamic`)
//! - **Shared types** (`types`) for messages, events, and configuration

pub mod agent;
pub mod agent_pool;
pub mod checkpoint;
pub mod cluster;
pub mod commands;
pub mod config;
pub mod conversation;
pub mod dynamic;
pub mod harness;
pub mod highlight;
pub mod hooks;
pub mod image;
pub mod llm_cache;
pub mod lsp;
pub mod mcp;
pub mod memory;
pub mod orchestrator;
pub mod outline;
pub mod perf;
pub mod pipeline;
pub mod plugins;
pub mod provider;
pub mod providers;
pub mod secure_env;
pub mod session;
pub mod skill;
pub mod subagent;
pub mod systools;
pub mod tools;
pub mod types;

// ═══ Stable public API re-exports ═══

pub use agent::Agent;
pub use agent_pool::{AgentDef, AgentRoleMode};
pub use checkpoint::{create_checkpoint, list_checkpoints, rollback, Checkpoint};
pub use cluster::{AgentCluster, ClusterEvent, WorkerSlot};
pub use config::Config;
pub use dynamic::{
    DynamicOrchestrator, DynamicTask, EventBus, Guard, Hook, HookAction, HookTrigger,
    TaskState as DynTaskState,
};
pub use harness::{Harness, ToolHook};
pub use image::{build_multipart_content, format_image_size, image_file_size, load_image_part};
pub use pipeline::PipelineRunner;
pub use provider::{create_provider, Provider};
pub use providers::{ProviderRegistry, ProviderSource};
pub use skill::{Skill, SkillMeta, SkillRegistry};
pub use subagent::{SubAgentHandle, SubAgentResult};
pub use types::{AgentMode, BackendCmd, Message, SessionConfig, UiEvent};
