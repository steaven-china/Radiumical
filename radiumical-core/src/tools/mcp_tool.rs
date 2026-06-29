//! MCP tool wrapper — adapts remote MCP server tools to the native `Tool` trait.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use crate::mcp::{McpClient, McpToolInfo};
use crate::types::{FunctionDef, ToolDefinition, ToolResult};
use crate::tools::Tool;

/// Wraps a remote MCP tool as a native tool.
pub struct McpToolAdapter {
    pub info: McpToolInfo,
    pub client: Arc<McpClient>,
}

#[async_trait]
impl Tool for McpToolAdapter {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: format!("mcp__{}__{}", self.info.server_name, self.info.name),
                description: format!("[MCP:{}] {}", self.info.server_name, self.info.description),
                parameters: self.info.input_schema.clone(),
            },
        }
    }

    async fn execute(&self, _workspace: &PathBuf, arguments: &str) -> ToolResult {
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
        match self.client.call_tool(&self.info.name, args) {
            Ok(content) => ToolResult {
                tool_call_id: String::new(),
                content,
                is_error: false,
            },
            Err(e) => ToolResult {
                tool_call_id: String::new(),
                content: format!("MCP error: {e}"),
                is_error: true,
            },
        }
    }
}
