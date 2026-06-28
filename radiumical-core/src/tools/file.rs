use std::path::PathBuf;

use crate::tools::interact::get_annotations;
use crate::tools::{crlf_to_lf, lf_to_crlf, Tool};
use crate::types::{FunctionDef, ToolDefinition, ToolResult};
use similar::{ChangeTag, TextDiff};

pub struct ReadFile;
pub struct WriteFile;
pub struct EditFile;

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

        let mut output = format!("File: {path_str} (lines {start}-{display_end} of {total}");
        if has_more {
            output.push_str(&format!(", page of {PAGE_SIZE}"));
        }
        output.push_str(")\n\n");
        for (i, line) in lines[start - 1..display_end].iter().enumerate() {
            let line_num = start + i;
            // Strip trailing \r from CRLF files for clean display
            let clean = line.trim_end_matches('\r');
            output.push_str(&format!("{:>6} | {}\n", line_num, clean));
        }
        if has_more {
            output.push_str(&format!(
                "\n  (Use read_file with start_line={} to see more)\n",
                display_end + 1
            ));
        }

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
                ChangeTag::Equal => {
                    skipped += 1;
                    continue;
                }
            };
            // Show context gap after changes
            if skipped > 0 {
                if skipped > 8 {
                    diff_out.push_str(&format!("  ... ({skipped} lines skipped)\n"));
                } else {
                    for _ in 0..skipped.min(2) {
                        diff_out.push_str("  ...\n");
                    }
                }
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
