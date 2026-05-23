use regex::Regex;
use std::path::PathBuf;
use std::process::Command;

use crate::types::{FunctionDef, ToolDefinition, ToolResult};
use similar::{ChangeTag, TextDiff};

/// A tool that the agent can call.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, workspace: &PathBuf, arguments: &str) -> ToolResult;
}

// ── Tool implementations ──

pub struct ReadFile;
pub struct WriteFile;
pub struct EditFile;
pub struct SearchCode;
pub struct FindFiles;
pub struct RunCommand;

// ── Registry ──

/// Returns all tools as Vec.
pub fn all_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ReadFile),
        Box::new(WriteFile),
        Box::new(EditFile),
        Box::new(SearchCode),
        Box::new(FindFiles),
        Box::new(RunCommand),
        Box::new(TodoList),
        Box::new(PlanTool),
        Box::new(GoalTool),
        Box::new(ChoiceTool),
        Box::new(LspDiagnostics),
        Box::new(SysInfo),
        Box::new(ListDir),
        Box::new(TreeDir),
        Box::new(TimeNow),
        Box::new(CronTab),
    ]
}

// ── ReadFile ──

#[async_trait::async_trait]
impl Tool for ReadFile {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "read_file".into(),
                description: "Read the contents of a file. Returns the file content with line numbers. Use this before editing any file.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to read, relative to workspace root"
                        },
                        "start_line": {
                            "type": "integer",
                            "description": "Optional 1-based start line number"
                        },
                        "end_line": {
                            "type": "integer",
                            "description": "Optional 1-based end line number (inclusive)"
                        }
                    },
                    "required": ["path"]
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

        let path_str = args["path"].as_str().unwrap_or("");
        let full_path = workspace.join(path_str);

        let content = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(e) => {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Error reading file {}: {e}", full_path.display()),
                    is_error: true,
                }
            }
        };

        let start = args["start_line"].as_u64().map(|n| n as usize).unwrap_or(1);
        let end = args["end_line"]
            .as_u64()
            .map(|n| n as usize)
            .unwrap_or(usize::MAX);

        // Use split('\n') instead of lines() to preserve trailing empty line.
        // lines() drops the empty string after a final \n, making the last line always missing.
        let lines: Vec<&str> = content.split('\n').collect();
        let total = lines.len();
        let end = end.min(total);
        let start = start.max(1).min(total).min(end); // clamp start ≤ end to avoid slice panic

        let mut output = format!("File: {path_str} (lines {start}-{end} of {total})\n\n");
        for (i, line) in lines[start - 1..end].iter().enumerate() {
            let line_num = start + i;
            // Strip trailing \r from CRLF files for clean display
            let clean = line.trim_end_matches('\r');
            output.push_str(&format!("{:>6} | {}\n", line_num, clean));
        }

        ToolResult {
            tool_call_id: String::new(),
            content: output,
            is_error: false,
        }
    }
}

// ── WriteFile ──

#[async_trait::async_trait]
impl Tool for WriteFile {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "write_file".into(),
                description: "Create a new file or overwrite an existing file with the given content. Use this for creating new files or completely rewriting existing ones.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file, relative to workspace root"
                        },
                        "content": {
                            "type": "string",
                            "description": "The complete file content"
                        }
                    },
                    "required": ["path", "content"]
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

        let path_str = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        let full_path = workspace.join(path_str);

        // Ensure parent dir exists
        if let Some(parent) = full_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Failed to create directory: {e}"),
                    is_error: true,
                };
            }
        }

        match std::fs::write(&full_path, content) {
            Ok(_) => ToolResult {
                tool_call_id: String::new(),
                content: format!("Wrote {} bytes to {}", content.len(), path_str),
                is_error: false,
            },
            Err(e) => ToolResult {
                tool_call_id: String::new(),
                content: format!("Failed to write {}: {e}", path_str),
                is_error: true,
            },
        }
    }
}

// ── EditFile (search & replace) ──

#[async_trait::async_trait]
impl Tool for EditFile {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "edit_file".into(),
                description: "Make targeted edits by replacing old_text with new_text. Line endings (CRLF/LF) are auto-detected and normalized — you don't need to worry about matching them exactly. The old_text must be unique within the file.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file, relative to workspace root"
                        },
                        "old_text": {
                            "type": "string",
                            "description": "The exact text to find and replace. Must be unique within the file."
                        },
                        "new_text": {
                            "type": "string",
                            "description": "The replacement text"
                        }
                    },
                    "required": ["path", "old_text", "new_text"]
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

        let path_str = args["path"].as_str().unwrap_or("");
        let mut old_text = args["old_text"].as_str().unwrap_or("").to_string();
        let mut new_text = args["new_text"].as_str().unwrap_or("").to_string();
        let full_path = workspace.join(path_str);

        let raw = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(e) => {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Error reading {}: {e}", full_path.display()),
                    is_error: true,
                }
            }
        };

        // Detect line ending: if the file contains \r\n, it's CRLF
        let is_crlf = raw.contains("\r\n");

        // Convert search/replace strings to match file's line ending
        if is_crlf {
            old_text = lf_to_crlf(&old_text);
            new_text = lf_to_crlf(&new_text);
        }

        // Count occurrences
        let count = raw.matches(&old_text).count();

        if count == 0 {
            // Fallback: normalize both sides to LF, then try matching.
            // This handles the case where LLM and file use opposing line endings.
            let old_lf = crlf_to_lf(&old_text);
            let raw_lf = crlf_to_lf(&raw);
            let new_lf = crlf_to_lf(&new_text);

            let lf_count = raw_lf.matches(&old_lf).count();
            if lf_count == 1 {
                let new_content_lf = raw_lf.replacen(&old_lf, &new_lf, 1);
                // Restore the original line ending style
                let new_content = if is_crlf {
                    lf_to_crlf(&new_content_lf)
                } else {
                    new_content_lf
                };
                std::fs::write(&full_path, &new_content).ok();
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!(
                        "Edited {} (auto-adjusted line endings). Replaced 1 occurrence.",
                        path_str
                    ),
                    is_error: false,
                };
            }

            return ToolResult {
                tool_call_id: String::new(),
                content: format!(
                    "old_text not found in {}. File has {} line endings.",
                    path_str,
                    if is_crlf { "CRLF" } else { "LF" }
                ),
                is_error: true,
            };
        }

        if count > 1 {
            return ToolResult {
                tool_call_id: String::new(),
                content: format!(
                    "old_text matches {count} times in {}. Provide more context for unique match.",
                    path_str
                ),
                is_error: true,
            };
        }

        let new_content = raw.replacen(&old_text, &new_text, 1);
        let diff = TextDiff::from_lines(&raw, &new_content);
        let mut diff_out = String::from("Changes:\n");
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "- ",
                ChangeTag::Insert => "+ ",
                ChangeTag::Equal => "  ",
            };
            diff_out.push_str(sign);
            diff_out.push_str(&change.value().replace('\n', "\n  "));
        }

        match std::fs::write(&full_path, &new_content) {
            Ok(_) => ToolResult {
                tool_call_id: String::new(),
                content: format!(
                    "Edited {} ({}). Replaced 1 occurrence.\n{diff_out}",
                    path_str,
                    if is_crlf { "CRLF" } else { "LF" }
                ),
                is_error: false,
            },
            Err(e) => ToolResult {
                tool_call_id: String::new(),
                content: format!("Failed to write {}: {e}", path_str),
                is_error: true,
            },
        }
    }
}

// ── SearchCode (regex grep) ──

#[async_trait::async_trait]
impl Tool for SearchCode {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "search_code".into(),
                description: "Search for a regex pattern across all files in the workspace. Returns file paths and matching lines. Use this to find definitions, usages, or patterns in the codebase.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Regex pattern to search for"
                        },
                        "include": {
                            "type": "string",
                            "description": "Optional glob pattern to filter files (e.g., '**/*.rs', 'src/**/*.ts')"
                        },
                        "case_sensitive": {
                            "type": "boolean",
                            "description": "Whether the search is case-sensitive. Default: false"
                        }
                    },
                    "required": ["pattern"]
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

        let pattern = args["pattern"].as_str().unwrap_or("");
        let case_sensitive = args["case_sensitive"].as_bool().unwrap_or(false);

        let re = match if case_sensitive {
            Regex::new(pattern)
        } else {
            Regex::new(&format!("(?i){pattern}"))
        } {
            Ok(r) => r,
            Err(e) => {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Invalid regex pattern: {e}"),
                    is_error: true,
                }
            }
        };

        let mut output = String::new();
        let mut total_matches = 0;
        let max_matches = 100;

        let walker = walkdir::WalkDir::new(workspace)
            .into_iter()
            .filter_entry(|e| !is_hidden(e));

        for entry in walker.filter_map(|e| e.ok()) {
            if total_matches >= max_matches {
                output.push_str("\n... (truncated, too many matches)\n");
                break;
            }

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let rel_path = match path.strip_prefix(workspace) {
                Ok(p) => p.display().to_string(),
                Err(_) => continue,
            };

            // Check include pattern
            if let Some(include) = args["include"].as_str() {
                if !simple_glob_match(include, &rel_path) {
                    continue;
                }
            }

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            for (line_num, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    if total_matches == 0
                        || output
                            .lines()
                            .last()
                            .map_or(true, |l| !l.starts_with(&rel_path))
                    {
                        output.push_str(&format!("\n{}:\n", rel_path));
                    }
                    output.push_str(&format!("  {:>4}: {}\n", line_num + 1, line.trim()));
                    total_matches += 1;
                    if total_matches >= max_matches {
                        break;
                    }
                }
            }
        }

        if total_matches == 0 {
            output = format!("No matches found for pattern: {pattern}");
        } else {
            output = format!("Found {total_matches} matches for pattern: {pattern}\n{output}");
        }

        ToolResult {
            tool_call_id: String::new(),
            content: output,
            is_error: false,
        }
    }
}

// ── FindFiles (glob) ──

#[async_trait::async_trait]
impl Tool for FindFiles {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "find_files".into(),
                description: "Find files matching a glob pattern. Returns sorted file paths. Use this to locate files by name.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Glob pattern (e.g., '**/*.rs', 'src/**/*.ts')"
                        }
                    },
                    "required": ["pattern"]
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

        let pattern = args["pattern"].as_str().unwrap_or("*");

        let mut matches: Vec<String> = Vec::new();
        let max_results = 200;

        let walker = walkdir::WalkDir::new(workspace)
            .into_iter()
            .filter_entry(|e| !is_hidden(e));

        for entry in walker.filter_map(|e| e.ok()) {
            if matches.len() >= max_results {
                break;
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let rel_path = match entry.path().strip_prefix(workspace) {
                Ok(p) => p.display().to_string(),
                Err(_) => continue,
            };
            if simple_glob_match(pattern, &rel_path) {
                matches.push(rel_path);
            }
        }

        matches.sort();

        let output = if matches.is_empty() {
            format!("No files found matching: {pattern}")
        } else {
            let count = matches.len();
            let truncated = if count >= max_results {
                " (truncated)"
            } else {
                ""
            };
            format!(
                "Found {count} files{truncated} matching {pattern}:\n{}",
                matches.join("\n")
            )
        };

        ToolResult {
            tool_call_id: String::new(),
            content: output,
            is_error: false,
        }
    }
}

// ── RunCommand ──

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

        let cmd_str = args["command"].as_str().unwrap_or("");

        // Execute via sh on unix, cmd on windows
        #[cfg(target_os = "windows")]
        let (shell, flag) = ("cmd", "/C");
        #[cfg(not(target_os = "windows"))]
        let (shell, flag) = ("sh", "-c");

        // Force UTF-8 codepage on Windows to avoid GBK mojibake in output
        #[cfg(target_os = "windows")]
        let cmd_str = format!("chcp 65001 > nul && {}", cmd_str);

        let output = match Command::new(shell)
            .arg(flag)
            .arg(&cmd_str)
            .current_dir(workspace)
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Failed to execute command: {e}"),
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

// ── Helpers ──

fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.') || s == "node_modules" || s == "target" || s == ".git")
        .unwrap_or(false)
}

fn simple_glob_match(pattern: &str, path: &str) -> bool {
    let parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = path.split('/').collect();

    /// Recursive matching with proper ** backtracking.
    fn match_from(pi: usize, si: usize, parts: &[&str], path_parts: &[&str]) -> bool {
        if pi == parts.len() {
            return si == path_parts.len();
        }

        if parts[pi] == "**" {
            // ** matches zero or more path segments — try zero first, then each prefix
            for next_si in si..=path_parts.len() {
                if match_from(pi + 1, next_si, parts, path_parts) {
                    return true;
                }
            }
            return false;
        }

        if si >= path_parts.len() {
            return false;
        }

        if part_match(parts[pi], path_parts[si]) {
            return match_from(pi + 1, si + 1, parts, path_parts);
        }

        false
    }

    match_from(0, 0, &parts, &path_parts)
}

fn part_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern == value;
    }
    // Very basic glob matching for single part
    let re_str = pattern
        .replace('.', "\\.")
        .replace('*', ".*")
        .replace('?', ".");
    regex::Regex::new(&format!("^{re_str}$")).map_or(false, |re| re.is_match(value))
}

/// Convert LF → CRLF (for Windows files)
fn lf_to_crlf(s: &str) -> String {
    // Normalize to LF first, then convert
    s.replace("\r\n", "\n").replace("\n", "\r\n")
}

/// Convert CRLF → LF (for matching)
fn crlf_to_lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

// ── TodoList tool ──

use std::sync::{Mutex, OnceLock};

fn todos() -> &'static Mutex<Vec<(String, bool)>> {
    static TODOS: OnceLock<Mutex<Vec<(String, bool)>>> = OnceLock::new();
    TODOS.get_or_init(|| Mutex::new(Vec::new()))
}

pub struct TodoList;

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
            Ok(v) => v, Err(e) => return ToolResult { tool_call_id: String::new(), content: format!("Invalid JSON: {e}"), is_error: true }
        };
        let action = args["action"].as_str().unwrap_or("");
        let mut todos = todos().lock().unwrap();

        if action == "list" || action.is_empty() {
            if todos.is_empty() { return ToolResult { tool_call_id: String::new(), content: "No todos yet.".into(), is_error: false }; }
            let list: String = todos.iter().enumerate().map(|(i, (t, done))| {
                format!("  [{}] {} {}\n", if *done { "x" } else { " " }, i + 1, t)
            }).collect();
            return ToolResult { tool_call_id: String::new(), content: list, is_error: false };
        }

        if let Some(task) = action.strip_prefix("add ") {
            todos.push((task.to_string(), false));
            return ToolResult { tool_call_id: String::new(), content: format!("Added todo #{}: {task}", todos.len()), is_error: false };
        }

        if let Some(idx_str) = action.strip_prefix("done ") {
            if let Ok(idx) = idx_str.trim().parse::<usize>() {
                if idx > 0 && idx <= todos.len() {
                    todos[idx - 1].1 = true;
                    return ToolResult { tool_call_id: String::new(), content: format!("Marked todo #{idx} as done."), is_error: false };
                }
            }
            return ToolResult { tool_call_id: String::new(), content: format!("Invalid index: {idx_str}"), is_error: true };
        }

        if action == "clear" { todos.clear(); return ToolResult { tool_call_id: String::new(), content: "Cleared all todos.".into(), is_error: false }; }

        ToolResult { tool_call_id: String::new(), content: format!("Unknown action: {action}. Use add/done/list/clear."), is_error: true }
    }
}

// ── Plan tool ──

fn plans() -> &'static Mutex<Vec<(String, bool)>> {
    static PLANS: OnceLock<Mutex<Vec<(String, bool)>>> = OnceLock::new();
    PLANS.get_or_init(|| Mutex::new(Vec::new()))
}

pub struct PlanTool;

#[async_trait::async_trait]
impl Tool for PlanTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "plan".into(),
                description: "Create and track a step-by-step plan. Actions: 'set step1; step2; ...', 'done <index>', 'list'. Use before making changes to organize your approach.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "description": "Action: 'set step1; step2', 'done <index>', 'list'"
                        }
                    },
                    "required": ["action"]
                }),
            },
        }
    }

    async fn execute(&self, _workspace: &PathBuf, arguments: &str) -> ToolResult {
        let args: serde_json::Value = match serde_json::from_str(arguments) {
            Ok(v) => v, Err(e) => return ToolResult { tool_call_id: String::new(), content: format!("Invalid JSON: {e}"), is_error: true }
        };
        let action = args["action"].as_str().unwrap_or("");
        let mut plans = plans().lock().unwrap();

        if action == "list" || action.is_empty() {
            if plans.is_empty() { return ToolResult { tool_call_id: String::new(), content: "No plan yet.".into(), is_error: false }; }
            let list: String = plans.iter().enumerate().map(|(i, (t, done))| {
                format!("  [{}] Step {}: {}\n", if *done { "x" } else { " " }, i + 1, t)
            }).collect();
            return ToolResult { tool_call_id: String::new(), content: list, is_error: false };
        }

        if let Some(steps) = action.strip_prefix("set ") {
            plans.clear();
            for step in steps.split(';') {
                let s = step.trim();
                if !s.is_empty() { plans.push((s.to_string(), false)); }
            }
            let count = plans.len();
            return ToolResult { tool_call_id: String::new(), content: format!("Plan set with {count} steps."), is_error: false };
        }

        if let Some(idx_str) = action.strip_prefix("done ") {
            if let Ok(idx) = idx_str.trim().parse::<usize>() {
                if idx > 0 && idx <= plans.len() {
                    plans[idx - 1].1 = true;
                    return ToolResult { tool_call_id: String::new(), content: format!("Step #{idx} completed."), is_error: false };
                }
            }
            return ToolResult { tool_call_id: String::new(), content: format!("Invalid index: {idx_str}"), is_error: true };
        }

        ToolResult { tool_call_id: String::new(), content: format!("Unknown action: {action}. Use set/done/list."), is_error: true }
    }
}

// ── Goal tool ──

fn goals() -> &'static Mutex<Vec<String>> {
    static GOALS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    GOALS.get_or_init(|| Mutex::new(Vec::new()))
}

pub struct GoalTool;

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
            Ok(v) => v, Err(e) => return ToolResult { tool_call_id: String::new(), content: format!("Invalid JSON: {e}"), is_error: true }
        };
        let action = args["action"].as_str().unwrap_or("");
        let mut g = goals().lock().unwrap();

        if action == "list" || action.is_empty() {
            if g.is_empty() { return ToolResult { tool_call_id: String::new(), content: "No goals set.".into(), is_error: false }; }
            let list: String = g.iter().enumerate().map(|(i, t)| format!("  {}. {}\n", i + 1, t)).collect();
            return ToolResult { tool_call_id: String::new(), content: list, is_error: false };
        }

        if let Some(goal) = action.strip_prefix("set ") {
            g.clear(); g.push(goal.to_string());
            return ToolResult { tool_call_id: String::new(), content: format!("Goal set: {goal}"), is_error: false };
        }

        if let Some(sub) = action.strip_prefix("add ") {
            g.push(sub.to_string());
            return ToolResult { tool_call_id: String::new(), content: format!("Added sub-goal #{}: {sub}", g.len()), is_error: false };
        }

        ToolResult { tool_call_id: String::new(), content: format!("Unknown action: {action}"), is_error: true }
    }
}

// ── Choice tool ──
// Stores pending choices; the TUI picks them up via UiEvent::Choice


static CHOICE_TX: OnceLock<Mutex<Option<tokio::sync::oneshot::Sender<String>>>> = OnceLock::new();

pub fn take_choice_tx() -> Option<tokio::sync::oneshot::Sender<String>> {
    CHOICE_TX.get_or_init(|| Mutex::new(None)).lock().unwrap().take()
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
            Ok(v) => v, Err(e) => return ToolResult { tool_call_id: String::new(), content: format!("Invalid JSON: {e}"), is_error: true }
        };
        let mode = args["mode"].as_str().unwrap_or("single");
        let options = args["options"].as_str().unwrap_or("");

        // For now, return the choice as plain text (TUI integration needs UiEvent plumbing)
        if mode == "input" {
            return ToolResult { tool_call_id: String::new(), content: format!("Prompt: {options}\n(Input not yet supported - reply with your answer)"), is_error: false };
        }

        let opts: Vec<&str> = options.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        if opts.is_empty() {
            return ToolResult { tool_call_id: String::new(), content: "No options provided.".into(), is_error: true };
        }

        let list: String = opts.iter().enumerate().map(|(i, o)| format!("  {}. {}\n", i + 1, o)).collect();
        let prompt = format!("Choose ({mode}):\n{list}\nReply with the number(s) of your choice.");
        ToolResult { tool_call_id: String::new(), content: prompt, is_error: false }
    }
}

// ── LSP Diagnostics tool ──

pub struct LspDiagnostics;

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
            return ToolResult { tool_call_id: String::new(), content: "No supported language detected in workspace.".into(), is_error: true };
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
        ToolResult { tool_call_id: String::new(), content: if out.is_empty() { "No diagnostics available.".into() } else { out }, is_error: false }
    }
}

// ── System tools ──

pub struct SysInfo;
pub struct ListDir;
pub struct TreeDir;
pub struct TimeNow;
pub struct CronTab;

#[async_trait::async_trait]
impl Tool for SysInfo {
    fn definition(&self) -> ToolDefinition { ToolDefinition { tool_type: "function".into(), function: FunctionDef {
        name: "sysinfo".into(), description: "Get system information: OS, CPU, memory, disk, uptime.".into(),
        parameters: serde_json::json!({"type":"object","properties":{},"required":[]}),
    }}}
    async fn execute(&self, _ws: &PathBuf, _args: &str) -> ToolResult {
        ToolResult { tool_call_id: String::new(), content: crate::systools::sysinfo(), is_error: false }
    }
}

#[async_trait::async_trait]
impl Tool for TimeNow {
    fn definition(&self) -> ToolDefinition { ToolDefinition { tool_type: "function".into(), function: FunctionDef {
        name: "time_now".into(), description: "Get current date and time.".into(),
        parameters: serde_json::json!({"type":"object","properties":{},"required":[]}),
    }}}
    async fn execute(&self, _ws: &PathBuf, _args: &str) -> ToolResult {
        ToolResult { tool_call_id: String::new(), content: crate::systools::time_now(), is_error: false }
    }
}

#[async_trait::async_trait]
impl Tool for CronTab {
    fn definition(&self) -> ToolDefinition { ToolDefinition { tool_type: "function".into(), function: FunctionDef {
        name: "cron_info".into(), description: "Show current user crontab entries.".into(),
        parameters: serde_json::json!({"type":"object","properties":{},"required":[]}),
    }}}
    async fn execute(&self, _ws: &PathBuf, _args: &str) -> ToolResult {
        ToolResult { tool_call_id: String::new(), content: crate::systools::cron_info(), is_error: false }
    }
}

// ListDir and TreeDir need custom impls due to path argument
#[async_trait::async_trait]
impl Tool for ListDir {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition { tool_type: "function".into(), function: FunctionDef {
            name: "list_dir".into(),
            description: "List directory contents with sizes and types.".into(),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"Directory path, default: workspace root"}},"required":[]}),
        }}
    }
    async fn execute(&self, workspace: &PathBuf, arguments: &str) -> ToolResult {
        let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
        let p = args["path"].as_str().unwrap_or("");
        let dir = if p.is_empty() { workspace.clone() } else { workspace.join(p) };
        ToolResult { tool_call_id: String::new(), content: crate::systools::list_dir(&dir), is_error: false }
    }
}

#[async_trait::async_trait]
impl Tool for TreeDir {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition { tool_type: "function".into(), function: FunctionDef {
            name: "tree".into(),
            description: "Show directory tree structure (max depth 3).".into(),
            parameters: serde_json::json!({"type":"object","properties":{"path":{"type":"string","description":"Root directory, default: workspace root"}},"required":[]}),
        }}
    }
    async fn execute(&self, workspace: &PathBuf, arguments: &str) -> ToolResult {
        let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
        let p = args["path"].as_str().unwrap_or("");
        let dir = if p.is_empty() { workspace.clone() } else { workspace.join(p) };
        ToolResult { tool_call_id: String::new(), content: crate::systools::tree(&dir, 3), is_error: false }
    }
}
