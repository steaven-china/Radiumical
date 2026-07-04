//! Source-code tool — queries registered source plugins for a file.

use std::path::Path;

use async_trait::async_trait;

use crate::tools::Tool;
use crate::types::{FunctionDef, ToolDefinition, ToolResult};

/// Tool that analyzes a source file using registered source plugins.
pub struct SourceCodeTool;

#[async_trait]
impl Tool for SourceCodeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "source_code".into(),
                description: "Analyze a source file using static-analysis plugins. Returns diagnostics, markers, and language info.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Workspace-relative path to the source file"
                        }
                    },
                    "required": ["path"]
                }),
            },
        }
    }

    async fn execute(&self, workspace: &Path, arguments: &str) -> ToolResult {
        self.execute_with_context(workspace, arguments, &crate::tools::ToolContext::default())
            .await
    }

    async fn execute_with_context(
        &self,
        workspace: &Path,
        arguments: &str,
        ctx: &crate::tools::ToolContext,
    ) -> ToolResult {
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

        let path = args["path"].as_str().unwrap_or("");
        if path.is_empty() {
            return ToolResult {
                tool_call_id: String::new(),
                content: "Missing 'path' argument".into(),
                is_error: true,
            };
        }

        let relative = std::path::Path::new(path);
        let registry = match ctx.source_plugins.as_ref() {
            Some(r) => r,
            None => {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: "No source plugins registered".into(),
                    is_error: true,
                }
            }
        };

        match registry.analyze(workspace, relative) {
            Ok(summary) => {
                let mut lines = Vec::new();
                if let Some(lang) = summary.language {
                    lines.push(format!("Language: {lang}"));
                }
                if summary.symbol_count > 0 {
                    lines.push(format!("Symbols: {}", summary.symbol_count));
                }
                for note in &summary.notes {
                    lines.push(note.clone());
                }
                for f in &summary.findings {
                    let sev = match f.severity {
                        crate::plugins::source::Severity::Info => "INFO",
                        crate::plugins::source::Severity::Warning => "WARN",
                        crate::plugins::source::Severity::Error => "ERROR",
                    };
                    lines.push(format!("{}:{}:{}: {}", sev, f.line, f.column, f.message));
                    if let Some(code) = &f.code {
                        lines.push(format!("    {code}"));
                    }
                }
                if lines.is_empty() {
                    lines.push("No findings.".into());
                }
                ToolResult {
                    tool_call_id: String::new(),
                    content: lines.join("\n"),
                    is_error: false,
                }
            }
            Err(e) => ToolResult {
                tool_call_id: String::new(),
                content: format!("Analysis failed: {e}"),
                is_error: true,
            },
        }
    }
}
