//! Cluster tool — dynamic orchestration with tasks, workers, guards, and hooks.

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use crate::cluster::AgentCluster;
use crate::dynamic::{
    CompareOp, DynamicOrchestrator, DynamicTask, Guard, Hook, HookAction, HookTrigger, TaskState,
};
use crate::provider::Provider;
use crate::tools::Tool;
use crate::types::{FunctionDef, SessionConfig, ToolDefinition, ToolResult};

fn clusters() -> &'static Mutex<std::collections::HashMap<String, AgentCluster>> {
    static C: OnceLock<Mutex<std::collections::HashMap<String, AgentCluster>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

static DEFAULT_CONFIG: OnceLock<Mutex<SessionConfig>> = OnceLock::new();
static DEFAULT_PROVIDER: OnceLock<Arc<dyn Provider>> = OnceLock::new();

pub fn set_defaults(config: SessionConfig, provider: Arc<dyn Provider>) {
    let _ = DEFAULT_CONFIG.set(Mutex::new(config));
    let _ = DEFAULT_PROVIDER.set(provider);
}

/// Dynamic orchestration cluster tool — create/manage task graphs with workers,
/// guards, hooks, events, and metrics in a single unified interface.
pub struct ClusterTool;

#[async_trait::async_trait]
impl Tool for ClusterTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "cluster".into(),
                description: "Dynamic orchestration cluster. Define tasks with dependency graphs, conditional guards, lifecycle hooks, and persistent worker agents. The cluster auto-schedules work to idle workers based on role matching.\n\n\
                    Actions:\n\
                    • plan — Create a full execution plan (tasks + workers + hooks in one shot)\n\
                    • task — Add/remove/modify a single task\n\
                    • worker — Add a persistent worker slot\n\
                    • hook — Attach a lifecycle hook to a task or global\n\
                    • guard — Set a conditional guard on a task\n\
                    • start — Start the cluster tick loop\n\
                    • status — Get current cluster state\n\
                    • emit — Emit an event onto the bus\n\
                    • metric — Set a numeric metric (for guards)\n\
                    • done — Manually tag a task as done\n\
                    • reset — Clear and restart the cluster"
                    .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["plan", "task", "worker", "hook", "guard", "start", "status", "emit", "metric", "done", "reset"],
                            "description": "Action to perform"
                        },
                        "name": {
                            "type": "string",
                            "description": "Cluster name (for 'plan' and 'reset'). Default: 'default'"
                        },
                        "tasks": {
                            "type": "array",
                            "description": "Task definitions (for 'plan'). Each: {title, id?, deps?, agent?, guard?, hooks?, retries?, persistent?}",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "title": { "type": "string" },
                                    "id": { "type": "integer" },
                                    "deps": { "type": "array", "items": { "type": "integer" } },
                                    "agent": { "type": "string", "description": "Agent role: coder, architect, debugger, reviewer, tester" },
                                    "guard": { "$ref": "#/$defs/guard" },
                                    "hooks": { "type": "array", "items": { "$ref": "#/$defs/hook_def" } },
                                    "retries": { "type": "integer" },
                                    "persistent": { "type": "boolean" }
                                },
                                "required": ["title"]
                            }
                        },
                        "workers": {
                            "type": "array",
                            "description": "Worker definitions (for 'plan'). Each: {id, role}",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": { "type": "string" },
                                    "role": { "type": "string" }
                                },
                                "required": ["id", "role"]
                            }
                        },
                        "hooks": {
                            "type": "array",
                            "description": "Global hooks (for 'plan'). Each: {id, trigger, action, guard?, max_fires?}",
                            "items": { "$ref": "#/$defs/hook_def" }
                        },
                        "task_id": {
                            "type": "integer",
                            "description": "Task ID (for task/guard/done/hook actions)"
                        },
                        "title": {
                            "type": "string",
                            "description": "Task title (for 'task' add)"
                        },
                        "worker_id": {
                            "type": "string",
                            "description": "Worker ID"
                        },
                        "role": {
                            "type": "string",
                            "description": "Agent role"
                        },
                        "event_key": {
                            "type": "string",
                            "description": "Event key (for 'emit')"
                        },
                        "metric_key": {
                            "type": "string",
                            "description": "Metric name (for 'metric')"
                        },
                        "metric_value": {
                            "type": "number",
                            "description": "Metric value (for 'metric')"
                        },
                        "guard": { "$ref": "#/$defs/guard" },
                        "hook": { "$ref": "#/$defs/hook_def" },
                        "output": {
                            "type": "string",
                            "description": "Task output (for 'done')"
                        },
                        "max_concurrency": {
                            "type": "integer",
                            "description": "Max concurrent workers (for 'plan')"
                        }
                    },
                    "required": ["action"],
                    "$defs": {
                        "guard": {
                            "type": "object",
                            "description": "A conditional guard. Use one of: always, never, task_done, event, metric, and, or, not",
                            "properties": {
                                "type": { "type": "string", "enum": ["always", "never", "task_done", "task_state", "event", "metric", "and", "or", "not"] },
                                "task_id": { "type": "integer" },
                                "state": { "type": "string" },
                                "key": { "type": "string" },
                                "op": { "type": "string", "enum": ["eq", "neq", "gt", "lt", "gte", "lte"] },
                                "value": { "type": "number" },
                                "guards": { "type": "array", "items": { "$ref": "#/$defs/guard" } },
                                "inner": { "$ref": "#/$defs/guard" }
                            },
                            "required": ["type"]
                        },
                        "hook_def": {
                            "type": "object",
                            "description": "A lifecycle hook",
                            "properties": {
                                "id": { "type": "string" },
                                "trigger": {
                                    "type": "string",
                                    "enum": ["on_start", "on_done", "on_error", "when", "while_running", "on_event"]
                                },
                                "guard": { "$ref": "#/$defs/guard" },
                                "event_key": { "type": "string" },
                                "action": {
                                    "type": "object",
                                    "properties": {
                                        "type": { "type": "string", "enum": ["start_task", "emit", "mark_done", "suspend", "resume", "set_metric", "spawn_agent", "sequence"] },
                                        "task_id": { "type": "integer" },
                                        "key": { "type": "string" },
                                        "value": { "type": "number" },
                                        "agent_id": { "type": "string" },
                                        "agent_task": { "type": "string" },
                                        "agent_role": { "type": "string" },
                                        "actions": { "type": "array" }
                                    },
                                    "required": ["type"]
                                },
                                "max_fires": { "type": "integer" }
                            },
                            "required": ["id", "trigger", "action"]
                        }
                    }
                }),
            },
        }
    }

    async fn execute(&self, workspace: &Path, arguments: &str) -> ToolResult {
        let args: serde_json::Value = match serde_json::from_str(arguments) {
            Ok(v) => v,
            Err(e) => {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Invalid JSON: {e}"),
                    is_error: true,
                };
            }
        };

        let action = args["action"].as_str().unwrap_or("");
        let cluster_name = args["name"].as_str().unwrap_or("default").to_string();

        match action {
            // ── plan: create full execution graph ──
            "plan" => {
                let mut orch = DynamicOrchestrator::new(Some(&cluster_name));

                // Parse tasks
                if let Some(tasks_json) = args["tasks"].as_array() {
                    for t in tasks_json {
                        let title = t["title"].as_str().unwrap_or("untitled").to_string();
                        let id = t["id"].as_u64().map(|n| n as u32).unwrap_or(0);
                        let deps: Vec<u32> = t["deps"]
                            .as_array()
                            .map(|arr| arr.iter().filter_map(|d| d.as_u64().map(|n| n as u32)).collect())
                            .unwrap_or_default();
                        let agent = t["agent"].as_str().map(|s| s.to_string());
                        let retries = t["retries"].as_u64().unwrap_or(0) as u32;
                        let persistent = t["persistent"].as_bool().unwrap_or(false);

                        let mut task = DynamicTask::new(id, title)
                            .with_deps(deps)
                            .with_retries(retries);
                        if let Some(a) = agent {
                            task = task.with_agent(&a);
                        }
                        if persistent {
                            task = task.persistent();
                        }
                        // Parse per-task guard
                        if let Some(guard_json) = t.get("guard") {
                            if let Some(guard) = parse_guard(guard_json) {
                                task = task.with_guard(guard);
                            }
                        }
                        // Parse per-task hooks
                        if let Some(hooks_json) = t["hooks"].as_array() {
                            for h in hooks_json {
                                if let Some(hook) = parse_hook(h) {
                                    task = task.with_hook(hook);
                                }
                            }
                        }
                        orch.add_task(task);
                    }
                }

                // Parse workers (handled below after cluster creation)

                // Parse global hooks
                if let Some(hooks_json) = args["hooks"].as_array() {
                    for h in hooks_json {
                        if let Some(hook) = parse_hook(h) {
                            orch.hooks.push(hook);
                        }
                    }
                }

                // Create cluster
                let (config, provider) = match get_defaults() {
                    Some((c, p)) => (c, p),
                    None => {
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: "Cluster defaults not set.".into(),
                            is_error: true,
                        };
                    }
                };
                let max_concurrency = args["max_concurrency"].as_u64().unwrap_or(4) as usize;
                let mut cluster = AgentCluster::new(orch, config, provider, workspace.to_path_buf());
                cluster.max_concurrency = max_concurrency;

                // Add workers
                if let Some(workers_json) = args["workers"].as_array() {
                    for w in workers_json {
                        let wid = w["id"].as_str().unwrap_or("w");
                        let wrole = w["role"].as_str().unwrap_or("coder");
                        cluster.add_worker(wid, wrole);
                    }
                }

                let status = cluster.format_status();
                clusters().lock().unwrap().insert(cluster_name.clone(), cluster);

                ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Cluster '{cluster_name}' created.\n\n{status}"),
                    is_error: false,
                }
            }

            // ── task: add/remove single task ──
            "task" => {
                let mut map = clusters().lock().unwrap();
                let cluster = match map.get_mut(&cluster_name) {
                    Some(c) => c,
                    None => {
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: format!("Cluster '{cluster_name}' not found. Use 'plan' first."),
                            is_error: true,
                        };
                    }
                };

                let title = args["title"].as_str().unwrap_or("");
                if title.is_empty() {
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: "Provide 'title' for the task.".into(),
                        is_error: true,
                    };
                }

                let mut task = DynamicTask::new(0, title.to_string());
                if let Some(deps) = args["deps"].as_array() {
                    task = task.with_deps(
                        deps.iter().filter_map(|d| d.as_u64().map(|n| n as u32)).collect(),
                    );
                }
                if let Some(agent) = args["agent"].as_str() {
                    task = task.with_agent(agent);
                }
                if let Some(retries) = args["retries"].as_u64() {
                    task = task.with_retries(retries as u32);
                }
                if args["persistent"].as_bool().unwrap_or(false) {
                    task = task.persistent();
                }
                if let Some(guard_json) = args.get("guard") {
                    if let Some(guard) = parse_guard(guard_json) {
                        task = task.with_guard(guard);
                    }
                }
                let id = cluster.orchestrator.add_task(task);
                let status = cluster.orchestrator.format_status();

                ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Task #{id} added.\n\n{status}"),
                    is_error: false,
                }
            }

            // ── worker: add worker slot ──
            "worker" => {
                let mut map = clusters().lock().unwrap();
                let cluster = match map.get_mut(&cluster_name) {
                    Some(c) => c,
                    None => {
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: format!("Cluster '{cluster_name}' not found."),
                            is_error: true,
                        };
                    }
                };

                let wid = args["worker_id"].as_str().unwrap_or("");
                let role = args["role"].as_str().unwrap_or("coder");
                if wid.is_empty() {
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: "Provide 'worker_id'.".into(),
                        is_error: true,
                    };
                }
                cluster.add_worker(wid, role);

                ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Worker '{wid}' ({role}) added. Idle: {}", cluster.idle_count()),
                    is_error: false,
                }
            }

            // ── hook: add hook to task or global ──
            "hook" => {
                let mut map = clusters().lock().unwrap();
                let cluster = match map.get_mut(&cluster_name) {
                    Some(c) => c,
                    None => {
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: format!("Cluster '{cluster_name}' not found."),
                            is_error: true,
                        };
                    }
                };

                let hook_json = args.get("hook").unwrap_or(&args);
                let hook = match parse_hook(hook_json) {
                    Some(h) => h,
                    None => {
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: "Invalid hook definition. Need id, trigger, action.".into(),
                            is_error: true,
                        };
                    }
                };

                if let Some(task_id) = args["task_id"].as_u64() {
                    if let Some(task) = cluster.orchestrator.tasks.get_mut(&(task_id as u32)) {
                        task.hooks.push(hook.clone());
                    }
                } else {
                    cluster.orchestrator.hooks.push(hook.clone());
                }

                ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Hook '{}' added.", hook.id),
                    is_error: false,
                }
            }

            // ── guard: set guard on task ──
            "guard" => {
                let mut map = clusters().lock().unwrap();
                let cluster = match map.get_mut(&cluster_name) {
                    Some(c) => c,
                    None => {
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: format!("Cluster '{cluster_name}' not found."),
                            is_error: true,
                        };
                    }
                };

                let task_id = match args["task_id"].as_u64() {
                    Some(id) => id as u32,
                    None => {
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: "Provide 'task_id'.".into(),
                            is_error: true,
                        };
                    }
                };

                let guard_json = args.get("guard").unwrap_or(&args);
                let guard = match parse_guard(guard_json) {
                    Some(g) => g,
                    None => {
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: "Invalid guard definition.".into(),
                            is_error: true,
                        };
                    }
                };

                if let Some(task) = cluster.orchestrator.tasks.get_mut(&task_id) {
                    task.guard = Some(guard);
                    ToolResult {
                        tool_call_id: String::new(),
                        content: format!("Guard set on task #{task_id}."),
                        is_error: false,
                    }
                } else {
                    ToolResult {
                        tool_call_id: String::new(),
                        content: format!("Task #{task_id} not found."),
                        is_error: true,
                    }
                }
            }

            // ── start: run one tick ──
            "start" => {
                let mut map = clusters().lock().unwrap();
                let cluster = match map.get_mut(&cluster_name) {
                    Some(c) => c,
                    None => {
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: format!("Cluster '{cluster_name}' not found."),
                            is_error: true,
                        };
                    }
                };

                let rt = tokio::runtime::Handle::current();
                let events = rt.block_on(cluster.tick());

                let mut out = "Tick complete:\n".to_string();
                for ev in &events {
                    match ev {
                        crate::cluster::ClusterEvent::TaskAssigned { task_id, worker_id } => {
                            out.push_str(&format!("  → Task #{task_id} assigned to {worker_id}\n"));
                        }
                        crate::cluster::ClusterEvent::TaskCompleted { task_id, worker_id, success } => {
                            let icon = if *success { "✓" } else { "✗" };
                            out.push_str(&format!("  {icon} Task #{task_id} done by {worker_id}\n"));
                        }
                        crate::cluster::ClusterEvent::HookFired { hook_id } => {
                            out.push_str(&format!("  ⚡ Hook '{hook_id}' fired\n"));
                        }
                        crate::cluster::ClusterEvent::AllDone => {
                            out.push_str("  ✓ All tasks complete!\n");
                        }
                        crate::cluster::ClusterEvent::ClusterTick { ready, running, idle } => {
                            out.push_str(&format!("  Status: {ready} ready, {running} running, {idle} idle\n"));
                        }
                        _ => {}
                    }
                }
                out.push('\n');
                out.push_str(&cluster.format_status());

                ToolResult {
                    tool_call_id: String::new(),
                    content: out,
                    is_error: false,
                }
            }

            // ── status ──
            "status" => {
                let map = clusters().lock().unwrap();
                match map.get(&cluster_name) {
                    Some(cluster) => ToolResult {
                        tool_call_id: String::new(),
                        content: cluster.format_status(),
                        is_error: false,
                    },
                    None => ToolResult {
                        tool_call_id: String::new(),
                        content: format!("Cluster '{cluster_name}' not found."),
                        is_error: true,
                    },
                }
            }

            // ── emit: emit event ──
            "emit" => {
                let mut map = clusters().lock().unwrap();
                let cluster = match map.get_mut(&cluster_name) {
                    Some(c) => c,
                    None => {
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: format!("Cluster '{cluster_name}' not found."),
                            is_error: true,
                        };
                    }
                };
                let key = args["event_key"].as_str().unwrap_or("");
                if key.is_empty() {
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: "Provide 'event_key'.".into(),
                        is_error: true,
                    };
                }
                cluster.orchestrator.event_bus.emit(crate::dynamic::Event {
                    key: key.to_string(),
                    source_task: args["task_id"].as_u64().map(|n| n as u32),
                    payload: args["output"].as_str().map(|s| s.to_string()),
                    timestamp: now_secs(),
                });
                ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Event '{key}' emitted."),
                    is_error: false,
                }
            }

            // ── metric ──
            "metric" => {
                let mut map = clusters().lock().unwrap();
                let cluster = match map.get_mut(&cluster_name) {
                    Some(c) => c,
                    None => {
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: format!("Cluster '{cluster_name}' not found."),
                            is_error: true,
                        };
                    }
                };
                let key = args["metric_key"].as_str().unwrap_or("");
                let value = args["metric_value"].as_f64().unwrap_or(0.0);
                if key.is_empty() {
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: "Provide 'metric_key' and 'metric_value'.".into(),
                        is_error: true,
                    };
                }
                cluster.orchestrator.metrics.insert(key.to_string(), value);
                ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Metric '{key}' = {value}"),
                    is_error: false,
                }
            }

            // ── done: manual tag ──
            "done" => {
                let mut map = clusters().lock().unwrap();
                let cluster = match map.get_mut(&cluster_name) {
                    Some(c) => c,
                    None => {
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: format!("Cluster '{cluster_name}' not found."),
                            is_error: true,
                        };
                    }
                };
                let task_id = match args["task_id"].as_u64() {
                    Some(id) => id as u32,
                    None => {
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: "Provide 'task_id'.".into(),
                            is_error: true,
                        };
                    }
                };
                let output = args["output"].as_str().map(|s| s.to_string());
                match cluster.orchestrator.tagged_done(task_id, output) {
                    Ok(()) => ToolResult {
                        tool_call_id: String::new(),
                        content: format!("Task #{task_id} marked done."),
                        is_error: false,
                    },
                    Err(e) => ToolResult {
                        tool_call_id: String::new(),
                        content: e,
                        is_error: true,
                    },
                }
            }

            // ── reset ──
            "reset" => {
                clusters().lock().unwrap().remove(&cluster_name);
                ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Cluster '{cluster_name}' cleared."),
                    is_error: false,
                }
            }

            _ => ToolResult {
                tool_call_id: String::new(),
                content: format!(
                    "Unknown action: '{action}'.\n\
                     Available: plan, task, worker, hook, guard, start, status, emit, metric, done, reset."
                ),
                is_error: true,
            },
        }
    }
}

// ═══ DSL Parsers ═══

fn parse_guard(v: &serde_json::Value) -> Option<Guard> {
    let gtype = v["type"].as_str()?;
    Some(match gtype {
        "always" => Guard::Always,
        "never" => Guard::Never,
        "task_done" => Guard::TaskDone(v["task_id"].as_u64()? as u32),
        "task_state" => {
            let id = v["task_id"].as_u64()? as u32;
            let state = parse_task_state(v["state"].as_str()?)?;
            Guard::TaskState(id, state)
        }
        "event" => Guard::EventEmitted(v["key"].as_str()?.to_string()),
        "metric" => Guard::MetricCompare {
            key: v["key"].as_str()?.to_string(),
            op: parse_compare_op(v["op"].as_str().unwrap_or("gt")),
            value: v["value"].as_f64().unwrap_or(0.0),
        },
        "and" => {
            let guards: Vec<Guard> = v["guards"]
                .as_array()?
                .iter()
                .filter_map(parse_guard)
                .collect();
            Guard::And(guards)
        }
        "or" => {
            let guards: Vec<Guard> = v["guards"]
                .as_array()?
                .iter()
                .filter_map(parse_guard)
                .collect();
            Guard::Or(guards)
        }
        "not" => Guard::Not(Box::new(parse_guard(v.get("inner")?)?)),
        _ => return None,
    })
}

fn parse_compare_op(s: &str) -> CompareOp {
    match s {
        "eq" => CompareOp::Eq,
        "neq" => CompareOp::Neq,
        "gt" => CompareOp::Gt,
        "lt" => CompareOp::Lt,
        "gte" => CompareOp::Gte,
        "lte" => CompareOp::Lte,
        _ => CompareOp::Gt,
    }
}

fn parse_task_state(s: &str) -> Option<TaskState> {
    Some(match s {
        "pending" => TaskState::Pending,
        "ready" => TaskState::Ready,
        "running" => TaskState::Running,
        "suspended" => TaskState::Suspended,
        "done" => TaskState::Done,
        "failed" => TaskState::Failed,
        "skipped" => TaskState::Skipped,
        "persistent" => TaskState::Persistent,
        _ => return None,
    })
}

fn parse_hook(v: &serde_json::Value) -> Option<Hook> {
    let id = v["id"].as_str()?.to_string();
    let trigger = match v["trigger"].as_str()? {
        "on_start" => HookTrigger::OnStart,
        "on_done" => HookTrigger::OnDone,
        "on_error" => HookTrigger::OnError,
        "while_running" => HookTrigger::WhileRunning,
        "when" => HookTrigger::When(parse_guard(v.get("guard")?)?),
        "on_event" => HookTrigger::OnEvent(v["event_key"].as_str().unwrap_or("").to_string()),
        _ => return None,
    };
    let action = parse_hook_action(v.get("action")?)?;
    let guard = v
        .get("guard")
        .filter(|g| g.get("type").is_some())
        .and_then(parse_guard);
    let max_fires = v["max_fires"].as_u64().map(|n| n as u32);

    Some(Hook {
        id,
        trigger,
        action,
        guard,
        max_fires,
        fire_count: 0,
    })
}

fn parse_hook_action(v: &serde_json::Value) -> Option<HookAction> {
    let atype = v["type"].as_str()?;
    Some(match atype {
        "start_task" => HookAction::StartTask(v["task_id"].as_u64()? as u32),
        "emit" => HookAction::EmitEvent(v["key"].as_str()?.to_string()),
        "mark_done" => HookAction::MarkDone(v["task_id"].as_u64()? as u32),
        "suspend" => HookAction::SuspendTask(v["task_id"].as_u64()? as u32),
        "resume" => HookAction::ResumeTask(v["task_id"].as_u64()? as u32),
        "set_metric" => HookAction::SetMetric(
            v["key"].as_str()?.to_string(),
            v["value"].as_f64().unwrap_or(0.0),
        ),
        "spawn_agent" => HookAction::SpawnAgent {
            id: v["agent_id"].as_str().unwrap_or("hook_agent").to_string(),
            task: v["agent_task"].as_str().unwrap_or("").to_string(),
            agent: v["agent_role"].as_str().map(|s| s.to_string()),
        },
        "sequence" => {
            let actions: Vec<HookAction> = v["actions"]
                .as_array()?
                .iter()
                .filter_map(parse_hook_action)
                .collect();
            HookAction::Sequence(actions)
        }
        _ => return None,
    })
}

fn get_defaults() -> Option<(SessionConfig, Arc<dyn Provider>)> {
    let config = DEFAULT_CONFIG.get()?.lock().ok()?.clone();
    let provider = DEFAULT_PROVIDER.get()?.clone();
    Some((config, provider))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
