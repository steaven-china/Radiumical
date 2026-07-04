use serde::{Deserialize, Serialize};

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
    pub deps: Vec<u32>,
    pub order: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Plan {
    pub title: String,
    pub tasks: Vec<Task>,
    pub next_id: u32,
}
