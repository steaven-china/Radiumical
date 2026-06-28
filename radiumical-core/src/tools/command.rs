use std::path::PathBuf;
use std::process::Command;

use crate::tools::Tool;
use crate::types::{FunctionDef, ToolDefinition, ToolResult};

pub struct RunCommand;

#[async_trait::async_trait]
impl Tool for RunCommand {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "run_command".into(),
                description: "Execute a shell command in the workspace directory. Use this to run builds, tests, linting, or any shell command. Returns stdout and stderr. Command times out after 120 seconds.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute"
                        }
                    },
                    "required": ["command"]
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
                    content: format!("Invalid JSON arguments: {e}"),
                    is_error: true,
                }
            }
        };

        let cmd_str = args["command"].as_str().unwrap_or("").to_string();

        // Execute via sh on unix, cmd on windows
        #[cfg(target_os = "windows")]
        let (shell, flag): (String, String) = ("cmd".into(), "/C".into());
        #[cfg(not(target_os = "windows"))]
        let (shell, flag): (String, String) = ("sh".into(), "-c".into());

        // Force UTF-8 codepage on Windows to avoid GBK mojibake in output
        #[cfg(target_os = "windows")]
        let cmd_str = format!("chcp 65001 > nul && {}", cmd_str);

        let ws_clone = workspace.clone();
        let cmd = cmd_str.clone();
        let output = match tokio::task::spawn_blocking(move || {
            Command::new(&shell)
                .arg(&flag)
                .arg(&cmd)
                .current_dir(&ws_clone)
                .output()
        })
        .await
        {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Failed to execute command: {e}"),
                    is_error: true,
                }
            }
            Err(je) => {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Command panicked: {je}"),
                    is_error: true,
                }
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit_code = output.status.code().unwrap_or(-1);

        let mut result = String::new();
        result.push_str(&format!("Command: {cmd_str}\n"));
        result.push_str(&format!("Exit code: {exit_code}\n\n"));

        if !stdout.is_empty() {
            result.push_str(&format!("STDOUT:\n{stdout}\n"));
        }
        if !stderr.is_empty() {
            result.push_str(&format!("STDERR:\n{stderr}\n"));
        }
        if stdout.is_empty() && stderr.is_empty() {
            result.push_str("(no output)\n");
        }

        ToolResult {
            tool_call_id: String::new(),
            content: result,
            is_error: exit_code != 0,
        }
    }
}
