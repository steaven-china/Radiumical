use crate::tui::app::App;
use crate::tui::BackendCmd;

use super::base64_encode;

impl App {
    pub(super) fn cmd_end(&mut self) -> bool {
        self.stick_to_bottom = true;
        self.scroll = 0.0;
        self.input.clear();
        self.cursor = 0;
        true
    }

    pub(super) fn cmd_exit(&mut self) -> bool {
        self.should_quit = true;
        true
    }

    pub(super) fn cmd_memory(&mut self) -> bool {
        self.output.push("> /memory".into());
        let mem = &self.memory;
        let mut show = |label: &str, entries: &[radiumical_core::memory::MemoryEntry]| {
            if entries.is_empty() {
                return;
            }
            self.output.push(format!("  [{label}]"));
            for (i, e) in entries.iter().enumerate() {
                let tags = if e.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", e.tags.join(", "))
                };
                self.output.push(format!("    {i}: {}{}", e.content, tags));
            }
        };
        show("core", &mem.core);
        show("mino", &mem.mino);
        show("short", &mem.short);
        if mem.core.is_empty() && mem.mino.is_empty() && mem.short.is_empty() {
            self.output.push("  No memories stored.".into());
        }
        self.output.push(String::new());
        self.input.clear();
        self.cursor = 0;
        self.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_memory_search(&mut self, task: &str) -> bool {
        let query = task[15..].trim();
        self.output.push(format!("> /memory search {query}"));
        let results = self.memory.search(query);
        if results.is_empty() {
            self.output.push("  No matches found.".into());
        } else {
            for (tier, entry) in &results {
                let tags = if entry.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", entry.tags.join(", "))
                };
                self.output
                    .push(format!("  [{}] {}{}", tier, entry.content, tags));
            }
        }
        self.output.push(String::new());
        self.input.clear();
        self.cursor = 0;
        self.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_memory_clear(&mut self, task: &str) -> bool {
        let tier = task[14..].trim();
        self.output.push(format!("> /memory clear {tier}"));
        match self.memory.clear(tier) {
            Ok(()) => self.output.push(format!("  [{tier}] Cleared.")),
            Err(e) => self.output.push(format!("  Error: {e}")),
        }
        self.output.push(String::new());
        self.input.clear();
        self.cursor = 0;
        self.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_remember(&mut self, task: &str) -> bool {
        let rest = task[10..].trim();
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        let tier = parts.first().copied().unwrap_or("short");
        let after_tier = parts.get(1).copied().unwrap_or("");
        let segments: Vec<&str> = after_tier.split(" --tag ").collect();
        let content = segments[0];
        let tags: Vec<&str> = segments[1..].to_vec();
        if !matches!(tier, "core" | "mino" | "short") {
            self.output.push(format!(
                "  Invalid tier: '{tier}'. Use core, mino, or short."
            ));
        } else if content.is_empty() {
            self.output.push("  Usage: /remember <tier> <content> [--tag t1]".into());
        } else {
            match self.memory.add(tier, content, &tags) {
                Ok(()) => {
                    let tag_str = if tags.is_empty() {
                        String::new()
                    } else {
                        format!(" [{}]", tags.join(", "))
                    };
                    self.output
                        .push(format!("  [{tier}] Remembered: {content}{tag_str}"));
                }
                Err(e) => self.output.push(format!("  Memory error: {e}")),
            }
        }
        self.input.clear();
        self.cursor = 0;
        self.stick_to_bottom = true;
        self.output.push(String::new());
        true
    }

    pub(super) fn cmd_subagents(&mut self) -> bool {
        self.subagents_panel_visible = !self.subagents_panel_visible;
        if self.subagents_panel_visible {
            self.panels.open(crate::panel::PanelId::SubAgents);
        } else {
            self.panels.close(crate::panel::PanelId::SubAgents);
        }
        self.output.push("> /subagents".into());
        if self.subagents_panel_visible {
            self.output.push("  Sub-agents panel opened".into());
        } else {
            self.output.push("  Sub-agents panel closed".into());
        }
        self.output.push(String::new());
        self.input.clear();
        self.cursor = 0;
        self.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_mcp(&mut self) -> bool {
        self.mcp_panel_visible = !self.mcp_panel_visible;
        if self.mcp_panel_visible {
            self.panels.open(crate::panel::PanelId::Mcp);
        } else {
            self.panels.close(crate::panel::PanelId::Mcp);
        }
        self.output.push("> /mcp".into());
        if self.mcp_panel_visible {
            self.output.push("  MCP servers panel opened".into());
        } else {
            self.output.push("  MCP servers panel closed".into());
        }
        self.output.push(String::new());
        self.input.clear();
        self.cursor = 0;
        self.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_mcp_toggle(&mut self, task: &str) -> bool {
        let rest = task[5..].trim();
        if rest == "toggle" {
            self.output.push("  Usage: /mcp toggle <name>".into());
        } else if let Some(name) = rest.strip_prefix("toggle ") {
            let name = name.trim().to_string();
            if let Some(server) = self.mcp_servers.iter_mut().find(|s| s.name == name) {
                server.enabled = !server.enabled;
                let enabled = server.enabled;
                let _ = self.cmd_tx.blocking_send(
                    BackendCmd::ToggleMcpServer { name: name.clone() },
                );
                self.toasts.push(crate::board::Toast::new(
                    format!(
                        "MCP '{}' {}",
                        name,
                        if enabled { "enabled" } else { "disabled" }
                    ),
                    crate::board::ToastLevel::Info,
                    std::time::Duration::from_secs(3),
                ));
                self.output.push(format!(
                    "> /mcp toggle {}",
                    name
                ));
                self.output.push(format!(
                    "  MCP '{}' {}",
                    name,
                    if enabled { "enabled" } else { "disabled" }
                ));
            } else {
                self.output.push(format!("  MCP server not found: {name}"));
            }
        } else {
            self.output
                .push("  /mcp — toggle panel | /mcp toggle <name>".into());
        }
        self.output.push(String::new());
        self.input.clear();
        self.cursor = 0;
        self.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_env(&mut self, task: &str) -> bool {
        let rest = task[4..].trim();
        let mut parts = rest.splitn(2, ' ');
        let sub = parts.next().unwrap_or("");
        match sub {
            "list" | "" => {
                let keys = radiumical_core::secure_env::list_keys();
                if keys.is_empty() {
                    self.output
                        .push("  No stored keys. Use /env set KEY=VALUE".into());
                } else {
                    self.output.push("  Stored keys:".into());
                    for k in &keys {
                        self.output.push(format!("    {k} = ***"));
                    }
                }
            }
            "set" => {
                if let Some(kv) = parts.next() {
                    if let Some((k, v)) = kv.split_once('=') {
                        match radiumical_core::secure_env::set(k.trim(), v.trim()) {
                            Ok(()) => {
                                self.output
                                    .push(format!("  Set: {}", k.trim()));
                                std::env::set_var(k.trim(), v.trim());
                            }
                            Err(e) => self.output.push(format!("  Error: {e}")),
                        }
                    } else {
                        self.output.push("  Usage: /env set KEY=VALUE".into());
                    }
                } else {
                    self.output.push("  Usage: /env set KEY=VALUE".into());
                }
            }
            "rm" | "del" | "remove" => {
                if let Some(key) = parts.next() {
                    match radiumical_core::secure_env::remove(key.trim()) {
                        Ok(()) => self.output.push(format!("  Removed: {}", key.trim())),
                        Err(e) => self.output.push(format!("  Error: {e}")),
                    }
                } else {
                    self.output.push("  Usage: /env rm KEY".into());
                }
            }
            "copy" | "import" => {
                let env_keys = [
                    "OPENAI_API_KEY",
                    "ANTHROPIC_API_KEY",
                    "GOOGLE_API_KEY",
                    "DEEPSEEK_API_KEY",
                    "MISTRAL_API_KEY",
                    "GROQ_API_KEY",
                    "COHERE_API_KEY",
                    "TOGETHER_API_KEY",
                    "OPENROUTER_API_KEY",
                    "FIREWORKS_API_KEY",
                    "DEEPINFRA_API_KEY",
                    "CEREBRAS_API_KEY",
                    "SAMBANOVA_API_KEY",
                ];
                let mut copied = 0;
                for key in &env_keys {
                    if let Ok(val) = std::env::var(key) {
                        if !val.is_empty() {
                            let _ = radiumical_core::secure_env::set(key, &val);
                            copied += 1;
                        }
                    }
                }
                self.output
                    .push(format!("  Copied {copied} key(s) from environment to secure store"));
            }
            _ => {
                self.output
                    .push("  /env [list] | set KEY=VALUE | rm KEY | copy".into());
            }
        }
        self.output.push(String::new());
        self.input.clear();
        self.cursor = 0;
        self.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_cod_on(&mut self) -> bool {
        self.cod_enabled = true;
        self.output.push("> /cod on".into());
        self.output.push("  Chain of Draft enabled".into());
        self.output.push(String::new());
        self.input.clear();
        self.cursor = 0;
        self.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_cod_off(&mut self) -> bool {
        self.cod_enabled = false;
        self.output.push("> /cod off".into());
        self.output.push("  Chain of Draft disabled".into());
        self.output.push(String::new());
        self.input.clear();
        self.cursor = 0;
        self.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_status(&mut self) -> bool {
        self.output.push("> /status".into());
        let mode = match self.mode {
            radiumical_core::types::AgentMode::Auto => "Auto",
            radiumical_core::types::AgentMode::Plan => "Plan",
            radiumical_core::types::AgentMode::Exec => "Exec",
        };
        self.output.push(format!("  Model:      {}", self.model));
        self.output.push(format!("  Provider:   {}", self.provider_name));
        self.output.push(format!("  Mode:       {}", mode));
        self.output.push(format!("  Effort:     {}", self.thinking_effort));
        self.output.push(format!("  CoD:        {}", if self.cod_enabled { "on" } else { "off" }));
        self.output.push(format!("  Messages:   {}", self.session_items.len()));
        self.output.push(format!("  History:    {}", self.history.len()));
        self.output.push(format!("  Agent role: {}", self.agent_role));
        if !self.mcp_servers.is_empty() {
            let alive = self.mcp_servers.iter().filter(|s| s.alive).count();
            self.output.push(format!("  MCP:        {}/{} servers alive", alive, self.mcp_servers.len()));
        }
        self.output.push(String::new());
        self.input.clear();
        self.cursor = 0;
        self.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_retry(&mut self) -> bool {
        if let Some(last_task) = self.history.last().cloned() {
            self.output.push("> /retry".to_string());
            self.output.push(format!("> {last_task}"));
            self.output.push(String::new());
            self.stick_to_bottom = true;
            self.full_reasoning.clear();
            self.show_full_reasoning = false;
            self.thinking_cancelled = false;
            let final_task = if self.cod_enabled {
                format!("{last_task}\n\n[Chain of Draft: think in <=5 word steps, be terse. Output reasoning as brief fragments, then final answer.]")
            } else {
                last_task
            };
            let _ = self.cmd_tx.blocking_send(BackendCmd::RunTask(final_task));
        } else {
            self.toasts.push(crate::board::Toast::new(
                "Nothing to retry",
                crate::board::ToastLevel::Warn,
                std::time::Duration::from_secs(3),
            ));
        }
        self.input.clear();
        self.cursor = 0;
        true
    }

    pub(super) fn cmd_copy(&mut self) -> bool {
        if let Some(last) = self.session_items.iter().rev().find_map(|item| {
            if let radiumical_core::session::SessionItem::Assistant { content } = item {
                if !content.is_empty() { Some(content.clone()) } else { None }
            } else {
                None
            }
        }) {
            let encoded = base64_encode(&last);
            let osc = format!("\x1b]52;c;{}\x07", encoded);
            use std::io::Write;
            let _ = std::io::stderr().write_all(osc.as_bytes());
            self.toasts.push(crate::board::Toast::new(
                "Copied last response to clipboard".to_string(),
                crate::board::ToastLevel::Info,
                std::time::Duration::from_secs(3),
            ));
        } else {
            self.toasts.push(crate::board::Toast::new(
                "Nothing to copy",
                crate::board::ToastLevel::Warn,
                std::time::Duration::from_secs(3),
            ));
        }
        self.input.clear();
        self.cursor = 0;
        true
    }

    pub(super) fn cmd_tips(&mut self) -> bool {
        let enabled = self.tip_state.toggle();
        self.output.push("> /tips".into());
        if enabled {
            self.output.push("  Tips enabled — shown in status bar".into());
        } else {
            self.output.push("  Tips disabled".into());
        }
        self.output.push("  /tip next — skip to next tip".into());
        self.output.push(String::new());
        self.input.clear();
        self.cursor = 0;
        self.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_tip(&mut self) -> bool {
        self.tip_state.next();
        self.output.push("> /tip next".into());
        self.output.push(format!("  {}", self.tip_state.text()));
        self.output.push(String::new());
        self.input.clear();
        self.cursor = 0;
        self.stick_to_bottom = true;
        true
    }
}
