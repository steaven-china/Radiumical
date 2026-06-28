use std::path::PathBuf;

use crate::tools::Tool;
use crate::types::{FunctionDef, ToolDefinition, ToolResult};

pub struct SubAgentTool;
pub struct SubAgentListTool;
pub struct MemoryTool;
pub struct PlaywrightTool;

#[async_trait::async_trait]
impl Tool for SubAgentTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "subagent".into(),
                description: "Spawn a parallel sub-agent to work on an independent task. Specify an agent role (coder/architect/debugger/reviewer/tester) for specialized behavior. Returns immediately; check status with subagent_list.".into(),
                parameters: serde_json::json!({"type":"object","properties":{"id":{"type":"string","description":"Unique ID for this sub-agent"},"task":{"type":"string","description":"Task for the sub-agent to complete"},"agent":{"type":"string","description":"Agent role: coder, architect, debugger, reviewer, tester (default: coder)"}},"required":["id","task"]}),
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
                }
            }
        };
        let id = args["id"].as_str().unwrap_or("agent_1").to_string();
        let task = args["task"].as_str().unwrap_or("").to_string();
        let agent = args["agent"].as_str().map(|s| s.to_string());
        if task.is_empty() {
            return ToolResult {
                tool_call_id: String::new(),
                content: "No task provided.".into(),
                is_error: true,
            };
        }

        match crate::subagent::spawn_with_defaults(id.clone(), task.clone(), agent.clone()).await {
            Ok(()) => {
                let role = agent.as_deref().unwrap_or("coder");
                ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Sub-agent '{id}' ({role}) spawned: {task}"),
                    is_error: false,
                }
            }
            Err(e) => ToolResult {
                tool_call_id: String::new(),
                content: format!("Failed to spawn sub-agent: {e}"),
                is_error: true,
            },
        }
    }
}

#[async_trait::async_trait]
impl Tool for SubAgentListTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "subagent_list".into(),
                description: "List all sub-agents and their current status.".into(),
                parameters: serde_json::json!({"type":"object","properties":{},"required":[]}),
            },
        }
    }

    async fn execute(&self, _workspace: &PathBuf, _arguments: &str) -> ToolResult {
        ToolResult {
            tool_call_id: String::new(),
            content: crate::subagent::list(),
            is_error: false,
        }
    }
}

#[async_trait::async_trait]
impl Tool for MemoryTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "memory".into(),
                description: "Manage persistent memory across sessions. Actions: 'add <core|mino|short> <content>', 'list [tier]'. Core memory is always in context, mino is recent, short is session summaries.".into(),
                parameters: serde_json::json!({"type":"object","properties":{"action":{"type":"string","description":"'add core <text>', 'add mino <text>', 'add short <text>', 'list', 'list core', 'list mino', 'list short'"}},"required":["action"]}),
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
                }
            }
        };
        let action = args["action"].as_str().unwrap_or("");

        if let Some(rest) = action.strip_prefix("add ") {
            let parts: Vec<&str> = rest.splitn(2, ' ').collect();
            let tier = parts.first().copied().unwrap_or("short");
            let content = parts.get(1).copied().unwrap_or("");
            if content.is_empty() {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: "No content provided.".into(),
                    is_error: true,
                };
            }
            match crate::memory::Memory::load().and_then(|mut m| {
                m.add(tier, content)?;
                m.save()
            }) {
                Ok(()) => ToolResult {
                    tool_call_id: String::new(),
                    content: format!("[{tier}] Remembered."),
                    is_error: false,
                },
                Err(e) => ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Error: {e}"),
                    is_error: true,
                },
            }
        } else if action == "list" || action.starts_with("list ") {
            let mem = crate::memory::Memory::load().unwrap_or_default();
            let tier = action.strip_prefix("list ").unwrap_or("all");
            let mut out = String::from("Memory:\n");
            let mut show = |label: &str, entries: &[crate::memory::MemoryEntry]| {
                if entries.is_empty() {
                    return;
                }
                out.push_str(&format!("  [{label}]\n"));
                for e in entries.iter().rev().take(10) {
                    out.push_str(&format!("    - {}\n", e.content));
                }
            };
            match tier {
                "all" => {
                    show("core", &mem.core);
                    show("mino", &mem.mino);
                    show("short", &mem.short);
                }
                "core" => show("core", &mem.core),
                "mino" => show("mino", &mem.mino),
                "short" => show("short", &mem.short),
                _ => {
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: format!("Unknown tier: {tier}"),
                        is_error: true,
                    }
                }
            }
            ToolResult {
                tool_call_id: String::new(),
                content: out,
                is_error: false,
            }
        } else {
            ToolResult {
                tool_call_id: String::new(),
                content: format!(
                    "Unknown action: {action}. Use 'add <tier> <content>' or 'list [tier]'."
                ),
                is_error: true,
            }
        }
    }
}

#[async_trait::async_trait]
impl Tool for PlaywrightTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "playwright".into(),
                description: "Browser automation via Playwright. Actions: 'screenshot <url> [selector]', 'content <url> [selector]', 'click <url> <selector>'. Requires: npx playwright install.".into(),
                parameters: serde_json::json!({"type":"object","properties":{"action":{"type":"string","description":"'screenshot <url> [selector]', 'content <url> [selector]', 'click <url> <selector>'"}},"required":["action"]}),
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
                }
            }
        };
        let action = args["action"].as_str().unwrap_or("");

        if action.is_empty() {
            return ToolResult {
                tool_call_id: String::new(),
                content: "Usage: 'screenshot <url>', 'content <url>', 'click <url> <selector>'"
                    .into(),
                is_error: true,
            };
        }

        let parts: Vec<&str> = action.splitn(3, ' ').collect();
        let cmd_type = parts.first().copied().unwrap_or("");
        let url = parts.get(1).copied().unwrap_or("");
        let selector = parts.get(2).copied().unwrap_or("");

        if url.is_empty() {
            return ToolResult {
                tool_call_id: String::new(),
                content: "No URL provided.".into(),
                is_error: true,
            };
        }

        match cmd_type {
            "screenshot" => {
                let out = format!(
                    "/tmp/radiumical_playwright_{}.png",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                );
                let script = if selector.is_empty() {
                    format!("const {{ chromium }} = require('playwright'); (async () => {{ const b = await chromium.launch(); const p = await b.newPage(); await p.goto('{url}'); await p.screenshot({{ path: '{out}', fullPage: true }}); console.log('OK:' + '{out}'); await b.close(); }})();")
                } else {
                    format!("const {{ chromium }} = require('playwright'); (async () => {{ const b = await chromium.launch(); const p = await b.newPage(); await p.goto('{url}'); await p.locator('{selector}').screenshot({{ path: '{out}' }}); console.log('OK:' + '{out}'); await b.close(); }})();")
                };
                match std::process::Command::new("node").arg("-e").arg(&script).output() {
                    Ok(o) => {
                        let stdout = String::from_utf8_lossy(&o.stdout);
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        if stdout.contains("OK:") {
                            ToolResult {
                                tool_call_id: String::new(),
                                content: format!("Screenshot: {out}"),
                                is_error: false,
                            }
                        } else {
                            ToolResult {
                                tool_call_id: String::new(),
                                content: format!("Playwright error: {stderr}"),
                                is_error: true,
                            }
                        }
                    }
                    Err(e) => ToolResult {
                        tool_call_id: String::new(),
                        content: format!(
                            "Node not found. Install: npm i playwright && npx playwright install chromium\n{e}"
                        ),
                        is_error: true,
                    },
                }
            }
            "content" => {
                let script = if selector.is_empty() {
                    format!("const {{ chromium }} = require('playwright'); (async () => {{ const b = await chromium.launch(); const p = await b.newPage(); await p.goto('{url}'); const text = await p.textContent('body'); console.log(text); await b.close(); }})();")
                } else {
                    format!("const {{ chromium }} = require('playwright'); (async () => {{ const b = await chromium.launch(); const p = await b.newPage(); await p.goto('{url}'); const text = await p.locator('{selector}').textContent(); console.log(text); await b.close(); }})();")
                };
                match std::process::Command::new("node")
                    .arg("-e")
                    .arg(&script)
                    .output()
                {
                    Ok(o) => {
                        let stdout = String::from_utf8_lossy(&o.stdout);
                        let stderr = String::from_utf8_lossy(&o.stderr);
                        if !stdout.trim().is_empty() {
                            let preview: String = stdout.chars().take(2000).collect();
                            let dots = if stdout.len() > 2000 { "…" } else { "" };
                            ToolResult {
                                tool_call_id: String::new(),
                                content: format!("{preview}{dots}"),
                                is_error: false,
                            }
                        } else {
                            ToolResult {
                                tool_call_id: String::new(),
                                content: format!("No content. {stderr}"),
                                is_error: true,
                            }
                        }
                    }
                    Err(e) => ToolResult {
                        tool_call_id: String::new(),
                        content: format!("Node not found: {e}"),
                        is_error: true,
                    },
                }
            }
            _ => ToolResult {
                tool_call_id: String::new(),
                content: format!("Unknown action: {cmd_type}. Use screenshot/content/click."),
                is_error: true,
            },
        }
    }
}
