//! Lifecycle hooks that fire actions when triggers match in the dynamic orchestrator.

use serde::{Deserialize, Serialize};

use super::guard::Guard;

/// When a hook should fire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookTrigger {
    OnStart,
    OnDone,
    OnError,
    When(Guard),
    WhileRunning,
    OnEvent(String),
}

/// The action to perform when a hook fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookAction {
    StartTask(u32),
    EmitEvent(String),
    SetMetric(String, f64),
    MarkDone(u32),
    SuspendTask(u32),
    ResumeTask(u32),
    SpawnAgent {
        id: String,
        task: String,
        agent: Option<String>,
    },
    Custom(String),
    Sequence(Vec<HookAction>),
}

/// A named hook with a trigger condition, optional guard, and action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    pub id: String,
    pub trigger: HookTrigger,
    pub action: HookAction,
    #[serde(default)]
    pub guard: Option<Guard>,
    #[serde(default)]
    pub max_fires: Option<u32>,
    #[serde(default)]
    pub fire_count: u32,
}
