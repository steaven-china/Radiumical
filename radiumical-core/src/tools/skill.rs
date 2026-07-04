//! Skill discovery and loading tools.

use std::path::Path;

use async_trait::async_trait;

use crate::skill;
use crate::tools::Tool;
use crate::types::{FunctionDef, ToolDefinition, ToolResult};

fn skill_tool_defs() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "list_skills".into(),
                description: "List available agent skills. Returns name and description for each \
                    skill. Use this to discover skills before loading one."
                    .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        },
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "load_skill".into(),
                description: "Load a skill's full instructions by name. Returns the SKILL.md \
                    content. Use list_skills first to see available skills, then load the one \
                    relevant to the current task."
                    .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "The skill name (e.g. 'code-review', 'debug')"
                        }
                    },
                    "required": ["name"]
                }),
            },
        },
    ]
}

/// Lists available agent skills with their names and descriptions.
pub struct ListSkillsTool;

#[async_trait]
impl Tool for ListSkillsTool {
    fn definition(&self) -> ToolDefinition {
        skill_tool_defs().into_iter().next().unwrap()
    }

    async fn execute(&self, _workspace: &Path, _arguments: &str) -> ToolResult {
        let metas = skill::discover();
        if metas.is_empty() {
            return ToolResult {
                tool_call_id: String::new(),
                content: "No skills installed. Place skills in ~/.radi/skills/{name}/SKILL.md"
                    .into(),
                is_error: false,
            };
        }
        let mut out = format!("Available skills ({}):\n\n", metas.len());
        for m in &metas {
            out.push_str(&format!("- **{}**: {}\n", m.name, m.description));
        }
        out.push_str("\nUse load_skill with a skill name to load its instructions.");
        ToolResult {
            tool_call_id: String::new(),
            content: out,
            is_error: false,
        }
    }
}

/// Loads a skill's full instructions by name.
pub struct LoadSkillTool;

#[async_trait]
impl Tool for LoadSkillTool {
    fn definition(&self) -> ToolDefinition {
        skill_tool_defs().into_iter().nth(1).unwrap()
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
        match skill::load(name) {
            Some(s) => ToolResult {
                tool_call_id: String::new(),
                content: format!(
                    "# Skill: {}\n\n{}\n\n---\n\n{}",
                    s.name, s.description, s.instructions
                ),
                is_error: false,
            },
            None => {
                let available: Vec<String> =
                    skill::discover().iter().map(|m| m.name.clone()).collect();
                ToolResult {
                    tool_call_id: String::new(),
                    content: format!(
                        "Skill '{}' not found. Available: {}",
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
    async fn test_list_skills() {
        let tool = ListSkillsTool;
        let result = tool.execute(&PathBuf::from("."), "{}").await;
        // Should not error (may be empty if no skills installed)
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_load_skill_not_found() {
        let tool = LoadSkillTool;
        let result = tool
            .execute(&PathBuf::from("."), r#"{"name":"nonexistent"}"#)
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("not found"));
    }
}
