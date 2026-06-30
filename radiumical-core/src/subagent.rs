//! Sub-agent system — spawn parallel workers for independent tasks.
use crate::agent_pool::get_agent;
use crate::pipeline::PipelineRunner;
use crate::provider::Provider;
use crate::types::SessionConfig;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone)]
pub struct SubAgentResult {
    pub id: String,
    pub task: String,
    pub agent: Option<String>,
    pub output: String,
    pub done: bool,
    pub error: Option<String>,
}

fn registry() -> &'static Mutex<HashMap<String, SubAgentResult>> {
    static R: OnceLock<Mutex<HashMap<String, SubAgentResult>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

// Global defaults so SubAgentTool can spawn without main-thread wiring
static DEFAULT_CONFIG: OnceLock<Mutex<SessionConfig>> = OnceLock::new();
static DEFAULT_PROVIDER: OnceLock<Arc<dyn Provider>> = OnceLock::new();

/// Set global defaults for sub-agent spawning (call once at startup).
pub fn set_defaults(config: SessionConfig, provider: Arc<dyn Provider>) {
    let _ = DEFAULT_CONFIG.set(Mutex::new(config));
    let _ = DEFAULT_PROVIDER.set(provider);
}

fn get_defaults() -> Option<(SessionConfig, Arc<dyn Provider>)> {
    let config = DEFAULT_CONFIG.get()?.lock().ok()?.clone();
    let provider = DEFAULT_PROVIDER.get()?.clone();
    Some((config, provider))
}

/// Build a config for a specific agent role.
fn build_agent_config(base: &SessionConfig, agent_name: Option<&str>) -> SessionConfig {
    let mut config = base.clone();

    if let Some(name) = agent_name {
        if let Some(agent) = get_agent(name) {
            // Override system prompt with agent's prompt
            if !agent.prompt.is_empty() {
                config.system_prompt = agent.prompt.clone();
            }
            // Override mode
            config.mode = agent.mode.to_agent_mode();
        }
    }

    config
}

/// Spawn a sub-agent to handle a task asynchronously.
/// If `agent_name` is provided, loads the role definition from ~/.radi/agents/.
/// If `notify` is provided, sends `UiEvent::SubAgentDone` when the agent completes.
pub async fn spawn(
    id: String,
    task: String,
    agent_name: Option<String>,
    config: SessionConfig,
    provider: Arc<dyn Provider>,
    notify: Option<tokio::sync::mpsc::Sender<crate::types::UiEvent>>,
) {
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::channel::<crate::types::UiEvent>(256);
    let agent_config = build_agent_config(&config, agent_name.as_deref());
    let mut runner = PipelineRunner::new(agent_config, provider);
    let workspace = std::env::current_dir().unwrap_or_default();

    // Register as in-progress
    registry().lock().unwrap().insert(
        id.clone(),
        SubAgentResult {
            id: id.clone(),
            task: task.clone(),
            agent: agent_name,
            output: String::new(),
            done: false,
            error: None,
        },
    );

    tokio::spawn(async move {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let result = runner
            .run(task.clone(), workspace, &[], None, ui_tx, cancel_rx)
            .await;
        let success = result.is_ok();
        {
            let mut reg = registry().lock().unwrap();
            if let Some(entry) = reg.get_mut(&id) {
                entry.done = true;
                match result {
                    Ok(()) => entry.output = format!("Sub-agent '{id}' completed: {task}"),
                    Err(e) => {
                        entry.error = Some(e.to_string());
                        entry.output = format!("Sub-agent '{id}' failed: {}", e);
                    }
                }
            }
        }
        if let Some(tx) = notify {
            let _ = tx.send(crate::types::UiEvent::SubAgentDone {
                id: id.clone(),
                success,
            }).await;
        }
        drop(cancel_tx);
    });
}

/// Spawn using global defaults (for SubAgentTool when it can't access main context).
pub async fn spawn_with_defaults(
    id: String,
    task: String,
    agent_name: Option<String>,
) -> Result<(), String> {
    let (config, provider) =
        get_defaults().ok_or("Sub-agent defaults not set. Run from main loop.")?;
    spawn(id, task, agent_name, config, provider, None).await;
    Ok(())
}

/// List all sub-agents and their status.
pub fn list() -> String {
    let reg = registry().lock().unwrap();
    if reg.is_empty() {
        return "No sub-agents running.".into();
    }
    let mut out = String::from("Sub-agents:\n");
    for (id, r) in reg.iter() {
        let status = if r.done {
            if r.error.is_some() {
                "❌"
            } else {
                "✓"
            }
        } else {
            "⏳"
        };
        let role = r.agent.as_deref().unwrap_or("coder");
        out.push_str(&format!("  [{status}] {id} ({role}): {}\n", r.task));
    }
    out
}

/// List all sub-agents as structured data for panel rendering.
pub fn list_all() -> Vec<SubAgentResult> {
    let reg = registry().lock().unwrap();
    reg.values().cloned().collect()
}

/// Get result of a specific sub-agent.
#[allow(dead_code)]
pub fn get(id: &str) -> Option<SubAgentResult> {
    registry().lock().unwrap().get(id).cloned()
}
