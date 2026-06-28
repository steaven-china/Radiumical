//! Orchestrator — task plan with dependency tracking.
//!
//! Ported from pi-agent extension. State is persisted to disk per session.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskStatus {
    Pending,
    Active,
    Done,
    Blocked,
    Skipped,
}

impl TaskStatus {
    pub fn icon(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "○",
            TaskStatus::Active => "◉",
            TaskStatus::Done => "✓",
            TaskStatus::Blocked => "⊘",
            TaskStatus::Skipped => "→",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Active => "active",
            TaskStatus::Done => "done",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: u32,
    pub title: String,
    pub status: TaskStatus,
    pub deps: Vec<u32>, // IDs of prerequisite tasks
    pub order: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Plan {
    pub title: String,
    pub tasks: Vec<Task>,
    pub next_id: u32,
}

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
            let _ = fs::create_dir_all(&dir);
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
                let _ = fs::write(path, json);
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
        let title = {
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

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

fn format_plan(plan: &Plan) -> String {
    let title = if plan.title.is_empty() {
        "".into()
    } else {
        format!("# {}\n\n", plan.title)
    };

    let stats = {
        let total = plan.tasks.len();
        let done = plan
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Done)
            .count();
        let active = plan
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Active)
            .count();
        let blocked = plan
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Blocked)
            .count();
        let mut parts = vec![format!("{done}/{total} done")];
        if active > 0 {
            parts.push(format!("{active} active"));
        }
        if blocked > 0 {
            parts.push(format!("{blocked} blocked"));
        }
        format!("progress: {}\n", parts.join(" · "))
    };

    let mut tasks: Vec<_> = plan.tasks.iter().collect();
    tasks.sort_by_key(|t| t.order);
    let lines: Vec<String> = tasks
        .into_iter()
        .map(|t| {
            let icon = t.status.icon();
            let label = t.status.label();
            let dep_str = if t.deps.is_empty() {
                "".into()
            } else {
                format!(
                    " ← deps: #{}",
                    t.deps
                        .iter()
                        .map(|d| d.to_string())
                        .collect::<Vec<_>>()
                        .join(", #")
                )
            };
            format!("  {icon} #{} [{}] {}{}", t.id, label, t.title, dep_str)
        })
        .collect();

    format!("{title}{stats}\n{}", lines.join("\n"))
}
