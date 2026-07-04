//! Dynamic orchestrator — event-driven, condition-guarded, programmable execution.
//!
//! This is the **advanced** orchestrator for reactive, non-linear workflows.
//! Use this when you need guards, hooks, events, persistent tasks, or sub-agent clusters.
//!
//! # When to use which orchestrator
//!
//! See [`crate::orchestrator`] for a comparison table. In short:
//! - **Simple tasks** → `Orchestrator` (linear plan)
//! - **Reactive workflows** → `DynamicOrchestrator` (this module)
//!
//! Convert between them with [`DynamicOrchestrator::export_plan`] and [`crate::orchestrator::Orchestrator::to_dynamic`].
//!
//! # Mental Model
//!
//! Think of it as a reactive DAG where nodes are tasks, edges are dependencies,
//! and each node has guards and hooks that make the graph dynamic at runtime.
//!
//! ```text
//! Pending ──(deps+guard OK)──► Ready ──(agent)──► Running ──► Done
//!   ▲                                                      │
//!   └──────────────────── re-trigger / retry ◄──────────────┘
//! ```

pub mod bridge;
pub mod event;
pub mod guard;
pub mod hook;
pub mod pool;
pub mod task;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use self::event::{Event, EventBus};
pub use self::guard::{CompareOp, Guard, GuardContext};
pub use self::hook::{Hook, HookAction, HookTrigger};
pub use self::pool::{AgentResult, AgentStatus, AgentWork, PersistentAgent, PersistentAgentPool};
pub use self::task::{DynamicTask, TaskState};

/// Action produced by a single tick of the dynamic orchestrator.
#[derive(Debug)]
pub enum TickAction {
    TaskReady(u32),
    NeedsAgent {
        task_id: u32,
        agent_hint: Option<String>,
    },
    FireHook(Hook),
    TaskRetry(u32),
    EventEmitted(String),
}

/// Event-driven, condition-guarded orchestrator for reactive workflows.
pub struct DynamicOrchestrator {
    pub tasks: HashMap<u32, DynamicTask>,
    pub hooks: Vec<Hook>,
    pub event_bus: EventBus,
    pub metrics: HashMap<String, f64>,
    pub custom_guards: HashMap<String, Box<dyn Fn() -> bool + Send + Sync>>,
    pub agent_pool: PersistentAgentPool,
    pub next_id: u32,
    state_path: Option<PathBuf>,
}

impl DynamicOrchestrator {
    pub fn new(session: Option<&str>) -> Self {
        let state_path = session.map(|name| {
            let dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".radi")
                .join("dynamic");
            if let Err(e) = std::fs::create_dir_all(&dir) {
                tracing::error!(error = %e, "failed to create dynamic orchestrator state directory");
            }
            dir.join(format!("{name}.json"))
        });

        let (tasks, next_id) = if let Some(ref path) = state_path {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|s| serde_json::from_str::<PersistedState>(&s).ok())
                .map(|ps| (ps.tasks, ps.next_id))
                .unwrap_or((HashMap::new(), 1))
        } else {
            (HashMap::new(), 1)
        };

        Self {
            tasks,
            hooks: Vec::new(),
            event_bus: EventBus::new(),
            metrics: HashMap::new(),
            custom_guards: HashMap::new(),
            agent_pool: PersistentAgentPool::new(),
            next_id,
            state_path,
        }
    }

    pub fn add_task(&mut self, task: DynamicTask) -> u32 {
        let id = if task.id == 0 { self.next_id } else { task.id };
        let mut task = task;
        task.id = id;
        if id >= self.next_id {
            self.next_id = id + 1;
        }
        self.tasks.insert(id, task);
        self.save();
        id
    }

    pub fn create_task(&mut self, title: &str) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.tasks
            .insert(id, DynamicTask::new(id, title.to_string()));
        self.save();
        id
    }

    pub fn remove_task(&mut self, id: u32) -> bool {
        let removed = self.tasks.remove(&id).is_some();
        if removed {
            for task in self.tasks.values_mut() {
                task.deps.retain(|d| *d != id);
            }
            self.save();
        }
        removed
    }

    pub fn transition(&mut self, id: u32, next: TaskState) -> Result<String, String> {
        let task = self.tasks.get_mut(&id).ok_or("Task not found")?;
        if task.try_transition(next.clone()) {
            let msg = format!("Task #{}: {:?} → {:?}", id, task.state, next);
            let hooks = task.hooks.clone();
            for hook in &hooks {
                match &hook.trigger {
                    HookTrigger::OnStart if next == TaskState::Running => {}
                    HookTrigger::OnDone if next == TaskState::Done => {}
                    HookTrigger::OnError if next == TaskState::Failed => {}
                    _ => continue,
                }
            }
            self.save();
            Ok(msg)
        } else {
            Err(format!("Invalid transition: {:?} → {:?}", task.state, next))
        }
    }

    pub fn tagged_done(&mut self, id: u32, output: Option<String>) -> Result<(), String> {
        let task = self.tasks.get_mut(&id).ok_or("Task not found")?;
        task.state = TaskState::Done;
        task.output = output;
        self.event_bus.emit(Event {
            key: format!("task.done.{id}"),
            source_task: Some(id),
            payload: None,
            timestamp: now_secs(),
        });
        self.save();
        Ok(())
    }

    pub fn tick(&mut self) -> Vec<TickAction> {
        let mut actions = Vec::new();

        let task_states: HashMap<u32, TaskState> = self
            .tasks
            .iter()
            .map(|(id, t)| (*id, t.state.clone()))
            .collect();
        let emitted = self.event_bus.emitted_keys.lock().unwrap().clone();
        let ctx = GuardContext {
            task_states: &task_states,
            emitted_events: &emitted,
            metrics: &self.metrics,
            custom_guards: &self.custom_guards,
        };

        let task_ids: Vec<u32> = self.tasks.keys().copied().collect();

        for id in task_ids {
            let task = match self.tasks.get(&id) {
                Some(t) => t,
                None => continue,
            };

            match task.state {
                TaskState::Pending => {
                    let deps_met = task.deps.iter().all(|dep_id| {
                        self.tasks
                            .get(dep_id)
                            .map(|t| t.state == TaskState::Done)
                            .unwrap_or(false)
                    });
                    if !deps_met {
                        continue;
                    }

                    let guard_pass = task
                        .guard
                        .as_ref()
                        .map(|g| g.evaluate(&ctx))
                        .unwrap_or(true);

                    if guard_pass {
                        if let Some(task) = self.tasks.get_mut(&id) {
                            task.state = TaskState::Ready;
                        }
                        actions.push(TickAction::TaskReady(id));
                    }
                }
                TaskState::Ready => {
                    let agent_hint = self.tasks.get(&id).and_then(|t| t.agent.clone());
                    actions.push(TickAction::NeedsAgent {
                        task_id: id,
                        agent_hint,
                    });
                }
                TaskState::Running => {
                    let hooks: Vec<Hook> = self
                        .tasks
                        .get(&id)
                        .map(|t| t.hooks.clone())
                        .unwrap_or_default();
                    for hook in &hooks {
                        if matches!(hook.trigger, HookTrigger::WhileRunning) {
                            actions.push(TickAction::FireHook(hook.clone()));
                        }
                    }
                }
                TaskState::Suspended => {
                    let guard_pass = self
                        .tasks
                        .get(&id)
                        .and_then(|t| t.guard.as_ref().map(|g| g.evaluate(&ctx)))
                        .unwrap_or(true);
                    if guard_pass {
                        if let Some(task) = self.tasks.get_mut(&id) {
                            task.state = TaskState::Ready;
                        }
                        actions.push(TickAction::TaskReady(id));
                    }
                }
                TaskState::Failed if task.retry_count < task.max_retries => {
                    if let Some(task) = self.tasks.get_mut(&id) {
                        task.retry_count += 1;
                        task.state = TaskState::Ready;
                        task.error = None;
                    }
                    actions.push(TickAction::TaskRetry(id));
                }
                TaskState::Persistent => {
                    let agent_hint = self.tasks.get(&id).and_then(|t| t.agent.clone());
                    actions.push(TickAction::NeedsAgent {
                        task_id: id,
                        agent_hint,
                    });
                }
                _ => {}
            }
        }

        let global_hooks = self.hooks.clone();
        for hook in &global_hooks {
            let should_fire = match &hook.trigger {
                HookTrigger::When(guard) => {
                    let guard_pass = hook
                        .guard
                        .as_ref()
                        .map(|g| g.evaluate(&ctx))
                        .unwrap_or(true);
                    guard_pass && guard.evaluate(&ctx)
                }
                HookTrigger::OnEvent(key) => self.event_bus.has_emitted(key),
                _ => false,
            };
            if should_fire {
                if let Some(max) = hook.max_fires {
                    if hook.fire_count >= max {
                        continue;
                    }
                }
                actions.push(TickAction::FireHook(hook.clone()));
            }
        }

        actions
    }

    pub fn execute_action(&mut self, action: &HookAction) {
        match action {
            HookAction::StartTask(id) => {
                if let Err(e) = self.transition(*id, TaskState::Ready) {
                    tracing::warn!(error = %e, task_id = *id, "state transition to Ready failed");
                }
            }
            HookAction::EmitEvent(key) => {
                self.event_bus.emit(Event {
                    key: key.clone(),
                    source_task: None,
                    payload: None,
                    timestamp: now_secs(),
                });
            }
            HookAction::SetMetric(key, value) => {
                self.metrics.insert(key.clone(), *value);
            }
            HookAction::MarkDone(id) => {
                if let Err(e) = self.tagged_done(*id, None) {
                    tracing::warn!(error = %e, task_id = *id, "tagged_done failed");
                }
            }
            HookAction::SuspendTask(id) => {
                if let Err(e) = self.transition(*id, TaskState::Suspended) {
                    tracing::warn!(error = %e, task_id = *id, "state transition to Suspended failed");
                }
            }
            HookAction::ResumeTask(id) => {
                if let Err(e) = self.transition(*id, TaskState::Ready) {
                    tracing::warn!(error = %e, task_id = *id, "state transition to Ready (resume) failed");
                }
            }
            HookAction::Sequence(actions) => {
                for a in actions {
                    self.execute_action(a);
                }
            }
            HookAction::Custom(_) | HookAction::SpawnAgent { .. } => {}
        }
    }

    fn save(&self) {
        if let Some(ref path) = self.state_path {
            let state = PersistedState {
                tasks: self.tasks.clone(),
                next_id: self.next_id,
            };
            if let Ok(json) = serde_json::to_string_pretty(&state) {
                if let Err(e) = std::fs::write(path, json) {
                    tracing::error!(error = %e, "failed to save dynamic orchestrator state");
                }
            }
        }
    }

    pub fn get_ready_tasks(&self) -> Vec<&DynamicTask> {
        self.tasks
            .values()
            .filter(|t| t.state == TaskState::Ready)
            .collect()
    }

    pub fn get_running_tasks(&self) -> Vec<&DynamicTask> {
        self.tasks
            .values()
            .filter(|t| t.state == TaskState::Running)
            .collect()
    }

    pub fn format_status(&self) -> String {
        let mut out = String::new();
        let mut tasks: Vec<_> = self.tasks.values().collect();
        tasks.sort_by_key(|t| t.order);

        for task in &tasks {
            let icon = match task.state {
                TaskState::Pending => "○",
                TaskState::Ready => "◐",
                TaskState::Running => "◉",
                TaskState::Suspended => "⏸",
                TaskState::Done => "✓",
                TaskState::Failed => "✗",
                TaskState::Skipped => "→",
                TaskState::Persistent => "∞",
            };
            let agent = task
                .agent
                .as_deref()
                .map(|a| format!(" @{a}"))
                .unwrap_or_default();
            let guard = if task.guard.is_some() {
                " [guarded]"
            } else {
                ""
            };
            let retry = if task.max_retries > 0 {
                format!(" (retry {}/{})", task.retry_count, task.max_retries)
            } else {
                String::new()
            };
            out.push_str(&format!(
                "  {icon} #{} {}{agent}{guard}{retry}\n",
                task.id, task.title
            ));
        }
        out
    }
}

#[derive(Serialize, Deserialize)]
struct PersistedState {
    tasks: HashMap<u32, DynamicTask>,
    next_id: u32,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
