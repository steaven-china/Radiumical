//! Shell command execution tool.

use std::path::Path;
use std::process::Command;

use crate::tools::Tool;
use crate::types::{FunctionDef, ToolDefinition, ToolResult};

/// Executes a shell command in the workspace directory and returns stdout/stderr.
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

    async fn execute(&self, workspace: &Path, arguments: &str) -> ToolResult {
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

        // Execute via sh on unix; on Windows prefer Git Bash if available,
        // otherwise fall back to cmd.
        #[cfg(target_os = "windows")]
        let (shell, flag, cmd_str): (String, String, String) = if let Some(bash) = find_git_bash() {
            (bash, "-c".into(), cmd_str)
        } else {
            (
                "cmd".into(),
                "/C".into(),
                format!("chcp 65001 > nul && {}", cmd_str),
            )
        };
        #[cfg(not(target_os = "windows"))]
        let (shell, flag, cmd_str): (String, String, String) = ("sh".into(), "-c".into(), cmd_str);

        let ws_clone = workspace.to_path_buf();
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

/// Locate a usable Git Bash executable on Windows.
///
/// Checks common installation paths first, then falls back to PATH lookup.
#[cfg(target_os = "windows")]
fn find_git_bash() -> Option<String> {
    use std::path::PathBuf;
    let candidates: Vec<PathBuf> = vec![
        PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"),
        PathBuf::from(r"C:\Program Files (x86)\Git\bin\bash.exe"),
        PathBuf::from(r"C:\Program Files\Git\usr\bin\bash.exe"),
        PathBuf::from(r"C:\Program Files (x86)\Git\usr\bin\bash.exe"),
    ];
    for candidate in candidates {
        if candidate.exists() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    // Fallback: search PATH for bash.exe.
    if let Ok(path_env) = std::env::var("PATH") {
        for dir in path_env.split(';') {
            let candidate = PathBuf::from(dir).join("bash.exe");
            if candidate.exists() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }
    None
}
