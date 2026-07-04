//! Session TUI — full-screen session manager overlay.
//!
//! Provides a browsable list of saved sessions with details and actions:
//! load, save (with name/description), delete, and new.
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use radiumical_core::session::SessionMeta;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionFocus {
    List,
    Actions,
    NameEdit,
    DescEdit,
    ConfirmDelete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    Load,
    Save,
    Delete,
    New,
}

const ACTIONS: &[SessionAction] = &[
    SessionAction::Load,
    SessionAction::Save,
    SessionAction::Delete,
    SessionAction::New,
];

pub struct SessionTui {
    pub visible: bool,
    pub focus: SessionFocus,
    pub sessions: Vec<SessionMeta>,
    pub selected: usize,
    pub action_selected: usize,
    pub name_buffer: String,
    pub desc_buffer: String,
    pub message: Option<String>,
}

impl Default for SessionTui {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionTui {
    pub fn new() -> Self {
        Self {
            visible: false,
            focus: SessionFocus::List,
            sessions: Vec::new(),
            selected: 0,
            action_selected: 0,
            name_buffer: String::new(),
            desc_buffer: String::new(),
            message: None,
        }
    }

    pub fn open(
        &mut self,
        sessions: Vec<SessionMeta>,
        current_name: Option<&str>,
        current_desc: Option<&str>,
    ) {
        self.visible = true;
        self.focus = SessionFocus::List;
        self.sessions = sessions;
        self.selected = 0;
        self.action_selected = 0;
        self.name_buffer = current_name.unwrap_or("").to_string();
        self.desc_buffer = current_desc.unwrap_or("").to_string();
        self.message = None;
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    pub fn set_message(&mut self, msg: impl Into<String>) {
        self.message = Some(msg.into());
    }

    pub fn clear_message(&mut self) {
        self.message = None;
    }

    pub fn selected_session(&self) -> Option<&SessionMeta> {
        self.sessions.get(self.selected)
    }

    pub fn selected_action(&self) -> SessionAction {
        ACTIONS
            .get(self.action_selected)
            .copied()
            .unwrap_or(SessionAction::Load)
    }

    pub fn select_prev(&mut self) {
        match self.focus {
            SessionFocus::List if !self.sessions.is_empty() => {
                self.selected = (self.selected + self.sessions.len() - 1) % self.sessions.len();
                self.sync_name_desc_from_selection();
            }
            SessionFocus::Actions => {
                self.action_selected = (self.action_selected + ACTIONS.len() - 1) % ACTIONS.len();
            }
            _ => {}
        }
    }

    pub fn select_next(&mut self) {
        match self.focus {
            SessionFocus::List if !self.sessions.is_empty() => {
                self.selected = (self.selected + 1) % self.sessions.len();
                self.sync_name_desc_from_selection();
            }
            SessionFocus::Actions => {
                self.action_selected = (self.action_selected + 1) % ACTIONS.len();
            }
            _ => {}
        }
    }

    pub fn focus_left(&mut self) {
        self.focus = match self.focus {
            SessionFocus::Actions | SessionFocus::NameEdit | SessionFocus::DescEdit => {
                SessionFocus::List
            }
            other => other,
        };
    }

    pub fn focus_right(&mut self) {
        self.focus = match self.focus {
            SessionFocus::List | SessionFocus::NameEdit | SessionFocus::DescEdit => {
                SessionFocus::Actions
            }
            other => other,
        };
    }

    pub fn focus_name(&mut self) {
        self.focus = SessionFocus::NameEdit;
    }

    pub fn focus_desc(&mut self) {
        self.focus = SessionFocus::DescEdit;
    }

    #[allow(dead_code)]
    pub fn start_delete(&mut self) {
        if !self.sessions.is_empty() {
            self.focus = SessionFocus::ConfirmDelete;
        }
    }

    pub fn sync_name_desc_from_selection(&mut self) {
        let (name, desc) = if let Some(meta) = self.selected_session() {
            (meta.name.clone(), meta.description.clone())
        } else {
            (String::new(), String::new())
        };
        self.name_buffer = name;
        self.desc_buffer = desc;
    }

    pub fn render(
        &self,
        f: &mut Frame,
        area: Rect,
        current_model: &str,
        current_mode: radiumical_core::types::AgentMode,
    ) {
        if !self.visible {
            return;
        }

        let w = (area.width as f32 * 0.75) as u16;
        let h = (area.height as f32 * 0.75) as u16;
        let x = (area.width.saturating_sub(w)) / 2;
        let y = (area.height.saturating_sub(h)) / 2;
        let r = Rect {
            x: area.x + x,
            y: area.y + y,
            width: w.max(40),
            height: h.max(18),
        };

        f.render_widget(Clear, r);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Session Manager ")
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Rgb(20, 20, 25)));
        let inner = block.inner(r);
        f.render_widget(block, r);

        let list_width = (inner.width as f32 * 0.42) as u16;
        let right_width = inner.width.saturating_sub(list_width).saturating_sub(1);
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(list_width),
                Constraint::Length(right_width),
            ])
            .split(inner);

        // Left: session list
        let list_lines: Vec<Line> = if self.sessions.is_empty() {
            vec![Line::from(Span::styled(
                "  No saved sessions",
                Style::default().fg(Color::Rgb(120, 120, 130)),
            ))]
        } else {
            self.sessions
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let selected = self.focus == SessionFocus::List && i == self.selected;
                    let prefix = if selected { "▸ " } else { "  " };
                    let style = if selected {
                        Style::default()
                            .bg(Color::Rgb(50, 50, 60))
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(200, 200, 210))
                    };
                    let name = if s.name.len() > (list_width as usize).saturating_sub(6) {
                        format!(
                            "{}…",
                            &s.name[..s
                                .name
                                .char_indices()
                                .nth(list_width as usize - 7)
                                .map(|(i, _)| i)
                                .unwrap_or(s.name.len())]
                        )
                    } else {
                        s.name.clone()
                    };
                    Line::from(Span::styled(format!("{}{}", prefix, name), style))
                })
                .collect()
        };
        let list_block = Block::default()
            .borders(Borders::RIGHT)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" Sessions ");
        f.render_widget(
            Paragraph::new(Text::from(list_lines))
                .block(list_block)
                .wrap(Wrap { trim: false }),
            chunks[0],
        );

        // Right: details + actions
        let detail_lines = if let Some(s) = self.selected_session() {
            let mode: radiumical_core::types::AgentMode = s.mode.into();
            vec![
                Line::from(vec![
                    Span::styled("Name: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(&s.name),
                ]),
                Line::from(vec![
                    Span::styled("Model: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(&s.model),
                ]),
                Line::from(vec![
                    Span::styled("Provider: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(&s.provider),
                ]),
                Line::from(vec![
                    Span::styled("Mode: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(format!("{:?}", mode)),
                ]),
                Line::from(vec![
                    Span::styled("Effort: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(&s.thinking_effort),
                ]),
                Line::from(vec![
                    Span::styled("Created: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(&s.created),
                ]),
                Line::from(vec![
                    Span::styled("Updated: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(&s.updated),
                ]),
                Line::from(vec![
                    Span::styled("Messages: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(s.message_count.to_string()),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("Description: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(if s.description.is_empty() {
                        "(none)"
                    } else {
                        &s.description
                    }),
                ]),
            ]
        } else {
            vec![Line::from(Span::styled(
                "  Select or create a session",
                Style::default().fg(Color::Rgb(120, 120, 130)),
            ))]
        };

        let detail_block = Block::default().borders(Borders::NONE).title(" Details ");

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(detail_lines.len() as u16 + 1),
                Constraint::Min(6),
            ])
            .split(chunks[1]);

        f.render_widget(
            Paragraph::new(Text::from(detail_lines))
                .block(detail_block)
                .wrap(Wrap { trim: false }),
            right_chunks[0],
        );

        // Action buttons + name/desc fields
        let action_lines = self.render_action_lines();
        f.render_widget(
            Paragraph::new(Text::from(action_lines)).wrap(Wrap { trim: false }),
            right_chunks[1],
        );

        // Message bar at bottom of panel
        if let Some(msg) = &self.message {
            let msg_h = 3u16.min(r.height);
            let msg_r = Rect {
                x: r.x,
                y: r.y + r.height - msg_h,
                width: r.width,
                height: msg_h,
            };
            let msg_block = Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::Yellow));
            f.render_widget(
                Paragraph::new(Span::styled(
                    format!("  {}", msg),
                    Style::default().fg(Color::Yellow),
                ))
                .block(msg_block),
                msg_r,
            );
        }

        // Current session hint below title
        let hint = format!("  current: {} @ {:?}", current_model, current_mode);
        let hint_r = Rect {
            x: r.x + 1,
            y: r.y + 1,
            width: r.width - 2,
            height: 1,
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                hint,
                Style::default().fg(Color::Rgb(120, 120, 130)),
            )),
            hint_r,
        );
    }

    fn render_action_lines(&self) -> Vec<Line<'_>> {
        let mut lines = Vec::new();
        lines.push(Line::from(Span::styled(
            "Actions",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        for (i, action) in ACTIONS.iter().enumerate() {
            let selected = self.focus == SessionFocus::Actions && i == self.action_selected;
            let label = action.label();
            let style = if selected {
                Style::default()
                    .bg(Color::Rgb(60, 60, 70))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(180, 180, 190))
            };
            lines.push(Line::from(Span::styled(format!("  {}", label), style)));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Session name",
            Style::default().fg(Color::DarkGray),
        )));
        let name_selected = self.focus == SessionFocus::NameEdit;
        let name_style = if name_selected {
            Style::default().bg(Color::Rgb(40, 40, 50)).fg(Color::White)
        } else {
            Style::default().fg(Color::Rgb(200, 200, 210))
        };
        let name_cursor = if name_selected { "▏" } else { "" };
        lines.push(Line::from(Span::styled(
            format!("  {}{}", self.name_buffer, name_cursor),
            name_style,
        )));

        lines.push(Line::from(Span::styled(
            "Description",
            Style::default().fg(Color::DarkGray),
        )));
        let desc_selected = self.focus == SessionFocus::DescEdit;
        let desc_style = if desc_selected {
            Style::default().bg(Color::Rgb(40, 40, 50)).fg(Color::White)
        } else {
            Style::default().fg(Color::Rgb(200, 200, 210))
        };
        let desc_cursor = if desc_selected { "▏" } else { "" };
        lines.push(Line::from(Span::styled(
            format!("  {}{}", self.desc_buffer, desc_cursor),
            desc_style,
        )));

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Enter: confirm | Tab: move | Esc: close",
            Style::default().fg(Color::Rgb(120, 120, 130)),
        )));

        lines
    }
}

impl SessionAction {
    pub fn label(&self) -> &'static str {
        match self {
            SessionAction::Load => "[ Load  ]",
            SessionAction::Save => "[ Save  ]",
            SessionAction::Delete => "[ Delete]",
            SessionAction::New => "[ New   ]",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radiumical_core::session::SessionMode;

    fn sample_meta(name: &str) -> SessionMeta {
        SessionMeta {
            name: name.into(),
            created: "2026-06-29 10:00".into(),
            updated: "2026-06-29 11:00".into(),
            model: "m".into(),
            provider: "p".into(),
            mode: SessionMode::Auto,
            thinking_effort: "max".into(),
            description: "desc".into(),
            message_count: 5,
        }
    }

    #[test]
    fn test_session_tui_nav() {
        let mut tui = SessionTui::new();
        tui.open(vec![sample_meta("a"), sample_meta("b")], None, None);
        assert_eq!(tui.selected, 0);
        tui.select_next();
        assert_eq!(tui.selected, 1);
        tui.select_prev();
        assert_eq!(tui.selected, 0);
    }

    #[test]
    fn test_session_tui_focus() {
        let mut tui = SessionTui::new();
        tui.open(vec![sample_meta("a")], None, None);
        assert_eq!(tui.focus, SessionFocus::List);
        tui.focus_right();
        assert_eq!(tui.focus, SessionFocus::Actions);
        tui.focus_left();
        assert_eq!(tui.focus, SessionFocus::List);
    }

    #[test]
    fn test_session_tui_action_selection() {
        let mut tui = SessionTui::new();
        tui.open(vec![sample_meta("a")], None, None);
        assert_eq!(tui.selected_action(), SessionAction::Load);
        tui.focus_right();
        tui.select_next();
        assert_eq!(tui.selected_action(), SessionAction::Save);
    }
}
