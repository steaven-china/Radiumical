//! Session-management slash commands (`/new`, `/clear`, `/session save|load|list|delete`).

use crate::tui::app::App;
use crate::tui::BackendCmd;

impl App {
    pub(super) fn cmd_new(&mut self) -> bool {
        if !self.session_items.is_empty() {
            let desc = self.input.history.first().cloned();
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
                &self.thinking.effort,
                desc.as_deref(),
            );
        }
        self.output.clear();
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.input.hints.clear();
        self.viewport.scroll = 0.0;
        self.viewport.stick_to_bottom = true;
        self.welcome = true;
        self.overlays.help = true;
        self.overlays.model_picker = false;
        self.provider_picker.close();
        self.input.hint_selected = None;
        self.help_board.visible = false;
        self.blocks.clear();
        self.session_items.clear();
        self.render_cache.clear();
        self.render_cache_order.clear();
        self.session_title = None;
        self.markdown = crate::markdown::MarkdownRenderer::new();
        self.thinking.full_reasoning.clear();
        self.thinking.show_full_reasoning = false;
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
            .push("  Type a task to get started, or use:".into());
        self.output.push("    //        — open dashboard".into());
        self.output.push("    /help     — show all commands".into());
        self.output.push("    /provider — switch model".into());
        self.output.push("    /sessions — manage sessions".into());
        self.output.push(String::new());
        self.output
            .push("  Ctrl+C cancel  |  Esc close overlay  |  ↑↓ history".into());
        self.output.push(String::new());
        true
    }

    pub(super) fn cmd_clear(&mut self) -> bool {
        self.output.clear();
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.input.hints.clear();
        self.viewport.scroll = 0.0;
        self.viewport.stick_to_bottom = true;
        self.welcome = false;
        self.overlays.help = false;
        self.overlays.model_picker = false;
        self.provider_picker.close();
        self.input.hint_selected = None;
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
        self.input.text.clear();
        self.input.cursor = 0;
        self.input.hints.clear();
        true
    }

    pub(super) fn cmd_ws_tui(&mut self) -> bool {
        let registry = radiumical_core::session::WorkspaceRegistry::load();
        self.session_tui.open_workspaces(registry.workspaces);
        self.input.text.clear();
        self.input.cursor = 0;
        self.input.hints.clear();
        true
    }

    pub(super) fn cmd_session_help(&mut self) -> bool {
        self.output.push(
            "  /session save <name> [desc] | load <name> | list | delete <name> | tui".into(),
        );
        self.output.push(
            "  /session ws | list-ws | switch-ws <name> | add-ws <path> [name] | remove-ws <name>"
                .into(),
        );
        self.output.push(
            "  /session tag <ws> <tag> | untag <ws> <tag> | pin <ws> | unpin <ws>".into(),
        );
        self.output.push(
            "  /session ws-set <key> <val> | ws-unset <key> | ws-settings".into(),
        );
        self.input.text.clear();
        self.input.cursor = 0;
        self.input.hints.clear();
        self.viewport.stick_to_bottom = true;
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
                    &self.thinking.effort,
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
                        self.thinking.effort = meta.thinking_effort.clone();
                        let _ = self
                            .cmd_tx
                            .blocking_send(BackendCmd::SetMode(self.mode.clone()));
                        let _ = self
                            .cmd_tx
                            .blocking_send(BackendCmd::SetModel(self.model.clone()));
                        let _ = self.cmd_tx.blocking_send(BackendCmd::SetThinkingEffort(
                            self.thinking.effort.clone(),
                        ));
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
            "ws" | "workspace" => {
                let registry = radiumical_core::session::WorkspaceRegistry::load();
                match registry.active_entry() {
                    Some(entry) => {
                        self.output
                            .push(format!("  Active workspace: {}", entry.name));
                        self.output.push(format!("    Path: {}", entry.path));
                        self.output.push(format!("    Hash: {}", entry.hash));
                        if !entry.tags.is_empty() {
                            self.output
                                .push(format!("    Tags: {}", entry.tags.join(", ")));
                        }
                        self.output
                            .push(format!("    Pinned: {}", entry.pinned));
                        self.output
                            .push(format!("    Last active: {}", entry.last_active));
                    }
                    None => {
                        self.output.push("  No active workspace".into());
                    }
                }
            }
            "list-ws" => {
                let registry = radiumical_core::session::WorkspaceRegistry::load();
                if registry.workspaces.is_empty() {
                    self.output.push("  No registered workspaces".into());
                } else {
                    for ws in &registry.workspaces {
                        let active = if registry.active.as_deref() == Some(ws.name.as_str()) {
                            " *"
                        } else {
                            ""
                        };
                        let pin = if ws.pinned { " [pinned]" } else { "" };
                        let tags = if ws.tags.is_empty() {
                            String::new()
                        } else {
                            format!(" [{}]", ws.tags.join(", "))
                        };
                        self.output.push(format!(
                            "  {}{}{} — {}{}",
                            ws.name, active, pin, ws.path, tags
                        ));
                    }
                }
            }
            "switch-ws" => {
                let name = parts.next().unwrap_or("");
                let mut registry = radiumical_core::session::WorkspaceRegistry::load();
                match registry.switch(name) {
                    Ok(()) => {
                        self.toasts.push(crate::board::Toast::new(
                            format!("Switched to workspace: {name}"),
                            crate::board::ToastLevel::Info,
                            std::time::Duration::from_secs(3),
                        ));
                        self.output.push(format!("  Switched to: {name}"));
                    }
                    Err(e) => self.output.push(format!("  Switch failed: {e}")),
                }
            }
            "add-ws" => {
                let path = parts.next().unwrap_or("");
                let name = parts.next();
                let mut registry = radiumical_core::session::WorkspaceRegistry::load();
                match registry.register(path, name) {
                    Ok(ws_name) => {
                        self.toasts.push(crate::board::Toast::new(
                            format!("Registered workspace: {ws_name}"),
                            crate::board::ToastLevel::Info,
                            std::time::Duration::from_secs(3),
                        ));
                        self.output.push(format!("  Registered: {ws_name}"));
                    }
                    Err(e) => self.output.push(format!("  Register failed: {e}")),
                }
            }
            "remove-ws" => {
                let name = parts.next().unwrap_or("");
                let mut registry = radiumical_core::session::WorkspaceRegistry::load();
                match registry.remove(name) {
                    Ok(true) => {
                        self.toasts.push(crate::board::Toast::new(
                            format!("Removed workspace: {name}"),
                            crate::board::ToastLevel::Info,
                            std::time::Duration::from_secs(3),
                        ));
                        self.output.push(format!("  Removed: {name}"));
                    }
                    Ok(false) => self.output.push(format!("  Not found: {name}")),
                    Err(e) => self.output.push(format!("  Remove failed: {e}")),
                }
            }
            "tag" => {
                let ws_name = parts.next().unwrap_or("");
                let tag = parts.next().unwrap_or("");
                let mut registry = radiumical_core::session::WorkspaceRegistry::load();
                match registry.add_tag(ws_name, tag) {
                    Ok(()) => self
                        .output
                        .push(format!("  Tagged '{ws_name}' with '{tag}'")),
                    Err(e) => self.output.push(format!("  Tag failed: {e}")),
                }
            }
            "untag" => {
                let ws_name = parts.next().unwrap_or("");
                let tag = parts.next().unwrap_or("");
                let mut registry = radiumical_core::session::WorkspaceRegistry::load();
                match registry.remove_tag(ws_name, tag) {
                    Ok(()) => self
                        .output
                        .push(format!("  Untagged '{ws_name}': '{tag}'")),
                    Err(e) => self.output.push(format!("  Untag failed: {e}")),
                }
            }
            "pin" => {
                let ws_name = parts.next().unwrap_or("");
                let mut registry = radiumical_core::session::WorkspaceRegistry::load();
                match registry.set_pinned(ws_name, true) {
                    Ok(()) => self.output.push(format!("  Pinned: {ws_name}")),
                    Err(e) => self.output.push(format!("  Pin failed: {e}")),
                }
            }
            "unpin" => {
                let ws_name = parts.next().unwrap_or("");
                let mut registry = radiumical_core::session::WorkspaceRegistry::load();
                match registry.set_pinned(ws_name, false) {
                    Ok(()) => self.output.push(format!("  Unpinned: {ws_name}")),
                    Err(e) => self.output.push(format!("  Unpin failed: {e}")),
                }
            }
            "ws-set" => {
                let key = parts.next().unwrap_or("");
                let value = parts.next().unwrap_or("");
                let ws_hash = radiumical_core::session::workspace_hash(&self.workspace);
                let mut settings =
                    radiumical_core::session::load_workspace_settings(&ws_hash);
                let valid = match key {
                    "model" => {
                        settings.model = Some(value.to_string());
                        true
                    }
                    "mode" => {
                        settings.mode = Some(value.to_string());
                        true
                    }
                    "thinking_effort" => {
                        settings.thinking_effort = Some(value.to_string());
                        true
                    }
                    "max_context_tokens" => match value.parse::<usize>() {
                        Ok(n) => {
                            settings.max_context_tokens = Some(n);
                            true
                        }
                        Err(_) => {
                            self.output.push(format!("  Invalid number: {value}"));
                            false
                        }
                    },
                    "llm_timeout_secs" => match value.parse::<u64>() {
                        Ok(n) => {
                            settings.llm_timeout_secs = Some(n);
                            true
                        }
                        Err(_) => {
                            self.output.push(format!("  Invalid number: {value}"));
                            false
                        }
                    },
                    "context_compress_ratio" => match value.parse::<f64>() {
                        Ok(n) => {
                            settings.context_compress_ratio = Some(n);
                            true
                        }
                        Err(_) => {
                            self.output.push(format!("  Invalid number: {value}"));
                            false
                        }
                    },
                    "auto_continue" => match value.parse::<bool>() {
                        Ok(b) => {
                            settings.auto_continue = Some(b);
                            true
                        }
                        Err(_) => {
                            self.output
                                .push(format!("  Invalid boolean: {value}"));
                            false
                        }
                    },
                    _ => {
                        self.output.push(format!("  Unknown key: {key}"));
                        self.output.push(
                            "  Valid: model, mode, thinking_effort, max_context_tokens, \
                             llm_timeout_secs, context_compress_ratio, auto_continue"
                                .into(),
                        );
                        false
                    }
                };
                if valid {
                    match radiumical_core::session::save_workspace_settings(
                        &ws_hash,
                        &settings,
                    ) {
                        Ok(()) => {
                            self.toasts.push(crate::board::Toast::new(
                                format!("Workspace setting saved: {key} = {value}"),
                                crate::board::ToastLevel::Info,
                                std::time::Duration::from_secs(3),
                            ));
                            self.output.push(format!("  Set {key} = {value}"));
                        }
                        Err(e) => self.output.push(format!("  Save failed: {e}")),
                    }
                }
            }
            "ws-unset" => {
                let key = parts.next().unwrap_or("");
                let ws_hash = radiumical_core::session::workspace_hash(&self.workspace);
                let mut settings =
                    radiumical_core::session::load_workspace_settings(&ws_hash);
                let valid = match key {
                    "model" => {
                        settings.model = None;
                        true
                    }
                    "mode" => {
                        settings.mode = None;
                        true
                    }
                    "thinking_effort" => {
                        settings.thinking_effort = None;
                        true
                    }
                    "max_context_tokens" => {
                        settings.max_context_tokens = None;
                        true
                    }
                    "llm_timeout_secs" => {
                        settings.llm_timeout_secs = None;
                        true
                    }
                    "context_compress_ratio" => {
                        settings.context_compress_ratio = None;
                        true
                    }
                    "auto_continue" => {
                        settings.auto_continue = None;
                        true
                    }
                    _ => {
                        self.output.push(format!("  Unknown key: {key}"));
                        false
                    }
                };
                if valid {
                    match radiumical_core::session::save_workspace_settings(
                        &ws_hash,
                        &settings,
                    ) {
                        Ok(()) => self.output.push(format!("  Unset {key}")),
                        Err(e) => self.output.push(format!("  Save failed: {e}")),
                    }
                }
            }
            "ws-settings" => {
                let ws_hash = radiumical_core::session::workspace_hash(&self.workspace);
                let settings =
                    radiumical_core::session::load_workspace_settings(&ws_hash);
                self.output.push("  Workspace config overrides:".into());
                let mut any = false;
                if let Some(ref v) = settings.model {
                    self.output.push(format!("    model = {v}"));
                    any = true;
                }
                if let Some(ref v) = settings.mode {
                    self.output.push(format!("    mode = {v}"));
                    any = true;
                }
                if let Some(ref v) = settings.thinking_effort {
                    self.output.push(format!("    thinking_effort = {v}"));
                    any = true;
                }
                if let Some(v) = settings.max_context_tokens {
                    self.output.push(format!("    max_context_tokens = {v}"));
                    any = true;
                }
                if let Some(v) = settings.llm_timeout_secs {
                    self.output.push(format!("    llm_timeout_secs = {v}"));
                    any = true;
                }
                if let Some(v) = settings.context_compress_ratio {
                    self.output
                        .push(format!("    context_compress_ratio = {v}"));
                    any = true;
                }
                if let Some(v) = settings.auto_continue {
                    self.output.push(format!("    auto_continue = {v}"));
                    any = true;
                }
                if !any {
                    self.output.push("    (none)".into());
                }
            }
            _ => {
                self.output.push(
                    "  /session save <name> [desc] | load <name> | list | delete <name> | tui"
                        .into(),
                );
                self.output.push(
                    "  /session ws | list-ws | switch-ws | add-ws | remove-ws | tag | untag | pin | unpin"
                        .into(),
                );
                self.output.push(
                    "  /session ws-set <key> <val> | ws-unset <key> | ws-settings".into(),
                );
            }
        }
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        self.output.push(String::new());
        true
    }
}
