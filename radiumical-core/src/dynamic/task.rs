//! Task model for the dynamic orchestrator — state machine with guards and hooks.

use serde::{Deserialize, Serialize};

use super::guard::Guard;
use super::hook::Hook;

/// Lifecycle states of a dynamic task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskState {
    Pending,
    Ready,
    Running,
    Suspended,
    Done,
    Failed,
    Skipped,
    Persistent,
}

impl TaskState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskState::Done | TaskState::Failed | TaskState::Skipped
        )
    }

    pub fn can_transition_to(&self, next: &TaskState) -> bool {
        matches!(
            (self, next),
            (TaskState::Pending, TaskState::Ready)
                | (TaskState::Ready, TaskState::Running)
                | (TaskState::Ready, TaskState::Skipped)
                | (TaskState::Running, TaskState::Done)
                | (TaskState::Running, TaskState::Failed)
                | (TaskState::Running, TaskState::Suspended)
                | (TaskState::Running, TaskState::Persistent)
                | (TaskState::Suspended, TaskState::Ready)
                | (TaskState::Suspended, TaskState::Running)
                | (TaskState::Done, TaskState::Ready)
                | (TaskState::Failed, TaskState::Ready)
                | (TaskState::Persistent, TaskState::Running)
                | (TaskState::Persistent, TaskState::Suspended)
                | (_, TaskState::Pending)
        )
    }
}

/// A task in the dynamic orchestrator with dependencies, guards, hooks, and retry logic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicTask {
    pub id: u32,
    pub title: String,
    pub state: TaskState,
    pub agent: Option<String>,
    pub guard: Option<Guard>,
    pub deps: Vec<u32>,
    pub hooks: Vec<Hook>,
    pub output: Option<String>,
    pub error: Option<String>,
    pub retry_count: u32,
    pub max_retries: u32,
    pub trigger_count: u32,
    pub order: usize,
}

impl DynamicTask {
    pub fn new(id: u32, title: String) -> Self {
        Self {
            id,
            title,
            state: TaskState::Pending,
            agent: None,
            guard: None,
            deps: Vec::new(),
            hooks: Vec::new(),
            output: None,
            error: None,
            retry_count: 0,
            max_retries: 0,
            trigger_count: 0,
            order: id as usize,
        }
    }

    pub fn with_agent(mut self, agent: &str) -> Self {
        self.agent = Some(agent.to_string());
        self
    }

    pub fn with_guard(mut self, guard: Guard) -> Self {
        self.guard = Some(guard);
        self
    }

    pub fn with_deps(mut self, deps: Vec<u32>) -> Self {
        self.deps = deps;
        self
    }

    pub fn with_hook(mut self, hook: Hook) -> Self {
        self.hooks.push(hook);
        self
    }

    pub fn with_retries(mut self, max: u32) -> Self {
        self.max_retries = max;
        self
    }

    pub fn persistent(mut self) -> Self {
        self.state = TaskState::Persistent;
        self
    }

    pub fn try_transition(&mut self, next: TaskState) -> bool {
        if self.state.can_transition_to(&next) {
            self.state = next;
            true
        } else {
            false
        }
    }
}
