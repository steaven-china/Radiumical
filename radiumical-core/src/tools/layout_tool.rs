use std::path::Path;

use async_trait::async_trait;

use crate::tools::layout_page;
use crate::tools::Tool;
use crate::types::{FunctionDef, ToolDefinition, ToolResult};

pub struct LayoutPageTool;

#[async_trait]
impl Tool for LayoutPageTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "layout_page".into(),
                description: "Render structured output using a layout DSL. \
                    Supports: grid(rows x cols), split(pct pct), rows, cols, box(title), table. \
                    Cells separated by | for grid/table, ||| for split/cols, --- for rows."
                    .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "spec": {
                            "type": "string",
                            "description": "Layout specification. Examples:\n\
                                grid 2x3\nHeader A | Header B | Header C\nCell 1 | Cell 2 | Cell 3\n\n\
                                table\nName | Age\nAlice | 30\nBob | 25\n\n\
                                box Title\nContent here\n\n\
                                split 60 40\nLeft pane\n|||\nRight pane\n\n\
                                rows\nBlock 1\n---\nBlock 2\n\n\
                                cols 2\nColumn 1\n|||\nColumn 2"
                        },
                        "width": {
                            "type": "integer",
                            "description": "Output width in characters (default 80)"
                        }
                    },
                    "required": ["spec"]
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
                };
            }
        };

        let spec = match args["spec"].as_str() {
            Some(s) => s,
            None => {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: "Missing required parameter: spec".into(),
                    is_error: true,
                };
            }
        };

        let width = args["width"].as_u64().unwrap_or(80) as usize;

        match layout_page::parse(spec) {
            Ok(layout) => {
                let output = layout_page::render(&layout, width);
                ToolResult {
                    tool_call_id: String::new(),
                    content: output,
                    is_error: false,
                }
            }
            Err(e) => ToolResult {
                tool_call_id: String::new(),
                content: format!("Layout parse error: {e}"),
                is_error: true,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_layout_grid() {
        let tool = LayoutPageTool;
        let result = tool
            .execute(
                &PathBuf::from("."),
                r#"{"spec":"grid 2x2\nA | B\nC | D","width":40}"#,
            )
            .await;
        assert!(!result.is_error);
        assert!(result.content.contains('A'));
        assert!(result.content.contains('D'));
    }

    #[tokio::test]
    async fn test_layout_box() {
        let tool = LayoutPageTool;
        let result = tool
            .execute(&PathBuf::from("."), r#"{"spec":"box Test\nHello world"}"#)
            .await;
        assert!(!result.is_error);
        assert!(result.content.contains("Test"));
        assert!(result.content.contains("Hello"));
    }

    #[tokio::test]
    async fn test_layout_error() {
        let tool = LayoutPageTool;
        let result = tool
            .execute(&PathBuf::from("."), r#"{"spec":"foobar"}"#)
            .await;
        assert!(result.is_error);
    }
}
