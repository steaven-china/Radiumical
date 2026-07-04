//! Agent Cluster — persistent sub-agent pool driven by DynamicOrchestrator.
//!
//! This is the integration layer that turns the DynamicOrchestrator's tick()
//! actions into actual sub-agent work. It manages a pool of worker slots,
//! assigns Ready tasks to idle workers, collects results, and feeds them
//! back into the orchestrator's event bus and state machine.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    AgentCluster                         │
//! │                                                         │
//! │  ┌──────────────┐    tick()    ┌───────────────────┐    │
//! │  │  Dynamic      │───────────►│  WorkerScheduler   │    │
//! │  │  Orchestrator │            │                    │    │
//! │  │              │◄───────────│  assign / collect   │    │
//! │  └──────────────┘  results    └───────────────────┘    │
//! │         │                                   │          │
//! │         ▼                                   ▼          │
//! │    EventBus                          WorkerSlot[]      │
//! │    (events,                          (idle/running)    │
//! │     guards,                                              │
//! │     hooks)                                               │
//! └─────────────────────────────────────────────────────────┘
//! ```

use crate::dynamic::{
    DynamicOrchestrator, Event, HookAction,
    TaskState, TickAction,
};
use crate::provider::Provider;
use crate::subagent;
use crate::types::SessionConfig;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;

// ═══════════════════════════════════════════════════════════════════════════
// Worker Slot
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq)]
pub enum WorkerState {
    Idle,
    Running {
        task_id: u32,
        agent_id: String,
        started_at: u64,
    },
    Draining,
}

pub struct WorkerSlot {
    pub id: String,
    pub role: String,
    pub state: WorkerState,
    pub tasks_completed: u32,
}

impl WorkerSlot {
    pub fn new(id: impl Into<String>, role: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: role.into(),
            state: WorkerState::Idle,
            tasks_completed: 0,
        }
    }

    pub fn is_idle(&self) -> bool {
        self.state == WorkerState::Idle
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Cluster Result — what a worker produced
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct ClusterResult {
    pub task_id: u32,
    pub worker_id: String,
    pub success: bool,
    pub output: String,
}

// ═══════════════════════════════════════════════════════════════════════════
// Cluster Event — high-level events emitted by the cluster
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub enum ClusterEvent {
    TaskAssigned {
        task_id: u32,
        worker_id: String,
    },
    TaskCompleted {
        task_id: u32,
        worker_id: String,
        success: bool,
    },
    TaskFailed {
        task_id: u32,
        worker_id: String,
        error: String,
    },
    HookFired {
        hook_id: String,
    },
    WorkerSpawned {
        worker_id: String,
    },
    ClusterTick {
        ready: usize,
        running: usize,
        idle: usize,
    },
    AllDone,
}

// ═══════════════════════════════════════════════════════════════════════════
// Agent Cluster — the orchestration runtime
// ═══════════════════════════════════════════════════════════════════════════

pub struct AgentCluster {
    pub orchestrator: DynamicOrchestrator,
    pub workers: HashMap<String, WorkerSlot>,
    pub config: SessionConfig,
    pub provider: Arc<dyn Provider>,
    pub workspace: PathBuf,

    /// Pending results from completed workers, waiting to be consumed.
    result_rx: mpsc::UnboundedReceiver<ClusterResult>,
    result_tx: mpsc::UnboundedSender<ClusterResult>,

    /// Events emitted by the cluster (for UI / logging).
    event_tx: mpsc::UnboundedSender<ClusterEvent>,
    event_log: Vec<ClusterEvent>,

    /// Max concurrent workers.
    pub max_concurrency: usize,

    /// Tick interval in milliseconds.
    pub tick_interval_ms: u64,
}

impl AgentCluster {
    pub fn new(
        orchestrator: DynamicOrchestrator,
        config: SessionConfig,
        provider: Arc<dyn Provider>,
        workspace: PathBuf,
    ) -> Self {
        let (result_tx, result_rx) = mpsc::unbounded_channel();
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        Self {
            orchestrator,
            workers: HashMap::new(),
            config,
            provider,
            workspace,
            result_rx,
            result_tx,
            event_tx,
            event_log: Vec::new(),
            max_concurrency: 4,
            tick_interval_ms: 500,
        }
    }

    // ── Worker management ──

    pub fn add_worker(&mut self, id: &str, role: &str) {
        self.workers.insert(
            id.to_string(),
            WorkerSlot::new(id, role),
        );
        self.emit(ClusterEvent::WorkerSpawned {
            worker_id: id.to_string(),
        });
    }

    pub fn idle_worker_for_role(&self, role: Option<&str>) -> Option<&str> {
        self.workers
            .values()
            .filter(|w| w.is_idle())
            .filter(|w| match role {
                Some(r) => w.role == r,
                None => true,
            })
            .min_by_key(|w| w.tasks_completed)
            .map(|w| w.id.as_str())
    }

    pub fn idle_count(&self) -> usize {
        self.workers.values().filter(|w| w.is_idle()).count()
    }

    pub fn running_count(&self) -> usize {
        self.workers.values().filter(|w| matches!(w.state, WorkerState::Running { .. })).count()
    }

    // ── Core loop ──

    /// Execute one tick of the cluster:
    /// 1. Collect completed worker results
    /// 2. Feed results into orchestrator (tagged_done, events)
    /// 3. Run orchestrator.tick() to get next actions
    /// 4. Assign Ready tasks to idle workers
    /// 5. Fire hooks
    pub async fn tick(&mut self) -> Vec<ClusterEvent> {
        let mut events = Vec::new();

        // ── 1. Collect completed results ──
        while let Ok(result) = self.result_rx.try_recv() {
            events.push(ClusterEvent::TaskCompleted {
                task_id: result.task_id,
                worker_id: result.worker_id.clone(),
                success: result.success,
            });

            // Release worker
            if let Some(worker) = self.workers.get_mut(&result.worker_id) {
                worker.state = WorkerState::Idle;
                worker.tasks_completed += 1;
            }

            // Feed result into orchestrator
            if result.success {
                let _ = self.orchestrator.tagged_done(
                    result.task_id,
                    Some(result.output.clone()),
                );
                self.orchestrator.event_bus.emit(Event {
                    key: format!("task.done.{}", result.task_id),
                    source_task: Some(result.task_id),
                    payload: Some(result.output),
                    timestamp: now_secs(),
                });
            } else {
                // Transition to Failed (which may trigger retry via tick)
                let _ = self.orchestrator.transition(
                    result.task_id,
                    TaskState::Failed,
                );
                self.orchestrator.event_bus.emit(Event {
                    key: format!("task.failed.{}", result.task_id),
                    source_task: Some(result.task_id),
                    payload: Some(result.output.clone()),
                    timestamp: now_secs(),
                });
                // Store error
                if let Some(task) = self.orchestrator.tasks.get_mut(&result.task_id) {
                    task.error = Some(result.output);
                }
            }
        }

        // ── 2. Orchestrator tick ──
        let actions = self.orchestrator.tick();

        for action in actions {
            match action {
                TickAction::TaskReady(id) => {
                    // Task just became Ready — will be picked up in NeedsAgent
                    events.push(ClusterEvent::TaskAssigned {
                        task_id: id,
                        worker_id: "pending".into(),
                    });
                }

                TickAction::NeedsAgent { task_id, agent_hint } => {
                    // Try to assign to an idle worker
                    let role_hint = agent_hint.as_deref();
                    if let Some(worker_id) = self.idle_worker_for_role(role_hint) {
                        let worker_id = worker_id.to_string();
                        self.assign_task(task_id, &worker_id).await;
                        events.push(ClusterEvent::TaskAssigned {
                            task_id,
                            worker_id,
                        });
                    }
                    // else: no idle worker, task stays Ready — will retry next tick
                }

                TickAction::FireHook(hook) => {
                    events.push(ClusterEvent::HookFired {
                        hook_id: hook.id.clone(),
                    });

                    // Execute the hook action
                    match &hook.action {
                        HookAction::SpawnAgent { id, task, agent } => {
                            // Spawn a new sub-agent for this hook
                            let handle = subagent::spawn(
                                id.clone(),
                                task.clone(),
                                agent.clone(),
                                self.config.clone(),
                                Arc::clone(&self.provider),
                                None,
                            )
                            .await;
                            let tx = self.result_tx.clone();
                            let hook_id = id.clone();
                            tokio::spawn(async move {
                                let mut handle = handle;
                                let result = handle.wait().await;
                                let _ = tx.send(ClusterResult {
                                    task_id: 0,
                                    worker_id: hook_id,
                                    success: result.success,
                                    output: result.output,
                                });
                            });
                        }
                        HookAction::StartTask(id) => {
                            let _ = self.orchestrator.transition(*id, TaskState::Ready);
                        }
                        HookAction::EmitEvent(key) => {
                            self.orchestrator.event_bus.emit(Event {
                                key: key.clone(),
                                source_task: None,
                                payload: None,
                                timestamp: now_secs(),
                            });
                        }
                        HookAction::MarkDone(id) => {
                            let _ = self.orchestrator.tagged_done(*id, None);
                        }
                        HookAction::SetMetric(key, value) => {
                            self.orchestrator.metrics.insert(key.clone(), *value);
                        }
                        HookAction::SuspendTask(id) => {
                            let _ = self.orchestrator.transition(*id, TaskState::Suspended);
                        }
                        HookAction::ResumeTask(id) => {
                            let _ = self.orchestrator.transition(*id, TaskState::Ready);
                        }
                        HookAction::Sequence(actions) => {
                            for a in actions {
                                self.orchestrator.execute_action(a);
                            }
                        }
                        HookAction::Custom(_) => {
                            // Custom actions are handled by the caller
                        }
                    }

                    // Increment hook fire count
                    for h in &mut self.orchestrator.hooks {
                        if h.id == hook.id {
                            h.fire_count += 1;
                        }
                    }
                    // Also increment task-level hooks
                    for task in self.orchestrator.tasks.values_mut() {
                        for h in &mut task.hooks {
                            if h.id == hook.id {
                                h.fire_count += 1;
                            }
                        }
                    }
                }

                TickAction::TaskRetry(id) => {
                    // Task was auto-retried by orchestrator — it's now Ready
                    events.push(ClusterEvent::TaskAssigned {
                        task_id: id,
                        worker_id: "retry".into(),
                    });
                }

                TickAction::EventEmitted(key) => {
                    // Already handled via event_bus
                    let _ = key;
                }
            }
        }

        // ── 3. Check if all done ──
        let all_terminal = !self.orchestrator.tasks.is_empty()
            && self.orchestrator.tasks.values().all(|t| {
                t.state == TaskState::Done
                    || t.state == TaskState::Failed
                    || t.state == TaskState::Skipped
                    || t.state == TaskState::Persistent
            });
        let all_idle = self.workers.values().all(|w| w.is_idle());

        if all_terminal && all_idle {
            events.push(ClusterEvent::AllDone);
        }

        // ── 4. Emit tick summary ──
        events.push(ClusterEvent::ClusterTick {
            ready: self.orchestrator.get_ready_tasks().len(),
            running: self.running_count(),
            idle: self.idle_count(),
        });

        self.event_log.extend(events.clone());
        events
    }

    // ── Task assignment ──

    async fn assign_task(&mut self, task_id: u32, worker_id: &str) {
        // Get task info before borrowing worker
        let (title, agent_name) = match self.orchestrator.tasks.get(&task_id) {
            Some(t) => (t.title.clone(), t.agent.clone()),
            None => return,
        };

        // Mark worker as running
        if let Some(worker) = self.workers.get_mut(worker_id) {
            worker.state = WorkerState::Running {
                task_id,
                agent_id: worker_id.to_string(),
                started_at: now_secs(),
            };
        }

        // Transition task to Running
        let _ = self.orchestrator.transition(task_id, TaskState::Running);

        // Build prompt for the sub-agent
        let prompt = format!(
            "Execute task #{}: {}\n\n\
             When done, your output will be captured as the task result.",
            task_id, title
        );

        // Spawn sub-agent
        let handle = subagent::spawn(
            format!("cluster-{}", task_id),
            prompt,
            agent_name,
            self.config.clone(),
            Arc::clone(&self.provider),
            None,
        )
        .await;

        // Wire result back to cluster
        let tx = self.result_tx.clone();
        let wid = worker_id.to_string();
        tokio::spawn(async move {
            let mut handle = handle;
            let result = handle.wait().await;
            let _ = tx.send(ClusterResult {
                task_id,
                worker_id: wid,
                success: result.success,
                output: result.output,
            });
        });
    }

    // ── Run until all tasks complete ──

    pub async fn run_to_completion(&mut self) -> Vec<ClusterEvent> {
        let mut all_events = Vec::new();
        let interval = std::time::Duration::from_millis(self.tick_interval_ms);

        loop {
            let events = self.tick().await;
            let has_all_done = events.iter().any(|e| matches!(e, ClusterEvent::AllDone));
            all_events.extend(events);

            if has_all_done {
                break;
            }

            tokio::time::sleep(interval).await;
        }

        all_events
    }

    // ── Queries ──

    pub fn format_status(&self) -> String {
        let mut out = String::from("Cluster Status:\n\n");

        // Workers
        out.push_str("Workers:\n");
        for worker in self.workers.values() {
            let (icon, status) = match &worker.state {
                WorkerState::Idle => ("○", "idle".to_string()),
                WorkerState::Running { task_id, .. } => {
                    ("◉", format!("working on #{}", task_id))
                }
                WorkerState::Draining => ("◐", "draining".to_string()),
            };
            out.push_str(&format!(
                "  {icon} {} ({}) — {} — {} tasks done\n",
                worker.id, worker.role, status, worker.tasks_completed
            ));
        }

        out.push('\n');
        out.push_str(&self.orchestrator.format_status());
        out
    }

    pub fn emit(&self, event: ClusterEvent) {
        let _ = self.event_tx.send(event);
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic::DynamicTask;

    fn make_provider() -> Arc<dyn Provider> {
        #[derive(Clone)]
        struct MockProvider;
        #[async_trait::async_trait]
        impl Provider for MockProvider {
            fn clone_box(&self) -> Box<dyn Provider> {
                Box::new(MockProvider)
            }
            async fn chat(
                &self,
                _messages: &[crate::types::Message],
                _tools: &[crate::types::ToolDefinition],
                _tx: tokio::sync::mpsc::Sender<crate::types::ProviderEvent>,
            ) -> anyhow::Result<()> {
                anyhow::bail!("mock provider")
            }
        }
        Arc::new(MockProvider)
    }

    fn make_cluster() -> AgentCluster {
        let orch = DynamicOrchestrator::new(None);
        let config = SessionConfig::default();
        let provider = make_provider();
        let workspace = PathBuf::from(".");
        AgentCluster::new(orch, config, provider, workspace)
    }

    #[test]
    fn test_worker_slot_lifecycle() {
        let mut cluster = make_cluster();
        cluster.add_worker("w1", "coder");
        cluster.add_worker("w2", "reviewer");

        assert_eq!(cluster.idle_count(), 2);
        assert_eq!(cluster.running_count(), 0);

        // Simulate assignment
        if let Some(w) = cluster.workers.get_mut("w1") {
            w.state = WorkerState::Running {
                task_id: 1,
                agent_id: "w1".into(),
                started_at: 0,
            };
        }
        assert_eq!(cluster.idle_count(), 1);
        assert_eq!(cluster.running_count(), 1);
    }

    #[test]
    fn test_idle_worker_for_role() {
        let mut cluster = make_cluster();
        cluster.add_worker("w1", "coder");
        cluster.add_worker("w2", "reviewer");
        cluster.add_worker("w3", "coder");

        // Any idle
        assert!(cluster.idle_worker_for_role(None).is_some());

        // Role-specific
        let coder = cluster.idle_worker_for_role(Some("coder"));
        assert!(coder.is_some());
        let w = coder.unwrap();
        assert!(w == "w1" || w == "w3");

        // Non-existent role
        assert!(cluster.idle_worker_for_role(Some("tester")).is_none());
    }

    #[test]
    fn test_orchestrator_integration() {
        let mut orch = DynamicOrchestrator::new(None);
        orch.add_task(DynamicTask::new(1, "setup".into()));
        orch.add_task(DynamicTask::new(2, "build".into()).with_deps(vec![1]));

        let mut cluster = AgentCluster::new(
            orch,
            SessionConfig::default(),
            make_provider(),
            PathBuf::from("."),
        );
        cluster.add_worker("w1", "coder");

        // First tick: task 1 should become Ready
        let rt = tokio::runtime::Runtime::new().unwrap();
        let events = rt.block_on(cluster.tick());

        // Task 1 should have been assigned
        assert!(events.iter().any(|e| matches!(
            e,
            ClusterEvent::TaskAssigned { task_id: 1, .. }
        )));

        // Task 2 should still be Pending
        assert_eq!(cluster.orchestrator.tasks[&2].state, TaskState::Pending);
    }

    #[test]
    fn test_cluster_status_format() {
        let mut cluster = make_cluster();
        cluster.add_worker("w1", "coder");
        cluster.add_worker("w2", "reviewer");
        cluster.orchestrator.add_task(DynamicTask::new(1, "test".into()));

        let status = cluster.format_status();
        assert!(status.contains("Workers:"));
        assert!(status.contains("w1"));
        assert!(status.contains("coder"));
        assert!(status.contains("test"));
    }
}
