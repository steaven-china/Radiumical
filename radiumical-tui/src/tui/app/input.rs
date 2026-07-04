//! Keyboard input handling for the TUI.
//!
//! This module implements `App::handle_key` and all related helpers.
//! It is responsible for routing key events to the correct sub-system:
//! choice panels, provider/model picker, settings overlay, MCP server list,
//! session manager, dashboard, or the main input line. It also handles
//! slash-command completion, input history navigation, and multi-line input.

use crate::session_tui::SessionAction;
use crate::tui::app::App;
use crate::tui::complete_slash;
use crate::tui::matching_hints;
use crate::tui::BackendCmd;
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

impl App {
    /// Dispatch a single crossterm key event to the appropriate handler.
    ///
    /// Modal overlays consume input first (choice panel, provider picker,
    /// settings, MCP list). If no overlay is active, keys are handled by the
    /// main input loop (history, hints, scrolling, and command dispatch).
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        // Choice panel: intercept ALL keys when visible.
        if self.choice_panel.visible {
            self.handle_choice_panel_key(key);
            return;
        }
        if self.provider_picker.visible {
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => {}
                _ => {
                    self.handle_provider_picker_key(key);
                    return;
                }
            }
        }
        if self.overlays.settings {
            if self.settings_board.is_editing() {
                self.handle_settings_edit_key(key);
            } else {
                self.handle_settings_key(key);
            }
            return;
        }
        if self.overlays.mcp && !self.mcp_servers.is_empty() {
            match (key.code, key.modifiers) {
                (KeyCode::Up, _) => {
                    self.overlays.mcp_selected = self.overlays.mcp_selected.saturating_sub(1);
                    return;
                }
                (KeyCode::Down, _) => {
                    if self.overlays.mcp_selected + 1 < self.mcp_servers.len() {
                        self.overlays.mcp_selected += 1;
                    }
                    return;
                }
                (KeyCode::Enter, _) => {
                    if let Some(server) = self.mcp_servers.get_mut(self.overlays.mcp_selected) {
                        server.enabled = !server.enabled;
                        let name = server.name.clone();
                        let enabled = server.enabled;
                        let _ = self
                            .cmd_tx
                            .blocking_send(BackendCmd::ToggleMcpServer { name: name.clone() });
                        self.toasts.push(crate::board::Toast::new(
                            format!(
                                "MCP '{}' {}",
                                name,
                                if enabled { "enabled" } else { "disabled" }
                            ),
                            crate::board::ToastLevel::Info,
                            std::time::Duration::from_secs(3),
                        ));
                    }
                    return;
                }
                (KeyCode::Esc, _) => {
                    self.overlays.mcp = false;
                    self.panels.close(crate::panel::PanelId::Mcp);
                    return;
                }
                _ => {}
            }
        }
        if self.overlays.timeline {
            match (key.code, key.modifiers) {
                (KeyCode::Up, _) => {
                    self.overlays.timeline_selected =
                        self.overlays.timeline_selected.saturating_sub(1);
                    return;
                }
                (KeyCode::Down, _) => {
                    if self.overlays.timeline_selected + 1 < self.overlays.timeline_items.len() {
                        self.overlays.timeline_selected += 1;
                    }
                    return;
                }
                (KeyCode::Enter, _) => {
                    if let Some(cp) = self.overlays.timeline_items.get(self.overlays.timeline_selected) {
                        let workspace = std::path::PathBuf::from(&self.workspace);
                        match radiumical_core::checkpoint::diff_checkpoint_sync(
                            &workspace, &self.session_id, &cp.id,
                        ) {
                            Ok(diff) => self.overlays.timeline_diff = Some(diff),
                            Err(e) => self.toasts.push(crate::board::Toast::new(
                                format!("diff failed: {e}"),
                                crate::board::ToastLevel::Error,
                                std::time::Duration::from_secs(5),
                            )),
                        }
                    }
                    return;
                }
                (KeyCode::Char('r'), _) => {
                    if let Some(cp) = self.overlays.timeline_items.get(self.overlays.timeline_selected) {
                        let workspace = std::path::PathBuf::from(&self.workspace);
                        match radiumical_core::checkpoint::rollback_sync(
                            &workspace, &self.session_id, &cp.id,
                        ) {
                            Ok(()) => {
                                self.toasts.push(crate::board::Toast::new(
                                    format!("Rolled back to {}", cp.id),
                                    crate::board::ToastLevel::Info,
                                    std::time::Duration::from_secs(3),
                                ));
                                self.overlays.timeline_diff = None;
                            }
                            Err(e) => self.toasts.push(crate::board::Toast::new(
                                format!("rollback failed: {e}"),
                                crate::board::ToastLevel::Error,
                                std::time::Duration::from_secs(5),
                            )),
                        }
                    }
                    return;
                }
                (KeyCode::Esc, _) => {
                    self.overlays.timeline = false;
                    self.overlays.timeline_diff = None;
                    self.panels.close(crate::panel::PanelId::Timeline);
                    return;
                }
                _ => {}
            }
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
                self.thinking.show_full_reasoning = !self.thinking.show_full_reasoning;
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if self.thinking.active {
                    let _ = self.cmd_tx.blocking_send(crate::tui::BackendCmd::Cancel);
                    self.thinking.active = false;
                    self.thinking.cancelled = true;
                    self.toasts.push(crate::board::Toast::new(
                        "Cancelled".to_string(),
                        crate::board::ToastLevel::Warn,
                        std::time::Duration::from_secs(2),
                    ));
                } else if !self.session_items.is_empty() {
                    self.confirm.visible = true;
                    self.confirm.message = "Exit? Unsaved session will be auto-saved.".to_string();
                    self.confirm.yes_selected = true;
                } else {
                    self.should_quit = true;
                }
            }
            (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                self.output.clear();
                self.output.push(String::new());
                self.viewport.scroll = 0.0;
                self.viewport.stick_to_bottom = true;
            }
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                self.input.cursor = 0;
            }
            (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                self.input.cursor = self.input.text.len();
            }
            (KeyCode::PageUp, _) => {
                if self.input.hint_selected.is_some() {
                    self.input.hint_page = self.input.hint_page.saturating_sub(1);
                    self.input.hint_selected = Some(0);
                } else if !self.welcome {
                    self.scroll_down(12.0);
                }
            }
            (KeyCode::PageDown, _) => {
                if self.input.hint_selected.is_some() {
                    let max_page = self.input.hints.len().saturating_sub(1) / 8;
                    self.input.hint_page = (self.input.hint_page + 1).min(max_page);
                    self.input.hint_selected = Some(0);
                } else if !self.welcome {
                    self.scroll_up(12.0);
                }
            }
            (KeyCode::Up, _) => {
                if self.session_tui.visible {
                    self.session_tui.select_prev();
                    return;
                }
                if self.dashboard.visible {
                    self.dashboard.up();
                    return;
                }
                if self.input.text.starts_with('/') && self.input.hint_selected.is_some() {
                    let max = self.input.hints.len().saturating_sub(1);
                    self.input.hint_selected = Some(
                        self.input
                            .hint_selected
                            .unwrap_or(0)
                            .saturating_sub(1)
                            .min(max),
                    );
                    self.sync_hint_page();
                } else if self.input.text.starts_with('/') && !self.input.hints.is_empty() {
                    self.input.hint_selected = Some(self.input.hints.len() - 1);
                    self.sync_hint_page();
                } else if !self.input.history.is_empty() {
                    let prefix = if self.input.text.is_empty() {
                        String::new()
                    } else if self.input.history_idx.is_none() {
                        self.input.history_filter_prefix = Some(self.input.text.clone());
                        self.input.text.clone()
                    } else {
                        self.input.history_filter_prefix.clone().unwrap_or_default()
                    };
                    if self.input.history_idx.is_none() {
                        self.input.history_draft = self.input.text.clone();
                    }
                    let from = self
                        .input
                        .history_idx
                        .map_or(self.input.history.len() - 1, |i| i.saturating_sub(1));
                    if let Some(i) = self.find_prev_history_match(&prefix, from) {
                        self.input.history_idx = Some(i);
                        self.input.text = self.input.history[i].clone();
                        self.input.cursor = self.input.text.len();
                        self.input.hints.clear();
                    }
                }
            }
            (KeyCode::Down, _) => {
                if self.session_tui.visible {
                    self.session_tui.select_next();
                    return;
                }
                if self.dashboard.visible {
                    self.dashboard.down();
                    return;
                }
                if self.input.text.starts_with('/') && self.input.hint_selected.is_some() {
                    let max = self.input.hints.len().saturating_sub(1);
                    let next = (self.input.hint_selected.unwrap_or(0) + 1).min(max);
                    self.input.hint_selected = Some(next);
                    self.sync_hint_page();
                } else if self.input.text.starts_with('/') && !self.input.hints.is_empty() {
                    self.input.hint_selected = Some(0);
                } else if let Some(i) = self.input.history_idx {
                    let prefix = self.input.history_filter_prefix.clone().unwrap_or_default();
                    let next_start = i + 1;
                    if next_start < self.input.history.len() {
                        if let Some(j) = self.find_next_history_match(&prefix, next_start) {
                            self.input.text = self.input.history[j].clone();
                            self.input.history_idx = Some(j);
                        } else {
                            self.input.text = self.input.history_draft.clone();
                            self.input.history_idx = None;
                            self.input.history_filter_prefix = None;
                        }
                    } else {
                        self.input.text = self.input.history_draft.clone();
                        self.input.history_idx = None;
                        self.input.history_filter_prefix = None;
                    }
                    self.input.cursor = self.input.text.len();
                    self.input.hints.clear();
                }
            }
            (KeyCode::Enter, KeyModifiers::SHIFT) => {
                self.input.history_idx = None;
                self.input.history_filter_prefix = None;
                self.input.text.insert(self.input.cursor, '\n');
                self.input.cursor += 1;
                self.update_hints();
            }
            (KeyCode::Enter, _) => {
                if self.session_tui.visible {
                    self.handle_session_tui_enter();
                    return;
                }
                if self.input.text.trim() == "//" {
                    self.dashboard.toggle();
                    self.input.text.clear();
                    self.input.cursor = 0;
                    return;
                }
                if self.confirm.visible {
                    if self.confirm.yes_selected {
                        if self.confirm.message.contains("Exit") {
                            // Auto-save session before exit
                            if !self.session_items.is_empty() {
                                let desc = self.input.history.first().cloned();
                                let mode: radiumical_core::session::SessionMode =
                                    self.mode.clone().into();
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
                            self.should_quit = true;
                        } else if self.confirm.message.contains("Clear") {
                            self.output.clear();
                            self.input.text.clear();
                            self.input.cursor = 0;
                            self.input.hints.clear();
                            self.viewport.scroll = 0.0;
                            self.viewport.stick_to_bottom = true;
                        }
                    }
                    self.confirm.visible = false;
                    return;
                }
                if self.dashboard.visible {
                    if let Some(action) = self.dashboard.selected_action() {
                        self.dispatch_dash_action(action);
                        self.dashboard.visible = false;
                    }
                    return;
                }
                if let Some(idx) = self.input.hint_selected {
                    if let Some((name, _)) = self.input.hints.get(idx) {
                        self.input.text = format!("{} ", name);
                        self.input.cursor = self.input.text.len();
                    }
                    self.input.hint_selected = None;
                    self.update_hints();
                    return;
                }
                let task = self.input.text.trim().to_string();
                self.handle_command(&task);
            }
            (KeyCode::Char(ch), mods) => {
                self.input.history_idx = None;
                self.input.history_filter_prefix = None;
                if mods.contains(KeyModifiers::CONTROL) {
                    match ch {
                        'w' if self.input.cursor > 0 => {
                            self.delete_word_before();
                        }
                        'u' => {
                            self.input.text.drain(..self.input.cursor);
                            self.input.cursor = 0;
                        }
                        _ => {}
                    }
                } else if self.session_tui.visible {
                    match self.session_tui.focus {
                        crate::session_tui::SessionFocus::NameEdit => {
                            self.session_tui.name_buffer.push(ch);
                        }
                        crate::session_tui::SessionFocus::DescEdit => {
                            self.session_tui.desc_buffer.push(ch);
                        }
                        _ => {}
                    }
                } else {
                    self.input.text.insert(self.input.cursor, ch);
                    self.input.cursor += ch.len_utf8();
                }
                self.update_hints();
            }
            (KeyCode::Backspace, _) if self.input.cursor > 0 => {
                self.input.history_idx = None;
                self.input.history_filter_prefix = None;
                let prev = self.prev_char_boundary(self.input.cursor);
                self.input.text.drain(prev..self.input.cursor);
                self.input.cursor = prev;
                self.update_hints();
            }
            (KeyCode::Delete, _) if self.input.cursor < self.input.text.len() => {
                self.input.history_idx = None;
                self.input.history_filter_prefix = None;
                let next = self.next_char_boundary(self.input.cursor);
                self.input.text.drain(self.input.cursor..next);
                self.update_hints();
            }
            (KeyCode::Left, _) => {
                if self.session_tui.visible {
                    self.session_tui.focus_left();
                    return;
                }
                if self.dashboard.visible {
                    self.dashboard.left();
                    return;
                }
                if self.settings_board.visible {
                    self.settings_board.adjust(-1);
                    return;
                }
                self.input.cursor = self.prev_char_boundary(self.input.cursor);
            }
            (KeyCode::Right, _) => {
                if self.session_tui.visible {
                    self.session_tui.focus_right();
                    return;
                }
                if self.dashboard.visible {
                    self.dashboard.right();
                    return;
                }
                self.input.history_idx = None;
                self.input.history_filter_prefix = None;
                self.input.cursor = self.next_char_boundary(self.input.cursor);
            }
            (KeyCode::Home, _) => {
                self.input.history_idx = None;
                self.input.history_filter_prefix = None;
                self.input.cursor = 0;
            }
            (KeyCode::End, _) => {
                if self.input.text.is_empty() {
                    self.viewport.stick_to_bottom = true;
                    self.viewport.scroll = 0.0;
                } else {
                    self.input.history_idx = None;
                    self.input.history_filter_prefix = None;
                    self.input.cursor = self.input.text.len();
                }
            }
            (KeyCode::Tab, _) => {
                if self.session_tui.visible {
                    match self.session_tui.focus {
                        crate::session_tui::SessionFocus::List => self.session_tui.focus_right(),
                        crate::session_tui::SessionFocus::Actions => self.session_tui.focus_name(),
                        crate::session_tui::SessionFocus::NameEdit => self.session_tui.focus_desc(),
                        crate::session_tui::SessionFocus::DescEdit => self.session_tui.focus_left(),
                        _ => {}
                    }
                    return;
                }
                if self.input.text.starts_with('/') && self.input.hint_selected.is_none() {
                    if let Some(completed) = complete_slash(&self.input.text) {
                        self.input.text = completed;
                        self.input.cursor = self.input.text.len();
                        self.update_hints();
                        return;
                    }
                }
                if self.input.text.starts_with('/') && !self.input.hints.is_empty() {
                    self.input.hint_selected = Some(0);
                    self.sync_hint_page();
                }
            }
            (KeyCode::BackTab, _) => {
                if self.session_tui.visible {
                    match self.session_tui.focus {
                        crate::session_tui::SessionFocus::Actions => self.session_tui.focus_left(),
                        crate::session_tui::SessionFocus::NameEdit => {
                            self.session_tui.focus_right()
                        }
                        crate::session_tui::SessionFocus::DescEdit => self.session_tui.focus_name(),
                        _ => {}
                    }
                    return;
                }
                if self.input.hint_selected.is_some() {
                    self.input.hint_selected =
                        Some(self.input.hint_selected.unwrap_or(0).saturating_sub(1));
                }
            }
            (KeyCode::Esc, _) => {
                if self.session_tui.visible {
                    self.session_tui.close();
                    return;
                }
                if self.dashboard.visible {
                    self.dashboard.visible = false;
                    return;
                }
                if self.confirm.visible {
                    self.confirm.visible = false;
                    return;
                }
                if self.overlays.settings {
                    self.commit_settings();
                    self.overlays.settings = false;
                    self.settings_board.visible = false;
                    return;
                }
                if self.thinking.active {
                    let _ = self.cmd_tx.blocking_send(crate::tui::BackendCmd::Cancel);
                    self.thinking.active = false;
                    self.thinking.cancelled = true;
                }
                self.overlays.help = false;
                self.overlays.model_picker = false;
                self.provider_picker.close();
                self.input.hint_selected = None;
                self.input.hint_page = 0;
                self.help_board.visible = false;
            }
            _ => {}
        }
    }

    /// Handle keys while the provider/model picker overlay is open.
    fn handle_provider_picker_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.overlays.model_picker = self.provider_picker.toggle(&self.cmd_tx);
            }
            (KeyCode::Up, _) => self.provider_picker.select_prev(),
            (KeyCode::Down, _) => self.provider_picker.select_next(),
            (KeyCode::Tab, _) => self.provider_picker.toggle_focus(),
            (KeyCode::Enter, _) => {
                if self.provider_picker.focus_providers {
                    if let Some(source) = self.provider_picker.current_provider().cloned() {
                        self.provider_name = source.name.clone();
                        let _ = self
                            .cmd_tx
                            .blocking_send(crate::tui::BackendCmd::FetchModels(source));
                    }
                } else if let Some(model) = self.provider_picker.current_model() {
                    let m = model.to_string();
                    self.model = m.clone();
                    let _ = self
                        .cmd_tx
                        .blocking_send(crate::tui::BackendCmd::SetModel(m.clone()));
                    self.toasts.push(crate::board::Toast::new(
                        format!("Model: {m}"),
                        crate::board::ToastLevel::Info,
                        std::time::Duration::from_secs(3),
                    ));
                    self.provider_picker.close();
                    self.overlays.model_picker = false;
                }
            }
            _ => {}
        }
    }

    /// Return the previous Unicode scalar boundary before `pos` in the input line.
    pub(crate) fn prev_char_boundary(&self, pos: usize) -> usize {
        self.input.text[..pos]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(pos.saturating_sub(1))
    }
    /// Return the next Unicode scalar boundary after `pos` in the input line.
    pub(crate) fn next_char_boundary(&self, pos: usize) -> usize {
        self.input.text[pos..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| pos + i)
            .unwrap_or(self.input.text.len())
    }
    /// Delete the whitespace-delimited word immediately before the cursor.
    pub(crate) fn delete_word_before(&mut self) {
        let before = &self.input.text[..self.input.cursor];
        let cut = before
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_whitespace())
            .map(|(i, _)| i + 1)
            .unwrap_or(0);
        self.input.text.drain(cut..self.input.cursor);
        self.input.cursor = cut;
    }
    /// Refresh slash-command hints based on the current input text.
    pub(crate) fn update_hints(&mut self) {
        if self.input.text.starts_with('/') && self.input.text.len() <= 30 {
            self.input.hints = matching_hints(&self.input.text)
                .into_iter()
                .map(|(n, d)| (n.to_string(), d.to_string()))
                .collect();
        } else {
            self.input.hints.clear();
        }
        self.input.hint_page = 0;
        self.input.hint_selected = None;
    }

    /// Update `hint_page` so the currently selected hint is visible.
    pub(crate) fn sync_hint_page(&mut self) {
        if let Some(sel) = self.input.hint_selected {
            self.input.hint_page = sel / 8;
        }
    }

    /// Find the most recent history entry at or before `from` that starts with `prefix`.
    pub(crate) fn find_prev_history_match(&self, prefix: &str, from: usize) -> Option<usize> {
        if prefix.is_empty() {
            return if from < self.input.history.len() {
                Some(from)
            } else {
                None
            };
        }
        (0..=from)
            .rev()
            .find(|&i| self.input.history[i].starts_with(prefix))
    }

    /// Find the next history entry at or after `from` that starts with `prefix`.
    pub(crate) fn find_next_history_match(&self, prefix: &str, from: usize) -> Option<usize> {
        if prefix.is_empty() {
            return if from < self.input.history.len() {
                Some(from)
            } else {
                None
            };
        }
        (from..self.input.history.len()).find(|&i| self.input.history[i].starts_with(prefix))
    }

    /// Handle keys while the settings overlay has focus (not editing a value).
    pub(crate) fn handle_settings_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('q'), _) => {
                self.commit_settings();
                self.overlays.settings = false;
                self.settings_board.visible = false;
            }
            (KeyCode::Up, _) => self.settings_board.select_prev(),
            (KeyCode::Down, _) => self.settings_board.select_next(),
            (KeyCode::Left, _) | (KeyCode::Char('-'), _) => self.settings_board.adjust(-1),
            (KeyCode::Right, _) | (KeyCode::Char('+'), _) | (KeyCode::Char('='), _) => {
                self.settings_board.adjust(1)
            }
            (KeyCode::Enter, _) => self.settings_board.begin_edit(),
            _ => {}
        }
    }

    /// Handle keys while editing a single settings value.
    pub(crate) fn handle_settings_edit_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        match (key.code, key.modifiers) {
            (KeyCode::Enter, _) => {
                self.settings_board.commit_edit();
                self.commit_settings();
            }
            (KeyCode::Esc, _) => self.settings_board.cancel_edit(),
            (KeyCode::Left, _) => self.settings_board.edit_left(),
            (KeyCode::Right, _) => self.settings_board.edit_right(),
            (KeyCode::Backspace, _) => self.settings_board.edit_backspace(),
            (KeyCode::Delete, _) => self.settings_board.edit_delete(),
            (KeyCode::Home, _) => self.settings_board.edit_cursor = 0,
            (KeyCode::End, _) => {
                self.settings_board.edit_cursor = self.settings_board.edit_buffer.len()
            }
            (KeyCode::Char(ch), mods) if !mods.contains(KeyModifiers::CONTROL) => {
                self.settings_board.edit_insert(ch);
            }
            _ => {}
        }
    }

    /// Dispatch Enter inside the full-screen session manager.
    fn handle_session_tui_enter(&mut self) {
        use crate::session_tui::{SessionAction, SessionFocus};
        match self.session_tui.focus {
            SessionFocus::List => {
                // Sync name from current list selection before executing action.
                self.session_tui.sync_name_desc_from_selection();
                let action = self.session_tui.selected_action();
                self.dispatch_session_action(action);
            }
            SessionFocus::Actions => {
                let action = self.session_tui.selected_action();
                self.dispatch_session_action(action);
            }
            SessionFocus::NameEdit | SessionFocus::DescEdit => {
                // Commit edit by moving focus back to actions
                self.session_tui.focus_right();
            }
            SessionFocus::ConfirmDelete => {
                self.dispatch_session_action(SessionAction::Delete);
                self.session_tui.focus = SessionFocus::List;
            }
        }
    }

    /// Execute a session manager action (new/load/save/delete).
    fn dispatch_session_action(&mut self, action: SessionAction) {
        use crate::session_tui::SessionFocus;
        use radiumical_core::session::SessionMode;

        match action {
            SessionAction::New => {
                self.handle_command("/new");
                self.session_tui.close();
            }
            SessionAction::Load => {
                let name = self.session_tui.name_buffer.trim().to_string();
                if name.is_empty() {
                    self.session_tui.set_message("Enter a session name to load");
                    return;
                }
                match self.session_pool.load(&name) {
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
                        self.session_tui.close();
                        self.toasts.push(crate::board::Toast::new(
                            format!("Loaded session: {name}"),
                            crate::board::ToastLevel::Info,
                            std::time::Duration::from_secs(3),
                        ));
                    }
                    Ok(None) => {
                        self.session_tui
                            .set_message(format!("Session not found: {name}"));
                    }
                    Err(e) => {
                        self.session_tui.set_message(format!("Load failed: {e}"));
                    }
                }
            }
            SessionAction::Save => {
                let name = self.session_tui.name_buffer.trim().to_string();
                if name.is_empty() {
                    self.session_tui.set_message("Enter a session name to save");
                    return;
                }
                let desc = self.session_tui.desc_buffer.trim();
                let desc = if desc.is_empty() {
                    None
                } else {
                    Some(desc.to_string())
                };
                let mode: SessionMode = self.mode.clone().into();
                match self.session_pool.save(
                    &name,
                    &self.session_items,
                    &self.model,
                    &self.provider_name,
                    mode,
                    &self.thinking.effort,
                    desc.as_deref(),
                ) {
                    Ok(()) => {
                        self.session_tui.clear_message();
                        self.session_tui.set_message(format!("Saved: {name}"));
                        self.refresh_session_tui_list();
                    }
                    Err(e) => {
                        self.session_tui.set_message(format!("Save failed: {e}"));
                    }
                }
            }
            SessionAction::Delete => {
                let name = self.session_tui.name_buffer.trim().to_string();
                if name.is_empty() {
                    self.session_tui
                        .set_message("No session selected to delete");
                    return;
                }
                if self.session_tui.focus != SessionFocus::ConfirmDelete {
                    self.session_tui
                        .set_message(format!("Press Enter again to confirm deleting '{name}'"));
                    self.session_tui.focus = SessionFocus::ConfirmDelete;
                    return;
                }
                match self.session_pool.delete(&name) {
                    Ok(true) => {
                        self.session_tui.clear_message();
                        self.session_tui.set_message(format!("Deleted: {name}"));
                        self.refresh_session_tui_list();
                        self.session_tui.name_buffer.clear();
                        self.session_tui.desc_buffer.clear();
                        self.session_tui.focus = SessionFocus::List;
                    }
                    Ok(false) => {
                        self.session_tui.set_message(format!("Not found: {name}"));
                        self.session_tui.focus = SessionFocus::List;
                    }
                    Err(e) => {
                        self.session_tui.set_message(format!("Delete failed: {e}"));
                        self.session_tui.focus = SessionFocus::List;
                    }
                }
            }
        }
    }

    /// Reload the session list and keep the current selection stable.
    fn refresh_session_tui_list(&mut self) {
        if let Ok(sessions) = self.session_pool.list() {
            let selected_name = self.session_tui.name_buffer.clone();
            self.session_tui.sessions = sessions;
            // Try to keep selection on the same session, or first one.
            self.session_tui.selected = self
                .session_tui
                .sessions
                .iter()
                .position(|s| s.name == selected_name)
                .unwrap_or(0)
                .min(self.session_tui.sessions.len().saturating_sub(1));
            // Sync name/desc from new selection.
            if let Some(meta) = self.session_tui.sessions.get(self.session_tui.selected) {
                self.session_tui.name_buffer = meta.name.clone();
                self.session_tui.desc_buffer = meta.description.clone();
            }
        }
    }

    /// Handle all keyboard input when the choice panel is visible.
    fn handle_choice_panel_key(&mut self, key: crossterm::event::KeyEvent) {
        use crate::choice_panel::ChoiceMode;
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Up => self.choice_panel.select_prev(),
            KeyCode::Down => self.choice_panel.select_next(),
            KeyCode::Esc => self.choice_panel.close(),
            KeyCode::Char(' ') if self.choice_panel.mode == ChoiceMode::Multi => {
                self.choice_panel.toggle_current();
            }
            KeyCode::Enter => {
                let response = self.choice_panel.get_response();
                let id = self.choice_panel.id.clone();
                self.choice_panel.close();
                let _ = self
                    .cmd_tx
                    .blocking_send(crate::tui::BackendCmd::ChoiceResponse {
                        id,
                        value: response,
                    });
            }
            KeyCode::Char(ch) => match self.choice_panel.mode {
                ChoiceMode::Input => {
                    self.choice_panel.input_buffer.push(ch);
                    self.choice_panel.input_cursor = self.choice_panel.input_buffer.len();
                }
                _ if ch.is_ascii_digit() => {
                    let n = ch.to_digit(10).unwrap() as usize;
                    if n >= 1 && n <= self.choice_panel.options.len() {
                        self.choice_panel.selected = n - 1;
                    }
                }
                _ => {}
            },
            KeyCode::Backspace
                if self.choice_panel.mode == ChoiceMode::Input
                    && self.choice_panel.input_cursor > 0 =>
            {
                let prev = self
                    .choice_panel
                    .input_buffer
                    .char_indices()
                    .nth(self.choice_panel.input_cursor - 1)
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                self.choice_panel.input_buffer.drain(prev..);
                self.choice_panel.input_cursor = prev;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_session_action_label() {
        use crate::session_tui::SessionAction;
        assert_eq!(SessionAction::Load.label(), "[ Load  ]");
        assert_eq!(SessionAction::Save.label(), "[ Save  ]");
        assert_eq!(SessionAction::Delete.label(), "[ Delete]");
        assert_eq!(SessionAction::New.label(), "[ New   ]");
    }
}
