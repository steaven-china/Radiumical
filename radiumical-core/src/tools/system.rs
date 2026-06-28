use std::path::PathBuf;

use crate::tools::Tool;
use crate::types::{FunctionDef, ToolDefinition, ToolResult};

pub struct LspDiagnostics;
pub struct SysInfo;
pub struct ListDir;
pub struct TreeDir;
pub struct TimeNow;
pub struct CronTab;

#[async_trait::async_trait]
impl Tool for LspDiagnostics {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "diagnostics".into(),
                description: "Run language-specific linter/checker on the workspace. Detects Rust, Python, JS/TS, Go automatically. Reports errors and warnings.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        }
    }

    async fn execute(&self, workspace: &PathBuf, _arguments: &str) -> ToolResult {
        let langs = crate::lsp::detect_language(workspace);
        if langs.is_empty() {
            return ToolResult {
                tool_call_id: String::new(),
                content: "No supported language detected in workspace.".into(),
                is_error: true,
            };
        }
        let mut out = String::new();
        for lang in &langs {
            match crate::lsp::run_diagnostics(workspace, lang) {
                Ok(diag) => {
                    if !diag.trim().is_empty() {
                        out.push_str(&format!("[{lang}]\n{diag}\n"));
                    } else {
                        out.push_str(&format!("[{lang}] No issues found.\n"));
                    }
                }
                Err(e) => out.push_str(&format!("[{lang}] {e}\n")),
            }
        }
        ToolResult {
            tool_call_id: String::new(),
            content: if out.is_empty() {
                "No diagnostics available.".into()
            } else {
                out
            },
            is_error: false,
        }
    }
}

#[async_trait::async_trait]
impl Tool for SysInfo {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "sysinfo".into(),
                description: "Get system information: OS, CPU, memory, disk, uptime.".into(),
                parameters: serde_json::json!({"type":"object","properties":{},"required":[]}),
            },
        }
    }

    async fn execute(&self, _ws: &PathBuf, _args: &str) -> ToolResult {
        ToolResult {
            tool_call_id: String::new(),
            content: crate::systools::sysinfo(),
            is_error: false,
        }
    }
}

#[async_trait::async_trait]
impl Tool for TimeNow {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "time_now".into(),
                description: "Get current date and time.".into(),
                parameters: serde_json::json!({"type":"object","properties":{},"required":[]}),
            },
        }
    }

    async fn execute(&self, _ws: &PathBuf, _args: &str) -> ToolResult {
        ToolResult {
            tool_call_id: String::new(),
            content: crate::systools::time_now(),
            is_error: false,
        }
    }
}

#[async_trait::async_trait]
impl Tool for CronTab {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "cron_info".into(),
                description: "Show current user crontab entries.".into(),
                parameters: serde_json::json!({"type":"object","properties":{},"required":[]}),
            },
        }
    }

    async fn execute(&self, _ws: &PathBuf, _args: &str) -> ToolResult {
        ToolResult {
            tool_call_id: String::new(),
            content: crate::systools::cron_info(),
            is_error: false,
        }
    }
}

#[async_trait::async_trait]
impl Tool for ListDir {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "list_dir".into(),
                description: "List directory contents with sizes and types.".into(),
                parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"Directory path, default: workspace root"}},"required":[]}),
            },
        }
    }

    async fn execute(&self, workspace: &PathBuf, arguments: &str) -> ToolResult {
        let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
        let p = args["path"].as_str().unwrap_or("");
        let dir = if p.is_empty() {
            workspace.clone()
        } else {
            workspace.join(p)
        };
        ToolResult {
            tool_call_id: String::new(),
            content: crate::systools::list_dir(&dir),
            is_error: false,
        }
    }
}

#[async_trait::async_trait]
impl Tool for TreeDir {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "tree".into(),
                description: "Show directory tree structure (max depth 3).".into(),
                parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"Root directory, default: workspace root"}},"required":[]}),
            },
        }
    }

    async fn execute(&self, workspace: &PathBuf, arguments: &str) -> ToolResult {
        let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
        let p = args["path"].as_str().unwrap_or("");
        let dir = if p.is_empty() {
            workspace.clone()
        } else {
            workspace.join(p)
        };
        ToolResult {
            tool_call_id: String::new(),
            content: crate::systools::tree(&dir, 3),
            is_error: false,
        }
    }
}
