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
                self.hint_selected = None;
                self.help_board.visible = false;
                self.model_board.visible = false;
                self.blocks.clear();
                self.session_items.clear();
                self.render_cache.clear();
                self.markdown = crate::markdown::MarkdownRenderer::new();
                self.full_reasoning.clear();
                self.show_full_reasoning = false;
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
                self.hint_selected = None;
                self.help_board.visible = false;
                self.model_board.visible = false;
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
                self.output.push("> /settings".into());
                self.show_settings();
                self.input.clear();
                self.cursor = 0;
                self.hints.clear();
                self.stick_to_bottom = true;
                return;
            }
            "/plan" => {
                self.mode = crate::tui::app::AgentMode::Plan;
                self.output.push("  Plan mode".into());
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            "/exec" => {
                self.mode = crate::tui::app::AgentMode::Exec;
                self.output.push("  Exec mode".into());
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                return;
            }
            "/auto" => {
                self.mode = crate::tui::app::AgentMode::Auto;
                self.output.push("  Auto mode".into());
                self.output.push(String::new());
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
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
                        match radiumical_core::session::Session::save(
                            name,
                            &self.session_items,
                            &self.model,
                            desc,
                        ) {
                            Ok(()) => self.output.push(format!("  Session saved: {name}")),
                            Err(e) => self.output.push(format!("  Save failed: {e}")),
                        }
                    }
                    "load" => {
                        let name = parts.next().unwrap_or("default");
                        match radiumical_core::session::Session::load(name) {
                            Ok(Some((_, items))) => {
                                self.session_items = items;
                                self.render_session_items_to_output();
                                self.output.push(format!("  Loaded: {name}"));
                            }
                            Ok(None) => self.output.push(format!("  Session not found: {name}")),
                            Err(e) => self.output.push(format!("  Load failed: {e}")),
                        }
                    }
                    "list" => match radiumical_core::session::Session::list() {
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
                        match radiumical_core::session::Session::delete(name) {
                            Ok(true) => self.output.push(format!("  Deleted: {name}")),
                            Ok(false) => self.output.push(format!("  Not found: {name}")),
                            Err(e) => self.output.push(format!("  Delete failed: {e}")),
                        }
                    }
                    _ => {
                        self.output.push(
                            "  /session save <name> [desc] | load <name> | list | delete <name>".into(),
                        );
                    }
                }
                self.input.clear();
                self.cursor = 0;
                self.stick_to_bottom = true;
                self.output.push(String::new());
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
                let content = parts.get(1).copied().unwrap_or("");
                match radiumical_core::memory::Memory::load().and_then(|mut m| {
                    m.add(tier, content)?;
                    m.save()
                }) {
                    Ok(()) => self
                        .output
                        .push(format!("  [{tier}] Remembered: {content}")),
                    Err(e) => self.output.push(format!("  Memory error: {e}")),
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
            _ if task == "/models" => {
                self.show_model_picker = !self.show_model_picker;
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
            self.output.push(format!("> {task}"));
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
