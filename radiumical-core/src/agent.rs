//! Agent — a configured role/persona for the harness.
//!
//! An agent is the policy layer: it decides which system prompt, which tools,
//! and which operating mode are in effect for a run. The [`Harness`](crate::harness::Harness) is the
//! generic execution runtime that actually drives the LLM loop.

use crate::agent_pool::AgentDef;
use crate::types::{AgentMode, ToolDefinition};

/// A concrete agent configuration used by the harness.
#[derive(Debug, Clone)]
pub struct Agent {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub allowed_tools: Vec<String>,
    pub mode: AgentMode,
}

impl Agent {
    /// Default coding assistant.
    pub fn default_coder() -> Self {
        Self {
            name: "coder".into(),
            description: "通用软件工程助手".into(),
            system_prompt: crate::types::default_system_prompt(),
            allowed_tools: Vec::new(), // empty = all tools
            mode: AgentMode::Auto,
        }
    }

    /// Filter a tool list down to the agent's allowed tools.
    pub fn filter_tools(&self, tools: &[ToolDefinition]) -> Vec<ToolDefinition> {
        if self.allowed_tools.is_empty() {
            return tools.to_vec();
        }
        let allowed: std::collections::HashSet<_> = self.allowed_tools.iter().cloned().collect();
        tools
            .iter()
            .filter(|d| allowed.contains(&d.function.name))
            .cloned()
            .collect()
    }
}

impl From<AgentDef> for Agent {
    fn from(def: AgentDef) -> Self {
        Self {
            name: def.name,
            description: def.description,
            system_prompt: def.prompt,
            allowed_tools: def.tools,
            mode: def.mode.to_agent_mode(),
        }
    }
}

impl Default for Agent {
    fn default() -> Self {
        Self::default_coder()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_filters_tools() {
        let defs = vec![
            ToolDefinition {
                tool_type: "function".into(),
                function: crate::types::FunctionDef {
                    name: "read_file".into(),
                    description: "read".into(),
                    parameters: serde_json::json!({}),
                },
            },
            ToolDefinition {
                tool_type: "function".into(),
                function: crate::types::FunctionDef {
                    name: "write_file".into(),
                    description: "write".into(),
                    parameters: serde_json::json!({}),
                },
            },
        ];
        let agent = Agent {
            name: "plan".into(),
            description: "".into(),
            system_prompt: "".into(),
            allowed_tools: vec!["read_file".into()],
            mode: AgentMode::Plan,
        };
        let filtered = agent.filter_tools(&defs);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].function.name, "read_file");
    }

    #[test]
    fn test_agent_empty_allowlist_allows_all() {
        let defs = vec![
            ToolDefinition {
                tool_type: "function".into(),
                function: crate::types::FunctionDef {
                    name: "a".into(),
                    description: "".into(),
                    parameters: serde_json::json!({}),
                },
            },
            ToolDefinition {
                tool_type: "function".into(),
                function: crate::types::FunctionDef {
                    name: "b".into(),
                    description: "".into(),
                    parameters: serde_json::json!({}),
                },
            },
        ];
        let agent = Agent::default_coder();
        assert_eq!(agent.filter_tools(&defs).len(), 2);
    }
}
