use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::tools::Tool;
use crate::types::{FunctionDef, ToolDefinition, ToolResult};

// Stores pending choices; the TUI picks them up via UiEvent::Choice
#[allow(dead_code)]
static CHOICE_TX: OnceLock<Mutex<Option<tokio::sync::oneshot::Sender<String>>>> = OnceLock::new();

#[allow(dead_code)]
pub fn take_choice_tx() -> Option<tokio::sync::oneshot::Sender<String>> {
    CHOICE_TX
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .take()
}

pub struct ChoiceTool;

#[async_trait::async_trait]
impl Tool for ChoiceTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "choice".into(),
                description: "Ask the user to pick from options. Choices format: 'single: opt1, opt2, opt3' or 'multi: opt1, opt2'. Blocks until user responds.".into(),
                parameters: serde_json::json!({
                    "type": "object", "properties": {
                        "mode": { "type": "string", "description": "'single' or 'multi' or 'input'" },
                        "options": { "type": "string", "description": "Comma-separated options (for single/multi), or prompt text (for input)" }
                    }, "required": ["mode", "options"]
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
        let mode = args["mode"].as_str().unwrap_or("single");
        let options = args["options"].as_str().unwrap_or("");

        // For now, return the choice as plain text (TUI integration needs UiEvent plumbing)
        if mode == "input" {
            return ToolResult {
                tool_call_id: String::new(),
                content: format!(
                    "Prompt: {options}\n(Input not yet supported - reply with your answer)"
                ),
                is_error: false,
            };
        }

        let opts: Vec<&str> = options
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if opts.is_empty() {
            return ToolResult {
                tool_call_id: String::new(),
                content: "No options provided.".into(),
                is_error: true,
            };
        }

        let list: String = opts
            .iter()
            .enumerate()
            .map(|(i, o)| format!("  {}. {}\n", i + 1, o))
            .collect();
        let prompt = format!("Choose ({mode}):\n{list}\nReply with the number(s) of your choice.");
        ToolResult {
            tool_call_id: String::new(),
            content: prompt,
            is_error: false,
        }
    }
}

type AnnotationMap = std::collections::HashMap<String, Vec<(usize, String)>>;

fn annotations() -> &'static Mutex<AnnotationMap> {
    static A: OnceLock<Mutex<AnnotationMap>> = OnceLock::new();
    A.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

pub struct AnnotateTool;

#[async_trait::async_trait]
impl Tool for AnnotateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "annotate".into(),
                description: "Add virtual notes/annotations to file lines without modifying the file. Actions: 'add <path> <line> <note>', 'list [path]', 'clear [path]'.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "description": "'add <path> <line> <note>', 'list [path]', 'clear [path]'" }
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
        let mut ann = annotations().lock().unwrap();

        // Parse: "add path.rs 42 this is a note"
        if let Some(rest) = action.strip_prefix("add ") {
            let parts: Vec<&str> = rest.splitn(3, ' ').collect();
            if parts.len() < 3 {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: "Usage: add <path> <line> <note>".into(),
                    is_error: true,
                };
            }
            let path = parts[0].to_string();
            let line: usize = match parts[1].parse() {
                Ok(n) => n,
                Err(_) => {
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: "Invalid line number".into(),
                        is_error: true,
                    }
                }
            };
            let note = parts[2].to_string();
            ann.entry(path.clone())
                .or_default()
                .push((line, note.clone()));
            return ToolResult {
                tool_call_id: String::new(),
                content: format!("Annotation added to {path}:{line} — {note}"),
                is_error: false,
            };
        }

        if action == "list" || action.starts_with("list ") {
            let filter = action
                .strip_prefix("list ")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            let mut out = String::from("Annotations:\n");
            let mut found = false;
            for (path, notes) in ann.iter() {
                if let Some(f) = filter {
                    if path != f {
                        continue;
                    }
                }
                for (line, note) in notes {
                    out.push_str(&format!("  {path}:{line} — {note}\n"));
                    found = true;
                }
            }
            if !found {
                out = "No annotations.".into();
            }
            return ToolResult {
                tool_call_id: String::new(),
                content: out,
                is_error: false,
            };
        }

        if action == "clear" || action.starts_with("clear ") {
            if let Some(path) = action
                .strip_prefix("clear ")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                ann.remove(path);
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Cleared annotations for {path}"),
                    is_error: false,
                };
            }
            ann.clear();
            return ToolResult {
                tool_call_id: String::new(),
                content: "Cleared all annotations.".into(),
                is_error: false,
            };
        }

        ToolResult {
            tool_call_id: String::new(),
            content: format!("Unknown action: {action}. Use add/list/clear."),
            is_error: true,
        }
    }
}

/// Get annotations for a file path (called by read_file to append notes).
pub(crate) fn get_annotations(path: &str) -> Vec<(usize, String)> {
    annotations()
        .lock()
        .unwrap()
        .get(path)
        .cloned()
        .unwrap_or_default()
}
