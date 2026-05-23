//! Sub-agent system — spawn parallel workers for independent tasks.
use crate::tui::UiEvent;
use crate::conversation::Conversation;
use crate::pipeline::PipelineRunner;
use crate::provider::Provider;
use crate::types::SessionConfig;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone)]
pub struct SubAgentResult {
    pub id: String,
    pub task: String,
    pub output: String,
    pub done: bool,
    pub error: Option<String>,
}

fn registry() -> &'static Mutex<HashMap<String, SubAgentResult>> {
    static R: OnceLock<Mutex<HashMap<String, SubAgentResult>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Spawn a sub-agent to handle a task asynchronously.
pub async fn spawn(
    id: String,
    task: String,
    config: SessionConfig,
    provider: Arc<dyn Provider>,
) {
    let (ui_tx, _ui_rx) = std::sync::mpsc::channel::<crate::tui::UiEvent>();
    let mut runner = PipelineRunner::new(config.clone(), provider);
    let workspace = std::env::current_dir().unwrap_or_default();

    // Register as in-progress
    registry().lock().unwrap().insert(id.clone(), SubAgentResult {
        id: id.clone(), task: task.clone(), output: String::new(), done: false, error: None,
    });

    tokio::spawn(async move {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let result = runner.run(task.clone(), workspace, None, ui_tx, cancel_rx).await;
        let mut reg = registry().lock().unwrap();
        if let Some(entry) = reg.get_mut(&id) {
            entry.done = true;
            match result {
                Ok(()) => entry.output = format!("Sub-agent '{id}' completed: {task}"),
                Err(e) => { entry.error = Some(e.to_string()); entry.output = format!("Sub-agent '{id}' failed: {}", e); }
            }
        }
        drop(cancel_tx);
    });
}

/// List all sub-agents and their status.
pub fn list() -> String {
    let reg = registry().lock().unwrap();
    if reg.is_empty() { return "No sub-agents running.".into(); }
    let mut out = String::from("Sub-agents:\n");
    for (id, r) in reg.iter() {
        let status = if r.done { if r.error.is_some() { "❌" } else { "✓" } } else { "⏳" };
        out.push_str(&format!("  [{status}] {id}: {}\n", r.task));
    }
    out
}

/// Get result of a specific sub-agent.
pub fn get(id: &str) -> Option<SubAgentResult> {
    registry().lock().unwrap().get(id).cloned()
}
