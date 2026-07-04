use std::path::Path;

use crate::tools::Tool;
use crate::types::{AgentMode, FunctionDef, ToolDefinition, ToolResult};

pub struct SettingsTool;

const AVAILABLE_SETTINGS: &[(&str, &str)] = &[
    (
        "model",
        "LLM model name (e.g. gpt-4o, claude-sonnet-4-20250514)",
    ),
    ("mode", "Agent mode: auto, plan, exec"),
    ("thinking_effort", "Reasoning effort: low, high, max"),
    ("cod", "Chain of Draft: on, off"),
    (
        "max_iterations",
        "Max tool-call iterations per turn (1-128)",
    ),
    (
        "llm_timeout_secs",
        "LLM request timeout in seconds (10-600)",
    ),
    (
        "tool_timeout_secs",
        "Tool execution timeout in seconds (10-1800)",
    ),
    (
        "max_context_tokens",
        "Max context tokens before compression (10000-2000000)",
    ),
    (
        "context_compress_ratio",
        "Compress when context exceeds this ratio (0.5-0.95)",
    ),
    ("auto_continue", "Auto-continue orchestrator tasks: on, off"),
];

#[async_trait::async_trait]
impl Tool for SettingsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "settings".into(),
                description: format!(
                    "Read or modify agent settings at runtime.\n\
                     Actions: 'get [key]', 'set <key> <value>', 'list', 'save'.\n\
                     Available settings:\n{}",
                    AVAILABLE_SETTINGS
                        .iter()
                        .map(|(k, d)| format!("  • {k} — {d}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["get", "set", "list", "save"],
                            "description": "Action: 'get [key]' to read, 'set <key> <value>' to change, 'list' to show all, 'save' to persist to disk"
                        },
                        "key": {
                            "type": "string",
                            "description": "Setting name (required for get/set)"
                        },
                        "value": {
                            "type": "string",
                            "description": "New value (required for set)"
                        }
                    },
                    "required": ["action"]
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

        let action = args["action"].as_str().unwrap_or("list");
        let key = args["key"].as_str().unwrap_or("");
        let value = args["value"].as_str().unwrap_or("");

        match action {
            // ── list ──
            "list" => {
                let mut out = String::from("Available settings:\n");
                for (k, d) in AVAILABLE_SETTINGS {
                    out.push_str(&format!("  • {k} — {d}\n"));
                }
                out.push_str(
                    "\nUse 'get <key>' to read current value, 'set <key> <value>' to change.",
                );
                ToolResult {
                    tool_call_id: String::new(),
                    content: out,
                    is_error: false,
                }
            }

            // ── get ──
            "get" => {
                if key.is_empty() {
                    // Return all current settings
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: "Specify a key, or use 'list' to see available settings.".into(),
                        is_error: true,
                    };
                }
                // The harness will intercept this and return the actual value.
                // For now, return a marker that the harness replaces.
                ToolResult {
                    tool_call_id: String::new(),
                    content: format!("__settings_get__:{key}"),
                    is_error: false,
                }
            }

            // ── set ──
            "set" => {
                if key.is_empty() || value.is_empty() {
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: "Usage: settings set <key> <value>".into(),
                        is_error: true,
                    };
                }

                // Validate the key
                if !AVAILABLE_SETTINGS.iter().any(|(k, _)| *k == key) {
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: format!(
                            "Unknown setting: '{key}'. Available: {}",
                            AVAILABLE_SETTINGS
                                .iter()
                                .map(|(k, _)| *k)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        is_error: true,
                    };
                }

                // Validate value types
                if let Err(e) = validate_setting(key, value) {
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: format!("Invalid value for '{key}': {e}"),
                        is_error: true,
                    };
                }

                // The harness will intercept this and apply the change.
                ToolResult {
                    tool_call_id: String::new(),
                    content: format!("__settings_set__:{key}={value}"),
                    is_error: false,
                }
            }

            // ── save ──
            "save" => {
                // The harness will intercept this and persist.
                ToolResult {
                    tool_call_id: String::new(),
                    content: "__settings_save__".into(),
                    is_error: false,
                }
            }

            _ => ToolResult {
                tool_call_id: String::new(),
                content: format!("Unknown action: '{action}'. Use: get, set, list, save."),
                is_error: true,
            },
        }
    }
}

fn validate_setting(key: &str, value: &str) -> Result<(), String> {
    match key {
        "model" => {
            if value.is_empty() {
                return Err("model name cannot be empty".into());
            }
        }
        "mode" => match value.to_lowercase().as_str() {
            "auto" | "plan" | "exec" => {}
            _ => return Err("must be: auto, plan, exec".into()),
        },
        "thinking_effort" => match value.to_lowercase().as_str() {
            "low" | "high" | "max" => {}
            _ => return Err("must be: low, high, max".into()),
        },
        "cod" => match value.to_lowercase().as_str() {
            "on" | "off" => {}
            _ => return Err("must be: on, off".into()),
        },
        "auto_continue" => match value.to_lowercase().as_str() {
            "on" | "off" => {}
            _ => return Err("must be: on, off".into()),
        },
        "max_iterations" => {
            let n: usize = value.parse().map_err(|_| "must be a number".to_string())?;
            if !(1..=128).contains(&n) {
                return Err("must be 1-128".into());
            }
        }
        "llm_timeout_secs" => {
            let n: u64 = value.parse().map_err(|_| "must be a number".to_string())?;
            if !(10..=600).contains(&n) {
                return Err("must be 10-600".into());
            }
        }
        "tool_timeout_secs" => {
            let n: u64 = value.parse().map_err(|_| "must be a number".to_string())?;
            if !(10..=1800).contains(&n) {
                return Err("must be 10-1800".into());
            }
        }
        "max_context_tokens" => {
            let n: usize = value.parse().map_err(|_| "must be a number".to_string())?;
            if !(10_000..=2_000_000).contains(&n) {
                return Err("must be 10000-2000000".into());
            }
        }
        "context_compress_ratio" => {
            let n: f64 = value.parse().map_err(|_| "must be a number".to_string())?;
            if !(0.5..=0.95).contains(&n) {
                return Err("must be 0.5-0.95".into());
            }
        }
        _ => {}
    }
    Ok(())
}

/// Apply a settings change to the session config. Called by the harness.
/// Returns a human-readable confirmation message.
pub fn apply_setting(
    config: &mut crate::types::SessionConfig,
    key: &str,
    value: &str,
) -> Result<String, String> {
    match key {
        "model" => {
            let old = config.model.clone();
            config.model = value.to_string();
            Ok(format!("Model: {old} → {value}"))
        }
        "mode" => {
            let new_mode = match value.to_lowercase().as_str() {
                "auto" => AgentMode::Auto,
                "plan" => AgentMode::Plan,
                "exec" => AgentMode::Exec,
                _ => return Err("must be: auto, plan, exec".into()),
            };
            let old = format!("{:?}", config.mode);
            config.mode = new_mode;
            Ok(format!("Mode: {old} → {value}"))
        }
        "thinking_effort" => {
            // This is handled by the TUI, not the harness config.
            // Return a marker that the harness forwards via UiEvent.
            Err(format!("__forward_effort__:{value}"))
        }
        "cod" => Err(format!("__forward_cod__:{value}")),
        "max_iterations" => {
            let n: usize = value.parse().map_err(|_| "invalid number")?;
            let old = config.max_iterations;
            config.max_iterations = n;
            Ok(format!("Max iterations: {old} → {n}"))
        }
        "llm_timeout_secs" => {
            let n: u64 = value.parse().map_err(|_| "invalid number")?;
            let old = config.llm_timeout_secs;
            config.llm_timeout_secs = n;
            Ok(format!("LLM timeout: {old}s → {n}s"))
        }
        "tool_timeout_secs" => {
            let n: u64 = value.parse().map_err(|_| "invalid number")?;
            let old = config.tool_timeout_secs;
            config.tool_timeout_secs = n;
            Ok(format!("Tool timeout: {old}s → {n}s"))
        }
        "max_context_tokens" => {
            let n: usize = value.parse().map_err(|_| "invalid number")?;
            let old = config.max_context_tokens;
            config.max_context_tokens = n;
            Ok(format!("Max context tokens: {old} → {n}"))
        }
        "context_compress_ratio" => {
            let n: f64 = value.parse().map_err(|_| "invalid number")?;
            let old = config.context_compress_ratio;
            config.context_compress_ratio = n;
            Ok(format!("Context compress ratio: {old} → {n}"))
        }
        "auto_continue" => {
            let new_val = value.to_lowercase() == "on";
            let old = config.auto_continue;
            config.auto_continue = new_val;
            Ok(format!("Auto-continue: {old} → {new_val}"))
        }
        _ => Err(format!("Unknown setting: {key}")),
    }
}

/// Read a setting value from the config. Called by the harness.
pub fn read_setting(config: &crate::types::SessionConfig, key: &str) -> Result<String, String> {
    match key {
        "model" => Ok(config.model.clone()),
        "mode" => Ok(format!("{:?}", config.mode).to_lowercase()),
        "max_iterations" => Ok(config.max_iterations.to_string()),
        "llm_timeout_secs" => Ok(config.llm_timeout_secs.to_string()),
        "tool_timeout_secs" => Ok(config.tool_timeout_secs.to_string()),
        "max_context_tokens" => Ok(config.max_context_tokens.to_string()),
        "context_compress_ratio" => Ok(config.context_compress_ratio.to_string()),
        "auto_continue" => Ok(if config.auto_continue { "on" } else { "off" }.into()),
        "thinking_effort" | "cod" => {
            // These live in the TUI, not in SessionConfig.
            Err(format!("__read_tui__:{key}"))
        }
        _ => Err(format!("Unknown setting: {key}")),
    }
}
