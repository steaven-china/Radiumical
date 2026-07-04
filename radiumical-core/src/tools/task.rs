use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::orchestrator::Orchestrator;
use crate::tools::Tool;
use crate::types::{FunctionDef, ToolDefinition, ToolResult};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TodoPriority {
    Low,
    Medium,
    High,
}

impl TodoPriority {
    pub fn icon(&self) -> &'static str {
        match self {
            TodoPriority::Low => "▽",
            TodoPriority::Medium => "◇",
            TodoPriority::High => "◆",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "high" | "h" | "!" => TodoPriority::High,
            "low" | "l" => TodoPriority::Low,
            _ => TodoPriority::Medium,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub text: String,
    pub done: bool,
    pub priority: TodoPriority,
    pub category: Option<String>,
    pub note: Option<String>,
    pub created_ts: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TodoStore {
    pub items: Vec<TodoItem>,
}

impl TodoStore {
    pub fn path_for(workspace: &Path) -> PathBuf {
        let hash = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            workspace.display().to_string().hash(&mut h);
            format!("{:016x}", h.finish())
        };
        let dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".radi")
            .join("todos");
        let _ = std::fs::create_dir_all(&dir);
        dir.join(format!("{hash}.json"))
    }

    pub fn load(workspace: &Path) -> Self {
        let path = Self::path_for(workspace);
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, workspace: &Path) {
        let path = Self::path_for(workspace);
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

pub struct TodoList;
pub struct OrchestrateTool;
pub struct GoalTool;

#[async_trait::async_trait]
impl Tool for TodoList {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "todo_list".into(),
                description: "Persistent task list with priorities. Actions: 'add <text> [!high] [cat:tag]', 'done <idx>', 'undone <idx>', 'edit <idx> <new text>', 'note <idx> <text>', 'priority <idx> <high|medium|low>', 'list [all|pending|done|high|cat:<name>]', 'stats', 'clear [done]', 'remove <idx>'. Persisted to disk per workspace.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "description": "Action + args. Examples: 'add Fix login bug !high cat:auth', 'done 1', 'list', 'list high', 'list cat:auth', 'stats', 'note 2 needs DB migration', 'edit 3 Fix login bug v2', 'priority 1 high', 'undone 2', 'clear done', 'remove 4'"
                        }
                    },
                    "required": ["action"]
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
                }
            }
        };
        let action = args["action"].as_str().unwrap_or("");
        let mut store = TodoStore::load(workspace);

        // ── list ──
        if action == "list" || action == "list all" || action.is_empty() {
            return render_list(&store.items, None);
        }
        if let Some(filter) = action.strip_prefix("list ") {
            return render_list(&store.items, Some(filter));
        }

        // ── stats ──
        if action == "stats" {
            let total = store.items.len();
            let done = store.items.iter().filter(|t| t.done).count();
            let pending = total - done;
            let high = store
                .items
                .iter()
                .filter(|t| !t.done && t.priority == TodoPriority::High)
                .count();
            let med = store
                .items
                .iter()
                .filter(|t| !t.done && t.priority == TodoPriority::Medium)
                .count();
            let low = store
                .items
                .iter()
                .filter(|t| !t.done && t.priority == TodoPriority::Low)
                .count();
            let cats: Vec<String> = {
                let mut set: std::collections::HashSet<String> = std::collections::HashSet::new();
                for t in &store.items {
                    if let Some(c) = &t.category {
                        set.insert(c.clone());
                    }
                }
                let mut v: Vec<String> = set.into_iter().collect();
                v.sort();
                v
            };
            let mut out = format!("Todo stats: {total} total, {done} done, {pending} pending\n");
            out.push_str(&format!(
                "  Priority: {high} high, {med} medium, {low} low\n"
            ));
            if !cats.is_empty() {
                out.push_str(&format!("  Categories: {}\n", cats.join(", ")));
            }
            return ToolResult {
                tool_call_id: String::new(),
                content: out,
                is_error: false,
            };
        }

        // ── add ──
        if let Some(rest) = action.strip_prefix("add ") {
            let (text, priority, category) = parse_add_args_todos(rest);
            if text.is_empty() {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: "No task text provided.".into(),
                    is_error: true,
                };
            }
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            store.items.push(TodoItem {
                text: text.to_string(),
                done: false,
                priority,
                category,
                note: None,
                created_ts: ts,
            });
            store.save(workspace);
            let idx = store.items.len();
            let item = store.items.last().unwrap();
            let pri = item.priority.icon();
            let cat = item
                .category
                .as_deref()
                .map(|c| format!(" [{c}]"))
                .unwrap_or_default();
            return ToolResult {
                tool_call_id: String::new(),
                content: format!("Added #{idx}: {pri} {}{cat}", item.text),
                is_error: false,
            };
        }

        // ── done ──
        if let Some(rest) = action.strip_prefix("done ") {
            if let Ok(idx) = rest.trim().parse::<usize>() {
                if idx > 0 && idx <= store.items.len() {
                    store.items[idx - 1].done = true;
                    store.save(workspace);
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: format!("✓ Done #{}: {}", idx, store.items[idx - 1].text),
                        is_error: false,
                    };
                }
            }
            return ToolResult {
                tool_call_id: String::new(),
                content: format!("Invalid index: {rest}. Use 1-{}.", store.items.len()),
                is_error: true,
            };
        }

        // ── undone ──
        if let Some(rest) = action.strip_prefix("undone ") {
            if let Ok(idx) = rest.trim().parse::<usize>() {
                if idx > 0 && idx <= store.items.len() {
                    store.items[idx - 1].done = false;
                    store.save(workspace);
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: format!("Reopened #{}: {}", idx, store.items[idx - 1].text),
                        is_error: false,
                    };
                }
            }
            return ToolResult {
                tool_call_id: String::new(),
                content: format!("Invalid index: {rest}"),
                is_error: true,
            };
        }

        // ── edit ──
        if let Some(rest) = action.strip_prefix("edit ") {
            let mut parts = rest.splitn(2, ' ');
            if let (Some(idx_str), Some(new_text)) = (parts.next(), parts.next()) {
                if let Ok(idx) = idx_str.trim().parse::<usize>() {
                    if idx > 0 && idx <= store.items.len() {
                        let old = store.items[idx - 1].text.clone();
                        store.items[idx - 1].text = new_text.trim().to_string();
                        store.save(workspace);
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: format!(
                                "Edited #{idx}: '{old}' → '{}'",
                                store.items[idx - 1].text
                            ),
                            is_error: false,
                        };
                    }
                }
            }
            return ToolResult {
                tool_call_id: String::new(),
                content: "Usage: edit <index> <new text>".into(),
                is_error: true,
            };
        }

        // ── note ──
        if let Some(rest) = action.strip_prefix("note ") {
            let mut parts = rest.splitn(2, ' ');
            if let (Some(idx_str), Some(note_text)) = (parts.next(), parts.next()) {
                if let Ok(idx) = idx_str.trim().parse::<usize>() {
                    if idx > 0 && idx <= store.items.len() {
                        store.items[idx - 1].note = Some(note_text.trim().to_string());
                        store.save(workspace);
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: format!("Note added to #{}: {}", idx, note_text.trim()),
                            is_error: false,
                        };
                    }
                }
            }
            return ToolResult {
                tool_call_id: String::new(),
                content: "Usage: note <index> <text>".into(),
                is_error: true,
            };
        }

        // ── priority ──
        if let Some(rest) = action.strip_prefix("priority ") {
            let mut parts = rest.splitn(2, ' ');
            if let (Some(idx_str), Some(pri_str)) = (parts.next(), parts.next()) {
                if let Ok(idx) = idx_str.trim().parse::<usize>() {
                    if idx > 0 && idx <= store.items.len() {
                        store.items[idx - 1].priority = TodoPriority::from_str(pri_str.trim());
                        store.save(workspace);
                        let p = store.items[idx - 1].priority.icon();
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: format!(
                                "Priority #{idx} → {p} {:?}",
                                store.items[idx - 1].priority
                            ),
                            is_error: false,
                        };
                    }
                }
            }
            return ToolResult {
                tool_call_id: String::new(),
                content: "Usage: priority <index> <high|medium|low>".into(),
                is_error: true,
            };
        }

        // ── remove ──
        if let Some(rest) = action.strip_prefix("remove ") {
            if let Ok(idx) = rest.trim().parse::<usize>() {
                if idx > 0 && idx <= store.items.len() {
                    let removed = store.items.remove(idx - 1);
                    store.save(workspace);
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: format!("Removed #{}: {}", idx, removed.text),
                        is_error: false,
                    };
                }
            }
            return ToolResult {
                tool_call_id: String::new(),
                content: format!("Invalid index: {rest}"),
                is_error: true,
            };
        }

        // ── clear ──
        if action == "clear" {
            let count = store.items.len();
            store.items.clear();
            store.save(workspace);
            return ToolResult {
                tool_call_id: String::new(),
                content: format!("Cleared {count} todos."),
                is_error: false,
            };
        }
        if action == "clear done" {
            let before = store.items.len();
            store.items.retain(|t| !t.done);
            let removed = before - store.items.len();
            store.save(workspace);
            return ToolResult {
                tool_call_id: String::new(),
                content: format!("Removed {removed} completed todos."),
                is_error: false,
            };
        }

        ToolResult {
            tool_call_id: String::new(),
            content: format!(
                "Unknown action: '{action}'.\n\
                 Available: add, done, undone, edit, note, priority, list, stats, clear, remove."
            ),
            is_error: true,
        }
    }
}

/// Parse add args: 'Fix login bug !high cat:auth' → (text, priority, category)
fn parse_add_args_todos(input: &str) -> (&str, TodoPriority, Option<String>) {
    let mut priority = TodoPriority::Medium;
    let mut category = None;
    let mut text_end = input.len();

    // Scan tokens from the end
    let words: Vec<(usize, &str)> = input.split_whitespace().enumerate().collect();
    for (i, word) in words.iter().rev() {
        if let Some(stripped) = word.strip_prefix("!") {
            priority = TodoPriority::from_str(stripped);
            text_end = input[..*i + word.len()].len() - word.len();
            // Trim trailing space
            while text_end > 0 && input.as_bytes()[text_end - 1] == b' ' {
                text_end -= 1;
            }
        } else if let Some(stripped) = word.strip_prefix("cat:") {
            category = Some(stripped.to_string());
            text_end = input[..*i + word.len()].len() - word.len();
            while text_end > 0 && input.as_bytes()[text_end - 1] == b' ' {
                text_end -= 1;
            }
        } else {
            break;
        }
    }

    (&input[..text_end], priority, category)
}

fn render_list(items: &[TodoItem], filter: Option<&str>) -> ToolResult {
    if items.is_empty() {
        return ToolResult {
            tool_call_id: String::new(),
            content: "No todos yet. Use 'add <task>' to create one.".into(),
            is_error: false,
        };
    }

    let filtered: Vec<(usize, &TodoItem)> = items
        .iter()
        .enumerate()
        .filter(|(_, t)| match filter {
            None | Some("all") => true,
            Some("pending") | Some("open") => !t.done,
            Some("done") | Some("completed") => t.done,
            Some("high") => !t.done && t.priority == TodoPriority::High,
            Some("medium") | Some("med") => !t.done && t.priority == TodoPriority::Medium,
            Some("low") => !t.done && t.priority == TodoPriority::Low,
            Some(f) if f.starts_with("cat:") => {
                let cat = &f[4..];
                t.category.as_deref() == Some(cat)
            }
            _ => true,
        })
        .collect();

    if filtered.is_empty() {
        return ToolResult {
            tool_call_id: String::new(),
            content: format!("No todos match filter: {}", filter.unwrap_or("")),
            is_error: false,
        };
    }

    let mut out = String::new();
    for (i, item) in &filtered {
        let idx = i + 1;
        let check = if item.done { "x" } else { " " };
        let pri = item.priority.icon();
        let cat = item
            .category
            .as_deref()
            .map(|c| format!(" [{c}]"))
            .unwrap_or_default();
        let note = item
            .note
            .as_deref()
            .map(|n| format!("\n      note: {n}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "  [{check}] {idx}. {pri} {}{cat}{note}\n",
            item.text
        ));
    }

    let pending = items.iter().filter(|t| !t.done).count();
    let done = items.iter().filter(|t| t.done).count();
    out.push_str(&format!("\n  {pending} pending, {done} done"));

    ToolResult {
        tool_call_id: String::new(),
        content: out,
        is_error: false,
    }
}

fn orchestrators() -> &'static Mutex<HashMap<String, Orchestrator>> {
    static ORCS: OnceLock<Mutex<HashMap<String, Orchestrator>>> = OnceLock::new();
    ORCS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn workspace_key(workspace: &Path) -> String {
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
                            "description": "Task list for 'create' or 'add'. Each item: {\"title\":\"...\",\"deps\":[1,2],\"agent\":\"debugger\"}",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "title": { "type": "string" },
                                    "deps": { "type": "array", "items": { "type": "integer" } },
                                    "agent": { "type": "string", "description": "Agent role: coder, architect, debugger, reviewer, tester" }
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

    async fn execute(&self, workspace: &Path, arguments: &str) -> ToolResult {
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
                let tasks: Vec<(String, Vec<u32>, Option<String>)> = tasks_json
                    .iter()
                    .filter_map(|t| {
                        let title = t["title"].as_str()?.to_string();
                        let deps: Vec<u32> = t["deps"]
                            .as_array()?
                            .iter()
                            .filter_map(|d| d.as_u64().map(|n| n as u32))
                            .collect();
                        let agent = t["agent"].as_str().map(|s| s.to_string());
                        Some((title, deps, agent))
                    })
                    .collect();
                if tasks.is_empty() {
                    Err("Please provide tasks list.".into())
                } else {
                    let has_agents = tasks.iter().any(|(_, _, a)| a.is_some());
                    if has_agents {
                        Ok(orch.create_with_agents(title, tasks))
                    } else {
                        Ok(orch.create(title, tasks.into_iter().map(|(t, d, _)| (t, d)).collect()))
                    }
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
                let tasks: Vec<(String, Vec<u32>, Option<String>)> = tasks_json
                    .iter()
                    .filter_map(|t| {
                        let title = t["title"].as_str()?.to_string();
                        let deps: Vec<u32> = t["deps"]
                            .as_array()?
                            .iter()
                            .filter_map(|d| d.as_u64().map(|n| n as u32))
                            .collect();
                        let agent = t["agent"].as_str().map(|s| s.to_string());
                        Some((title, deps, agent))
                    })
                    .collect();
                if tasks.is_empty() {
                    Err("Please provide tasks to add.".into())
                } else {
                    let has_agents = tasks.iter().any(|(_, _, a)| a.is_some());
                    if has_agents {
                        orch.add_with_agents(tasks)
                    } else {
                        orch.add(tasks.into_iter().map(|(t, d, _)| (t, d)).collect())
                    }
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

    async fn execute(&self, _workspace: &Path, arguments: &str) -> ToolResult {
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
