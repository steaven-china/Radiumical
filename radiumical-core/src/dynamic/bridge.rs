//! Conversion bridge between the simple [`Orchestrator`] and the [`DynamicOrchestrator`].

use super::task::{DynamicTask, TaskState};
use super::DynamicOrchestrator;
use crate::orchestrator::{Orchestrator, Plan, Task, TaskStatus};

impl TaskState {
    pub fn from_task_status(s: &TaskStatus) -> Self {
        match s {
            TaskStatus::Pending => TaskState::Pending,
            TaskStatus::Active => TaskState::Running,
            TaskStatus::Done => TaskState::Done,
            TaskStatus::Blocked => TaskState::Suspended,
            TaskStatus::Skipped => TaskState::Skipped,
        }
    }

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
    pub fn import_plan(&mut self, plan: &Plan) {
        for t in &plan.tasks {
            let mut dt = DynamicTask::new(t.id, t.title.clone()).with_deps(t.deps.clone());
            dt.state = TaskState::from_task_status(&t.status);
            dt.order = t.order;
            dt.agent = t.agent.clone();
            self.tasks.insert(t.id, dt);
        }
        if plan.next_id > self.next_id {
            self.next_id = plan.next_id;
        }
    }

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

impl Orchestrator {
    pub fn to_dynamic(&self) -> DynamicOrchestrator {
        let mut dyn_orch = DynamicOrchestrator::new(None);
        dyn_orch.import_plan(self.plan());
        dyn_orch
    }
}
