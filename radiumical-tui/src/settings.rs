use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};

#[derive(Debug, Clone)]
pub enum SettingKind {
    String {
        value: String,
        mask: bool,
    },
    U64 {
        value: u64,
        min: u64,
        max: u64,
        step: u64,
    },
    Usize {
        value: usize,
        min: usize,
        max: usize,
        step: usize,
    },
    Enum {
        value: String,
        options: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct SettingItem {
    pub label: String,
    pub kind: SettingKind,
}

#[derive(Debug, Clone)]
pub struct SettingsBoard {
    pub visible: bool,
    pub selected: usize,
    pub items: Vec<SettingItem>,
    pub editing: Option<usize>,
    pub edit_buffer: String,
    pub edit_cursor: usize,
}

impl SettingItem {
    pub fn display_value(&self) -> String {
        match &self.kind {
            SettingKind::String { value, mask } => {
                if *mask && !value.is_empty() {
                    "*".repeat(value.chars().count().min(32))
                } else {
                    value.clone()
                }
            }
            SettingKind::U64 { value, .. } => value.to_string(),
            SettingKind::Usize { value, .. } => value.to_string(),
            SettingKind::Enum { value, .. } => value.clone(),
        }
    }
}

impl SettingsBoard {
    pub fn from_config(
        config: &radiumical_core::config::Config,
        mode: &radiumical_core::types::AgentMode,
    ) -> Self {
        let provider = config.provider.clone().unwrap_or_else(|| "deepseek".into());
        let model = config
            .model
            .clone()
            .unwrap_or_else(|| "deepseek-v4-pro".into());
        let api_key = config.api_key.clone().unwrap_or_default();
        let api_base = config.api_base.clone().unwrap_or_default();
        let heartbeat_secs = config.heartbeat_secs.unwrap_or(10);
        let llm_timeout_secs = config.llm_timeout_secs.unwrap_or(120);
        let max_iterations = config.max_iterations.unwrap_or(32);
        let reasoning_effort = config
            .reasoning_effort
            .clone()
            .unwrap_or_else(|| "max".into());
        let max_context_tokens = config.max_context_tokens.unwrap_or(128000);
        let context_compress_ratio = config
            .context_compress_ratio
            .map(|v| format!("{v:.2}"))
            .unwrap_or_else(|| "0.75".into());
        let mode_str = match mode {
            radiumical_core::types::AgentMode::Auto => "auto",
            radiumical_core::types::AgentMode::Plan => "plan",
            radiumical_core::types::AgentMode::Exec => "exec",
        }
        .to_string();
        Self {
            visible: false,
            selected: 0,
            editing: None,
            edit_buffer: String::new(),
            edit_cursor: 0,
            items: vec![
                SettingItem {
                    label: "Provider".into(),
                    kind: SettingKind::Enum {
                        value: provider,
                        options: vec![
                            "openai".into(),
                            "deepseek".into(),
                            "anthropic".into(),
                            "ollama".into(),
                        ],
                    },
                },
                SettingItem {
                    label: "Model".into(),
                    kind: SettingKind::String {
                        value: model,
                        mask: false,
                    },
                },
                SettingItem {
                    label: "API key".into(),
                    kind: SettingKind::String {
                        value: api_key,
                        mask: true,
                    },
                },
                SettingItem {
                    label: "API base".into(),
                    kind: SettingKind::String {
                        value: api_base,
                        mask: false,
                    },
                },
                SettingItem {
                    label: "Mode".into(),
                    kind: SettingKind::Enum {
                        value: mode_str,
                        options: vec!["auto".into(), "plan".into(), "exec".into()],
                    },
                },
                SettingItem {
                    label: "Max iterations".into(),
                    kind: SettingKind::Usize {
                        value: max_iterations,
                        min: 1,
                        max: 999,
                        step: 1,
                    },
                },
                SettingItem {
                    label: "LLM timeout".into(),
                    kind: SettingKind::U64 {
                        value: llm_timeout_secs,
                        min: 1,
                        max: 3600,
                        step: 10,
                    },
                },
                SettingItem {
                    label: "Heartbeat".into(),
                    kind: SettingKind::U64 {
                        value: heartbeat_secs,
                        min: 0,
                        max: 300,
                        step: 5,
                    },
                },
                SettingItem {
                    label: "Reasoning effort".into(),
                    kind: SettingKind::String {
                        value: reasoning_effort,
                        mask: false,
                    },
                },
                SettingItem {
                    label: "Max context tokens".into(),
                    kind: SettingKind::String {
                        value: max_context_tokens.to_string(),
                        mask: false,
                    },
                },
                SettingItem {
                    label: "Context compress ratio".into(),
                    kind: SettingKind::String {
                        value: context_compress_ratio,
                        mask: false,
                    },
                },
            ],
        }
    }

    pub fn to_config(&self) -> radiumical_core::config::Config {
        let mut config = radiumical_core::config::Config {
            model: None,
            provider: None,
            api_key: None,
            api_base: None,
            heartbeat_secs: None,
            llm_timeout_secs: None,
            max_iterations: None,
            reasoning_effort: None,
            mode: None,
            max_context_tokens: None,
            context_compress_ratio: None,
        };
        for item in &self.items {
            match item.label.as_str() {
                "Provider" => {
                    if let SettingKind::Enum { value, .. } = &item.kind {
                        config.provider = Some(value.clone());
                    }
                }
                "Model" => {
                    if let SettingKind::String { value, .. } = &item.kind {
                        config.model = Some(value.clone());
                    }
                }
                "API key" => {
                    if let SettingKind::String { value, .. } = &item.kind {
                        config.api_key = Some(value.clone());
                    }
                }
                "API base" => {
                    if let SettingKind::String { value, .. } = &item.kind {
                        config.api_base = Some(value.clone());
                    }
                }
                "Mode" => {
                    if let SettingKind::Enum { value, .. } = &item.kind {
                        config.mode = Some(value.clone());
                    }
                }
                "Max iterations" => {
                    if let SettingKind::Usize { value, .. } = &item.kind {
                        config.max_iterations = Some(*value);
                    }
                }
                "LLM timeout" => {
                    if let SettingKind::U64 { value, .. } = &item.kind {
                        config.llm_timeout_secs = Some(*value);
                    }
                }
                "Heartbeat" => {
                    if let SettingKind::U64 { value, .. } = &item.kind {
                        config.heartbeat_secs = Some(*value);
                    }
                }
                "Reasoning effort" => {
                    if let SettingKind::String { value, .. } = &item.kind {
                        config.reasoning_effort = Some(value.clone());
                    }
                }
                "Max context tokens" => {
                    if let SettingKind::String { value, .. } = &item.kind {
                        if let Ok(v) = value.parse::<usize>() {
                            config.max_context_tokens = Some(v.max(10000));
                        }
                    }
                }
                "Context compress ratio" => {
                    if let SettingKind::String { value, .. } = &item.kind {
                        if let Ok(v) = value.parse::<f64>() {
                            config.context_compress_ratio = Some(v.clamp(0.5, 0.95));
                        }
                    }
                }
                _ => {}
            }
        }
        config
    }

    pub fn apply_to_app(&self, app: &mut crate::tui::app::App) {
        for item in &self.items {
            match item.label.as_str() {
                "Provider" => {
                    if let SettingKind::Enum { value, .. } = &item.kind {
                        app.provider_name = value.clone();
                    }
                }
                "Model" => {
                    if let SettingKind::String { value, .. } = &item.kind {
                        app.model = value.clone();
                    }
                }
                "Mode" => {
                    if let SettingKind::Enum { value, .. } = &item.kind {
                        app.mode = match value.as_str() {
                            "plan" => radiumical_core::types::AgentMode::Plan,
                            "exec" => radiumical_core::types::AgentMode::Exec,
                            _ => radiumical_core::types::AgentMode::Auto,
                        };
                    }
                }
                "Reasoning effort" => {
                    if let SettingKind::String { value, .. } = &item.kind {
                        app.thinking_effort = value.clone();
                    }
                }
                _ => {}
            }
        }
    }

    pub fn select_next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1) % self.items.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + self.items.len() - 1) % self.items.len();
        }
    }

    pub fn adjust(&mut self, delta: i32) {
        let Some(item) = self.items.get_mut(self.selected) else {
            return;
        };
        match &mut item.kind {
            SettingKind::U64 {
                value,
                min,
                max,
                step,
            } => {
                let new_val = if delta < 0 {
                    value.saturating_sub(*step)
                } else {
                    value.saturating_add(*step)
                };
                *value = new_val.clamp(*min, *max);
            }
            SettingKind::Usize {
                value,
                min,
                max,
                step,
            } => {
                let new_val = if delta < 0 {
                    value.saturating_sub(*step)
                } else {
                    value.saturating_add(*step)
                };
                *value = new_val.clamp(*min, *max);
            }
            SettingKind::Enum { value, options } => {
                if options.len() > 1 {
                    let idx = options.iter().position(|o| o == value).unwrap_or(0);
                    let new_idx = if delta < 0 {
                        (idx + options.len() - 1) % options.len()
                    } else {
                        (idx + 1) % options.len()
                    };
                    *value = options[new_idx].clone();
                }
            }
            SettingKind::String { .. } => {}
        }
    }

    pub fn can_edit(&self) -> bool {
        let Some(item) = self.items.get(self.selected) else {
            return false;
        };
        matches!(item.kind, SettingKind::String { .. })
    }

    pub fn begin_edit(&mut self) {
        if !self.can_edit() {
            return;
        }
        self.editing = Some(self.selected);
        if let Some(item) = self.items.get(self.selected) {
            self.edit_buffer = item.display_value();
            self.edit_cursor = self.edit_buffer.len();
        } else {
            self.edit_buffer.clear();
            self.edit_cursor = 0;
        }
    }

    pub fn commit_edit(&mut self) {
        let Some(idx) = self.editing else { return };
        if let Some(item) = self.items.get_mut(idx) {
            if let SettingKind::String { value, .. } = &mut item.kind {
                *value = self.edit_buffer.clone();
            }
        }
        self.editing = None;
        self.edit_buffer.clear();
        self.edit_cursor = 0;
    }

    pub fn cancel_edit(&mut self) {
        self.editing = None;
        self.edit_buffer.clear();
        self.edit_cursor = 0;
    }

    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    pub fn edit_insert(&mut self, ch: char) {
        self.edit_buffer.insert(self.edit_cursor, ch);
        self.edit_cursor += ch.len_utf8();
    }

    pub fn edit_backspace(&mut self) {
        if self.edit_cursor > 0 {
            let prev = self.prev_char_boundary(self.edit_cursor);
            self.edit_buffer.drain(prev..self.edit_cursor);
            self.edit_cursor = prev;
        }
    }

    pub fn edit_delete(&mut self) {
        if self.edit_cursor < self.edit_buffer.len() {
            let next = self.next_char_boundary(self.edit_cursor);
            self.edit_buffer.drain(self.edit_cursor..next);
        }
    }

    pub fn edit_left(&mut self) {
        if self.edit_cursor > 0 {
            self.edit_cursor = self.prev_char_boundary(self.edit_cursor);
        }
    }

    pub fn edit_right(&mut self) {
        if self.edit_cursor < self.edit_buffer.len() {
            self.edit_cursor = self.next_char_boundary(self.edit_cursor);
        }
    }

    fn prev_char_boundary(&self, pos: usize) -> usize {
        self.edit_buffer[..pos]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(pos.saturating_sub(1))
    }

    fn next_char_boundary(&self, pos: usize) -> usize {
        self.edit_buffer[pos..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| pos + i)
            .unwrap_or(self.edit_buffer.len())
    }

    pub fn save(&self) {
        let config = self.to_config();
        let _ = config.save();
    }

    #[allow(dead_code)]
    pub fn render(&self, f: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }
        let h = (self.items.len() as u16 + 4)
            .min(area.height.saturating_sub(4))
            .max(6);
        let label_w = self
            .items
            .iter()
            .map(|i| i.label.chars().count())
            .max()
            .unwrap_or(12) as u16;
        let value_w = self
            .items
            .iter()
            .map(|i| i.display_value().chars().count())
            .max()
            .unwrap_or(20) as u16;
        let w = (label_w + value_w + 12)
            .min(area.width.saturating_sub(4))
            .max(40);
        let x = (area.width.saturating_sub(w)) / 2;
        let y = (area.height.saturating_sub(h)) / 2;
        let r = Rect {
            x: area.x + x,
            y: area.y + y,
            width: w,
            height: h,
        };
        f.render_widget(Clear, r);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Settings ")
            .border_style(Style::default().fg(Color::Rgb(100, 160, 220)));
        let inner = r.inner(ratatui::layout::Margin {
            horizontal: 1,
            vertical: 1,
        });
        let max_label = label_w as usize;
        let max_value = (inner.width.saturating_sub(max_label as u16 + 6)) as usize;
        let lines: Vec<Line> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let selected = i == self.selected;
                let mut spans = Vec::new();
                let marker = if selected { "> " } else { "  " };
                spans.push(Span::styled(
                    marker,
                    Style::default().fg(Color::Rgb(100, 160, 220)),
                ));
                let label_style = if selected {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(180, 180, 190))
                };
                spans.push(Span::styled(
                    format!("{:<width$}", item.label, width = max_label),
                    label_style,
                ));
                spans.push(Span::raw("  "));
                let is_editing = self.editing == Some(i);
                let value = if is_editing {
                    self.edit_buffer.clone()
                } else {
                    item.display_value()
                };
                let truncated = if value.chars().count() > max_value {
                    value
                        .chars()
                        .take(max_value.saturating_sub(1))
                        .collect::<String>()
                        + "…"
                } else {
                    value
                };
                let value_style = if is_editing {
                    Style::default().bg(Color::Rgb(40, 60, 80)).fg(Color::White)
                } else if selected {
                    Style::default().bg(Color::Rgb(45, 45, 55)).fg(Color::White)
                } else {
                    Style::default().fg(Color::Rgb(210, 210, 210))
                };
                spans.push(Span::styled(truncated, value_style));
                Line::from(spans)
            })
            .collect();
        let help = if self.is_editing() {
            "Enter: save  Esc: cancel"
        } else {
            "↑↓ navigate  ←→ adjust  Enter: edit string  Esc: close"
        };
        let mut text = Text::from(lines);
        text.lines.push(Line::from(""));
        text.lines.push(Line::from(Span::styled(
            help,
            Style::default().fg(Color::Rgb(120, 120, 130)),
        )));
        f.render_widget(
            Paragraph::new(text)
                .block(block)
                .wrap(Wrap { trim: false })
                .alignment(Alignment::Left),
            r,
        );
    }

    /// Render at a specific rect (from PanelManager layout).
    pub fn render_at(&self, f: &mut Frame, r: Rect) {
        use ratatui::widgets::Clear;
        if !self.visible {
            return;
        }
        f.render_widget(Clear, r);
        let label_w = self
            .items
            .iter()
            .map(|i| i.label.chars().count())
            .max()
            .unwrap_or(12) as u16;
        let max_value = (r.width.saturating_sub(label_w + 8) as u16).max(10) as usize;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Settings ")
            .border_style(Style::default().fg(Color::Rgb(100, 160, 220)));
        let _inner = r.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 });
        let max_label = label_w as usize;
        let lines: Vec<Line> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let selected = i == self.selected;
                let mut spans = Vec::new();
                let marker = if selected { "> " } else { "  " };
                spans.push(Span::styled(marker, Style::default().fg(Color::Rgb(100, 160, 220))));
                let label_style = if selected {
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(180, 180, 190))
                };
                spans.push(Span::styled(format!("{:<width$}", item.label, width = max_label), label_style));
                spans.push(Span::raw("  "));
                let is_editing = self.editing == Some(i);
                let value = if is_editing { self.edit_buffer.clone() } else { item.display_value() };
                let truncated = if value.chars().count() > max_value {
                    value.chars().take(max_value.saturating_sub(1)).collect::<String>() + "…"
                } else {
                    value
                };
                let value_style = if is_editing {
                    Style::default().bg(Color::Rgb(40, 60, 80)).fg(Color::White)
                } else if selected {
                    Style::default().bg(Color::Rgb(45, 45, 55)).fg(Color::White)
                } else {
                    Style::default().fg(Color::Rgb(210, 210, 210))
                };
                spans.push(Span::styled(truncated, value_style));
                Line::from(spans)
            })
            .collect();
        let help = if self.is_editing() { "Enter: save  Esc: cancel" } else { "↑↓ ←→ Enter:edit Esc:close" };
        let mut text = Text::from(lines);
        text.lines.push(Line::from(""));
        text.lines.push(Line::from(Span::styled(help, Style::default().fg(Color::Rgb(120, 120, 130)))));
        f.render_widget(Paragraph::new(text).block(block).wrap(Wrap { trim: false }), r);
    }
}
