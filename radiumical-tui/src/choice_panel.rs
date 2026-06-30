//! Choice panel — interactive selection UI for the choice tool.
//!
//! Supports single, multi, and input modes with arrow key navigation.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceMode {
    Single,
    Multi,
    Input,
}

impl ChoiceMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "multi" => ChoiceMode::Multi,
            "input" => ChoiceMode::Input,
            _ => ChoiceMode::Single,
        }
    }
}

pub struct ChoicePanel {
    pub visible: bool,
    pub id: String,
    pub mode: ChoiceMode,
    pub options: Vec<String>,
    pub selected: usize,
    pub checked: Vec<bool>,  // for multi mode
    pub input_buffer: String,  // for input mode
    pub input_cursor: usize,
}

impl ChoicePanel {
    pub fn new() -> Self {
        Self {
            visible: false,
            id: String::new(),
            mode: ChoiceMode::Single,
            options: Vec::new(),
            selected: 0,
            checked: Vec::new(),
            input_buffer: String::new(),
            input_cursor: 0,
        }
    }

    pub fn open(&mut self, id: String, mode: &str, options: Vec<String>) {
        self.visible = true;
        self.id = id;
        self.mode = ChoiceMode::from_str(mode);
        self.options = options;
        self.selected = 0;
        self.checked = vec![false; self.options.len()];
        self.input_buffer.clear();
        self.input_cursor = 0;
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.options.clear();
        self.input_buffer.clear();
    }

    pub fn select_prev(&mut self) {
        if !self.options.is_empty() {
            self.selected = (self.selected + self.options.len() - 1) % self.options.len();
        }
    }

    pub fn select_next(&mut self) {
        if !self.options.is_empty() {
            self.selected = (self.selected + 1) % self.options.len();
        }
    }

    #[allow(dead_code)]
    pub fn toggle_current(&mut self) {
        if self.mode == ChoiceMode::Multi && self.selected < self.checked.len() {
            self.checked[self.selected] = !self.checked[self.selected];
        }
    }

    /// Get the final response value.
    pub fn get_response(&self) -> String {
        match self.mode {
            ChoiceMode::Single => {
                (self.selected + 1).to_string()
            }
            ChoiceMode::Multi => {
                let selected: Vec<String> = self.checked
                    .iter()
                    .enumerate()
                    .filter(|(_, &c)| c)
                    .map(|(i, _)| (i + 1).to_string())
                    .collect();
                if selected.is_empty() {
                    (self.selected + 1).to_string()
                } else {
                    selected.join(",")
                }
            }
            ChoiceMode::Input => {
                self.input_buffer.clone()
            }
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        let n = self.options.len().max(1) as u16;
        let prompt_h: u16 = match self.mode {
            ChoiceMode::Input => 5,
            _ => 3,
        };
        let h = (n + prompt_h + 2).min(area.height.saturating_sub(4)).max(6);
        let w = self.options
            .iter()
            .map(|o| o.len())
            .max()
            .unwrap_or(20)
            .saturating_add(10)
            .min(area.width.saturating_sub(4) as usize)
            .max(30) as u16;
        let x = area.x + (area.width.saturating_sub(w)) / 2;
        let y = area.y + (area.height.saturating_sub(h)) / 2;
        let r = Rect { x, y, width: w, height: h };

        f.render_widget(Clear, r);

        let title = match self.mode {
            ChoiceMode::Single => " Choice ",
            ChoiceMode::Multi => " Choice (multi) ",
            ChoiceMode::Input => " Input ",
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(title)
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Rgb(20, 20, 25)));
        let inner = block.inner(r);
        f.render_widget(block, r);

        let mut lines: Vec<Line> = Vec::new();

        match self.mode {
            ChoiceMode::Single => {
                for (i, opt) in self.options.iter().enumerate() {
                    let selected = i == self.selected;
                    let prefix = if selected { "▸ " } else { "  " };
                    let style = if selected {
                        Style::default()
                            .bg(Color::Rgb(50, 50, 60))
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(180, 180, 190))
                    };
                    lines.push(Line::from(Span::styled(
                        format!("{}{}. {}", prefix, i + 1, opt),
                        style,
                    )));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  ↑↓ navigate  Enter: select",
                    Style::default().fg(Color::Rgb(100, 100, 110)),
                )));
            }
            ChoiceMode::Multi => {
                for (i, opt) in self.options.iter().enumerate() {
                    let selected = i == self.selected;
                    let checked = self.checked.get(i).copied().unwrap_or(false);
                    let prefix = if selected { "▸ " } else { "  " };
                    let mark = if checked { "☑" } else { "☐" };
                    let style = if selected {
                        Style::default()
                            .bg(Color::Rgb(50, 50, 60))
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(180, 180, 190))
                    };
                    lines.push(Line::from(Span::styled(
                        format!("{}{} {}. {}", prefix, mark, i + 1, opt),
                        style,
                    )));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  ↑↓ navigate  Space: toggle  Enter: confirm",
                    Style::default().fg(Color::Rgb(100, 100, 110)),
                )));
            }
            ChoiceMode::Input => {
                if let Some(prompt) = self.options.first() {
                    lines.push(Line::from(Span::styled(
                        format!("  {prompt}"),
                        Style::default().fg(Color::Rgb(180, 180, 190)),
                    )));
                }
                lines.push(Line::from(""));
                let cursor_ch = if self.input_cursor < self.input_buffer.len() {
                    self.input_buffer[self.input_cursor..].chars().next().unwrap_or(' ')
                } else {
                    ' '
                };
                let before = &self.input_buffer[..self.input_cursor.min(self.input_buffer.len())];
                let after = if self.input_cursor < self.input_buffer.len() {
                    &self.input_buffer[self.input_cursor + cursor_ch.len_utf8()..]
                } else {
                    ""
                };
                lines.push(Line::from(vec![
                    Span::raw("  > "),
                    Span::raw(before.to_string()),
                    Span::styled(
                        cursor_ch.to_string(),
                        Style::default().bg(Color::White).fg(Color::Black),
                    ),
                    Span::raw(after.to_string()),
                ]));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Type answer  Enter: submit  Esc: cancel",
                    Style::default().fg(Color::Rgb(100, 100, 110)),
                )));
            }
        }

        f.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }),
            inner,
        );
    }
}
