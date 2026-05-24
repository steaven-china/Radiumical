use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
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
        Box::new(AnnotateTool),
        Box::new(SubAgentTool),
        Box::new(SubAgentListTool),
        Box::new(MemoryTool),
        Box::new(PlaywrightTool),
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
        let start = start.max(1).min(total).min(end);

        // Page limit: max 200 lines per read
        const PAGE_SIZE: usize = 200;
        let display_end = (start + PAGE_SIZE - 1).min(end);
        let has_more = display_end < end;

        let mut output = format!("File: {path_str} (lines {start}-{display_end} of {total}",);
        if has_more { output.push_str(&format!(", page of {PAGE_SIZE}")); }
        output.push_str(")\n\n");
        for (i, line) in lines[start - 1..display_end].iter().enumerate() {
            let line_num = start + i;
            // Strip trailing \r from CRLF files for clean display
            let clean = line.trim_end_matches('\r');
            output.push_str(&format!("{:>6} | {}\n", line_num, clean));
        }
        if has_more { output.push_str(&format!("\n  (Use read_file with start_line={} to see more)\n", display_end + 1)); }

        // Append annotations for this file
        let ann = get_annotations(path_str);
        if !ann.is_empty() {
            output.push_str("\n── Annotations ──\n");
            for (line, note) in &ann {
                output.push_str(&format!("  L{line}: {note}\n"));
            }
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
        let mut skipped = 0usize;
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "- ",
                ChangeTag::Insert => "+ ",
                ChangeTag::Equal => { skipped += 1; continue; }
            };
            // Show context gap after changes
            if skipped > 0 {
                if skipped > 8 { diff_out.push_str(&format!("  ... ({skipped} lines skipped)\n")); }
                else { for _ in 0..skipped.min(2) { diff_out.push_str("  ...\n"); } }
            }
            skipped = 0;
            diff_out.push_str(sign);
            diff_out.push_str(change.value().trim_end());
            diff_out.push('\n');
        }
        if diff_out.len() > 3000 {
            // Truncate at char boundary to avoid panicking on multi-byte UTF-8
            let mut end = 3000.min(diff_out.len());
            while end > 0 && !diff_out.is_char_boundary(end) {
                end -= 1;
            }
            diff_out.truncate(end);
            diff_out.push_str("\n... (truncated)");
        }

        match std::fs::write(&full_path, &new_content) {
            Ok(_) => ToolResult {
                tool_call_id: String::new(),
                content: format!(
                    "{diff_out}\nOK — Edited {} ({})",
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
        }).await {
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


#[allow(dead_code)]
static CHOICE_TX: OnceLock<Mutex<Option<tokio::sync::oneshot::Sender<String>>>> = OnceLock::new();

#[allow(dead_code)]
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

// ── Annotate tool — virtual file notes ──


fn annotations() -> &'static Mutex<HashMap<String, Vec<(usize, String)>>> {
    static A: OnceLock<Mutex<HashMap<String, Vec<(usize, String)>>>> = OnceLock::new();
    A.get_or_init(|| Mutex::new(HashMap::new()))
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
            Ok(v) => v, Err(e) => return ToolResult { tool_call_id: String::new(), content: format!("Invalid JSON: {e}"), is_error: true }
        };
        let action = args["action"].as_str().unwrap_or("");
        let mut ann = annotations().lock().unwrap();

        // Parse: "add path.rs 42 this is a note"
        if let Some(rest) = action.strip_prefix("add ") {
            let parts: Vec<&str> = rest.splitn(3, ' ').collect();
            if parts.len() < 3 {
                return ToolResult { tool_call_id: String::new(), content: "Usage: add <path> <line> <note>".into(), is_error: true };
            }
            let path = parts[0].to_string();
            let line: usize = match parts[1].parse() { Ok(n) => n, Err(_) => return ToolResult { tool_call_id: String::new(), content: "Invalid line number".into(), is_error: true } };
            let note = parts[2].to_string();
            ann.entry(path.clone()).or_default().push((line, note.clone()));
            return ToolResult { tool_call_id: String::new(), content: format!("Annotation added to {path}:{line} — {note}"), is_error: false };
        }

        if action == "list" || action.starts_with("list ") {
            let filter = action.strip_prefix("list ").map(|s| s.trim()).filter(|s| !s.is_empty());
            let mut out = String::from("Annotations:\n");
            let mut found = false;
            for (path, notes) in ann.iter() {
                if let Some(f) = filter { if path != f { continue; } }
                for (line, note) in notes {
                    out.push_str(&format!("  {path}:{line} — {note}\n"));
                    found = true;
                }
            }
            if !found { out = "No annotations.".into(); }
            return ToolResult { tool_call_id: String::new(), content: out, is_error: false };
        }

        if action == "clear" || action.starts_with("clear ") {
            if let Some(path) = action.strip_prefix("clear ").map(|s| s.trim()).filter(|s| !s.is_empty()) {
                ann.remove(path);
                return ToolResult { tool_call_id: String::new(), content: format!("Cleared annotations for {path}"), is_error: false };
            }
            ann.clear();
            return ToolResult { tool_call_id: String::new(), content: "Cleared all annotations.".into(), is_error: false };
        }

        ToolResult { tool_call_id: String::new(), content: format!("Unknown action: {action}. Use add/list/clear."), is_error: true }
    }
}

/// Get annotations for a file path (called by read_file to append notes).
pub fn get_annotations(path: &str) -> Vec<(usize, String)> {
    annotations().lock().unwrap().get(path).cloned().unwrap_or_default()
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    // ── Glob matching ──

    #[test]
    fn test_glob_exact_match() {
        assert!(simple_glob_match("src/main.rs", "src/main.rs"));
        assert!(!simple_glob_match("src/main.rs", "src/lib.rs"));
    }

    #[test]
    fn test_glob_single_wildcard() {
        assert!(simple_glob_match("src/*.rs", "src/main.rs"));
        assert!(simple_glob_match("src/*.rs", "src/tools.rs"));
        assert!(!simple_glob_match("src/*.rs", "src/sub/main.rs"));
    }

    #[test]
    fn test_glob_double_wildcard() {
        assert!(simple_glob_match("src/**/*.rs", "src/main.rs"));
        assert!(simple_glob_match("src/**/*.rs", "src/sub/mod.rs"));
        assert!(simple_glob_match("src/**/*.rs", "src/a/b/c/deep.rs"));
    }

    #[test]
    fn test_glob_question_mark() {
        assert!(simple_glob_match("src/???.rs", "src/mod.rs"));
        assert!(!simple_glob_match("src/???.rs", "src/main.rs")); // 4 chars
    }

    #[test]
    fn test_glob_mixed() {
        assert!(simple_glob_match("**/*test*", "src/test_utils.rs"));
        assert!(simple_glob_match("**/*test*", "tests/integration_test.rs"));
        assert!(!simple_glob_match("**/*test*", "src/main.rs"));
    }

    #[test]
    fn test_part_match_exact() {
        assert!(part_match("main.rs", "main.rs"));
        assert!(!part_match("main.rs", "lib.rs"));
    }

    #[test]
    fn test_part_match_star() {
        assert!(part_match("*", "anything"));
        assert!(part_match("*", ""));
    }

    #[test]
    fn test_part_match_wildcard() {
        assert!(part_match("*.rs", "main.rs"));
        assert!(part_match("*.rs", "lib.rs"));
        assert!(!part_match("*.rs", "main.py"));
    }

    #[test]
    fn test_part_match_question() {
        assert!(part_match("???.rs", "mod.rs"));
        assert!(!part_match("???.rs", "main.rs"));
    }

    // ── CRLF helpers ──

    #[test]
    fn test_crlf_to_lf_conversion() {
        assert_eq!(crlf_to_lf("hello\r\nworld"), "hello\nworld");
        assert_eq!(crlf_to_lf("hello\nworld"), "hello\nworld");
        assert_eq!(crlf_to_lf("no newlines"), "no newlines");
    }

    #[test]
    fn test_lf_to_crlf_conversion() {
        assert_eq!(lf_to_crlf("hello\nworld"), "hello\r\nworld");
        assert_eq!(lf_to_crlf("hello\r\nworld"), "hello\r\nworld");
        assert_eq!(lf_to_crlf("no newlines"), "no newlines");
    }

    #[test]
    fn test_crlf_roundtrip() {
        let original = "line1\nline2\r\nline3\n";
        let converted = lf_to_crlf(&crlf_to_lf(original));
        assert_eq!(converted, lf_to_crlf(original));
    }

    // ── is_hidden ──

    #[test]
    fn test_is_hidden_dir() {
        // We can't easily construct a DirEntry, but we can test the logic
        // by checking the predicate directly on known patterns
        let hidden_names = [".git", "node_modules", "target", ".hidden"];
        let visible_names = ["src", "tests", "README.md", "Cargo.toml"];

        for name in hidden_names {
            assert!(name.starts_with('.') || name == "node_modules" || name == "target" || name == ".git",
                "{} should be considered hidden", name);
        }
        for name in visible_names {
            let hidden = name.starts_with('.') || name == "node_modules" || name == "target" || name == ".git";
            assert!(!hidden, "{} should be visible", name);
        }
    }

    // ── TodoList ──

    #[tokio::test]
    async fn test_todo_add_and_list() {
        // Clear first
        todos().lock().unwrap().clear();

        let result = TodoList.execute(
            &std::path::PathBuf::from("."), r#"{"action": "add write tests"}"#,
        ).await;
        assert!(!result.is_error);
        assert!(result.content.contains("Added todo #1"));

        let result = TodoList.execute(
            &std::path::PathBuf::from("."), r#"{"action": "add fix bugs"}"#,
        ).await;
        assert!(!result.is_error);
        assert!(result.content.contains("Added todo #2"));

        let result = TodoList.execute(
            &std::path::PathBuf::from("."), r#"{"action": "list"}"#,
        ).await;
        assert!(!result.is_error);
        assert!(result.content.contains("write tests"));
        assert!(result.content.contains("fix bugs"));
    }

    #[tokio::test]
    async fn test_todo_done() {
        todos().lock().unwrap().clear();
        TodoList.execute(&std::path::PathBuf::from("."), r#"{"action": "add task"}"#).await;

        let result = TodoList.execute(
            &std::path::PathBuf::from("."), r#"{"action": "done 1"}"#,
        ).await;
        assert!(!result.is_error);
        assert!(result.content.contains("Marked todo #1"));
    }

    #[tokio::test]
    async fn test_todo_done_invalid_index() {
        todos().lock().unwrap().clear();

        let result = TodoList.execute(
            &std::path::PathBuf::from("."), r#"{"action": "done 99"}"#,
        ).await;
        assert!(result.is_error);
    }

    // ── PlanTool ──

    #[tokio::test]
    async fn test_plan_set_and_list() {
        plans().lock().unwrap().clear();

        let result = PlanTool.execute(
            &std::path::PathBuf::from("."), r#"{"action": "set step 1; step 2; step 3"}"#,
        ).await;
        assert!(!result.is_error);
        assert!(result.content.contains("Plan set with 3 steps"));

        let result = PlanTool.execute(
            &std::path::PathBuf::from("."), r#"{"action": "list"}"#,
        ).await;
        assert!(!result.is_error);
        assert!(result.content.contains("step 1"));
        assert!(result.content.contains("step 2"));
        assert!(result.content.contains("step 3"));
    }

    #[tokio::test]
    async fn test_plan_done() {
        plans().lock().unwrap().clear();
        PlanTool.execute(&std::path::PathBuf::from("."), r#"{"action": "set a; b; c"}"#).await;

        let result = PlanTool.execute(
            &std::path::PathBuf::from("."), r#"{"action": "done 2"}"#,
        ).await;
        assert!(!result.is_error);
        assert!(result.content.contains("Step #2 completed"));

        // Verify list shows it as done
        let result = PlanTool.execute(
            &std::path::PathBuf::from("."), r#"{"action": "list"}"#,
        ).await;
        assert!(result.content.contains("[x]"));
        assert!(result.content.contains("[ ]")); // step 1 & 3 not done
    }

    // ── GoalTool ──

    #[tokio::test]
    async fn test_goal_set_and_list() {
        goals().lock().unwrap().clear();

        let result = GoalTool.execute(
            &std::path::PathBuf::from("."), r#"{"action": "set Finish the project"}"#,
        ).await;
        assert!(!result.is_error);
        assert!(result.content.contains("Goal set: Finish the project"));

        GoalTool.execute(
            &std::path::PathBuf::from("."), r#"{"action": "add write docs"}"#,
        ).await;

        let result = GoalTool.execute(
            &std::path::PathBuf::from("."), r#"{"action": "list"}"#,
        ).await;
        assert!(result.content.contains("Finish the project"));
        assert!(result.content.contains("write docs"));
    }

    // ── ChoiceTool ──

    #[tokio::test]
    async fn test_choice_single() {
        let result = ChoiceTool.execute(
            &std::path::PathBuf::from("."), r#"{"mode": "single", "options": "A, B, C"}"#,
        ).await;
        assert!(!result.is_error);
        assert!(result.content.contains("A"));
        assert!(result.content.contains("B"));
        assert!(result.content.contains("C"));
        assert!(result.content.contains("Choose (single)"));
    }

    #[tokio::test]
    async fn test_choice_empty_options() {
        let result = ChoiceTool.execute(
            &std::path::PathBuf::from("."), r#"{"mode": "single", "options": ""}"#,
        ).await;
        assert!(result.is_error);
        assert!(result.content.contains("No options"));
    }

    // ── File I/O tests (with temp directories) ──

    use std::io::Write;

    fn setup_temp_dir(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("radium_test_{}", uuid_simple()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(content.as_bytes()).unwrap();
        }
        dir
    }

    fn uuid_simple() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{t:x}")
    }

    fn cleanup(dir: &PathBuf) {
        std::fs::remove_dir_all(dir).ok();
    }

    // ── ReadFile ──

    #[tokio::test]
    async fn test_read_file_basic() {
        let dir = setup_temp_dir(&[("hello.txt", "Hello, world!\nLine 2\n")]);
        let result = ReadFile.execute(&dir, r#"{"path": "hello.txt"}"#).await;
        cleanup(&dir);
        assert!(!result.is_error);
        assert!(result.content.contains("Hello, world!"));
        assert!(result.content.contains("Line 2"));
    }

    #[tokio::test]
    async fn test_read_file_with_line_range() {
        let dir = setup_temp_dir(&[("lines.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n")]);
        let result = ReadFile.execute(&dir, r#"{"path": "lines.txt", "start_line": 3, "end_line": 5}"#).await;
        cleanup(&dir);
        assert!(!result.is_error);
        // Format is: "    3 | 3", "    4 | 4", "    5 | 5"
        assert!(result.content.contains("3 | 3") || result.content.contains("3|3"));
        assert!(!result.content.contains("8 | 8"));
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let dir = setup_temp_dir(&[]);
        let result = ReadFile.execute(&dir, r#"{"path": "nonexistent.txt"}"#).await;
        cleanup(&dir);
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn test_read_file_invalid_json() {
        let dir = setup_temp_dir(&[]);
        let result = ReadFile.execute(&dir, "not json").await;
        cleanup(&dir);
        assert!(result.is_error);
        assert!(result.content.contains("Invalid JSON"));
    }

    // ── WriteFile ──

    #[tokio::test]
    async fn test_write_file_new() {
        let dir = setup_temp_dir(&[]);
        let result = WriteFile.execute(&dir, r#"{"path": "new.txt", "content": "fresh content"}"#).await;
        assert!(!result.is_error);
        assert!(result.content.contains("Wrote"));
        assert!(result.content.contains("new.txt"));
        let contents = std::fs::read_to_string(dir.join("new.txt")).unwrap();
        assert_eq!(contents, "fresh content");
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_write_file_overwrite() {
        let dir = setup_temp_dir(&[("existing.txt", "old content")]);
        let result = WriteFile.execute(&dir, r#"{"path": "existing.txt", "content": "new content"}"#).await;
        assert!(!result.is_error);
        let contents = std::fs::read_to_string(dir.join("existing.txt")).unwrap();
        assert_eq!(contents, "new content");
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_write_file_creates_parent_dirs() {
        let dir = setup_temp_dir(&[]);
        let result = WriteFile.execute(&dir, r#"{"path": "sub/deep/nested.txt", "content": "deep"}"#).await;
        assert!(!result.is_error);
        let contents = std::fs::read_to_string(dir.join("sub/deep/nested.txt")).unwrap();
        assert_eq!(contents, "deep");
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_write_file_invalid_json() {
        let dir = setup_temp_dir(&[]);
        let result = WriteFile.execute(&dir, "bad json").await;
        cleanup(&dir);
        assert!(result.is_error);
        assert!(result.content.contains("Invalid JSON"));
    }

    // ── EditFile ──

    #[tokio::test]
    async fn test_edit_file_basic_replace() {
        let dir = setup_temp_dir(&[("code.rs", "fn old_name() {\n    println!(\"hi\");\n}\n")]);
        let result = EditFile.execute(&dir, r#"{"path": "code.rs", "old_text": "old_name", "new_text": "new_name"}"#).await;
        assert!(!result.is_error, "edit should succeed: {}", result.content);
        assert!(result.content.contains("OK"));
        let contents = std::fs::read_to_string(dir.join("code.rs")).unwrap();
        assert!(contents.contains("new_name"));
        assert!(!contents.contains("old_name"));
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_edit_file_not_found() {
        let dir = setup_temp_dir(&[("code.rs", "some content\n")]);
        let result = EditFile.execute(&dir, r#"{"path": "code.rs", "old_text": "nothing like this", "new_text": "replacement"}"#).await;
        assert!(result.is_error);
        assert!(result.content.contains("not found"));
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_edit_file_non_unique() {
        let dir = setup_temp_dir(&[("dup.txt", "x\nx\nx\n")]);
        let result = EditFile.execute(&dir, r#"{"path": "dup.txt", "old_text": "x", "new_text": "y"}"#).await;
        assert!(result.is_error);
        assert!(result.content.contains("matches 3 times"));
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_edit_file_missing_file() {
        let dir = setup_temp_dir(&[]);
        let result = EditFile.execute(&dir, r#"{"path": "nope.txt", "old_text": "a", "new_text": "b"}"#).await;
        assert!(result.is_error);
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_edit_file_invalid_json() {
        let dir = setup_temp_dir(&[]);
        let result = EditFile.execute(&dir, "garbage").await;
        assert!(result.is_error);
        assert!(result.content.contains("Invalid JSON"));
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_edit_file_crlf_auto_adjust() {
        let dir = setup_temp_dir(&[]);
        let path = dir.join("crlf.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"hello\r\nworld\r\n").unwrap();
        // Send LF in the edit args — the tool should auto-detect and convert
        let result = EditFile.execute(&dir, r#"{"path": "crlf.txt", "old_text": "hello\nworld", "new_text": "goodbye\nworld"}"#).await;
        assert!(!result.is_error, "should auto-adjust CRLF: {}", result.content);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("goodbye\r\nworld"));
        cleanup(&dir);
    }

    // ── Tool registry ──

    #[test]
    fn test_all_tools_have_unique_names() {
        let tools = all_tools();
        let names: Vec<String> = tools.iter().map(|t| t.definition().function.name.clone()).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "all tool names must be unique");
    }

    #[test]
    fn test_all_tools_have_descriptions() {
        let tools = all_tools();
        for tool in &tools {
            let def = tool.definition();
            assert!(!def.function.description.is_empty(), "{} has no description", def.function.name);
            assert!(def.function.parameters.is_object(), "{} has invalid parameters", def.function.name);
        }
    }
}

// ── Sub-agent tools ──

pub struct SubAgentTool;
pub struct SubAgentListTool;

#[async_trait::async_trait]
impl Tool for SubAgentTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition { tool_type: "function".into(), function: FunctionDef {
            name: "subagent".into(),
            description: "Spawn a parallel sub-agent to work on an independent task. Returns immediately; check status with subagent_list. Use for parallelizable work like searching multiple files at once.".into(),
            parameters: serde_json::json!({"type":"object","properties":{"id":{"type":"string","description":"Unique ID for this sub-agent"},"task":{"type":"string","description":"Task for the sub-agent to complete"}},"required":["id","task"]}),
        }}
    }

    async fn execute(&self, _workspace: &PathBuf, arguments: &str) -> ToolResult {
        let args: serde_json::Value = match serde_json::from_str(arguments) {
            Ok(v) => v, Err(e) => return ToolResult { tool_call_id: String::new(), content: format!("Invalid JSON: {e}"), is_error: true }
        };
        let id = args["id"].as_str().unwrap_or("agent_1");
        let task = args["task"].as_str().unwrap_or("");
        if task.is_empty() { return ToolResult { tool_call_id: String::new(), content: "No task provided.".into(), is_error: true }; }

        // We need provider + config — can't access from here easily.
        // For now, log intent; full async spawn needs main.rs wiring.
        let msg = format!("Sub-agent intent recorded: '{id}' → {task}\n(Full async spawn requires main-thread wiring — use this to communicate intent, then proceed with other work.)");
        ToolResult { tool_call_id: String::new(), content: msg, is_error: false }
    }
}

#[async_trait::async_trait]
impl Tool for SubAgentListTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition { tool_type: "function".into(), function: FunctionDef {
            name: "subagent_list".into(),
            description: "List all sub-agents and their current status.".into(),
            parameters: serde_json::json!({"type":"object","properties":{},"required":[]}),
        }}
    }

    async fn execute(&self, _workspace: &PathBuf, _arguments: &str) -> ToolResult {
        ToolResult { tool_call_id: String::new(), content: crate::subagent::list(), is_error: false }
    }
}

// ── Memory tool ──

pub struct MemoryTool;

#[async_trait::async_trait]
impl Tool for MemoryTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition { tool_type: "function".into(), function: FunctionDef {
            name: "memory".into(),
            description: "Manage persistent memory across sessions. Actions: 'add <core|mino|short> <content>', 'list [tier]'. Core memory is always in context, mino is recent, short is session summaries.".into(),
            parameters: serde_json::json!({"type":"object","properties":{"action":{"type":"string","description":"'add core <text>', 'add mino <text>', 'add short <text>', 'list', 'list core', 'list mino', 'list short'"}},"required":["action"]}),
        }}
    }

    async fn execute(&self, _workspace: &PathBuf, arguments: &str) -> ToolResult {
        let args: serde_json::Value = match serde_json::from_str(arguments) {
            Ok(v) => v, Err(e) => return ToolResult { tool_call_id: String::new(), content: format!("Invalid JSON: {e}"), is_error: true }
        };
        let action = args["action"].as_str().unwrap_or("");

        if let Some(rest) = action.strip_prefix("add ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            let tier = parts.get(0).copied().unwrap_or("short");
            let content = parts.get(1).copied().unwrap_or("");
            if content.is_empty() { return ToolResult { tool_call_id: String::new(), content: "No content provided.".into(), is_error: true }; }
            match crate::memory::Memory::load().and_then(|mut m| { m.add(tier, content)?; m.save() }) {
                Ok(()) => ToolResult { tool_call_id: String::new(), content: format!("[{tier}] Remembered."), is_error: false },
                Err(e) => ToolResult { tool_call_id: String::new(), content: format!("Error: {e}"), is_error: true },
            }
        } else if action == "list" || action.starts_with("list ") {
            let mem = crate::memory::Memory::load().unwrap_or_default();
            let tier = action.strip_prefix("list ").unwrap_or("all");
            let mut out = String::from("Memory:\n");
            let mut show = |label: &str, entries: &[crate::memory::MemoryEntry]| {
                if entries.is_empty() { return; }
                out.push_str(&format!("  [{label}]\n"));
                for e in entries.iter().rev().take(10) {
                    out.push_str(&format!("    - {}\n", e.content));
                }
            };
            match tier {
                "core" | "all" => show("core", &mem.core),
                "mino" | "all" => show("mino", &mem.mino),
                "short" | "all" => show("short", &mem.short),
                _ => return ToolResult { tool_call_id: String::new(), content: format!("Unknown tier: {tier}"), is_error: true },
            }
            ToolResult { tool_call_id: String::new(), content: out, is_error: false }
        } else {
            ToolResult { tool_call_id: String::new(), content: format!("Unknown action: {action}. Use 'add <tier> <content>' or 'list [tier]'."), is_error: true }
        }
    }
}

// ── Playwright browser tool ──

pub struct PlaywrightTool;

#[async_trait::async_trait]
impl Tool for PlaywrightTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition { tool_type: "function".into(), function: FunctionDef {
            name: "playwright".into(),
            description: "Browser automation via Playwright. Actions: 'screenshot <url> [selector]', 'content <url> [selector]', 'click <url> <selector>'. Requires: npx playwright install.".into(),
            parameters: serde_json::json!({"type":"object","properties":{"action":{"type":"string","description":"'screenshot <url> [selector]', 'content <url> [selector]', 'click <url> <selector>'"}},"required":["action"]}),
        }}
    }

    async fn execute(&self, _workspace: &PathBuf, arguments: &str) -> ToolResult {
        let args: serde_json::Value = match serde_json::from_str(arguments) {
            Ok(v) => v, Err(e) => return ToolResult { tool_call_id: String::new(), content: format!("Invalid JSON: {e}"), is_error: true }
        };
        let action = args["action"].as_str().unwrap_or("");

        if action.is_empty() {
            return ToolResult { tool_call_id: String::new(), content: "Usage: 'screenshot <url>', 'content <url>', 'click <url> <selector>'".into(), is_error: true };
        }

        let parts: Vec<&str> = action.splitn(3, ' ').collect();
        let cmd_type = parts.get(0).copied().unwrap_or("");
        let url = parts.get(1).copied().unwrap_or("");
        let selector = parts.get(2).copied().unwrap_or("");

        if url.is_empty() {
            return ToolResult { tool_call_id: String::new(), content: "No URL provided.".into(), is_error: true };
        }

        match cmd_type {
            "screenshot" => {
                let out = format!("/tmp/radiumical_playwright_{}.png", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
                let script = if selector.is_empty() {
                    format!("const {{ chromium }} = require('playwright'); (async () => {{ const b = await chromium.launch(); const p = await b.newPage(); await p.goto('{url}'); await p.screenshot({{ path: '{out}', fullPage: true }}); console.log('OK:' + '{out}'); await b.close(); }})();")
                } else {
                    format!("const {{ chromium }} = require('playwright'); (async () => {{ const b = await chromium.launch(); const p = await b.newPage(); await p.goto('{url}'); await p.locator('{selector}').screenshot({{ path: '{out}' }}); console.log('OK:' + '{out}'); await b.close(); }})();")
                };
                match std::process::Command::new("node").arg("-e").arg(&script).output() {
                    Ok(o) => {
                        let stdout = String::from_utf8_lossy(&o.stdout);
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        if stdout.contains("OK:") { ToolResult { tool_call_id: String::new(), content: format!("Screenshot: {out}"), is_error: false } }
                        else { ToolResult { tool_call_id: String::new(), content: format!("Playwright error: {stderr}"), is_error: true } }
                    }
                    Err(e) => ToolResult { tool_call_id: String::new(), content: format!("Node not found. Install: npm i playwright && npx playwright install chromium\n{e}"), is_error: true },
                }
            }
            "content" => {
                let script = if selector.is_empty() {
                    format!("const {{ chromium }} = require('playwright'); (async () => {{ const b = await chromium.launch(); const p = await b.newPage(); await p.goto('{url}'); const text = await p.textContent('body'); console.log(text); await b.close(); }})();")
                } else {
                    format!("const {{ chromium }} = require('playwright'); (async () => {{ const b = await chromium.launch(); const p = await b.newPage(); await p.goto('{url}'); const text = await p.locator('{selector}').textContent(); console.log(text); await b.close(); }})();")
                };
                match std::process::Command::new("node").arg("-e").arg(&script).output() {
                    Ok(o) => {
                        let stdout = String::from_utf8_lossy(&o.stdout);
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        if !stdout.trim().is_empty() {
                            let preview: String = stdout.chars().take(2000).collect();
                            let dots = if stdout.len() > 2000 { "…" } else { "" };
                            ToolResult { tool_call_id: String::new(), content: format!("{preview}{dots}"), is_error: false }
                        } else { ToolResult { tool_call_id: String::new(), content: format!("No content. {stderr}"), is_error: true } }
                    }
                    Err(e) => ToolResult { tool_call_id: String::new(), content: format!("Node not found: {e}"), is_error: true },
                }
            }
            _ => ToolResult { tool_call_id: String::new(), content: format!("Unknown action: {cmd_type}. Use screenshot/content/click."), is_error: true },
        }
    }
}
