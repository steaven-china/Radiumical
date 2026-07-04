//! Agent pool tools — list and load agent role definitions.

use std::path::Path;

use async_trait::async_trait;

use crate::agent_pool;
use crate::tools::Tool;
use crate::types::{FunctionDef, ToolDefinition, ToolResult};

/// Lists available agent roles with their names, descriptions, and modes.
pub struct ListAgentsTool;

#[async_trait]
impl Tool for ListAgentsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "list_agents".into(),
                description: "List available agent roles. Returns name, description, and mode \
                    for each agent. Use this to discover roles before loading one."
                    .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        }
    }

    async fn execute(&self, _workspace: &Path, _arguments: &str) -> ToolResult {
        let agents = agent_pool::load_agents();
        if agents.is_empty() {
            return ToolResult {
                tool_call_id: String::new(),
                content: "No agent roles installed. Place agents in ~/.radi/agents/{name}.md"
                    .into(),
                is_error: false,
            };
        }
        let mut out = format!("Available agent roles ({}):\n\n", agents.len());
        for a in &agents {
            out.push_str(&format!(
                "- **{}** ({:?}): {}\n",
                a.name, a.mode, a.description
            ));
        }
        out.push_str("\nUse load_agent with an agent name to load its full prompt and tools.");
        ToolResult {
            tool_call_id: String::new(),
            content: out,
            is_error: false,
        }
    }
}

/// Loads an agent role's full definition (prompt, mode, allowed tools) by name.
pub struct LoadAgentTool;

#[async_trait]
impl Tool for LoadAgentTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "load_agent".into(),
                description: "Load an agent role's full definition by name. Returns the prompt, \
                    mode, and allowed tools. Use list_agents first to see available roles."
                    .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "The agent name (e.g. 'coder', 'architect', 'debugger')"
                        }
                    },
                    "required": ["name"]
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
        let name = match args["name"].as_str() {
            Some(n) => n,
            None => {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: "Missing required parameter: name".into(),
                    is_error: true,
                };
            }
        };
        match agent_pool::get_agent(name) {
            Some(a) => {
                let tools_str = if a.tools.is_empty() {
                    "all".to_string()
                } else {
                    a.tools.join(", ")
                };
                ToolResult {
                    tool_call_id: String::new(),
                    content: format!(
                        "# Agent: {}\n\n**Description:** {}\n**Mode:** {:?}\n**Tools:** {}\n\n---\n\n{}",
                        a.name, a.description, a.mode, tools_str, a.prompt
                    ),
                    is_error: false,
                }
            }
            None => {
                let available: Vec<String> = agent_pool::load_agents()
                    .iter()
                    .map(|a| a.name.clone())
                    .collect();
                ToolResult {
                    tool_call_id: String::new(),
                    content: format!(
                        "Agent '{}' not found. Available: {}",
                        name,
                        available.join(", ")
                    ),
                    is_error: true,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_list_agents() {
        let tool = ListAgentsTool;
        let result = tool.execute(&PathBuf::from("."), "{}").await;
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_load_agent_not_found() {
        let tool = LoadAgentTool;
        let result = tool
            .execute(&PathBuf::from("."), r#"{"name":"nonexistent"}"#)
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("not found"));
    }
}
