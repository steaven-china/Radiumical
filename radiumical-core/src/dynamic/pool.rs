//! Persistent agent pool for long-running workers in the dynamic orchestrator.

use std::collections::HashMap;

use tokio::sync::mpsc;

/// Current status of a persistent agent worker.
pub enum AgentStatus {
    Idle,
    Working(u32),
    Draining,
}

/// A long-lived worker agent that processes tasks from the orchestrator.
pub struct PersistentAgent {
    pub id: String,
    pub role: String,
    pub status: AgentStatus,
    pub tasks_completed: u32,
    pub work_tx: mpsc::UnboundedSender<AgentWork>,
    pub result_rx: Option<mpsc::UnboundedReceiver<AgentResult>>,
}

/// A work item dispatched to a persistent agent.
pub struct AgentWork {
    pub task_id: u32,
    pub task_title: String,
    pub prompt: String,
}

/// The result returned by a persistent agent after completing work.
pub struct AgentResult {
    pub task_id: u32,
    pub success: bool,
    pub output: String,
}

/// Manages a set of persistent agent workers, tracking idle/busy state.
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
