//! Sub-agent and memory management tools.
//!
//! Provides tools for spawning parallel sub-agents, waiting for their results,
//! managing persistent memory, and browser automation via Playwright.

use std::path::Path;

use crate::tools::Tool;
use crate::types::{FunctionDef, ToolDefinition, ToolResult};

/// Spawns a parallel sub-agent to work on an independent task.
pub struct SubAgentTool;
/// Lists all sub-agents and their current status.
pub struct SubAgentListTool;
/// Waits for a spawned sub-agent to complete and returns its output.
pub struct SubAgentWaitTool;
/// Manages persistent memory across sessions (core/mino/short tiers).
pub struct MemoryTool;
/// Browser automation tool using Playwright (screenshot, content, click).
pub struct PlaywrightTool;

#[async_trait::async_trait]
impl Tool for SubAgentTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "subagent".into(),
                description: "Spawn a parallel sub-agent to work on an independent task. Specify an agent role (coder/architect/debugger/reviewer/tester) for specialized behavior. Returns a handle ID; use subagent_wait to get the result when needed.".into(),
                parameters: serde_json::json!({"type":"object","properties":{"id":{"type":"string","description":"Unique ID for this sub-agent"},"task":{"type":"string","description":"Task for the sub-agent to complete"},"agent":{"type":"string","description":"Agent role: coder, architect, debugger, reviewer, tester (default: coder)"}},"required":["id","task"]}),
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
            Ok(_handle) => {
                let role = agent.as_deref().unwrap_or("coder");
                ToolResult {
                    tool_call_id: String::new(),
                    content: format!(
                        "Sub-agent '{id}' ({role}) spawned.\n\
                         Task: {task}\n\
                         Use subagent_wait(id=\"{id}\") to get the result when ready."
                    ),
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
impl Tool for SubAgentWaitTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".into(),
            function: FunctionDef {
                name: "subagent_wait".into(),
                description: "Wait for a sub-agent to complete and return its output. Blocks until the sub-agent finishes. Use after spawning with subagent.".into(),
                parameters: serde_json::json!({"type":"object","properties":{"id":{"type":"string","description":"Sub-agent ID to wait for"},"timeout_secs":{"type":"integer","description":"Max seconds to wait (default: 300)"}},"required":["id"]}),
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
                }
            }
        };
        let id = args["id"].as_str().unwrap_or("").to_string();
        let timeout_secs = args["timeout_secs"].as_u64().unwrap_or(300);

        if id.is_empty() {
            return ToolResult {
                tool_call_id: String::new(),
                content: "No sub-agent ID provided.".into(),
                is_error: true,
            };
        }

        // Check if already done first (non-blocking).
        if let Some(result) = crate::subagent::get_result(&id) {
            if result.done {
                let status = if result.success { "✓" } else { "❌" };
                return ToolResult {
                    tool_call_id: String::new(),
                    content: format!("[{status}] Sub-agent '{id}' output:\n\n{}", result.output),
                    is_error: !result.success,
                };
            }
        }

        // Wait with timeout.
        let wait_result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            crate::subagent::wait_for(&id),
        )
        .await;

        match wait_result {
            Ok(Ok(result)) => {
                let status = if result.success { "✓" } else { "❌" };
                let error_hint = result
                    .error
                    .as_deref()
                    .map(|e| format!("\n[Error: {e}]"))
                    .unwrap_or_default();
                ToolResult {
                    tool_call_id: String::new(),
                    content: format!(
                        "[{status}] Sub-agent '{id}' completed.\n\nOutput:\n{}{error_hint}",
                        result.output
                    ),
                    is_error: !result.success,
                }
            }
            Ok(Err(e)) => ToolResult {
                tool_call_id: String::new(),
                content: format!("Failed to wait for sub-agent '{id}': {e}"),
                is_error: true,
            },
            Err(_) => ToolResult {
                tool_call_id: String::new(),
                content: format!(
                    "Sub-agent '{id}' timed out after {timeout_secs}s. \
                     It may still be running — check with subagent_list."
                ),
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

    async fn execute(&self, _workspace: &Path, _arguments: &str) -> ToolResult {
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
                description: "Manage persistent memory across sessions. Actions: 'add <core|mino|short> <content> [--tag t1 --tag t2]', 'list [tier]', 'delete <tier> <index>', 'clear <tier>', 'search <query>'. Core memory is always in context, mino is recent, short is session summaries.".into(),
                parameters: serde_json::json!({"type":"object","properties":{"action":{"type":"string","description":"'add core <text> [--tag t1]', 'add mino <text>', 'add short <text>', 'list', 'list core', 'delete core 0', 'clear short', 'search <query>'"}},"required":["action"]}),
            },
        }
    }

    async fn execute(&self, workspace: &Path, arguments: &str) -> ToolResult {
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
        let mut mem = crate::memory::Memory::for_workspace(&workspace.to_string_lossy());

        if let Some(rest) = action.strip_prefix("add ") {
            let (content, tags) = parse_add_args(rest);
            if content.is_empty() {
                return ToolResult {
                    tool_call_id: String::new(),
                    content: "No content provided.".into(),
                    is_error: true,
                };
            }
            let tier = rest.split(' ').next().unwrap_or("short");
            let tag_refs: Vec<&str> = tags.iter().map(|s| s.as_str()).collect();
            match mem.add(tier, content, &tag_refs) {
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
            let tier = action.strip_prefix("list ").unwrap_or("all");
            let mut out = String::from("Memory:\n");
            let mut show = |label: &str, entries: &[crate::memory::MemoryEntry]| {
                if entries.is_empty() {
                    return;
                }
                out.push_str(&format!("  [{label}]\n"));
                for (i, e) in entries.iter().enumerate() {
                    let tags = if e.tags.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", e.tags.join(", "))
                    };
                    out.push_str(&format!("    {i}: {}{}\n", e.content, tags));
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
                        content: format!("Unknown tier: {tier}. Use core/mino/short."),
                        is_error: true,
                    }
                }
            }
            ToolResult {
                tool_call_id: String::new(),
                content: out,
                is_error: false,
            }
        } else if let Some(rest) = action.strip_prefix("delete ") {
            let mut parts = rest.splitn(2, ' ');
            let tier = parts.next().unwrap_or("");
            let index: usize = match parts.next().and_then(|s| s.parse().ok()) {
                Some(i) => i,
                None => {
                    return ToolResult {
                        tool_call_id: String::new(),
                        content: "Usage: delete <tier> <index>".into(),
                        is_error: true,
                    }
                }
            };
            match mem.delete(tier, index) {
                Ok(()) => ToolResult {
                    tool_call_id: String::new(),
                    content: format!("[{tier}] Deleted entry {index}."),
                    is_error: false,
                },
                Err(e) => ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Error: {e}"),
                    is_error: true,
                },
            }
        } else if let Some(rest) = action.strip_prefix("clear ") {
            let tier = rest.trim();
            match mem.clear(tier) {
                Ok(()) => ToolResult {
                    tool_call_id: String::new(),
                    content: format!("[{tier}] Cleared."),
                    is_error: false,
                },
                Err(e) => ToolResult {
                    tool_call_id: String::new(),
                    content: format!("Error: {e}"),
                    is_error: true,
                },
            }
        } else if let Some(query) = action.strip_prefix("search ") {
            let results = mem.search(query);
            if results.is_empty() {
                ToolResult {
                    tool_call_id: String::new(),
                    content: "No matches found.".into(),
                    is_error: false,
                }
            } else {
                let mut out = format!("Search results for '{}':\n", query);
                for (tier, entry) in &results {
                    let tags = if entry.tags.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", entry.tags.join(", "))
                    };
                    out.push_str(&format!("  [{}] {}{}\n", tier, entry.content, tags));
                }
                ToolResult {
                    tool_call_id: String::new(),
                    content: out,
                    is_error: false,
                }
            }
        } else {
            ToolResult {
                tool_call_id: String::new(),
                content: format!(
                    "Unknown action: {action}. Use 'add <tier> <content> [--tag t1]', 'list [tier]', 'delete <tier> <index>', 'clear <tier>', 'search <query>'."
                ),
                is_error: true,
            }
        }
    }
}

fn parse_add_args(rest: &str) -> (&str, Vec<String>) {
    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
    if parts.len() < 2 {
        return ("", Vec::new());
    }
    let after_tier = parts[1];
    let segments: Vec<&str> = after_tier.split(" --tag ").collect();
    let content = segments[0];
    let tags: Vec<String> = segments[1..].iter().map(|s| s.to_string()).collect();
    (content, tags)
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

    async fn execute(&self, _workspace: &Path, arguments: &str) -> ToolResult {
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
                match crate::process_util::std_command("node").arg("-e").arg(&script).output() {
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
                match crate::process_util::std_command("node")
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
