use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::tools::{Tool, ToolContext};
use crate::types::{FunctionDef, ToolDefinition, ToolResult, UiEvent};

// Stores pending choice response sender; the backend loop fills it via BackendCmd::ChoiceResponse.
static CHOICE_TX: OnceLock<Mutex<Option<tokio::sync::oneshot::Sender<String>>>> = OnceLock::new();

pub fn take_choice_tx() -> Option<tokio::sync::oneshot::Sender<String>> {
    CHOICE_TX
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .take()
}

pub fn set_choice_tx(tx: tokio::sync::oneshot::Sender<String>) {
    CHOICE_TX
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap()
        .replace(tx);
}

pub struct ChoiceTool;

/// Non-interactive fallback: return a prompt listing the options.
fn choice_prompt(arguments: &str) -> ToolResult {
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
    let options_str = args["options"].as_str().unwrap_or("");

    if mode == "input" {
        return ToolResult {
            tool_call_id: String::new(),
            content: format!("Prompt: {options_str}\n(Input not available in this context)"),
            is_error: false,
        };
    }

    let opts: Vec<&str> = options_str
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

#[async_trait::async_trait]
impl Tool for ChoiceTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "choice".into(),
                description: "Ask the user to pick from options. mode='single' | 'multi' | 'input'. For single/multi, options is a comma-separated list. For input, options is the prompt text. Blocks until the user responds.".into(),
                parameters: serde_json::json!({
                    "type": "object", "properties": {
                        "mode": { "type": "string", "description": "'single' or 'multi' or 'input'" },
                        "options": { "type": "string", "description": "Comma-separated options (for single/multi), or prompt text (for input)" }
                    }, "required": ["mode", "options"]
                }),
            },
        }
    }

    async fn execute(&self, _workspace: &Path, arguments: &str) -> ToolResult {
        choice_prompt(arguments)
    }

    async fn execute_with_context(
        &self,
        _workspace: &Path,
        arguments: &str,
        ctx: &ToolContext,
    ) -> ToolResult {
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
        let options_str = args["options"].as_str().unwrap_or("");

        if mode == "input" {
            let id = format!(
                "choice_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            );
            let (tx, rx) = tokio::sync::oneshot::channel::<String>();
            set_choice_tx(tx);
            let _ = ctx.ui_tx.send(UiEvent::Choice {
                id: id.clone(),
                mode: "input".into(),
                options: vec![options_str.into()],
            }).await;
            return match rx.await {
                Ok(value) => ToolResult {
                    tool_call_id: String::new(),
                    content: value,
                    is_error: false,
                },
                Err(_) => ToolResult {
                    tool_call_id: String::new(),
                    content: "Choice cancelled or timed out.".into(),
                    is_error: true,
                },
            };
        }

        let opts: Vec<String> = options_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if opts.is_empty() {
            return ToolResult {
                tool_call_id: String::new(),
                content: "No options provided.".into(),
                is_error: true,
            };
        }

        let id = format!(
            "choice_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        set_choice_tx(tx);
        let _ = ctx.ui_tx.send(UiEvent::Choice {
            id: id.clone(),
            mode: mode.into(),
            options: opts.clone(),
        }).await;

        match rx.await {
            Ok(value) => {
                let selected: Vec<String> = value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if mode == "single" {
                    if selected.len() != 1 {
                        return ToolResult {
                            tool_call_id: String::new(),
                            content: format!("Expected a single choice, got: {value}"),
                            is_error: true,
                        };
                    }
                    let idx = match selected[0].parse::<usize>() {
                        Ok(i) if i > 0 && i <= opts.len() => i - 1,
                        _ => {
                            return ToolResult {
                                tool_call_id: String::new(),
                                content: format!("Invalid choice number: {value}"),
                                is_error: true,
                            }
                        }
                    };
                    ToolResult {
                        tool_call_id: String::new(),
                        content: opts[idx].clone(),
                        is_error: false,
                    }
                } else {
                    let mut values = Vec::new();
                    for s in &selected {
                        match s.parse::<usize>() {
                            Ok(i) if i > 0 && i <= opts.len() => {
                                values.push(opts[i - 1].clone());
                            }
                            _ => {
                                return ToolResult {
                                    tool_call_id: String::new(),
                                    content: format!("Invalid choice number: {s}"),
                                    is_error: true,
                                }
                            }
                        }
                    }
                    ToolResult {
                        tool_call_id: String::new(),
                        content: values.join(", "),
                        is_error: false,
                    }
                }
            }
            Err(_) => ToolResult {
                tool_call_id: String::new(),
                content: "Choice cancelled or timed out.".into(),
                is_error: true,
            },
        }
    }
}

type AnnotationMap = std::collections::HashMap<String, Vec<(usize, String)>>;

fn annotations() -> &'static Mutex<AnnotationMap> {
    static A: OnceLock<Mutex<AnnotationMap>> = OnceLock::new();
    A.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

pub fn get_annotations(path: &str) -> Vec<(usize, String)> {
    annotations()
        .lock()
        .unwrap()
        .get(path)
        .cloned()
        .unwrap_or_default()
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
            let path = parts[0];
            let line: usize = match parts[1].parse() {
                Ok(n) => n,
                Err(_) => {
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: "Invalid line number.".into(),
                        is_error: true,
                    }
                }
            };
            let note = parts[2];
            ann.entry(path.to_string())
                .or_default()
                .push((line, note.to_string()));
            return ToolResult {
                tool_call_id: String::new(),
                content: format!("Annotation added at {path}:{line}"),
                is_error: false,
            };
        }

        if let Some(path) = action.strip_prefix("list ") {
            let list = ann.get(path).cloned().unwrap_or_default();
            if list.is_empty() {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!("No annotations for {path}"),
                    is_error: false,
                };
            }
            let lines: Vec<String> = list
                .iter()
                .map(|(line, note)| format!("  {line}: {note}"))
                .collect();
            return ToolResult {
                tool_call_id: String::new(),
                content: lines.join("\n"),
                is_error: false,
            };
        }

        if action == "list" {
            if ann.is_empty() {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: "No annotations.".into(),
                    is_error: false,
                };
            }
            let mut out = Vec::new();
            for (path, list) in ann.iter() {
                out.push(path.clone());
                for (line, note) in list {
                    out.push(format!("  {line}: {note}"));
                }
            }
            return ToolResult {
                tool_call_id: String::new(),
                content: out.join("\n"),
                is_error: false,
            };
        }

        if let Some(path) = action.strip_prefix("clear ") {
            ann.remove(path);
            return ToolResult {
                tool_call_id: String::new(),
                content: format!("Annotations cleared for {path}"),
                is_error: false,
            };
        }

        if action == "clear" {
            ann.clear();
            return ToolResult {
                tool_call_id: String::new(),
                content: "All annotations cleared.".into(),
                is_error: false,
            };
        }

        ToolResult {
            tool_call_id: String::new(),
            content: "Usage: add <path> <line> <note> | list [path] | clear [path]".into(),
            is_error: true,
        }
    }
}
