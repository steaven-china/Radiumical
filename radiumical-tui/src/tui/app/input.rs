use crate::session_tui::SessionAction;
use crate::tui::app::App;
use crate::tui::BackendCmd;
use crate::tui::matching_hints;
use crate::tui::complete_slash;
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};

impl App {
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        if key.kind == KeyEventKind::Release {
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
        if self.settings_visible {
            if self.settings_board.is_editing() {
                self.handle_settings_edit_key(key);
            } else {
                self.handle_settings_key(key);
            }
            return;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
                self.show_full_reasoning = !self.show_full_reasoning;
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => self.should_quit = true,
            (KeyCode::PageUp, _) => {
                if self.hint_selected.is_some() {
                    self.hint_page = self.hint_page.saturating_sub(1);
                    self.hint_selected = Some(0);
                } else if !self.welcome {
                    self.scroll_down(12.0);
                }
            }
            (KeyCode::PageDown, _) => {
                if self.hint_selected.is_some() {
                    let max_page = self.hints.len().saturating_sub(1) / 8;
                    self.hint_page = (self.hint_page + 1).min(max_page);
                    self.hint_selected = Some(0);
                } else if !self.welcome {
                    self.scroll_up(12.0);
                }
            }
            (KeyCode::Up, _) => {
                if self.choice_panel.visible {
                    self.choice_panel.select_prev();
                    return;
                }
                if self.session_tui.visible {
                    self.session_tui.select_prev();
                    return;
                }
                if self.dashboard.visible {
                    self.dashboard.up();
                    return;
                }
                if self.input.starts_with('/') && self.hint_selected.is_some() {
                    let max = self.hints.len().saturating_sub(1);
                    self.hint_selected =
                        Some(self.hint_selected.unwrap_or(0).saturating_sub(1).min(max));
                    self.sync_hint_page();
                } else if self.input.starts_with('/') && !self.hints.is_empty() {
                    self.hint_selected = Some(self.hints.len() - 1);
                    self.sync_hint_page();
                } else if !self.history.is_empty() {
                    if self.history_idx.is_none() {
                        self.history_draft = self.input.clone();
                    }
                    let i = self
                        .history_idx
                        .map_or(self.history.len() - 1, |i| i.saturating_sub(1));
                    self.history_idx = Some(i);
                    self.input = self.history[i].clone();
                    self.cursor = self.input.len();
                    self.hints.clear();
                }
            }
            (KeyCode::Down, _) => {
                if self.choice_panel.visible {
                    self.choice_panel.select_next();
                    return;
                }
                if self.session_tui.visible {
                    self.session_tui.select_next();
                    return;
                }
                if self.dashboard.visible {
                    self.dashboard.down();
                    return;
                }
                if self.input.starts_with('/') && self.hint_selected.is_some() {
                    let max = self.hints.len().saturating_sub(1);
                    let next = (self.hint_selected.unwrap_or(0) + 1).min(max);
                    self.hint_selected = Some(next);
                    self.sync_hint_page();
                } else if self.input.starts_with('/') && !self.hints.is_empty() {
                    self.hint_selected = Some(0);
                } else if let Some(i) = self.history_idx {
                    let next = i + 1;
                    if next >= self.history.len() {
                        self.input = self.history_draft.clone();
                        self.history_idx = None;
                    } else {
                        self.input = self.history[next].clone();
                        self.history_idx = Some(next);
                    }
                    self.cursor = self.input.len();
                    self.hints.clear();
                }
            }
            (KeyCode::Enter, KeyModifiers::SHIFT) => {
                self.history_idx = None;
                self.input.insert(self.cursor, '\n');
                self.cursor += 1;
                self.update_hints();
            }
            (KeyCode::Enter, _) => {
                if self.session_tui.visible {
                    self.handle_session_tui_enter();
                    return;
                }
                if self.choice_panel.visible {
                    let response = self.choice_panel.get_response();
                    let id = self.choice_panel.id.clone();
                    self.choice_panel.close();
                    let _ = self
                        .cmd_tx
                        .blocking_send(crate::tui::BackendCmd::ChoiceResponse { id, value: response });
                    return;
                }
                if self.input.trim() == "//" {
                    self.dashboard.toggle();
                    self.input.clear();
                    self.cursor = 0;
                    return;
                }
                if self.confirm.visible {
                    if self.confirm.yes_selected {
                        if self.confirm.message.contains("Exit") {
                            self.should_quit = true;
                        } else if self.confirm.message.contains("Clear") {
                            self.output.clear();
                            self.input.clear();
                            self.cursor = 0;
                            self.hints.clear();
                            self.scroll = 0.0;
                            self.stick_to_bottom = true;
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
                if let Some(idx) = self.hint_selected {
                    if let Some((name, _)) = self.hints.get(idx) {
                        self.input = format!("{} ", name);
                        self.cursor = self.input.len();
                    }
                    self.hint_selected = None;
                    self.update_hints();
                    return;
                }
                let task = self.input.trim().to_string();
                self.handle_command(&task);
            }
            (KeyCode::Char(ch), mods) => {
                if self.session_tui.visible {
                    match self.session_tui.focus {
                        crate::session_tui::SessionFocus::NameEdit => {
                            self.session_tui.name_buffer.push(ch);
                        }
                        crate::session_tui::SessionFocus::DescEdit => {
                            self.session_tui.desc_buffer.push(ch);
                        }
                        _ => {}
                    }
                    return;
                }
                self.history_idx = None;
                if mods.contains(KeyModifiers::CONTROL) {
                    match ch {
                        'w' if self.cursor > 0 => {
                            self.delete_word_before();
                        }
                        'u' => {
                            self.input.drain(..self.cursor);
                            self.cursor = 0;
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
                    self.input.insert(self.cursor, ch);
                    self.cursor += ch.len_utf8();
                }
                self.update_hints();
            }
            (KeyCode::Backspace, _) if self.cursor > 0 => {
                if self.choice_panel.visible {
                    if self.choice_panel.input_cursor > 0 {
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
                    return;
                }
                self.history_idx = None;
                let prev = self.prev_char_boundary(self.cursor);
                self.input.drain(prev..self.cursor);
                self.cursor = prev;
                self.update_hints();
            }
            (KeyCode::Delete, _) if self.cursor < self.input.len() => {
                self.history_idx = None;
                let next = self.next_char_boundary(self.cursor);
                self.input.drain(self.cursor..next);
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
                self.cursor = self.prev_char_boundary(self.cursor);
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
                self.history_idx = None;
                self.cursor = self.next_char_boundary(self.cursor);
            }
            (KeyCode::Home, _) => {
                self.history_idx = None;
                self.cursor = 0;
            }
            (KeyCode::End, _) => {
                if self.input.is_empty() {
                    self.stick_to_bottom = true;
                    self.scroll = 0.0;
                } else {
                    self.history_idx = None;
                    self.cursor = self.input.len();
                }
            }
            (KeyCode::Tab, _) => {
                if self.session_tui.visible {
                    match self.session_tui.focus {
                        crate::session_tui::SessionFocus::List => self.session_tui.focus_right(),
                        crate::session_tui::SessionFocus::Actions => self.session_tui.focus_name(),
                        crate::session_tui::SessionFocus::NameEdit => {
                            self.session_tui.focus_desc()
                        }
                        crate::session_tui::SessionFocus::DescEdit => {
                            self.session_tui.focus_left()
                        }
                        _ => {}
                    }
                    return;
                }
                if self.input.starts_with('/') && self.hint_selected.is_none() {
                    if let Some(completed) = complete_slash(&self.input) {
                        self.input = completed;
                        self.cursor = self.input.len();
                        self.update_hints();
                        return;
                    }
                }
                if self.input.starts_with('/') && !self.hints.is_empty() {
                    self.hint_selected = Some(0);
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
                        crate::session_tui::SessionFocus::DescEdit => {
                            self.session_tui.focus_name()
                        }
                        _ => {}
                    }
                    return;
                }
                if self.hint_selected.is_some() {
                    self.hint_selected = Some(self.hint_selected.unwrap_or(0).saturating_sub(1));
                }
            }
            (KeyCode::Esc, _) => {
                if self.choice_panel.visible {
                    self.choice_panel.close();
                    return;
                }
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
                if self.settings_visible {
                    self.commit_settings();
                    self.settings_visible = false;
                    self.settings_board.visible = false;
                    return;
                }
                if self.thinking {
                    let _ = self.cmd_tx.blocking_send(crate::tui::BackendCmd::Cancel);
                    self.thinking = false;
                    self.thinking_cancelled = true;
                }
                self.show_help_overlay = false;
                self.show_model_picker = false;
                self.provider_picker.close();
                self.hint_selected = None;
                self.hint_page = 0;
                self.help_board.visible = false;
            }
            _ => {}
        }
    }

    fn handle_provider_picker_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.show_model_picker = self.provider_picker.toggle(&self.cmd_tx);
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
                    self.show_model_picker = false;
                }
            }
            _ => {}
        }
    }

    pub(crate) fn prev_char_boundary(&self, pos: usize) -> usize {
        self.input[..pos]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(pos.saturating_sub(1))
    }
    pub(crate) fn next_char_boundary(&self, pos: usize) -> usize {
        self.input[pos..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| pos + i)
            .unwrap_or(self.input.len())
    }
    pub(crate) fn delete_word_before(&mut self) {
        let before = &self.input[..self.cursor];
        let cut = before
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_whitespace())
            .map(|(i, _)| i + 1)
            .unwrap_or(0);
        self.input.drain(cut..self.cursor);
        self.cursor = cut;
    }
    pub(crate) fn update_hints(&mut self) {
        if self.input.starts_with('/') && self.input.len() <= 30 {
            self.hints = matching_hints(&self.input)
                .into_iter()
                .map(|(n, d)| (n.to_string(), d.to_string()))
                .collect();
        } else {
            self.hints.clear();
        }
        self.hint_page = 0;
        self.hint_selected = None;
    }

    pub(crate) fn sync_hint_page(&mut self) {
        if let Some(sel) = self.hint_selected {
            self.hint_page = sel / 8;
        }
    }

    pub(crate) fn handle_settings_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('q'), _) => {
                self.commit_settings();
                self.settings_visible = false;
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
            (KeyCode::Char(ch), mods) => {
                if !mods.contains(KeyModifiers::CONTROL) {
                    self.settings_board.edit_insert(ch);
                }
            }
            _ => {}
        }
    }

    fn handle_session_tui_enter(&mut self,
    ) {
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

    fn dispatch_session_action(&mut self,
        action: SessionAction,
    ) {
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
                        self.thinking_effort = meta.thinking_effort.clone();
                        let _ = self
                            .cmd_tx
                            .blocking_send(BackendCmd::SetMode(self.mode.clone()));
                        let _ = self
                            .cmd_tx
                            .blocking_send(BackendCmd::SetModel(self.model.clone()));
                        let _ = self.cmd_tx.blocking_send(BackendCmd::SetThinkingEffort(
                            self.thinking_effort.clone(),
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
                        self.session_tui.set_message(format!("Session not found: {name}"));
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
                let desc = if desc.is_empty() { None } else { Some(desc.to_string()) };
                let mode: SessionMode = self.mode.clone().into();
                match self.session_pool.save(
                    &name,
                    &self.session_items,
                    &self.model,
                    &self.provider_name,
                    mode,
                    &self.thinking_effort,
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
                    self.session_tui.set_message("No session selected to delete");
                    return;
                }
                if self.session_tui.focus != SessionFocus::ConfirmDelete {
                    self.session_tui.set_message(format!(
                        "Press Enter again to confirm deleting '{name}'"
                    ));
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

    fn refresh_session_tui_list(&mut self,
    ) {
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
