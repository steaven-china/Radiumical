//! Orchestrator — simple linear task plan with dependency tracking.
//!
//! This is the **simple** orchestrator for straightforward sequential workflows.
//! Use this when you need: create plan → start task → done → auto-continue next.
//!
//! # When to use which orchestrator
//!
//! | Feature | `Orchestrator` (this) | `DynamicOrchestrator` |
//! |---------|----------------------|----------------------|
//! | Linear plan | ✓ | ✓ |
//! | Dependency tracking | ✓ | ✓ |
//! | Auto-continue | ✓ | ✓ |
//! | Conditional guards | — | ✓ |
//! | Lifecycle hooks | — | ✓ |
//! | Event bus | — | ✓ |
//! | Persistent tasks | — | ✓ |
//! | Retry on failure | — | ✓ |
//! | Sub-agent cluster | — | ✓ |
//!
//! **Use `Orchestrator`** for: simple task lists, `/plan` commands, the `orchestrate` tool.
//! **Use `DynamicOrchestrator`** for: reactive workflows, conditional triggers, the `cluster` tool.
//!
//! Convert between them with `Orchestrator::to_dynamic()` and `DynamicOrchestrator::export_plan()`.
//!
//! State is persisted to disk per session at `~/.radi/orchestrator/{workspace}.json`.

mod format;
mod types;

pub use types::{Plan, Task, TaskStatus};

use format::format_plan;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Orchestrator
// ---------------------------------------------------------------------------

pub struct Orchestrator {
    plan: Plan,
    state_path: Option<PathBuf>,
}

impl Orchestrator {
    pub fn new(session_name: Option<&str>) -> Self {
        let state_path = session_name.map(|name| {
            let dir = dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".radi")
                .join("orchestrator");
            if let Err(e) = fs::create_dir_all(&dir) {
                tracing::error!(error = %e, "failed to create orchestrator state directory");
            }
            dir.join(format!("{name}.json"))
        });

        let plan = state_path
            .as_ref()
            .and_then(|p| {
                fs::read_to_string(p)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
            })
            .unwrap_or_default();

        Self { plan, state_path }
    }

    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    pub fn is_empty(&self) -> bool {
        self.plan.tasks.is_empty()
    }

    fn save(&self) {
        if let Some(ref path) = self.state_path {
            if let Ok(json) = serde_json::to_string_pretty(&self.plan) {
                if let Err(e) = fs::write(path, json) {
                    tracing::error!(error = %e, "failed to save orchestrator state");
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Operations
    // -----------------------------------------------------------------------

    pub fn create(&mut self, title: &str, tasks: Vec<(String, Vec<u32>)>) -> String {
        self.plan.title = title.to_string();
        self.plan.tasks = tasks
            .into_iter()
            .enumerate()
            .map(|(i, (t, deps))| Task {
                id: (i + 1) as u32,
                title: t,
                status: TaskStatus::Pending,
                deps,
                order: i + 1,
                agent: None,
            })
            .collect();
        self.plan.next_id = self.plan.tasks.len() as u32 + 1;
        self.save();
        format!(
            "Plan created: {} ({} tasks)",
            self.plan.title,
            self.plan.tasks.len()
        )
    }

    /// Create a plan with agent assignments on tasks.
    pub fn create_with_agents(
        &mut self,
        title: &str,
        tasks: Vec<(String, Vec<u32>, Option<String>)>,
    ) -> String {
        self.plan.title = title.to_string();
        self.plan.tasks = tasks
            .into_iter()
            .enumerate()
            .map(|(i, (t, deps, agent))| Task {
                id: (i + 1) as u32,
                title: t,
                status: TaskStatus::Pending,
                deps,
                order: i + 1,
                agent,
            })
            .collect();
        self.plan.next_id = self.plan.tasks.len() as u32 + 1;
        self.save();
        format!(
            "Plan created: {} ({} tasks)",
            self.plan.title,
            self.plan.tasks.len()
        )
    }

    pub fn list(&self) -> String {
        if self.plan.tasks.is_empty() {
            return "No plan. Use orchestrate action='create' first.".into();
        }
        format_plan(&self.plan)
    }

    pub fn start(&mut self, id: u32) -> Result<String, String> {
        let task = self
            .plan
            .tasks
            .iter()
            .find(|t| t.id == id)
            .ok_or("Task not found")?;

        // Check deps
        let done_ids: HashSet<u32> = self
            .plan
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Done)
            .map(|t| t.id)
            .collect();
        let unmet: Vec<u32> = task
            .deps
            .iter()
            .copied()
            .filter(|d| !done_ids.contains(d))
            .collect();
        if !unmet.is_empty() {
            return Err(format!(
                "Cannot start #{}: dependencies #{} not done",
                id,
                unmet
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(", #")
            ));
        }

        // Deactivate other active tasks
        for t in &mut self.plan.tasks {
            if t.status == TaskStatus::Active {
                t.status = TaskStatus::Pending;
            }
        }

        let title = {
            let task = self.plan.tasks.iter_mut().find(|t| t.id == id).unwrap();
            task.status = TaskStatus::Active;
            task.title.clone()
        };
        self.save();
        Ok(format!(
            "▶ Started #{}: {}\n\n{}",
            id,
            title,
            format_plan(&self.plan)
        ))
    }

    pub fn done(&mut self, id: u32) -> Result<String, String> {
        let title = {
            let task = self
                .plan
                .tasks
                .iter_mut()
                .find(|t| t.id == id)
                .ok_or("Task not found")?;
            task.status = TaskStatus::Done;
            task.title.clone()
        };

        // Auto-start next ready task if nothing active
        let has_active = self
            .plan
            .tasks
            .iter()
            .any(|t| t.status == TaskStatus::Active);
        if !has_active {
            let next_id = self.get_ready_tasks().first().map(|t| t.id);
            if let Some(next_id) = next_id {
                if let Some(t) = self.plan.tasks.iter_mut().find(|t| t.id == next_id) {
                    t.status = TaskStatus::Active;
                }
            }
        }

        self.save();
        Ok(format!(
            "✓ Done #{}: {}\n\n{}",
            id,
            title,
            format_plan(&self.plan)
        ))
    }

    pub fn block(&mut self, id: u32, reason: Option<&str>) -> Result<String, String> {
        let _title = {
            let task = self
                .plan
                .tasks
                .iter_mut()
                .find(|t| t.id == id)
                .ok_or("Task not found")?;
            task.status = TaskStatus::Blocked;
            task.title.clone()
        };
        self.save();
        let reason_str = reason.map(|r| format!(": {}", r)).unwrap_or_default();
        Ok(format!(
            "⊘ Blocked #{}{}\n\n{}",
            id,
            reason_str,
            format_plan(&self.plan)
        ))
    }

    pub fn skip(&mut self, id: u32) -> Result<String, String> {
        let title = {
            let task = self
                .plan
                .tasks
                .iter_mut()
                .find(|t| t.id == id)
                .ok_or("Task not found")?;
            task.status = TaskStatus::Skipped;
            task.title.clone()
        };
        self.save();
        Ok(format!(
            "→ Skipped #{}: {}\n\n{}",
            id,
            title,
            format_plan(&self.plan)
        ))
    }

    pub fn add(&mut self, tasks: Vec<(String, Vec<u32>)>) -> Result<String, String> {
        let start_id = self.plan.next_id;
        let count = tasks.len();
        let new_tasks: Vec<Task> = tasks
            .into_iter()
            .enumerate()
            .map(|(i, (t, deps))| Task {
                id: start_id + i as u32,
                title: t,
                status: TaskStatus::Pending,
                deps,
                order: self.plan.tasks.len() + i + 1,
                agent: None,
            })
            .collect();

        self.plan.tasks.extend(new_tasks);
        self.plan.next_id = start_id + count as u32;
        self.save();
        Ok(format!(
            "Added {} task(s).\n\n{}",
            count,
            format_plan(&self.plan)
        ))
    }

    /// Add tasks with agent assignments.
    pub fn add_with_agents(
        &mut self,
        tasks: Vec<(String, Vec<u32>, Option<String>)>,
    ) -> Result<String, String> {
        let start_id = self.plan.next_id;
        let count = tasks.len();
        let new_tasks: Vec<Task> = tasks
            .into_iter()
            .enumerate()
            .map(|(i, (t, deps, agent))| Task {
                id: start_id + i as u32,
                title: t,
                status: TaskStatus::Pending,
                deps,
                order: self.plan.tasks.len() + i + 1,
                agent,
            })
            .collect();

        self.plan.tasks.extend(new_tasks);
        self.plan.next_id = start_id + count as u32;
        self.save();
        Ok(format!(
            "Added {} task(s).\n\n{}",
            count,
            format_plan(&self.plan)
        ))
    }

    pub fn remove(&mut self, id: u32) -> Result<String, String> {
        let title = {
            let removed = self
                .plan
                .tasks
                .iter()
                .find(|t| t.id == id)
                .ok_or("Task not found")?;
            removed.title.clone()
        };
        self.plan.tasks.retain(|t| t.id != id);
        // Remove deps pointing to this task
        for t in &mut self.plan.tasks {
            t.deps.retain(|d| *d != id);
        }
        self.save();
        Ok(format!(
            "Removed #{}: {}\n\n{}",
            id,
            title,
            format_plan(&self.plan)
        ))
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    pub fn get_ready_tasks(&self) -> Vec<&Task> {
        let done_ids: HashSet<u32> = self
            .plan
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Done)
            .map(|t| t.id)
            .collect();
        self.plan
            .tasks
            .iter()
            .filter(|t| {
                t.status == TaskStatus::Pending && t.deps.iter().all(|d| done_ids.contains(d))
            })
            .collect()
    }

    pub fn get_active_tasks(&self) -> Vec<&Task> {
        self.plan
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Active)
            .collect()
    }

    pub fn get_blocked_tasks(&self) -> Vec<&Task> {
        self.plan
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Blocked)
            .collect()
    }
}

/// Quick helper: load plan for workspace and return context prompt.
pub fn get_context_for_workspace(workspace: &str) -> Option<String> {
    let orch = Orchestrator::new(Some(workspace));
    orch.build_context_prompt()
}

impl Orchestrator {
    pub fn build_context_prompt(&self) -> Option<String> {
        if self.plan.tasks.is_empty() {
            return None;
        }

        let active: Vec<String> = self
            .get_active_tasks()
            .into_iter()
            .map(|t| format!("#{} {}", t.id, t.title))
            .collect();
        let ready: Vec<String> = self
            .get_ready_tasks()
            .into_iter()
            .map(|t| format!("#{} {}", t.id, t.title))
            .collect();
        let blocked: Vec<String> = self
            .get_blocked_tasks()
            .into_iter()
            .map(|t| format!("#{} {}", t.id, t.title))
            .collect();
        let pending_deps: Vec<String> = self
            .plan
            .tasks
            .iter()
            .filter(|t| {
                if t.status != TaskStatus::Pending {
                    return false;
                }
                let done_ids: HashSet<u32> = self
                    .plan
                    .tasks
                    .iter()
                    .filter(|x| x.status == TaskStatus::Done)
                    .map(|x| x.id)
                    .collect();
                !t.deps.iter().all(|d| done_ids.contains(d))
            })
            .map(|t| format!("#{} {}", t.id, t.title))
            .collect();

        let mut parts = vec!["[Current Orchestration Plan]".to_string()];
        if !active.is_empty() {
            parts.push(format!("active: {}", active.join(", ")));
        }
        if !ready.is_empty() {
            parts.push(format!("ready: {}", ready.join(", ")));
        }
        if !pending_deps.is_empty() {
            parts.push(format!("pending (deps unmet): {}", pending_deps.join(", ")));
        }
        if !blocked.is_empty() {
            parts.push(format!("blocked: {}", blocked.join(", ")));
        }
        parts.push("Use orchestrate tool to advance the plan (start/done/block/skip).".to_string());

        Some(parts.join("\n"))
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_orch() -> Orchestrator {
        Orchestrator::new(None)
    }

    fn make_orch_with_plan() -> Orchestrator {
        let mut orch = make_orch();
        orch.create(
            "Test Plan",
            vec![
                ("Task 1".to_string(), vec![]),
                ("Task 2".to_string(), vec![1]),
                ("Task 3".to_string(), vec![]),
            ],
        );
        orch
    }

    // ─── 1. Plan creation ───────────────────────────────────────────────

    #[test]
    fn test_create_sets_title_and_next_id() {
        let orch = make_orch_with_plan();
        let plan = orch.plan();
        assert_eq!(plan.title, "Test Plan");
        assert_eq!(plan.next_id, 4);
    }

    #[test]
    fn test_create_assigns_ids_sequentially() {
        let orch = make_orch_with_plan();
        let plan = orch.plan();
        assert_eq!(plan.tasks[0].id, 1);
        assert_eq!(plan.tasks[1].id, 2);
        assert_eq!(plan.tasks[2].id, 3);
    }

    #[test]
    fn test_create_assigns_order_sequentially() {
        let orch = make_orch_with_plan();
        let plan = orch.plan();
        assert_eq!(plan.tasks[0].order, 1);
        assert_eq!(plan.tasks[1].order, 2);
        assert_eq!(plan.tasks[2].order, 3);
    }

    #[test]
    fn test_create_stores_dependencies() {
        let orch = make_orch_with_plan();
        let plan = orch.plan();
        assert!(plan.tasks[0].deps.is_empty());
        assert_eq!(plan.tasks[1].deps, vec![1]);
        assert!(plan.tasks[2].deps.is_empty());
    }

    #[test]
    fn test_create_all_tasks_start_pending() {
        let orch = make_orch_with_plan();
        for t in &orch.plan().tasks {
            assert_eq!(t.status, TaskStatus::Pending);
        }
    }

    #[test]
    fn test_create_empty_tasks() {
        let mut orch = make_orch();
        orch.create("Empty", vec![]);
        assert!(orch.is_empty());
        assert_eq!(orch.plan().next_id, 1);
    }

    #[test]
    fn test_is_empty() {
        let orch = make_orch();
        assert!(orch.is_empty());
        let orch = make_orch_with_plan();
        assert!(!orch.is_empty());
    }

    #[test]
    fn test_list_empty_plan() {
        let orch = make_orch();
        let out = orch.list();
        assert!(out.contains("No plan"));
    }

    #[test]
    fn test_list_shows_tasks() {
        let orch = make_orch_with_plan();
        let out = orch.list();
        assert!(out.contains("Test Plan"));
        assert!(out.contains("Task 1"));
        assert!(out.contains("Task 2"));
        assert!(out.contains("← deps: #1"));
    }

    // ─── 2. start() ─────────────────────────────────────────────────────

    #[test]
    fn test_start_blocks_on_unmet_deps() {
        let mut orch = make_orch_with_plan();
        let result = orch.start(2);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Cannot start #2"));
        assert!(err.contains("dependencies #1 not done"));
        let t2 = orch.plan().tasks.iter().find(|t| t.id == 2).unwrap();
        assert_eq!(t2.status, TaskStatus::Pending);
    }

    #[test]
    fn test_start_succeeds_when_deps_met() {
        let mut orch = make_orch();
        orch.create(
            "P",
            vec![
                ("A".to_string(), vec![]),
                ("B".to_string(), vec![1]),
                ("C".to_string(), vec![1]),
            ],
        );
        orch.start(1).unwrap();
        orch.done(1).unwrap();

        let result = orch.start(3);
        assert!(result.is_ok());
        let t3 = orch.plan().tasks.iter().find(|t| t.id == 3).unwrap();
        assert_eq!(t3.status, TaskStatus::Active);
    }

    #[test]
    fn test_start_deactivates_previous_active() {
        let mut orch = make_orch();
        orch.create(
            "P",
            vec![("A".to_string(), vec![]), ("B".to_string(), vec![])],
        );
        orch.start(1).unwrap();
        orch.start(2).unwrap();

        let t1 = orch.plan().tasks.iter().find(|t| t.id == 1).unwrap();
        let t2 = orch.plan().tasks.iter().find(|t| t.id == 2).unwrap();
        assert_eq!(t1.status, TaskStatus::Pending);
        assert_eq!(t2.status, TaskStatus::Active);
    }

    #[test]
    fn test_start_nonexistent_task() {
        let mut orch = make_orch_with_plan();
        let result = orch.start(99);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Task not found");
    }

    // ─── 3. done() ──────────────────────────────────────────────────────

    #[test]
    fn test_done_marks_task_done() {
        let mut orch = make_orch_with_plan();
        orch.start(1).unwrap();
        orch.done(1).unwrap();
        let t1 = orch.plan().tasks.iter().find(|t| t.id == 1).unwrap();
        assert_eq!(t1.status, TaskStatus::Done);
    }

    #[test]
    fn test_done_auto_starts_next_ready() {
        let mut orch = make_orch();
        orch.create(
            "P",
            vec![("A".to_string(), vec![]), ("B".to_string(), vec![1])],
        );
        orch.start(1).unwrap();

        orch.done(1).unwrap();

        let t2 = orch.plan().tasks.iter().find(|t| t.id == 2).unwrap();
        assert_eq!(t2.status, TaskStatus::Active);
    }

    #[test]
    fn test_done_no_auto_start_when_active_exists() {
        let mut orch = make_orch();
        orch.create(
            "P",
            vec![("A".to_string(), vec![]), ("B".to_string(), vec![1])],
        );
        orch.start(1).unwrap();

        orch.done(2).unwrap();

        let t2 = orch.plan().tasks.iter().find(|t| t.id == 2).unwrap();
        assert_eq!(t2.status, TaskStatus::Done);
        let t1 = orch.plan().tasks.iter().find(|t| t.id == 1).unwrap();
        assert_eq!(t1.status, TaskStatus::Active);
    }

    #[test]
    fn test_done_multiple_ready_starts_first_by_order() {
        let mut orch = make_orch();
        orch.create(
            "P",
            vec![
                ("A".to_string(), vec![]),
                ("B".to_string(), vec![1]),
                ("C".to_string(), vec![1]),
            ],
        );
        orch.start(1).unwrap();
        orch.done(1).unwrap();

        let t2 = orch.plan().tasks.iter().find(|t| t.id == 2).unwrap();
        let t3 = orch.plan().tasks.iter().find(|t| t.id == 3).unwrap();
        assert_eq!(t2.status, TaskStatus::Active);
        assert_eq!(t3.status, TaskStatus::Pending);
    }

    #[test]
    fn test_done_nonexistent_task() {
        let mut orch = make_orch_with_plan();
        let result = orch.done(99);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Task not found");
    }

    // ─── 4. block() / skip() ────────────────────────────────────────────

    #[test]
    fn test_block_sets_status() {
        let mut orch = make_orch_with_plan();
        orch.block(1, None).unwrap();
        let t1 = orch.plan().tasks.iter().find(|t| t.id == 1).unwrap();
        assert_eq!(t1.status, TaskStatus::Blocked);
    }

    #[test]
    fn test_block_with_reason() {
        let mut orch = make_orch_with_plan();
        let out = orch.block(2, Some("need more info")).unwrap();
        assert!(out.contains("need more info"));
        let t2 = orch.plan().tasks.iter().find(|t| t.id == 2).unwrap();
        assert_eq!(t2.status, TaskStatus::Blocked);
    }

    #[test]
    fn test_block_nonexistent_task() {
        let mut orch = make_orch_with_plan();
        let result = orch.block(99, None);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Task not found");
    }

    #[test]
    fn test_skip_sets_status() {
        let mut orch = make_orch_with_plan();
        orch.skip(1).unwrap();
        let t1 = orch.plan().tasks.iter().find(|t| t.id == 1).unwrap();
        assert_eq!(t1.status, TaskStatus::Skipped);
    }

    #[test]
    fn test_skip_output_contains_title() {
        let mut orch = make_orch_with_plan();
        let out = orch.skip(2).unwrap();
        assert!(out.contains("Task 2"));
        assert!(out.contains("Skipped"));
    }

    #[test]
    fn test_skip_nonexistent_task() {
        let mut orch = make_orch_with_plan();
        let result = orch.skip(99);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Task not found");
    }

    // ─── 5. get_ready_tasks() ────────────────────────────────────────────

    #[test]
    fn test_get_ready_tasks_returns_tasks_with_deps_met() {
        let mut orch = make_orch();
        orch.create(
            "P",
            vec![
                ("No deps".to_string(), vec![]),
                ("Dep on 1".to_string(), vec![1]),
            ],
        );
        let ready: Vec<u32> = orch.get_ready_tasks().iter().map(|t| t.id).collect();
        assert_eq!(ready, vec![1]);

        orch.start(1).unwrap();
        orch.done(1).unwrap();

        let t2 = orch.plan().tasks.iter().find(|t| t.id == 2).unwrap();
        assert_eq!(t2.status, TaskStatus::Active);
        let ready: Vec<u32> = orch.get_ready_tasks().iter().map(|t| t.id).collect();
        assert!(ready.is_empty());
    }

    #[test]
    fn test_get_ready_tasks_excludes_unmet_deps() {
        let mut orch = make_orch();
        orch.create(
            "P",
            vec![("A".to_string(), vec![]), ("B".to_string(), vec![2])],
        );
        let ready_ids: Vec<u32> = orch.get_ready_tasks().iter().map(|t| t.id).collect();
        assert!(ready_ids.contains(&1));
        assert!(!ready_ids.contains(&2));
    }

    #[test]
    fn test_get_ready_tasks_excludes_non_pending() {
        let mut orch = make_orch();
        orch.create(
            "P",
            vec![
                ("A".to_string(), vec![]),
                ("B".to_string(), vec![]),
                ("C".to_string(), vec![]),
                ("D".to_string(), vec![]),
            ],
        );
        orch.start(1).unwrap();
        orch.block(2, None).unwrap();
        orch.skip(3).unwrap();
        orch.start(4).unwrap();
        orch.done(4).unwrap();

        let ready: Vec<u32> = orch.get_ready_tasks().iter().map(|t| t.id).collect();
        assert!(ready.is_empty());
    }

    #[test]
    fn test_get_ready_tasks_skipped_is_not_done_for_deps() {
        let mut orch = make_orch();
        orch.create(
            "P",
            vec![("A".to_string(), vec![]), ("B".to_string(), vec![1])],
        );
        orch.skip(1).unwrap(); // Skipped ≠ Done

        let ready: Vec<u32> = orch.get_ready_tasks().iter().map(|t| t.id).collect();
        assert!(!ready.contains(&2));
    }

    // ─── 6. add() / remove() ────────────────────────────────────────────

    #[test]
    fn test_add_assigns_ids_from_next_id() {
        let mut orch = make_orch_with_plan();
        orch.add(vec![
            ("Task 4".to_string(), vec![]),
            ("Task 5".to_string(), vec![1, 3]),
        ])
        .unwrap();

        let plan = orch.plan();
        assert_eq!(plan.tasks.len(), 5);
        assert_eq!(plan.next_id, 6);

        let t4 = plan.tasks.iter().find(|t| t.id == 4).unwrap();
        assert_eq!(t4.title, "Task 4");
        assert_eq!(t4.order, 4);

        let t5 = plan.tasks.iter().find(|t| t.id == 5).unwrap();
        assert_eq!(t5.title, "Task 5");
        assert_eq!(t5.deps, vec![1, 3]);
        assert_eq!(t5.order, 5);
        assert_eq!(t5.status, TaskStatus::Pending);
    }

    #[test]
    fn test_add_preserves_existing_tasks() {
        let mut orch = make_orch_with_plan();
        orch.add(vec![("Task 4".to_string(), vec![])]).unwrap();

        let t1 = orch.plan().tasks.iter().find(|t| t.id == 1).unwrap();
        assert_eq!(t1.title, "Task 1");
        assert_eq!(t1.status, TaskStatus::Pending);
    }

    #[test]
    fn test_remove_deletes_task() {
        let mut orch = make_orch_with_plan();
        orch.remove(2).unwrap();
        assert!(orch.plan().tasks.iter().find(|t| t.id == 2).is_none());
        assert_eq!(orch.plan().tasks.len(), 2);
    }

    #[test]
    fn test_remove_cleans_deps_pointing_to_removed_task() {
        let mut orch = make_orch();
        orch.create(
            "P",
            vec![
                ("A".to_string(), vec![]),
                ("B".to_string(), vec![1]),
                ("C".to_string(), vec![1, 2]),
            ],
        );

        orch.remove(1).unwrap();

        let tb = orch.plan().tasks.iter().find(|t| t.id == 2).unwrap();
        assert!(!tb.deps.contains(&1));

        let tc = orch.plan().tasks.iter().find(|t| t.id == 3).unwrap();
        assert!(!tc.deps.contains(&1));
        assert!(tc.deps.contains(&2));
    }

    #[test]
    fn test_remove_nonexistent_task() {
        let mut orch = make_orch_with_plan();
        let result = orch.remove(99);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Task not found");
    }

    // ─── 7. build_context_prompt() ──────────────────────────────────────

    #[test]
    fn test_build_context_prompt_empty() {
        let orch = make_orch();
        assert!(orch.build_context_prompt().is_none());
    }

    #[test]
    fn test_build_context_prompt_has_header_and_footer() {
        let orch = make_orch_with_plan();
        let prompt = orch.build_context_prompt().unwrap();
        assert!(prompt.starts_with("[Current Orchestration Plan]"));
        assert!(prompt.contains("Use orchestrate tool to advance the plan"));
    }

    #[test]
    fn test_build_context_prompt_lists_ready() {
        let orch = make_orch_with_plan();
        let prompt = orch.build_context_prompt().unwrap();
        assert!(prompt.contains("ready:"));
        assert!(prompt.contains("#1 Task 1"));
        assert!(prompt.contains("#3 Task 3"));
    }

    #[test]
    fn test_build_context_prompt_lists_pending_with_unmet_deps() {
        let orch = make_orch_with_plan();
        let prompt = orch.build_context_prompt().unwrap();
        assert!(prompt.contains("pending (deps unmet):"));
        assert!(prompt.contains("#2 Task 2"));
    }

    #[test]
    fn test_build_context_prompt_lists_active() {
        let mut orch = make_orch_with_plan();
        orch.start(1).unwrap();
        let prompt = orch.build_context_prompt().unwrap();
        assert!(prompt.contains("active:"));
        assert!(prompt.contains("#1 Task 1"));
    }

    #[test]
    fn test_build_context_prompt_lists_blocked() {
        let mut orch = make_orch_with_plan();
        orch.block(1, None).unwrap();
        let prompt = orch.build_context_prompt().unwrap();
        assert!(prompt.contains("blocked:"));
        assert!(prompt.contains("#1 Task 1"));
    }

    // ─── 8. save / load persistence ─────────────────────────────────────

    #[test]
    fn test_save_and_load_roundtrip() {
        let session = "__test_orch_save_load__";
        let test_file = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".radi")
            .join("orchestrator")
            .join(format!("{session}.json"));

        let _ = fs::remove_file(&test_file);

        {
            let mut orch = Orchestrator::new(Some(session));
            orch.create(
                "Persistent",
                vec![("Alpha".to_string(), vec![]), ("Beta".to_string(), vec![1])],
            );
            orch.start(1).unwrap();
            orch.done(1).unwrap();
        }

        {
            let orch = Orchestrator::new(Some(session));
            let plan = orch.plan();
            assert_eq!(plan.title, "Persistent");
            assert_eq!(plan.tasks.len(), 2);
            assert_eq!(plan.tasks[0].id, 1);
            assert_eq!(plan.tasks[0].title, "Alpha");
            assert_eq!(plan.tasks[0].status, TaskStatus::Done);
            assert_eq!(plan.tasks[1].id, 2);
            assert_eq!(plan.tasks[1].title, "Beta");
            assert_eq!(plan.tasks[1].deps, vec![1]);
            assert_eq!(plan.tasks[1].status, TaskStatus::Active);
        }

        let _ = fs::remove_file(&test_file);
    }

    #[test]
    fn test_no_session_name_no_persistence() {
        let mut orch = make_orch();
        orch.create("Ephemeral", vec![("X".to_string(), vec![])]);
        drop(orch);

        let orch2 = make_orch();
        assert!(orch2.is_empty());
        assert_ne!(orch2.plan().title, "Ephemeral");
    }

    // ─── 9. Edge cases & task-status helpers ────────────────────────────

    #[test]
    fn test_task_status_icon_values() {
        assert_eq!(TaskStatus::Pending.icon(), "○");
        assert_eq!(TaskStatus::Active.icon(), "◉");
        assert_eq!(TaskStatus::Done.icon(), "✓");
        assert_eq!(TaskStatus::Blocked.icon(), "⊘");
        assert_eq!(TaskStatus::Skipped.icon(), "→");
    }

    #[test]
    fn test_task_status_label_values() {
        assert_eq!(TaskStatus::Pending.label(), "pending");
        assert_eq!(TaskStatus::Active.label(), "active");
        assert_eq!(TaskStatus::Done.label(), "done");
        assert_eq!(TaskStatus::Blocked.label(), "blocked");
        assert_eq!(TaskStatus::Skipped.label(), "skipped");
    }

    #[test]
    fn test_start_on_already_active_task_reactivates() {
        let mut orch = make_orch_with_plan();
        orch.start(1).unwrap();
        orch.start(1).unwrap();
        let t1 = orch.plan().tasks.iter().find(|t| t.id == 1).unwrap();
        assert_eq!(t1.status, TaskStatus::Active);
    }

    #[test]
    fn test_done_chain_resolves_dependencies() {
        let mut orch = make_orch();
        orch.create(
            "Chain",
            vec![
                ("A".to_string(), vec![]),
                ("B".to_string(), vec![1]),
                ("C".to_string(), vec![2]),
            ],
        );

        orch.start(1).unwrap();
        orch.done(1).unwrap();

        let t2 = orch.plan().tasks.iter().find(|t| t.id == 2).unwrap();
        assert_eq!(t2.status, TaskStatus::Active);

        orch.done(2).unwrap();

        let t2 = orch.plan().tasks.iter().find(|t| t.id == 2).unwrap();
        assert_eq!(t2.status, TaskStatus::Done);
        let t3 = orch.plan().tasks.iter().find(|t| t.id == 3).unwrap();
        assert_eq!(t3.status, TaskStatus::Active);
    }

    #[test]
    fn test_get_active_tasks() {
        let mut orch = make_orch_with_plan();
        assert!(orch.get_active_tasks().is_empty());
        orch.start(1).unwrap();
        let active = orch.get_active_tasks();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, 1);
    }

    #[test]
    fn test_get_blocked_tasks() {
        let mut orch = make_orch_with_plan();
        assert!(orch.get_blocked_tasks().is_empty());
        orch.block(2, None).unwrap();
        let blocked = orch.get_blocked_tasks();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].id, 2);
    }

    #[test]
    fn test_create_replaces_previous_plan() {
        let mut orch = make_orch();
        orch.create("First", vec![("X".to_string(), vec![])]);
        assert_eq!(orch.plan().tasks.len(), 1);

        orch.create(
            "Second",
            vec![("A".to_string(), vec![]), ("B".to_string(), vec![])],
        );
        assert_eq!(orch.plan().tasks.len(), 2);
        assert_eq!(orch.plan().title, "Second");
        assert_eq!(orch.plan().next_id, 3);
    }

    #[test]
    fn test_remove_clears_all_deps_across_multiple_tasks() {
        let mut orch = make_orch();
        orch.create(
            "P",
            vec![
                ("A".to_string(), vec![]),
                ("B".to_string(), vec![1]),
                ("C".to_string(), vec![1]),
                ("D".to_string(), vec![1, 2]),
            ],
        );

        orch.remove(1).unwrap();

        for t in &orch.plan().tasks {
            assert!(!t.deps.contains(&1));
        }
    }

    #[test]
    fn test_plan_default_is_empty() {
        let plan = Plan::default();
        assert!(plan.title.is_empty());
        assert!(plan.tasks.is_empty());
        assert_eq!(plan.next_id, 0);
    }

    #[test]
    fn test_task_status_partial_eq() {
        assert_eq!(TaskStatus::Pending, TaskStatus::Pending);
        assert_ne!(TaskStatus::Pending, TaskStatus::Done);
    }
}
