use crate::tui::app::App;
use crate::tui::BackendCmd;

impl App {
    pub(crate) fn handle_command(&mut self, task: &str) {
        match task {
            "/exit" | "/quit" | "/q" => {
                self.should_quit = true;
                return;
            }
            "/new" => {
                // Auto-save current session before clearing (avoid data loss).
                if !self.session_items.is_empty() {
                    let desc = self.history.first().cloned();
                    let mode: radiumical_core::session::SessionMode = self.mode.clone().into();
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let auto_name = format!("auto-{ts}");
                    let _ = self.session_pool.save(
                        &auto_name,
                        &self.session_items,
                        &self.model,
                        &self.provider_name,
                        mode,
                        &self.thinking_effort,
                        desc.as_deref(),
                    );
                }
                self.output.clear();
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.hints.clear();
                self.scroll = 0.0;
                self.stick_to_bottom = true;
                self.welcome = true;
                self.show_help_overlay = true;
                self.show_model_picker = false;
                self.provider_picker.close();
                self.hint_selected = None;
                self.help_board.visible = false;
                self.blocks.clear();
                self.session_items.clear();
                self.render_cache.clear();
                self.render_cache_order.clear();
                self.session_title = None;
                self.markdown = crate::markdown::MarkdownRenderer::new();
                self.full_reasoning.clear();
                self.show_full_reasoning = false;
                let _ = self.cmd_tx.blocking_send(BackendCmd::ResetConversation);
                self.output
                    .push(format!("Radiumical — {} @ {}", self.model, "."));
                self.output.push(String::new());
                for line in crate::tui::LOGO {
                    self.output.push(format!("  {line}"));
                }
                self.output.push(String::new());
                self.output.push("  lean CLI coding agent".into());
                self.output.push(String::new());
                self.output
                    .push("Type a task or /help for commands.".into());
                self.output.push(String::new());
                return;
            }
            "/clear" | "/cls" => {
                self.output.clear();
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.hints.clear();
                self.scroll = 0.0;
                self.stick_to_bottom = true;
                self.welcome = false;
                self.show_help_overlay = false;
                self.show_model_picker = false;
                self.provider_picker.close();
                self.hint_selected = None;
                self.help_board.visible = false;
                return;
            }
            "/end" | "/bottom" => {
                self.stick_to_bottom = true;
                self.scroll = 0.0;
                self.input.clear();
                self.cursor = 0;
                return;
            }
            "/help" | "/?" => {
                self.show_help_overlay = !self.show_help_overlay;
                if !self.welcome {
                    self.output.push("> /help".into());
                    self.show_help();
                }
                self.input.clear();
                self.cursor = 0;
                self.hints.clear();
                self.stick_to_bottom = true;
                return;
            }
            "/settings" | "/config" => {
                if !self.settings_board.visible {
                    self.settings_board.visible = true;
                    self.settings_visible = true;
                    self.panels.open(crate::panel::PanelId::Settings);
                } else {
                    self.commit_settings();
                    self.settings_board.visible = false;
                    self.settings_visible = false;
                    self.panels.close(crate::panel::PanelId::Settings);
                }
                self.input.clear();
                self.cursor = 0;
                self.hints.clear();
                self.stick_to_bottom = true;
                return;
            }
            "/plan" => {
                self.mode = radiumical_core::types::AgentMode::Plan;
                let _ = self
                    .cmd_tx
                    .blocking_send(BackendCmd::SetMode(radiumical_core::types::AgentMode::Plan));
                self.output.push("  Plan mode".into());
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            "/plan vis" => {
                self.plan_visible = !self.plan_visible;
                if self.plan_visible {
                    self.panels.open(crate::panel::PanelId::Plan);
                    self.output.push("> /plan vis".into());
                    self.output.push("  Plan panel opened".into());
                } else {
                    self.panels.close(crate::panel::PanelId::Plan);
                    self.output.push("> /plan vis".into());
                    self.output.push("  Plan panel closed".into());
                }
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            "/plan show" => {
                self.output.push("> /plan show".into());
                if self.plan_tasks.is_empty() {
                    self.output.push("  No plan active.".into());
                } else {
                    self.output.push(format!("# {}", self.plan_title));
                    let total = self.plan_tasks.len();
                    let done = self.plan_tasks.iter().filter(|t| t.status == radiumical_core::orchestrator::TaskStatus::Done).count();
                    self.output.push(format!("  progress: {}/{} done", done, total));
                    self.output.push(String::new());
                    for task in &self.plan_tasks {
                        let icon = task.status.icon();
                        self.output.push(format!("  {} #{} {}", icon, task.id, task.title));
                    }
                }
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            "/agents" => {
                self.agents_panel_visible = !self.agents_panel_visible;
                if self.agents_panel_visible {
                    self.agents_list = radiumical_core::agent_pool::load_agents();
                    self.panels.open(crate::panel::PanelId::Agents);
                    self.output.push("> /agents".into());
                    self.output.push("  Agent roles panel opened".into());
                } else {
                    self.panels.close(crate::panel::PanelId::Agents);
                    self.output.push("> /agents".into());
                    self.output.push("  Agent roles panel closed".into());
                }
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            _ if task.starts_with("/agents ") => {
                let name = task[8..].trim();
                match radiumical_core::agent_pool::get_agent(name) {
                    Some(agent) => {
                        self.agent_role = agent.name.clone();
                        let agent_mode = agent.mode.to_agent_mode();
                        self.mode = agent_mode.clone();
                        let _ = self.cmd_tx.blocking_send(BackendCmd::SetMode(agent_mode));
                        self.toasts.push(crate::board::Toast::new(
                            format!("Switched to: {}", agent.name),
                            crate::board::ToastLevel::Info,
                            std::time::Duration::from_secs(3),
                        ));
                        self.output.push(format!("> /agents {}", name));
                        self.output.push(format!("  Role: {} ({})", agent.name, agent.description));
                    }
                    None => {
                        self.output.push(format!("  Agent not found: {name}"));
                        let available = radiumical_core::agent_pool::load_agents();
                        let names: Vec<&str> = available.iter().map(|a| a.name.as_str()).collect();
                        self.output.push(format!("  Available: {}", names.join(", ")));
                    }
                }
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            "/exec" => {
                self.mode = radiumical_core::types::AgentMode::Exec;
                let _ = self
                    .cmd_tx
                    .blocking_send(BackendCmd::SetMode(radiumical_core::types::AgentMode::Exec));
                self.output.push("  Exec mode".into());
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            "/auto" => {
                self.mode = radiumical_core::types::AgentMode::Auto;
                let _ = self
                    .cmd_tx
                    .blocking_send(BackendCmd::SetMode(radiumical_core::types::AgentMode::Auto));
                self.output.push("  Auto mode".into());
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            _ if task == "/sessions" || task == "/session tui" => {
                if let Ok(sessions) = self.session_pool.list() {
                    self.session_tui.open(sessions, None, None);
                    if let Some(first) = self.session_tui.sessions.first() {
                        self.session_tui.name_buffer = first.name.clone();
                        self.session_tui.desc_buffer = first.description.clone();
                    }
                }
                self.panels.open(crate::panel::PanelId::SessionList);
                self.input.clear();
                self.cursor = 0;
                self.hints.clear();
                return;
            }
            _ if task == "/session" => {
                self.output.push(
                    "  /session save <name> [desc] | load <name> | list | delete <name> | tui".into(),
                );
                self.input.clear();
                self.cursor = 0;
                self.hints.clear();
                self.stick_to_bottom = true;
                self.output.push(String::new());
                return;
            }
            _ if task.starts_with("/session") => {
                let rest = task[8..].trim();
                let mut parts = rest.splitn(3, ' ');
                let sub = parts.next().unwrap_or("");
                match sub {
                    "save" => {
                        let name = parts.next().unwrap_or("default");
                        let desc = parts.next();
                        let mode: radiumical_core::session::SessionMode = self.mode.clone().into();
                        match self.session_pool.save(
                            name,
                            &self.session_items,
                            &self.model,
                            &self.provider_name,
                            mode,
                            &self.thinking_effort,
                            desc,
                        ) {
                            Ok(()) => self.output.push(format!("  Session saved: {name}")),
                            Err(e) => self.output.push(format!("  Save failed: {e}")),
                        }
                    }
                    "load" => {
                        let name = parts.next().unwrap_or("default");
                        match self.session_pool.load(name) {
                            Ok(Some((meta, items))) => {
                                self.session_items = items;
                                self.render_session_items_to_output();
                                self.mode = meta.mode.into();
                                self.model = meta.model.clone();
                                self.provider_name = meta.provider.clone();
                                self.thinking_effort = meta.thinking_effort.clone();
                                let _ = self
                                    .cmd_tx
                                    .blocking_send(BackendCmd::SetMode(self.mode.clone()));
                                let _ = self
                                    .cmd_tx
                                    .blocking_send(BackendCmd::SetModel(self.model.clone()));
                                let _ = self.cmd_tx.blocking_send(
                                    BackendCmd::SetThinkingEffort(self.thinking_effort.clone()),
                                );
                                let _ = self
                                    .cmd_tx
                                    .blocking_send(BackendCmd::LoadSession(self.session_items.clone()));
                                self.output.push(format!("  Loaded: {name}"));
                            }
                            Ok(None) => self.output.push(format!("  Session not found: {name}")),
                            Err(e) => self.output.push(format!("  Load failed: {e}")),
                        }
                    }
                    "list" => match self.session_pool.list() {
                        Ok(sessions) => {
                            if sessions.is_empty() {
                                self.output.push("  No saved sessions".into());
                            } else {
                                for s in &sessions {
                                    let desc = if s.description.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" | {}", s.description)
                                    };
                                    self.output.push(format!(
                                        "  {} — {} messages | {}{}",
                                        s.name, s.message_count, s.created, desc
                                    ));
                                }
                            }
                        }
                        Err(e) => self.output.push(format!("  List failed: {e}")),
                    },
                    "delete" => {
                        let name = parts.next().unwrap_or("");
                        match self.session_pool.delete(name) {
                            Ok(true) => self.output.push(format!("  Deleted: {name}")),
                            Ok(false) => self.output.push(format!("  Not found: {name}")),
                            Err(e) => self.output.push(format!("  Delete failed: {e}")),
                        }
                    }
                    _ => {
                        self.output.push(
                            "  /session save <name> [desc] | load <name> | list | delete <name> | tui"
                                .into(),
                        );
                    }
                }
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                self.output.push(String::new());
                return;
            }
            "/skills" => {
                self.output.push("> /skills".into());
                let metas = self.skill_registry.all_meta();
                if metas.is_empty() {
                    self.output.push("  No skills installed.".into());
                    self.output
                        .push("  Place skills in ~/.radi/skills/{name}/SKILL.md".into());
                } else {
                    self.output
                        .push(format!("  Available skills ({}):", metas.len()));
                    for m in metas {
                        let marker = if self.skill_registry.get(&m.name).is_some() {
                            "●"
                        } else {
                            "○"
                        };
                        self.output
                            .push(format!("  {} {:<16} {}", marker, m.name, m.description));
                    }
                    self.output.push(String::new());
                    self.output.push("  ● = active  ○ = inactive".into());
                    self.output
                        .push("  /skill <name> to activate, /skill off <name> to deactivate".into());
                }
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            _ if task.starts_with("/skill ") => {
                let rest = task[7..].trim();
                if rest == "off" {
                    self.skill_registry.deactivate_all();
                    self.output.push("  All skills deactivated.".into());
                } else if rest.starts_with("off ") {
                    let name = rest[4..].trim();
                    self.skill_registry.deactivate(name);
                    self.output.push(format!("  Deactivated: {name}"));
                } else if rest == "list" {
                    self.output.push("> /skill list".into());
                    let activated = self.skill_registry.activated();
                    if activated.is_empty() {
                        self.output.push("  No active skills.".into());
                    } else {
                        self.output.push("  Active skills:".into());
                        for s in &activated {
                            self.output
                                .push(format!("  ● {:<16} {}", s.name, s.description));
                        }
                    }
                } else {
                    // Activate skill by name or auto-match
                    let name = rest.to_string();
                    if self.skill_registry.activate(&name).is_some() {
                        self.output
                            .push(format!("  Activated skill: {name}"));
                    } else {
                        // Try auto-match
                        let matched = radiumical_core::skill::match_by_input(&name);
                        if matched.is_empty() {
                            self.output
                                .push(format!("  Skill not found: {name}"));
                        } else {
                            let m = &matched[0];
                            if self.skill_registry.activate(&m.name).is_some() {
                                self.output.push(format!(
                                    "  Auto-matched and activated: {}",
                                    m.name
                                ));
                            }
                        }
                    }
                }
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            "/perf" => {
                self.perf_visible = !self.perf_visible;
                self.output.push("> /perf".into());
                self.output.push(radiumical_core::perf::report());
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            "/outline" | "/lint" | "/diagnostics" => {
                self.output.push("  Command not yet implemented.".into());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                self.output.push(String::new());
                return;
            }
            _ if task == "/memory" => {
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
                return;
            }
            _ if task.starts_with("/memory search ") => {
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
                return;
            }
            _ if task.starts_with("/memory clear ") => {
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
                return;
            }
            "/subagents" => {
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
                return;
            }
            "/mcp" => {
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
                return;
            }
            "/debug linevis" => {
                self.output.push("> /debug linevis".into());
                self.output.push(format!(
                    "  total: {} | vis: {} | scroll: {:.0} | stick: {}",
                    self.output.len(),
                    self.output_vis,
                    self.scroll,
                    self.stick_to_bottom
                ));
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            _ if task.starts_with("/remember ") => {
                let rest = task[10..].trim();
                let parts: Vec<&str> = rest.splitn(2, ' ').collect();
                let tier = parts.first().copied().unwrap_or("short");
                let after_tier = parts.get(1).copied().unwrap_or("");
                let segments: Vec<&str> = after_tier.split(" --tag ").collect();
                let content = segments[0];
                let tags: Vec<&str> = segments[1..].iter().map(|s| *s).collect();
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
                return;
            }
            _ if task.starts_with("/debug") => {
                let topic = task[6..].trim();
                self.show_debug(topic);
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            "/review" => {
                let has_history = !self.session_items.is_empty();
                if has_history {
                    let prompt = "Review the changes made in this session and suggest improvements. Check for bugs, style issues, missing tests, and dead code. Report findings concisely.";
                    let _ = self
                        .cmd_tx
                        .blocking_send(BackendCmd::RunTask(prompt.into()));
                    self.output.push("> /review".into());
                    self.output.push("  Reviewing session…".into());
                } else {
                    self.toasts.push(crate::board::Toast::new(
                        "Nothing to review yet",
                        crate::board::ToastLevel::Warn,
                        std::time::Duration::from_secs(3),
                    ));
                    self.output.push("> /review".into());
                    self.output.push("  No session history to review.".into());
                }
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            "/tools" => {
                self.output.push("> /tools".into());
                let tools = radiumical_core::tools::all_tools();
                self.output
                    .push(format!("  Available tools ({}):", tools.len()));
                for t in tools {
                    let def = t.definition();
                    let marker = match self.mode {
                        radiumical_core::types::AgentMode::Plan => {
                            if matches!(
                                def.function.name.as_str(),
                                "read_file" | "search_code" | "find_files"
                            ) {
                                "✅"
                            } else {
                                "🔒"
                            }
                        }
                        _ => "✅",
                    };
                    self.output.push(format!(
                        "  {} {:<14} {}",
                        marker, def.function.name, def.function.description
                    ));
                }
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            "/provider" => {
                self.show_model_picker = self.provider_picker.toggle(&self.cmd_tx);
                if self.show_model_picker {
                    self.panels.open(crate::panel::PanelId::ProviderPicker);
                } else {
                    self.panels.close(crate::panel::PanelId::ProviderPicker);
                }
                self.input.clear();
                self.cursor = 0;
                return;
            }
            _ if task == "/models" => {
                self.show_model_picker = self.provider_picker.toggle(&self.cmd_tx);
                if self.show_model_picker {
                    self.panels.open(crate::panel::PanelId::ProviderPicker);
                } else {
                    self.panels.close(crate::panel::PanelId::ProviderPicker);
                }
                if self.show_model_picker && self.available_models.len() <= 1 {
                    let _ = self.cmd_tx.blocking_send(BackendCmd::RefreshModels);
                    self.output.push("> /models".into());
                    self.output.push("  Refreshing models…".into());
                    self.output.push(String::new());
                }
                self.input.clear();
                self.cursor = 0;
                return;
            }
            _ if task == "/cod on" => {
                self.cod_enabled = true;
                self.output.push("> /cod on".into());
                self.output.push("  Chain of Draft enabled".into());
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            _ if task == "/cod off" => {
                self.cod_enabled = false;
                self.output.push("> /cod off".into());
                self.output.push("  Chain of Draft disabled".into());
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            "/think high" => {
                self.thinking_effort = "high".into();
                let _ = self
                    .cmd_tx
                    .blocking_send(BackendCmd::SetThinkingEffort("high".into()));
                self.output.push("> /think high".into());
                self.output.push("  Reasoning: high".into());
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            "/think max" | "/think xhigh" => {
                self.thinking_effort = "max".into();
                let _ = self
                    .cmd_tx
                    .blocking_send(BackendCmd::SetThinkingEffort("max".into()));
                self.output.push("> /think max".into());
                self.output.push("  Reasoning: max".into());
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            _ if task.starts_with("/model ") => {
                let m = task[7..].trim().to_string();
                self.model = m.clone();
                let _ = self.cmd_tx.blocking_send(BackendCmd::SetModel(m.clone()));
                self.toasts.push(crate::board::Toast::new(
                    format!("Model: {m}"),
                    crate::board::ToastLevel::Info,
                    std::time::Duration::from_secs(3),
                ));
                self.output.push(format!(" > /model {m}"));
                self.output.push(format!("  Model -> {m}"));
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            _ => {}
        }
        self.input.clear();
        self.cursor = 0;
        self.hints.clear();
        self.history_idx = None;
        self.welcome = false;
        self.show_help_overlay = false;
        if !task.is_empty() {
            self.history.push(task.to_string());
            self.session_items
                .push(radiumical_core::session::SessionItem::User {
                    content: task.to_string(),
                });
            for line in task.lines() {
                self.output.push(format!("> {line}"));
            }
            self.output.push(String::new());
            self.stick_to_bottom = true;
            self.full_reasoning.clear();
            self.show_full_reasoning = false;
            self.thinking_cancelled = false;
            let final_task = if self.cod_enabled {
                format!("{task}\n\n[Chain of Draft: think in <=5 word steps, be terse. Output reasoning as brief fragments, then final answer.]")
            } else {
                task.to_string()
            };
            let _ = self.cmd_tx.blocking_send(BackendCmd::RunTask(final_task));
        }
    }
}
