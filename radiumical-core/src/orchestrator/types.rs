//! Core types for the simple orchestrator: task status, task, and plan.

use serde::{Deserialize, Serialize};

/// Status of a task in the orchestration plan.
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

/// A single task in the orchestration plan with optional dependencies and agent assignment.
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

/// An ordered list of tasks with a title and auto-incrementing ID counter.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Plan {
    pub title: String,
    pub tasks: Vec<Task>,
    pub next_id: u32,
}
