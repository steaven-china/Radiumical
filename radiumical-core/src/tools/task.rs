use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::orchestrator::Orchestrator;
use crate::tools::Tool;
use crate::types::{FunctionDef, ToolDefinition, ToolResult};

pub struct TodoList;
pub struct OrchestrateTool;
pub struct GoalTool;

pub(crate) fn todos() -> &'static Mutex<Vec<(String, bool)>> {
    static TODOS: OnceLock<Mutex<Vec<(String, bool)>>> = OnceLock::new();
    TODOS.get_or_init(|| Mutex::new(Vec::new()))
}

#[async_trait::async_trait]
impl Tool for TodoList {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "todo_list".into(),
                description: "Manage a task list. Actions: 'add <task>', 'done <index>', 'list', 'clear'. Use to track progress on multi-step tasks.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "description": "Action: 'add <task>', 'done <index>', 'list', 'clear'"
                        }
                    },
                    "required": ["action"]
                }),
            },
        }
    }

    async fn execute(&self, _workspace: &PathBuf, arguments: &str) -> ToolResult {
        let args: serde_json::Value = match serde_json::from_str(arguments) {
            Ok(v) => v,
            Err(e) => {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Invalid JSON: {e}"),
                    is_error: true,
                }
            }
        };
        let action = args["action"].as_str().unwrap_or("");
        let mut todos = todos().lock().unwrap();

        if action == "list" || action.is_empty() {
            if todos.is_empty() {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: "No todos yet.".into(),
                    is_error: false,
                };
            }
            let list: String = todos
                .iter()
                .enumerate()
                .map(|(i, (t, done))| {
                    format!("  [{}] {} {}\n", if *done { "x" } else { " " }, i + 1, t)
                })
                .collect();
            return ToolResult {
                tool_call_id: String::new(),
                content: list,
                is_error: false,
            };
        }

        if let Some(task) = action.strip_prefix("add ") {
            todos.push((task.to_string(), false));
            return ToolResult {
                tool_call_id: String::new(),
                content: format!("Added todo #{}: {task}", todos.len()),
                is_error: false,
            };
        }

        if let Some(idx_str) = action.strip_prefix("done ") {
            if let Ok(idx) = idx_str.trim().parse::<usize>() {
                if idx > 0 && idx <= todos.len() {
                    todos[idx - 1].1 = true;
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: format!("Marked todo #{idx} as done."),
                        is_error: false,
                    };
                }
            }
            return ToolResult {
                tool_call_id: String::new(),
                content: format!("Invalid index: {idx_str}"),
                is_error: true,
            };
        }

        if action == "clear" {
            todos.clear();
            return ToolResult {
                tool_call_id: String::new(),
                content: "Cleared all todos.".into(),
                is_error: false,
            };
        }

        ToolResult {
            tool_call_id: String::new(),
            content: format!("Unknown action: {action}. Use add/done/list/clear."),
            is_error: true,
        }
    }
}

fn orchestrators() -> &'static Mutex<HashMap<String, Orchestrator>> {
    static ORCS: OnceLock<Mutex<HashMap<String, Orchestrator>>> = OnceLock::new();
    ORCS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn workspace_key(workspace: &PathBuf) -> String {
    workspace.display().to_string()
}

#[async_trait::async_trait]
impl Tool for OrchestrateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "orchestrate".into(),
                description: "Manage a task orchestration plan with dependency tracking. Actions: create, list, start, done, block, skip, add, remove. Tasks can have dependencies (deps) that must be completed before starting.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["create", "list", "start", "done", "block", "skip", "add", "remove"],
                            "description": "Action to perform"
                        },
                        "title": {
                            "type": "string",
                            "description": "Plan title (for 'create')"
                        },
                        "tasks": {
                            "type": "array",
                            "description": "Task list for 'create' or 'add'. Each item: {\"title\":\"...\",\"deps\":[1,2]}",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "title": { "type": "string" },
                                    "deps": { "type": "array", "items": { "type": "integer" } }
                                }
                            }
                        },
                        "id": {
                            "type": "integer",
                            "description": "Task ID (for start/done/block/skip/remove)"
                        },
                        "reason": {
                            "type": "string",
                            "description": "Reason for blocking (for 'block')"
                        }
                    },
                    "required": ["action"]
                }),
            },
        }
    }

    async fn execute(&self, workspace: &PathBuf, arguments: &str) -> ToolResult {
        let args: serde_json::Value = match serde_json::from_str(arguments) {
            Ok(v) => v,
            Err(e) => {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Invalid JSON: {e}"),
                    is_error: true,
                }
            }
        };
        let action = args["action"].as_str().unwrap_or("");
        let key = workspace_key(workspace);
        let mut orcs = orchestrators().lock().unwrap();

        // Lazy init orchestrator for this workspace
        if !orcs.contains_key(&key) {
            orcs.insert(key.clone(), Orchestrator::new(Some(&key)));
        }
        let orch = orcs.get_mut(&key).unwrap();

        let result = match action {
            "create" => {
                let title = args["title"].as_str().unwrap_or("");
                let tasks_json = args["tasks"].as_array().cloned().unwrap_or_default();
                let tasks: Vec<(String, Vec<u32>)> = tasks_json
                    .iter()
                    .filter_map(|t| {
                        let title = t["title"].as_str()?.to_string();
                        let deps: Vec<u32> = t["deps"]
                            .as_array()?
                            .iter()
                            .filter_map(|d| d.as_u64().map(|n| n as u32))
                            .collect();
                        Some((title, deps))
                    })
                    .collect();
                if tasks.is_empty() {
                    Err("Please provide tasks list.".into())
                } else {
                    Ok(orch.create(title, tasks))
                }
            }
            "list" => Ok(orch.list()),
            "start" => match args["id"].as_u64() {
                Some(id) => orch.start(id as u32),
                None => Err("Provide task id".into()),
            },
            "done" => match args["id"].as_u64() {
                Some(id) => orch.done(id as u32),
                None => Err("Provide task id".into()),
            },
            "block" => match args["id"].as_u64() {
                Some(id) => {
                    let reason = args["reason"].as_str();
                    orch.block(id as u32, reason)
                }
                None => Err("Provide task id".into()),
            },
            "skip" => match args["id"].as_u64() {
                Some(id) => orch.skip(id as u32),
                None => Err("Provide task id".into()),
            },
            "add" => {
                let tasks_json = args["tasks"].as_array().cloned().unwrap_or_default();
                let tasks: Vec<(String, Vec<u32>)> = tasks_json
                    .iter()
                    .filter_map(|t| {
                        let title = t["title"].as_str()?.to_string();
                        let deps: Vec<u32> = t["deps"]
                            .as_array()?
                            .iter()
                            .filter_map(|d| d.as_u64().map(|n| n as u32))
                            .collect();
                        Some((title, deps))
                    })
                    .collect();
                if tasks.is_empty() {
                    Err("Please provide tasks to add.".into())
                } else {
                    orch.add(tasks)
                }
            }
            "remove" => match args["id"].as_u64() {
                Some(id) => orch.remove(id as u32),
                None => Err("Provide task id".into()),
            },
            _ => Err(format!(
                "Unknown action: {action}. Use create/list/start/done/block/skip/add/remove."
            )),
        };

        match result {
            Ok(content) => ToolResult {
                tool_call_id: String::new(),
                content,
                is_error: false,
            },
            Err(content) => ToolResult {
                tool_call_id: String::new(),
                content,
                is_error: true,
            },
        }
    }
}

pub(crate) fn goals() -> &'static Mutex<Vec<String>> {
    static GOALS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    GOALS.get_or_init(|| Mutex::new(Vec::new()))
}

#[async_trait::async_trait]
impl Tool for GoalTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "goal".into(),
                description: "Set or view the current goal and sub-goals. Actions: 'set <goal>', 'add <sub-goal>', 'done <index>', 'list'. Use to decompose a task into goals, then work through them.".into(),
                parameters: serde_json::json!({
                    "type": "object", "properties": {
                        "action": { "type": "string", "description": "Action: 'set <goal>', 'add <sub-goal>', 'done <index>', 'list'" }
                    }, "required": ["action"]
                }),
            },
        }
    }

    async fn execute(&self, _workspace: &PathBuf, arguments: &str) -> ToolResult {
        let args: serde_json::Value = match serde_json::from_str(arguments) {
            Ok(v) => v,
            Err(e) => {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Invalid JSON: {e}"),
                    is_error: true,
                }
            }
        };
        let action = args["action"].as_str().unwrap_or("");
        let mut g = goals().lock().unwrap();

        if action == "list" || action.is_empty() {
            if g.is_empty() {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: "No goals set.".into(),
                    is_error: false,
                };
            }
            let list: String = g
                .iter()
                .enumerate()
                .map(|(i, t)| format!("  {}. {}\n", i + 1, t))
                .collect();
            return ToolResult {
                tool_call_id: String::new(),
                content: list,
                is_error: false,
            };
        }

        if let Some(goal) = action.strip_prefix("set ") {
            g.clear();
            g.push(goal.to_string());
            return ToolResult {
                tool_call_id: String::new(),
                content: format!("Goal set: {goal}"),
                is_error: false,
            };
        }

        if let Some(sub) = action.strip_prefix("add ") {
            g.push(sub.to_string());
            return ToolResult {
                tool_call_id: String::new(),
                content: format!("Added sub-goal #{}: {sub}", g.len()),
                is_error: false,
            };
        }

        ToolResult {
            tool_call_id: String::new(),
            content: format!("Unknown action: {action}"),
            is_error: true,
        }
    }
}
