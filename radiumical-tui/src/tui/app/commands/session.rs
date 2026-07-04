use crate::tui::app::App;
use crate::tui::BackendCmd;

impl App {
    pub(super) fn cmd_new(&mut self) -> bool {
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
        self.output.push("  Type a task to get started, or use:".into());
        self.output.push("    //        — open dashboard".into());
        self.output.push("    /help     — show all commands".into());
        self.output.push("    /provider — switch model".into());
        self.output.push("    /sessions — manage sessions".into());
        self.output.push(String::new());
        self.output.push("  Ctrl+C cancel  |  Esc close overlay  |  ↑↓ history".into());
        self.output.push(String::new());
        true
    }

    pub(super) fn cmd_clear(&mut self) -> bool {
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
        true
    }

    pub(super) fn cmd_sessions_tui(&mut self) -> bool {
        if let Ok(sessions) = self.session_pool.list() {
            self.session_tui.open(sessions, None, None);
            if let Some(first) = self.session_tui.sessions.first() {
                self.session_tui.name_buffer = first.name.clone();
                self.session_tui.desc_buffer = first.description.clone();
            }
        }
        self.input.clear();
        self.cursor = 0;
        self.hints.clear();
        true
    }

    pub(super) fn cmd_session_help(&mut self) -> bool {
        self.output.push(
            "  /session save <name> [desc] | load <name> | list | delete <name> | tui".into(),
        );
        self.input.clear();
        self.cursor = 0;
        self.hints.clear();
        self.stick_to_bottom = true;
        self.output.push(String::new());
        true
    }

    pub(super) fn cmd_session(&mut self, task: &str) -> bool {
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
        true
    }
}
