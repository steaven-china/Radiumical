//! Sub-agent system — spawn parallel workers for independent tasks.
//!
//! Sub-agents run in isolated tokio tasks with their own Harness instance.
//! Results flow back via `tokio::sync::watch` channels, allowing the main
//! agent to await completion via the `subagent_wait` tool.

use crate::agent_pool::get_agent;
use crate::pipeline::PipelineRunner;
use crate::provider::Provider;
use crate::types::{SessionConfig, UiEvent};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::watch;

#[derive(Debug, Clone)]
pub struct SubAgentResult {
    pub id: String,
    pub task: String,
    pub agent: Option<String>,
    pub output: String,
    pub done: bool,
    pub success: bool,
    pub error: Option<String>,
}

/// Entry in the sub-agent registry — holds result + watch channel.
struct RegistryEntry {
    result: SubAgentResult,
    /// Watch channel: sends `true` when done. Receivers can block until done.
    done_tx: watch::Sender<bool>,
    /// Cancel sender — external code can send `true` to cancel this sub-agent.
    cancel_tx: tokio::sync::watch::Sender<bool>,
    /// Snapshot of all UI events (chunks/errors) produced by this sub-agent.
    /// Stored so the main agent can retrieve the full output after waiting.
    output_buffer: Vec<String>,
}

fn registry() -> &'static Mutex<HashMap<String, RegistryEntry>> {
    static R: OnceLock<Mutex<HashMap<String, RegistryEntry>>> = OnceLock::new();
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
            if !agent.prompt.is_empty() {
                config.system_prompt = agent.prompt.clone();
            }
            config.mode = agent.mode.to_agent_mode();
        }
    }

    config
}

/// A handle to a spawned sub-agent. Can be used to await completion.
#[derive(Clone)]
pub struct SubAgentHandle {
    pub id: String,
    done_rx: watch::Receiver<bool>,
}

impl SubAgentHandle {
    /// Block until the sub-agent completes. Returns the final result.
    pub async fn wait(&mut self) -> SubAgentResult {
        // Wait until the watch channel signals done=true.
        let _ = self.done_rx.wait_for(|done| *done).await;
        get_result(&self.id).unwrap_or(SubAgentResult {
            id: self.id.clone(),
            task: String::new(),
            agent: None,
            output: "Sub-agent result lost".into(),
            done: true,
            success: false,
            error: Some("Result not found in registry".into()),
        })
    }

    /// Check if done without blocking.
    pub fn is_done(&self) -> bool {
        *self.done_rx.borrow()
    }
}

/// Spawn a sub-agent to handle a task asynchronously.
/// Returns a handle that can be used to await the result.
pub async fn spawn(
    id: String,
    task: String,
    agent_name: Option<String>,
    config: SessionConfig,
    provider: Arc<dyn Provider>,
    notify: Option<tokio::sync::mpsc::Sender<UiEvent>>,
) -> SubAgentHandle {
    let (ui_tx, mut ui_rx) = tokio::sync::mpsc::channel::<UiEvent>(256);
    let agent_config = build_agent_config(&config, agent_name.as_deref());
    let mut runner = PipelineRunner::new(agent_config, provider);
    let workspace = std::env::current_dir().unwrap_or_default();

    let (done_tx, done_rx) = watch::channel(false);
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    // Register as in-progress
    registry().lock().unwrap().insert(
        id.clone(),
        RegistryEntry {
            result: SubAgentResult {
                id: id.clone(),
                task: task.clone(),
                agent: agent_name,
                output: String::new(),
                done: false,
                success: false,
                error: None,
            },
            done_tx: done_tx.clone(),
            cancel_tx: cancel_tx.clone(),
            output_buffer: Vec::new(),
        },
    );

    // Drain UI events into the output buffer in the background.
    let id_clone = id.clone();
    let buffer_handle = tokio::spawn(async move {
        while let Some(event) = ui_rx.recv().await {
            match &event {
                UiEvent::LlmChunk(chunk) => {
                    if let Ok(mut reg) = registry().lock() {
                        if let Some(entry) = reg.get_mut(&id_clone) {
                            entry.output_buffer.push(chunk.clone());
                        }
                    }
                }
                UiEvent::Error(e) => {
                    if let Ok(mut reg) = registry().lock() {
                        if let Some(entry) = reg.get_mut(&id_clone) {
                            entry.output_buffer.push(format!("ERROR: {e}"));
                        }
                    }
                }
                _ => {}
            }
        }
    });

    let id_for_task = id.clone();
    tokio::spawn(async move {
        let result = runner
            .run(task.clone(), workspace, &[], None, ui_tx, cancel_rx)
            .await;
        let success = result.is_ok();

        // Wait for buffer drain to finish
        let _ = buffer_handle.await;

        // Collect output from buffer
        let output_text = {
            let reg = registry().lock().unwrap();
            reg.get(&id_for_task)
                .map(|e| e.output_buffer.join(""))
                .unwrap_or_default()
        };

        {
            let mut reg = registry().lock().unwrap();
            if let Some(entry) = reg.get_mut(&id_for_task) {
                entry.result.done = true;
                entry.result.success = success;
                match result {
                    Ok(()) => {
                        entry.result.output = if output_text.is_empty() {
                            format!("Sub-agent '{id_for_task}' completed: {task}")
                        } else {
                            output_text
                        };
                    }
                    Err(e) => {
                        entry.result.error = Some(e.to_string());
                        entry.result.output = if output_text.is_empty() {
                            format!("Sub-agent '{id_for_task}' failed: {e}")
                        } else {
                            format!("{output_text}\n[Failed: {e}]")
                        };
                    }
                }
                let _ = entry.done_tx.send(true);
            }
        }
        if let Some(tx) = notify {
            if let Err(e) = tx
                .send(UiEvent::SubAgentDone {
                    id: id_for_task.clone(),
                    success,
                })
                .await
            {
                tracing::warn!(error = %e, subagent_id = %id_for_task, "failed to send SubAgentDone");
            }
        }
    });

    SubAgentHandle { id, done_rx }
}

/// Spawn using global defaults (for SubAgentTool when it can't access main context).
pub async fn spawn_with_defaults(
    id: String,
    task: String,
    agent_name: Option<String>,
) -> Result<SubAgentHandle, String> {
    let (config, provider) =
        get_defaults().ok_or("Sub-agent defaults not set. Run from main loop.")?;
    let handle = spawn(id, task, agent_name, config, provider, None).await;
    Ok(handle)
}

/// Wait for a sub-agent by ID. Blocks until it completes. Returns output.
pub async fn wait_for(id: &str) -> Result<SubAgentResult, String> {
    let rx = {
        let reg = registry().lock().unwrap();
        reg.get(id)
            .map(|e| e.done_tx.subscribe())
            .ok_or(format!("Sub-agent '{id}' not found"))?
    };

    let mut done_rx = rx;
    let _ = done_rx.wait_for(|done| *done).await;

    get_result(id).ok_or(format!("Sub-agent '{id}' result lost"))
}

/// Get result of a specific sub-agent (non-blocking).
pub fn get_result(id: &str) -> Option<SubAgentResult> {
    registry().lock().unwrap().get(id).map(|e| e.result.clone())
}

/// List all sub-agents and their status (for tool output).
pub fn list() -> String {
    let reg = registry().lock().unwrap();
    if reg.is_empty() {
        return "No sub-agents running.".into();
    }
    let mut out = String::from("Sub-agents:\n");
    for (id, entry) in reg.iter() {
        let r = &entry.result;
        let status = if r.done {
            if r.success {
                "✓"
            } else {
                "❌"
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
    reg.values().map(|e| e.result.clone()).collect()
}

/// Get result of a specific sub-agent (legacy compat).
#[allow(dead_code)]
pub fn get(id: &str) -> Option<SubAgentResult> {
    get_result(id)
}

/// Remove completed sub-agents from the registry (cleanup).
pub fn cleanup_done() {
    let mut reg = registry().lock().unwrap();
    reg.retain(|_, entry| !entry.result.done);
}

/// Cancel a running sub-agent by ID. Returns true if the sub-agent was found
/// and signalled to cancel.
pub fn cancel(id: &str) -> bool {
    let reg = registry().lock().unwrap();
    if let Some(entry) = reg.get(id) {
        let _ = entry.cancel_tx.send(true);
        true
    } else {
        false
    }
}

/// Cancel ALL running sub-agents.
pub fn cancel_all() {
    let reg = registry().lock().unwrap();
    for (_, entry) in reg.iter() {
        if !entry.result.done {
            let _ = entry.cancel_tx.send(true);
        }
    }
}
