//! Codebase search tools — regex search and glob-based file finding.

use std::path::Path;

use regex::Regex;

use crate::tools::{is_hidden, simple_glob_match, Tool};
use crate::types::{FunctionDef, ToolDefinition, ToolResult};

/// Searches for a regex pattern across all files in the workspace.
pub struct SearchCode;
/// Finds files matching a glob pattern in the workspace.
pub struct FindFiles;

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
                            .is_none_or(|l| !l.starts_with(&rel_path))
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
