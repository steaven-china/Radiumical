//! Dynamic orchestrator — event-driven, condition-guarded, programmable execution.
//!
//! Unlike the linear `Orchestrator` (plan → start → done → next), this system
//! supports:
//! - **Persistent sub-agents** (`sub agent inf`) that stay alive and accept work
//! - **Persistent tasks** (`sub tasks inf`) with suspend/resume lifecycle
//! - **Conditional guards** (`when condition => start`) that gate execution
//! - **Lifecycle hooks** (`on_done`, `on_start`, `on_error`) for reactive flows
//! - **Event bus** for cross-task communication
//!
//! # Mental Model
//!
//! Think of it as a reactive DAG where nodes are tasks, edges are dependencies,
//! and each node has guards and hooks that make the graph dynamic at runtime.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

// ═══════════════════════════════════════════════════════════════════════════
// Task Lifecycle State Machine
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskState {
    /// Created but not yet eligible (guards not met or deps unmet).
    Pending,
    /// Eligible to run, waiting for an agent to pick it up.
    Ready,
    /// Currently executing.
    Running,
    /// Paused — will resume when condition met.
    Suspended,
    /// Completed successfully.
    Done,
    /// Failed with error.
    Failed,
    /// Explicitly skipped.
    Skipped,
    /// Infinite lifecycle — never auto-completes, re-triggerable.
    /// Used for persistent workers and monitoring tasks.
    Persistent,
}

impl TaskState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskState::Done | TaskState::Failed | TaskState::Skipped)
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
                | (TaskState::Done, TaskState::Ready)     // re-trigger
                | (TaskState::Failed, TaskState::Ready)   // retry
                | (TaskState::Persistent, TaskState::Running)
                | (TaskState::Persistent, TaskState::Suspended)
                | (_, TaskState::Pending)                 // reset
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Guards — conditional execution gates
// ═══════════════════════════════════════════════════════════════════════════

/// A guard is a condition that must evaluate to true before a task can proceed.
///
/// Guards are checked:
/// - Before a Pending task becomes Ready
/// - Before a Suspended task resumes
/// - When a hook condition triggers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Guard {
    /// Always passes.
    Always,
    /// Never passes (used to manually hold a task).
    Never,
    /// A specific task must be Done.
    TaskDone(u32),
    /// A specific task must be in a given state.
    TaskState(u32, TaskState),
    /// An event with this key must have been emitted.
    EventEmitted(String),
    /// A named metric must satisfy the comparison.
    MetricCompare {
        key: String,
        op: CompareOp,
        value: f64,
    },
    /// All sub-guards must pass.
    And(Vec<Guard>),
    /// Any sub-guard must pass.
    Or(Vec<Guard>),
    /// Negate a sub-guard.
    Not(Box<Guard>),
    /// Custom string expression — evaluated by the harness.
    /// Format: "fn_name(arg1,arg2)" — the harness maps fn_name to a Rust function.
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CompareOp {
    Eq,
    Neq,
    Gt,
    Lt,
    Gte,
    Lte,
}

impl Guard {
    /// Evaluate this guard against the current task graph and event log.
    pub fn evaluate(&self, ctx: &GuardContext) -> bool {
        match self {
            Guard::Always => true,
            Guard::Never => false,
            Guard::TaskDone(id) => ctx
                .task_states
                .get(id)
                .map(|s| *s == TaskState::Done)
                .unwrap_or(false),
            Guard::TaskState(id, expected) => ctx
                .task_states
                .get(id)
                .map(|s| s == expected)
                .unwrap_or(false),
            Guard::EventEmitted(key) => ctx.emitted_events.contains(key),
            Guard::MetricCompare { key, op, value } => {
                let actual = ctx.metrics.get(key).copied().unwrap_or(0.0);
                match op {
                    CompareOp::Eq => (actual - value).abs() < f64::EPSILON,
                    CompareOp::Neq => (actual - value).abs() >= f64::EPSILON,
                    CompareOp::Gt => actual > *value,
                    CompareOp::Lt => actual < *value,
                    CompareOp::Gte => actual >= *value,
                    CompareOp::Lte => actual <= *value,
                }
            }
            Guard::And(guards) => guards.iter().all(|g| g.evaluate(ctx)),
            Guard::Or(guards) => guards.iter().any(|g| g.evaluate(ctx)),
            Guard::Not(inner) => !inner.evaluate(ctx),
            Guard::Custom(expr) => {
                // Check registered custom guards
                ctx.custom_guards
                    .get(expr)
                    .map(|f| f())
                    .unwrap_or(false)
            }
        }
    }
}

/// Snapshot of the world state for guard evaluation.
pub struct GuardContext<'a> {
    pub task_states: &'a HashMap<u32, TaskState>,
    pub emitted_events: &'a std::collections::HashSet<String>,
    pub metrics: &'a HashMap<String, f64>,
    pub custom_guards: &'a HashMap<String, Box<dyn Fn() -> bool + Send + Sync>>,
}

// ═══════════════════════════════════════════════════════════════════════════
// Hooks — lifecycle callbacks
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookTrigger {
    /// Fire when task transitions to Running.
    OnStart,
    /// Fire when task transitions to Done.
    OnDone,
    /// Fire when task transitions to Failed.
    OnError,
    /// Fire when a guard becomes true (condition-triggered).
    When(Guard),
    /// Fire on every tick while task is Running.
    WhileRunning,
    /// Fire when a specific event is emitted on the bus.
    OnEvent(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookAction {
    /// Start another task by ID.
    StartTask(u32),
    /// Emit an event onto the bus.
    EmitEvent(String),
    /// Set a metric value.
    SetMetric(String, f64),
    /// Mark a task as done.
    MarkDone(u32),
    /// Suspend a task.
    SuspendTask(u32),
    /// Resume a task.
    ResumeTask(u32),
    /// Run a sub-agent with a prompt.
    SpawnAgent { id: String, task: String, agent: Option<String> },
    /// Execute a custom action name (harness maps to Rust fn).
    Custom(String),
    /// Chain multiple actions.
    Sequence(Vec<HookAction>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    pub id: String,
    pub trigger: HookTrigger,
    pub action: HookAction,
    /// If set, this guard must pass before the hook fires.
    #[serde(default)]
    pub guard: Option<Guard>,
    /// Max times this hook can fire (None = unlimited).
    #[serde(default)]
    pub max_fires: Option<u32>,
    #[serde(default)]
    pub fire_count: u32,
}

// ═══════════════════════════════════════════════════════════════════════════
// Dynamic Task
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicTask {
    pub id: u32,
    pub title: String,
    pub state: TaskState,
    pub agent: Option<String>,
    /// Guard that must pass before this task becomes Ready.
    pub guard: Option<Guard>,
    /// Dependencies (other task IDs that must be Done).
    pub deps: Vec<u32>,
    /// Hooks attached to this task.
    pub hooks: Vec<Hook>,
    /// Output/result of the task (populated on completion).
    pub output: Option<String>,
    /// Error message (populated on failure).
    pub error: Option<String>,
    /// Number of times this task has been retried.
    pub retry_count: u32,
    /// Max retries (0 = no retry on failure).
    pub max_retries: u32,
    /// For persistent tasks: how many times they've been triggered.
    pub trigger_count: u32,
    /// Execution order.
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

// ═══════════════════════════════════════════════════════════════════════════
// Event Bus — cross-task communication
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct Event {
    pub key: String,
    pub source_task: Option<u32>,
    pub payload: Option<String>,
    pub timestamp: u64,
}

pub struct EventBus {
    log: Arc<Mutex<Vec<Event>>>,
    emitted_keys: Arc<Mutex<std::collections::HashSet<String>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            log: Arc::new(Mutex::new(Vec::new())),
            emitted_keys: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }

    pub fn emit(&self, event: Event) {
        self.emitted_keys.lock().unwrap().insert(event.key.clone());
        self.log.lock().unwrap().push(event);
    }

    pub fn has_emitted(&self, key: &str) -> bool {
        self.emitted_keys.lock().unwrap().contains(key)
    }

    pub fn log(&self) -> Vec<Event> {
        self.log.lock().unwrap().clone()
    }

    pub fn events_since(&self, ts: u64) -> Vec<Event> {
        self.log.lock().unwrap().iter().filter(|e| e.timestamp > ts).cloned().collect()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Persistent Agent Pool
// ═══════════════════════════════════════════════════════════════════════════

pub enum AgentStatus {
    Idle,
    Working(u32), // task id
    Draining,     // finishing current work, won't accept new tasks
}

pub struct PersistentAgent {
    pub id: String,
    pub role: String,
    pub status: AgentStatus,
    pub tasks_completed: u32,
    /// Channel to send work to this agent.
    pub work_tx: mpsc::UnboundedSender<AgentWork>,
    /// Channel to receive results from this agent.
    pub result_rx: Option<mpsc::UnboundedReceiver<AgentResult>>,
}

pub struct AgentWork {
    pub task_id: u32,
    pub task_title: String,
    pub prompt: String,
}

pub struct AgentResult {
    pub task_id: u32,
    pub success: bool,
    pub output: String,
}

pub struct PersistentAgentPool {
    agents: HashMap<String, PersistentAgent>,
}

impl Default for PersistentAgentPool {
    fn default() -> Self {
        Self::new()
    }
}

impl PersistentAgentPool {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    pub fn idle_agents(&self) -> Vec<&str> {
        self.agents
            .iter()
            .filter(|(_, a)| matches!(a.status, AgentStatus::Idle))
            .map(|(id, _)| id.as_str())
            .collect()
    }

    pub fn assign(&mut self, agent_id: &str, task_id: u32) -> bool {
        if let Some(agent) = self.agents.get_mut(agent_id) {
            if matches!(agent.status, AgentStatus::Idle) {
                agent.status = AgentStatus::Working(task_id);
                return true;
            }
        }
        false
    }

    pub fn release(&mut self, agent_id: &str) {
        if let Some(agent) = self.agents.get_mut(agent_id) {
            agent.status = AgentStatus::Idle;
            agent.tasks_completed += 1;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Dynamic Orchestrator — the engine
// ═══════════════════════════════════════════════════════════════════════════

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

        // Load persisted state if available
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

    // ── Task management ──

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
        self.tasks.insert(id, DynamicTask::new(id, title.to_string()));
        self.save();
        id
    }

    pub fn remove_task(&mut self, id: u32) -> bool {
        let removed = self.tasks.remove(&id).is_some();
        if removed {
            // Clean deps referencing this task
            for task in self.tasks.values_mut() {
                task.deps.retain(|d| *d != id);
            }
            self.save();
        }
        removed
    }

    // ── State transitions ──

    pub fn transition(&mut self, id: u32, next: TaskState) -> Result<String, String> {
        let task = self.tasks.get_mut(&id).ok_or("Task not found")?;
        if task.try_transition(next.clone()) {
            let msg = format!("Task #{}: {:?} → {:?}", id, task.state, next);
            // Fire hooks
            let hooks = task.hooks.clone();
            for hook in &hooks {
                match &hook.trigger {
                    HookTrigger::OnStart if next == TaskState::Running => {
                        // fire
                    }
                    HookTrigger::OnDone if next == TaskState::Done => {
                        // fire
                    }
                    HookTrigger::OnError if next == TaskState::Failed => {
                        // fire
                    }
                    _ => continue,
                }
            }
            self.save();
            Ok(msg)
        } else {
            Err(format!(
                "Invalid transition: {:?} → {:?}",
                task.state, next
            ))
        }
    }

    /// Tagged done — the primary way to complete a task from code/hook.
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

    // ── Tick — the main evaluation loop ──

    /// Evaluate all guards, advance state, fire hooks.
    /// Call this on every iteration of the main loop.
    pub fn tick(&mut self) -> Vec<TickAction> {
        let mut actions = Vec::new();

        // Build guard context
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

        // Collect task IDs to avoid borrow issues
        let task_ids: Vec<u32> = self.tasks.keys().copied().collect();

        for id in task_ids {
            let task = match self.tasks.get(&id) {
                Some(t) => t,
                None => continue,
            };

            match task.state {
                TaskState::Pending => {
                    // Check deps
                    let deps_met = task.deps.iter().all(|dep_id| {
                        self.tasks
                            .get(dep_id)
                            .map(|t| t.state == TaskState::Done)
                            .unwrap_or(false)
                    });
                    if !deps_met {
                        continue;
                    }

                    // Check guard
                    let guard_pass = task
                        .guard
                        .as_ref()
                        .map(|g| g.evaluate(&ctx))
                        .unwrap_or(true);

                    if guard_pass {
                        // Pending → Ready
                        if let Some(task) = self.tasks.get_mut(&id) {
                            task.state = TaskState::Ready;
                        }
                        actions.push(TickAction::TaskReady(id));
                    }
                }
                TaskState::Ready => {
                    // Ready tasks need an agent to pick them up
                    let agent_hint = self.tasks.get(&id).and_then(|t| t.agent.clone());
                    actions.push(TickAction::NeedsAgent {
                        task_id: id,
                        agent_hint,
                    });
                }
                TaskState::Running => {
                    // Fire WhileRunning hooks
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
                    // Check if guard allows resume
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
                    // Auto-retry
                    if let Some(task) = self.tasks.get_mut(&id) {
                        task.retry_count += 1;
                        task.state = TaskState::Ready;
                        task.error = None;
                    }
                    actions.push(TickAction::TaskRetry(id));
                }
                TaskState::Persistent => {
                    // Persistent tasks are always eligible for re-trigger
                    let agent_hint = self.tasks.get(&id).and_then(|t| t.agent.clone());
                    actions.push(TickAction::NeedsAgent {
                        task_id: id,
                        agent_hint,
                    });
                }
                _ => {}
            }
        }

        // Evaluate global hooks
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

    // ── Hook actions ──

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
            HookAction::Custom(_) | HookAction::SpawnAgent { .. } => {
                // These are handled by the harness, not the orchestrator.
            }
        }
    }

    // ── Persistence ──

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

    // ── Queries ──

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
            let guard = if task.guard.is_some() { " [guarded]" } else { "" };
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

// ═══════════════════════════════════════════════════════════════════════════
// Tick Actions — what the harness should do
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug)]
pub enum TickAction {
    /// Task is now Ready — needs an agent to execute it.
    TaskReady(u32),
    /// Task needs an agent assigned. The harness should spawn one.
    NeedsAgent {
        task_id: u32,
        agent_hint: Option<String>,
    },
    /// A hook should be fired.
    FireHook(Hook),
    /// Task is being retried after failure.
    TaskRetry(u32),
    /// An event was emitted (for UI notification).
    EventEmitted(String),
}

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ═══════════════════════════════════════════════════════════════════════════
// Bidirectional conversion: Orchestrator (simple) ↔ DynamicOrchestrator
// ═══════════════════════════════════════════════════════════════════════════

use crate::orchestrator::{Plan, Task, TaskStatus};

impl TaskState {
    /// Map from the simple orchestrator's TaskStatus.
    pub fn from_task_status(s: &TaskStatus) -> Self {
        match s {
            TaskStatus::Pending => TaskState::Pending,
            TaskStatus::Active => TaskState::Running,
            TaskStatus::Done => TaskState::Done,
            TaskStatus::Blocked => TaskState::Suspended,
            TaskStatus::Skipped => TaskState::Skipped,
        }
    }

    /// Map to the simple orchestrator's TaskStatus (lossy — Persistent/Failed/Ready collapse).
    pub fn to_task_status(&self) -> TaskStatus {
        match self {
            TaskState::Pending => TaskStatus::Pending,
            TaskState::Ready => TaskStatus::Pending,
            TaskState::Running => TaskStatus::Active,
            TaskState::Suspended => TaskStatus::Blocked,
            TaskState::Done => TaskStatus::Done,
            TaskState::Failed => TaskStatus::Blocked,
            TaskState::Skipped => TaskStatus::Skipped,
            TaskState::Persistent => TaskStatus::Active,
        }
    }
}

impl DynamicOrchestrator {
    /// Import a simple Plan into this dynamic orchestrator.
    /// Preserves task IDs, titles, deps, agent assignments, and status.
    pub fn import_plan(&mut self, plan: &Plan) {
        for t in &plan.tasks {
            let mut dt = DynamicTask::new(t.id, t.title.clone())
                .with_deps(t.deps.clone());
            dt.state = TaskState::from_task_status(&t.status);
            dt.order = t.order;
            dt.agent = t.agent.clone();
            self.tasks.insert(t.id, dt);
        }
        if plan.next_id > self.next_id {
            self.next_id = plan.next_id;
        }
    }

    /// Export to a simple Plan (lossy — hooks, guards, events are dropped).
    pub fn export_plan(&self, title: &str) -> Plan {
        let mut tasks: Vec<Task> = self
            .tasks
            .values()
            .map(|dt| Task {
                id: dt.id,
                title: dt.title.clone(),
                status: dt.state.to_task_status(),
                deps: dt.deps.clone(),
                order: dt.order,
                agent: dt.agent.clone(),
            })
            .collect();
        tasks.sort_by_key(|t| t.order);
        Plan {
            title: title.to_string(),
            tasks,
            next_id: self.next_id,
        }
    }
}

use crate::orchestrator::Orchestrator;

impl Orchestrator {
    /// Upgrade to a DynamicOrchestrator, preserving all plan state.
    /// The returned DynamicOrchestrator has no hooks/guards — add them after.
    pub fn to_dynamic(&self) -> DynamicOrchestrator {
        let mut dyn_orch = DynamicOrchestrator::new(None);
        dyn_orch.import_plan(self.plan());
        dyn_orch
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dyn() -> DynamicOrchestrator {
        DynamicOrchestrator::new(None)
    }

    #[test]
    fn test_task_state_transitions() {
        let mut task = DynamicTask::new(1, "test".into());
        assert_eq!(task.state, TaskState::Pending);

        assert!(task.try_transition(TaskState::Ready));
        assert_eq!(task.state, TaskState::Ready);

        assert!(task.try_transition(TaskState::Running));
        assert_eq!(task.state, TaskState::Running);

        assert!(task.try_transition(TaskState::Done));
        assert_eq!(task.state, TaskState::Done);

        // Can re-trigger from Done
        assert!(task.try_transition(TaskState::Ready));
    }

    #[test]
    fn test_invalid_transition() {
        let mut task = DynamicTask::new(1, "test".into());
        assert!(!task.try_transition(TaskState::Done)); // Pending → Done invalid
        assert_eq!(task.state, TaskState::Pending);
    }

    #[test]
    fn test_guard_always_never() {
        let ctx = GuardContext {
            task_states: &HashMap::new(),
            emitted_events: &std::collections::HashSet::new(),
            metrics: &HashMap::new(),
            custom_guards: &HashMap::new(),
        };
        assert!(Guard::Always.evaluate(&ctx));
        assert!(!Guard::Never.evaluate(&ctx));
    }

    #[test]
    fn test_guard_task_done() {
        let mut states = HashMap::new();
        states.insert(1, TaskState::Done);
        states.insert(2, TaskState::Running);
        let ctx = GuardContext {
            task_states: &states,
            emitted_events: &std::collections::HashSet::new(),
            metrics: &HashMap::new(),
            custom_guards: &HashMap::new(),
        };
        assert!(Guard::TaskDone(1).evaluate(&ctx));
        assert!(!Guard::TaskDone(2).evaluate(&ctx));
        assert!(!Guard::TaskDone(99).evaluate(&ctx));
    }

    #[test]
    fn test_guard_and_or_not() {
        let mut states = HashMap::new();
        states.insert(1, TaskState::Done);
        states.insert(2, TaskState::Running);
        let ctx = GuardContext {
            task_states: &states,
            emitted_events: &std::collections::HashSet::new(),
            metrics: &HashMap::new(),
            custom_guards: &HashMap::new(),
        };

        let g = Guard::And(vec![Guard::TaskDone(1), Guard::TaskDone(2)]);
        assert!(!g.evaluate(&ctx)); // 2 is not Done

        let g = Guard::Or(vec![Guard::TaskDone(1), Guard::TaskDone(2)]);
        assert!(g.evaluate(&ctx)); // 1 is Done

        let g = Guard::Not(Box::new(Guard::TaskDone(2)));
        assert!(g.evaluate(&ctx)); // 2 is not Done, so Not passes
    }

    #[test]
    fn test_guard_metric_compare() {
        let mut metrics = HashMap::new();
        metrics.insert("price".to_string(), 42.5);
        let ctx = GuardContext {
            task_states: &HashMap::new(),
            emitted_events: &std::collections::HashSet::new(),
            metrics: &metrics,
            custom_guards: &HashMap::new(),
        };

        assert!(Guard::MetricCompare {
            key: "price".into(),
            op: CompareOp::Gt,
            value: 40.0,
        }
        .evaluate(&ctx));

        assert!(!Guard::MetricCompare {
            key: "price".into(),
            op: CompareOp::Lt,
            value: 40.0,
        }
        .evaluate(&ctx));
    }

    #[test]
    fn test_guard_event_emitted() {
        let mut events = std::collections::HashSet::new();
        events.insert("deploy.ready".to_string());
        let ctx = GuardContext {
            task_states: &HashMap::new(),
            emitted_events: &events,
            metrics: &HashMap::new(),
            custom_guards: &HashMap::new(),
        };

        assert!(Guard::EventEmitted("deploy.ready".into()).evaluate(&ctx));
        assert!(!Guard::EventEmitted("deploy.failed".into()).evaluate(&ctx));
    }

    #[test]
    fn test_dynamic_orchestrator_tick() {
        let mut orch = make_dyn();

        let t1 = DynamicTask::new(1, "setup".into());
        let t2 = DynamicTask::new(2, "build".into()).with_deps(vec![1]);
        let t3 = DynamicTask::new(3, "test".into())
            .with_deps(vec![2])
            .with_guard(Guard::EventEmitted("build.success".into()));

        orch.add_task(t1);
        orch.add_task(t2);
        orch.add_task(t3);

        // First tick: t1 has no deps, should become Ready
        let actions = orch.tick();
        assert!(actions.iter().any(|a| matches!(a, TickAction::TaskReady(1))));

        // t1 is now Ready, t2/t3 still Pending (deps not met)
        assert_eq!(orch.tasks[&1].state, TaskState::Ready);
        assert_eq!(orch.tasks[&2].state, TaskState::Pending);
        assert_eq!(orch.tasks[&3].state, TaskState::Pending);
    }

    #[test]
    fn test_tagged_done_and_event() {
        let mut orch = make_dyn();
        orch.add_task(DynamicTask::new(1, "task".into()));

        orch.tagged_done(1, Some("output".into())).unwrap();

        assert_eq!(orch.tasks[&1].state, TaskState::Done);
        assert_eq!(orch.tasks[&1].output.as_deref(), Some("output"));
        assert!(orch.event_bus.has_emitted("task.done.1"));
    }

    #[test]
    fn test_persistent_task() {
        let mut orch = make_dyn();
        let t = DynamicTask::new(1, "monitor".into()).persistent();
        orch.add_task(t);

        assert_eq!(orch.tasks[&1].state, TaskState::Persistent);

        let actions = orch.tick();
        assert!(actions
            .iter()
            .any(|a| matches!(a, TickAction::NeedsAgent { task_id: 1, .. })));
    }

    #[test]
    fn test_retry_on_failure() {
        let mut orch = make_dyn();
        let t = DynamicTask::new(1, "flaky".into()).with_retries(3);
        orch.add_task(t);

        // Move to Running then Failed
        orch.transition(1, TaskState::Ready).unwrap();
        orch.transition(1, TaskState::Running).unwrap();
        orch.transition(1, TaskState::Failed).unwrap();

        // Tick should auto-retry
        let actions = orch.tick();
        assert!(actions.iter().any(|a| matches!(a, TickAction::TaskRetry(1))));
        assert_eq!(orch.tasks[&1].state, TaskState::Ready);
        assert_eq!(orch.tasks[&1].retry_count, 1);
    }

    #[test]
    fn test_hook_condition_trigger() {
        let mut orch = make_dyn();

        // Add a metric
        orch.metrics.insert("price".to_string(), 100.0);

        // Global hook: when price > 50, start task 2
        orch.hooks.push(Hook {
            id: "price_trigger".into(),
            trigger: HookTrigger::When(Guard::MetricCompare {
                key: "price".into(),
                op: CompareOp::Gt,
                value: 50.0,
            }),
            action: HookAction::StartTask(2),
            guard: None,
            max_fires: Some(1),
            fire_count: 0,
        });

        orch.add_task(DynamicTask::new(1, "watch".into()));
        orch.add_task(DynamicTask::new(2, "trade".into()));

        let actions = orch.tick();
        assert!(actions
            .iter()
            .any(|a| matches!(a, TickAction::FireHook(h) if h.id == "price_trigger")));
    }

    #[test]
    fn test_format_status() {
        let mut orch = make_dyn();
        orch.add_task(DynamicTask::new(1, "setup".into()));
        orch.add_task(
            DynamicTask::new(2, "deploy".into())
                .with_agent("coder")
                .with_guard(Guard::TaskDone(1)),
        );
        orch.tasks.get_mut(&1).unwrap().state = TaskState::Done;

        let status = orch.format_status();
        assert!(status.contains("✓"));
        assert!(status.contains("○"));
        assert!(status.contains("@coder"));
        assert!(status.contains("[guarded]"));
    }

    #[test]
    fn test_import_plan_roundtrip() {
        let mut orch = make_dyn();
        orch.add_task(DynamicTask::new(1, "A".into()));
        orch.add_task(DynamicTask::new(2, "B".into()).with_deps(vec![1]).with_agent("coder"));
        orch.tasks.get_mut(&1).unwrap().state = TaskState::Done;

        let plan = orch.export_plan("test");
        assert_eq!(plan.title, "test");
        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[0].status, TaskStatus::Done);
        assert_eq!(plan.tasks[1].agent.as_deref(), Some("coder"));

        // Re-import into a fresh orchestrator
        let mut orch2 = make_dyn();
        orch2.import_plan(&plan);
        assert_eq!(orch2.tasks.len(), 2);
        assert_eq!(orch2.tasks[&1].state, TaskState::Done);
        assert_eq!(orch2.tasks[&2].agent.as_deref(), Some("coder"));
    }

    #[test]
    fn test_orchestrator_to_dynamic() {
        let mut simple = crate::orchestrator::Orchestrator::new(None);
        simple.create("Test", vec![
            ("Step 1".into(), vec![]),
            ("Step 2".into(), vec![1]),
        ]);
        simple.start(1).unwrap();
        simple.done(1).unwrap();

        let dyn_orch = simple.to_dynamic();
        assert_eq!(dyn_orch.tasks.len(), 2);
        assert_eq!(dyn_orch.tasks[&1].state, TaskState::Done);
        // After done(1), old orchestrator auto-starts task 2 as Active → Running
        assert_eq!(dyn_orch.tasks[&2].state, TaskState::Running);
    }
}
