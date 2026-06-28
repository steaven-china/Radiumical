//! Board — unified visual panel widget. Consistent borders, padding, colors.
//! Use for: output area, help overlay, toasts, welcome screen, etc.
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use radiumical_core::providers::ProviderSource;

// ── Board stack: auto-positions boards so they don't overlap ──

#[derive(Default)]
pub struct BoardStack {
    boards: Vec<(Corner, u16, u16, u16)>, // (corner, x_offset, y_offset, height)
}

impl BoardStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserve space at a corner. Returns the y-offset for this board.
    pub fn push(&mut self, corner: Corner, w: u16, h: u16, area: Rect) -> Rect {
        let w = w.min(area.width.saturating_sub(2));
        let h = h.min(area.height.saturating_sub(2));
        let total_h: u16 = self
            .boards
            .iter()
            .filter(|(c, _, _, _)| *c == corner)
            .map(|(_, _, _, bh)| bh + 1) // +1 gap
            .sum();
        let (x, y) = match corner {
            Corner::TopLeft => (0, total_h),
            Corner::TopRight => (area.width.saturating_sub(w), total_h),
            Corner::BottomLeft => (0, area.height.saturating_sub(h + total_h)),
            Corner::BottomRight => (
                area.width.saturating_sub(w),
                area.height.saturating_sub(h + total_h),
            ),
        };
        self.boards.push((corner, 0, total_h, h));
        Rect {
            x: area.x + x,
            y: area.y + y,
            width: w,
            height: h,
        }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.boards.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

// ── Persistent board state with mouse-drag resize ──

#[derive(Debug, Clone)]
pub struct BoardState {
    pub w: u16,
    pub h: u16,
    pub visible: bool,
    pub corner: Corner,
    pub title: String,
    pub border_fg: Color,
    pub show_border: bool,
    pub dragging: bool,
    drag_start: Option<(u16, u16, u16, u16)>,
}

impl BoardState {
    pub fn new(title: &str, w: u16, h: u16, corner: Corner) -> Self {
        Self {
            w,
            h,
            visible: true,
            corner,
            title: title.into(),
            border_fg: Color::Rgb(80, 80, 90),
            show_border: true,
            dragging: false,
            drag_start: None,
        }
    }

    /// Get the overlay rect for this board within an area.
    pub fn rect(&self, area: Rect) -> Rect {
        let w = self.w.min(area.width.saturating_sub(2));
        let h = self.h.min(area.height.saturating_sub(2));
        let (x, y) = match self.corner {
            Corner::TopLeft => (0, 0),
            Corner::TopRight => (area.width.saturating_sub(w), 0),
            Corner::BottomLeft => (0, area.height.saturating_sub(h)),
            Corner::BottomRight => (area.width.saturating_sub(w), area.height.saturating_sub(h)),
        };
        Rect {
            x: area.x + x,
            y: area.y + y,
            width: w,
            height: h,
        }
    }

    /// Check if a mouse position is on the border (within 1 cell of edge).
    pub fn hit_border(&self, mouse_x: u16, mouse_y: u16, area: Rect) -> bool {
        let r = self.rect(area);
        mouse_x >= r.x
            && mouse_x < r.x + r.width
            && mouse_y >= r.y
            && mouse_y < r.y + r.height
            && (mouse_x == r.x
                || mouse_x == r.x + r.width - 1
                || mouse_y == r.y
                || mouse_y == r.y + r.height - 1)
    }

    /// Start dragging — save origin dimensions.
    pub fn start_drag(&mut self, mouse_x: u16, mouse_y: u16) {
        self.dragging = true;
        self.drag_start = Some((mouse_x, mouse_y, self.w, self.h));
    }

    /// Update size during drag.
    #[allow(dead_code)]
    pub fn drag_to(&mut self, mouse_x: u16, mouse_y: u16, area: Rect) {
        const MIN_W: u16 = 10;
        const MIN_H: u16 = 3;
        const MAX_W: u16 = 80;
        const MAX_H: u16 = 40;
        if let Some((sx, sy, ow, oh)) = self.drag_start {
            let dx = mouse_x as i32 - sx as i32;
            let dy = mouse_y as i32 - sy as i32;
            let new_w = (ow as i32 + dx)
                .max(MIN_W as i32)
                .min(MAX_W as i32) as u16;
            let new_h = (oh as i32 + dy)
                .max(MIN_H as i32)
                .min(MAX_H as i32) as u16;
            self.w = new_w.min(area.width.saturating_sub(2));
            self.h = new_h.min(area.height.saturating_sub(2));
        }
    }

    #[allow(dead_code)]
    pub fn end_drag(&mut self) {
        self.dragging = false;
        self.drag_start = None;
    }

    /// Render with a dimmed background scrim behind the panel.
    #[allow(dead_code)]
    pub fn render_scrim(&self, f: &mut Frame, area: Rect, content: Text) {
        if !self.visible {
            return;
        }
        let r = self.rect(area);
        // Dimmed scrim behind panel only
        let scrim = Paragraph::new("").style(Style::default().bg(Color::Rgb(20, 20, 25)));
        f.render_widget(scrim, r);
        self.render(f, area, content);
    }

    /// Render with auto-stacking (no overlap with other boards at same corner).
    pub fn render_stacked(&self, f: &mut Frame, area: Rect, content: Text, stack: &mut BoardStack) {
        if !self.visible {
            return;
        }
        let r = stack.push(self.corner, self.w, self.h, area);
        let scrim = Paragraph::new("").style(Style::default().bg(Color::Rgb(20, 20, 25)));
        f.render_widget(scrim, r);
        let mut para = Paragraph::new(content).wrap(Wrap { trim: false });
        if self.show_border {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(self.border_fg));
            let block = if self.title.is_empty() {
                block
            } else {
                block.title(self.title.as_str())
            };
            para = para.block(block);
        }
        f.render_widget(para, r);
    }

    /// Render using the persistent state.
    #[allow(dead_code)]
    #[allow(dead_code)]
    pub fn render(&self, f: &mut Frame, area: Rect, content: Text) {
        if !self.visible {
            return;
        }
        let r = self.rect(area);
        let mut para = Paragraph::new(content).wrap(Wrap { trim: false });
        if self.show_border {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(self.border_fg));
            let block = if self.title.is_empty() {
                block
            } else {
                block.title(self.title.as_str())
            };
            para = para.block(block);
        }
        f.render_widget(para, r);
    }
}

// ── Stateless Board (builder pattern) ──

pub struct Board {
    title: String,
    border_fg: Color,
    show_border: bool,
    padding_h: u16,
    wrap: bool,
    /// Overlay positioning (None = fill available area)
    overlay: Option<(Corner, u16, u16)>, // (corner, width, height)
}

// ── Toast: auto-dismiss notification ──

#[allow(dead_code)]
pub struct Toast {
    pub message: String,
    pub level: ToastLevel,
    pub expires: std::time::Instant,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum ToastLevel {
    Info,
    Warn,
    Error,
}

impl Toast {
    pub fn new(msg: impl Into<String>, level: ToastLevel, duration: std::time::Duration) -> Self {
        Self {
            message: msg.into(),
            level,
            expires: std::time::Instant::now() + duration,
        }
    }

    pub fn is_expired(&self) -> bool {
        std::time::Instant::now() > self.expires
    }

    #[allow(dead_code)]
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let color = match self.level {
            ToastLevel::Info => Color::Cyan,
            ToastLevel::Warn => Color::Yellow,
            ToastLevel::Error => Color::Red,
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(color));
        let w = (self.message.len() + 4).min(area.width as usize - 4) as u16;
        let r = Rect {
            x: area.x + area.width.saturating_sub(w + 2),
            y: area.y + 1,
            width: w,
            height: 3,
        };
        f.render_widget(Paragraph::new(self.message.as_str()).block(block), r);
    }
}

// ── ListBoard: selectable item list ──

#[allow(dead_code)]
pub struct ListBoard {
    pub items: Vec<String>,
    pub selected: usize,
    pub title: String,
    pub visible: bool,
}

impl ListBoard {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            items: Vec::new(),
            selected: 0,
            title: title.into(),
            visible: false,
        }
    }

    pub fn set_items(&mut self, items: Vec<String>) {
        self.items = items;
        self.selected = 0;
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

    pub fn current(&self) -> Option<&str> {
        self.items.get(self.selected).map(|s| s.as_str())
    }

    #[allow(dead_code)]
    pub fn render(&self, f: &mut Frame, area: Rect) {
        if !self.visible || self.items.is_empty() {
            return;
        }
        let lines: Vec<Line> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let prefix = if i == self.selected { "* " } else { "  " };
                let style = if i == self.selected {
                    Style::default()
                        .bg(Color::Rgb(50, 50, 60))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                Line::from(Span::styled(format!("{prefix}{item}"), style))
            })
            .collect();
        let h = (lines.len() as u16 + 2).min(area.height);
        let w = 40u16.min(area.width.saturating_sub(4));
        let r = Rect {
            x: area.x + area.width.saturating_sub(w),
            y: area.y + area.height.saturating_sub(h),
            width: w,
            height: h,
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(self.title.as_str())
            .border_style(Style::default().fg(Color::DarkGray));
        f.render_widget(Paragraph::new(Text::from(lines)).block(block), r);
    }
}

// ── ProviderPicker: two-pane provider / model selector ──

#[allow(dead_code)]
pub struct ProviderPicker {
    pub providers: Vec<ProviderSource>,
    pub models: Vec<String>,
    pub provider_selected: usize,
    pub model_selected: usize,
    pub focus_providers: bool,
    pub title: String,
    pub visible: bool,
    pub w: u16,
    pub h: u16,
    pub corner: Corner,
}

#[allow(dead_code)]
impl ProviderPicker {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            providers: Vec::new(),
            models: Vec::new(),
            provider_selected: 0,
            model_selected: 0,
            focus_providers: true,
            title: title.into(),
            visible: false,
            w: 64,
            h: 20,
            corner: Corner::BottomRight,
        }
    }

    pub fn set_providers(&mut self, providers: Vec<ProviderSource>) {
        self.providers = providers;
        self.provider_selected = 0;
        self.models.clear();
        self.model_selected = 0;
        self.focus_providers = true;
    }

    pub fn set_models(&mut self, models: Vec<String>) {
        self.models = models;
        self.model_selected = 0;
    }

    pub fn current_provider(&self) -> Option<&ProviderSource> {
        self.providers.get(self.provider_selected)
    }

    pub fn current_model(&self) -> Option<&str> {
        self.models.get(self.model_selected).map(|s| s.as_str())
    }

    pub fn select_next(&mut self) {
        if self.focus_providers {
            if !self.providers.is_empty() {
                self.provider_selected = (self.provider_selected + 1) % self.providers.len();
            }
        } else if !self.models.is_empty() {
            self.model_selected = (self.model_selected + 1) % self.models.len();
        }
    }

    pub fn select_prev(&mut self) {
        if self.focus_providers {
            if !self.providers.is_empty() {
                self.provider_selected =
                    (self.provider_selected + self.providers.len() - 1) % self.providers.len();
            }
        } else if !self.models.is_empty() {
            self.model_selected =
                (self.model_selected + self.models.len() - 1) % self.models.len();
        }
    }

    pub fn toggle_focus(&mut self) {
        self.focus_providers = !self.focus_providers;
    }

    pub fn render_stacked(&self, f: &mut Frame, area: Rect, stack: &mut BoardStack) {
        if !self.visible {
            return;
        }
        let r = stack.push(self.corner, self.w, self.h, area);
        let scrim = Paragraph::new("").style(Style::default().bg(Color::Rgb(20, 20, 25)));
        f.render_widget(scrim, r);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(80, 80, 90)))
            .title(self.title.as_str());
        let inner = block.inner(r);
        f.render_widget(block, r);

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(inner);

        let left_lines: Vec<Line> = if self.providers.is_empty() {
            vec![Line::from("  Loading providers…")]
        } else {
            self.providers
                .iter()
                .enumerate()
                .map(|(i, source)| {
                    let selected = self.focus_providers && i == self.provider_selected;
                    let prefix = if selected { "* " } else { "  " };
                    let key_ok = source.api_key().is_some();
                    let key_mark = if key_ok { "✓" } else { "✗" };
                    let style = if selected {
                        Style::default()
                            .bg(Color::Rgb(50, 50, 60))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    Line::from(Span::styled(
                        format!("{prefix}{} ({}) [{}]", source.name, source.api_type, key_mark),
                        style,
                    ))
                })
                .collect()
        };
        let left_block = Block::default()
            .borders(Borders::RIGHT)
            .title(" Providers ");
        let left_para = Paragraph::new(Text::from(left_lines))
            .block(left_block)
            .wrap(Wrap { trim: false });
        f.render_widget(left_para, chunks[0]);

        let right_lines: Vec<Line> = if self.models.is_empty() {
            vec![Line::from("  (no models)")]
        } else {
            self.models
                .iter()
                .enumerate()
                .map(|(i, model)| {
                    let selected = !self.focus_providers && i == self.model_selected;
                    let prefix = if selected { "* " } else { "  " };
                    let style = if selected {
                        Style::default()
                            .bg(Color::Rgb(50, 50, 60))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    Line::from(Span::styled(format!("{prefix}{model}"), style))
                })
                .collect()
        };
        let right_block = Block::default().title(" Models ");
        let right_para = Paragraph::new(Text::from(right_lines))
            .block(right_block)
            .wrap(Wrap { trim: false });
        f.render_widget(right_para, chunks[1]);
    }
}

// ── ConfirmBoard: yes/no dialog ──

#[allow(dead_code)]
pub struct ConfirmBoard {
    pub message: String,
    pub visible: bool,
    pub yes_selected: bool,
}

impl ConfirmBoard {
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            visible: false,
            yes_selected: true,
        }
    }

    pub fn toggle(&mut self) {
        self.yes_selected = !self.yes_selected;
    }

    #[allow(dead_code)]
    pub fn render(&self, f: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }
        let yes_style = if self.yes_selected {
            Style::default()
                .bg(Color::Rgb(50, 50, 60))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let no_style = if !self.yes_selected {
            Style::default()
                .bg(Color::Rgb(50, 50, 60))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let lines = vec![
            Line::from(Span::raw(&self.message)),
            Line::from(""),
            Line::from(vec![
                Span::styled("  [ Yes ]", yes_style),
                Span::raw("  "),
                Span::styled("[ No ]", no_style),
            ]),
        ];
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Confirm ")
            .border_style(Style::default().fg(Color::Yellow));
        let h = 5u16;
        let w = (self.message.len() as u16 + 6).min(area.width - 4);
        let x = (area.width.saturating_sub(w)) / 2;
        let y = (area.height.saturating_sub(h)) / 2;
        f.render_widget(
            Paragraph::new(Text::from(lines)).block(block),
            Rect {
                x: area.x + x,
                y: area.y + y,
                width: w,
                height: h,
            },
        );
    }
}

// ── FormBoard: labeled editable fields ──

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum FieldValue {
    String(String),
    Password(String),
    Integer(i64),
    Enum { options: Vec<String>, selected: usize },
    Boolean(bool),
}

#[allow(dead_code)]
pub struct FormBoard {
    pub title: String,
    pub visible: bool,
    pub selected: usize,
    fields: Vec<FormField>,
}

#[derive(Debug, Clone)]
struct FormField {
    label: String,
    value: FieldValue,
    editing: bool,
    edit_buffer: String,
}

#[allow(dead_code)]
impl FormBoard {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            visible: false,
            selected: 0,
            fields: Vec::new(),
        }
    }

    pub fn add_field(&mut self, label: impl Into<String>, value: FieldValue) {
        self.fields.push(FormField {
            label: label.into(),
            value,
            editing: false,
            edit_buffer: String::new(),
        });
    }

    pub fn next(&mut self) {
        self.commit_edit();
        if !self.fields.is_empty() {
            self.selected = (self.selected + 1) % self.fields.len();
        }
    }

    pub fn prev(&mut self) {
        self.commit_edit();
        if !self.fields.is_empty() {
            self.selected = (self.selected + self.fields.len() - 1) % self.fields.len();
        }
    }

    pub fn edit(&mut self) {
        if let Some(field) = self.fields.get_mut(self.selected) {
            match &mut field.value {
                FieldValue::Boolean(b) => *b = !*b,
                FieldValue::Enum { options, selected } => {
                    if !options.is_empty() {
                        *selected = (*selected + 1) % options.len();
                    }
                }
                _ => {
                    field.editing = true;
                    field.edit_buffer = field.display_value();
                }
            }
        }
    }

    pub fn toggle(&mut self) {
        self.edit();
    }

    pub fn insert(&mut self, c: char) {
        if let Some(field) = self.fields.get_mut(self.selected) {
            if field.editing {
                field.edit_buffer.push(c);
            }
        }
    }

    pub fn backspace(&mut self) {
        if let Some(field) = self.fields.get_mut(self.selected) {
            if field.editing {
                field.edit_buffer.pop();
            }
        }
    }

    pub fn commit_edit(&mut self) {
        if let Some(field) = self.fields.get_mut(self.selected) {
            if field.editing {
                field.set_from_buffer();
                field.editing = false;
                field.edit_buffer.clear();
            }
        }
    }

    pub fn current_value(&self) -> Option<String> {
        self.fields.get(self.selected).map(|f| {
            if f.editing {
                f.edit_buffer.clone()
            } else {
                f.display_value()
            }
        })
    }

    pub fn set_value(&mut self, value: &str) {
        if let Some(field) = self.fields.get_mut(self.selected) {
            field.set_from_str(value);
            field.editing = false;
            field.edit_buffer.clear();
        }
    }

    pub fn current_label(&self) -> Option<&str> {
        self.fields.get(self.selected).map(|f| f.label.as_str())
    }

    #[allow(dead_code)]
    pub fn render(&self, f: &mut Frame, area: Rect) {
        if !self.visible || self.fields.is_empty() {
            return;
        }
        let lines: Vec<Line> = self
            .fields
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let selected = i == self.selected;
                let value = if field.editing {
                    format!("{}{}", field.edit_buffer, "▏")
                } else {
                    field.display_value()
                };
                let display = if matches!(field.value, FieldValue::Boolean(_)) {
                    let marker = if field.value.bool_value() { "[x] " } else { "[ ] " };
                    format!("{}{}", marker, field.label)
                } else {
                    format!("{}: {}", field.label, value)
                };
                let style = if selected {
                    Style::default()
                        .bg(Color::Rgb(50, 50, 60))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(180, 180, 190))
                };
                Line::from(Span::styled(display, style))
            })
            .collect();
        let h = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
        let w = 50u16.min(area.width.saturating_sub(4));
        let x = (area.width.saturating_sub(w)) / 2;
        let y = (area.height.saturating_sub(h)) / 2;
        let r = Rect {
            x: area.x + x,
            y: area.y + y,
            width: w,
            height: h,
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(self.title.as_str())
            .border_style(Style::default().fg(Color::Cyan));
        f.render_widget(Paragraph::new(Text::from(lines)).block(block), r);
    }
}

#[allow(dead_code)]
impl FieldValue {
    fn bool_value(&self) -> bool {
        match self {
            FieldValue::Boolean(b) => *b,
            _ => false,
        }
    }
}

#[allow(dead_code)]
impl FormField {
    fn display_value(&self) -> String {
        match &self.value {
            FieldValue::String(s) => s.clone(),
            FieldValue::Password(s) => "*".repeat(s.len()),
            FieldValue::Integer(n) => n.to_string(),
            FieldValue::Enum { options, selected } => {
                options.get(*selected).cloned().unwrap_or_default()
            }
            FieldValue::Boolean(b) => (if *b { "On" } else { "Off" }).into(),
        }
    }

    fn bool_value(&self) -> bool {
        match &self.value {
            FieldValue::Boolean(b) => *b,
            _ => false,
        }
    }

    fn set_from_str(&mut self, s: &str) {
        match &mut self.value {
            FieldValue::String(v) => *v = s.into(),
            FieldValue::Password(v) => *v = s.into(),
            FieldValue::Integer(v) => {
                if let Ok(n) = s.parse::<i64>() {
                    *v = n;
                }
            }
            FieldValue::Enum { options, selected } => {
                if let Some(idx) = options.iter().position(|o| o == s) {
                    *selected = idx;
                }
            }
            FieldValue::Boolean(v) => {
                *v = matches!(s.to_lowercase().as_str(), "true" | "on" | "yes" | "1");
            }
        }
    }

    fn set_from_buffer(&mut self) {
        if self.editing {
            let buf = self.edit_buffer.clone();
            self.set_from_str(&buf);
        }
    }
}

// ── TwoPaneBoard: left list + right details ──

#[allow(dead_code)]
pub struct TwoPaneBoard {
    pub title: String,
    pub visible: bool,
    pub left: Vec<String>,
    pub right: Vec<String>,
    pub left_selected: usize,
    pub right_selected: usize,
    pub focus_left: bool,
}

#[allow(dead_code)]
impl TwoPaneBoard {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            visible: false,
            left: Vec::new(),
            right: Vec::new(),
            left_selected: 0,
            right_selected: 0,
            focus_left: true,
        }
    }

    pub fn set_left(&mut self, items: Vec<String>) {
        self.left = items;
        self.left_selected = 0;
    }

    pub fn set_right(&mut self, items: Vec<String>) {
        self.right = items;
        self.right_selected = 0;
    }

    pub fn select_left_next(&mut self) {
        if !self.left.is_empty() {
            self.left_selected = (self.left_selected + 1) % self.left.len();
        }
    }

    pub fn select_left_prev(&mut self) {
        if !self.left.is_empty() {
            self.left_selected = (self.left_selected + self.left.len() - 1) % self.left.len();
        }
    }

    pub fn select_right_next(&mut self) {
        if !self.right.is_empty() {
            self.right_selected = (self.right_selected + 1) % self.right.len();
        }
    }

    pub fn select_right_prev(&mut self) {
        if !self.right.is_empty() {
            self.right_selected = (self.right_selected + self.right.len() - 1) % self.right.len();
        }
    }

    pub fn focus_left(&mut self) {
        self.focus_left = true;
    }

    pub fn focus_right(&mut self) {
        self.focus_left = false;
    }

    pub fn current_left(&self) -> Option<&str> {
        self.left.get(self.left_selected).map(|s| s.as_str())
    }

    pub fn current_right(&self) -> Option<&str> {
        self.right.get(self.right_selected).map(|s| s.as_str())
    }

    #[allow(dead_code)]
    pub fn render(&self, f: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }
        let w = (area.width as f32 * 0.65) as u16;
        let h = (area.height as f32 * 0.65) as u16;
        let x = (area.width.saturating_sub(w)) / 2;
        let y = (area.height.saturating_sub(h)) / 2;
        let r = Rect {
            x: area.x + x,
            y: area.y + y,
            width: w,
            height: h,
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(self.title.as_str())
            .border_style(Style::default().fg(Color::Cyan));
        let inner = block.inner(r);
        f.render_widget(block, r);
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(inner);

        let left_lines: Vec<Line> = self
            .left
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let selected = self.focus_left && i == self.left_selected;
                let style = if selected {
                    Style::default()
                        .bg(Color::Rgb(50, 50, 60))
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else if i == self.left_selected && !self.focus_left {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Rgb(160, 160, 170))
                };
                Line::from(Span::styled(format!("  {}", item), style))
            })
            .collect();
        let left_block = Block::default()
            .borders(Borders::RIGHT)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray));
        f.render_widget(
            Paragraph::new(Text::from(left_lines)).block(left_block),
            chunks[0],
        );

        let right_lines: Vec<Line> = self
            .right
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let selected = !self.focus_left && i == self.right_selected;
                let style = if selected {
                    Style::default()
                        .bg(Color::Rgb(60, 60, 70))
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(180, 180, 190))
                };
                Line::from(Span::styled(format!("  {}", item), style))
            })
            .collect();
        f.render_widget(Paragraph::new(Text::from(right_lines)), chunks[1]);
    }
}

// ── Stateless Board (builder pattern) ──

impl Board {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            border_fg: Color::Rgb(80, 80, 90),
            show_border: true,
            padding_h: 0,
            wrap: true,
            overlay: None,
        }
    }

    /// Full-width, no border (for output area).
    #[allow(dead_code)]
    pub fn plain() -> Self {
        let mut b = Self::new("");
        b.show_border = false;
        b
    }

    /// Floating overlay at a corner.
    pub fn overlay(mut self, corner: Corner, w: u16, h: u16) -> Self {
        self.overlay = Some((corner, w, h));
        self
    }

    #[allow(dead_code)]
    pub fn border_color(mut self, c: Color) -> Self {
        self.border_fg = c;
        self
    }

    #[allow(dead_code)]
    pub fn no_border(mut self) -> Self {
        self.show_border = false;
        self
    }

    #[allow(dead_code)]
    pub fn padding(mut self, h: u16) -> Self {
        self.padding_h = h;
        self
    }

    /// Render content within this board.
    /// If overlay is set, content is placed at the specified corner of `area`.
    /// Otherwise, content fills `area`.
    #[allow(dead_code)]
    pub fn render(&self, f: &mut Frame, area: Rect, content: Text) {
        let render_area = if let Some((corner, w, h)) = self.overlay {
            let w = w.min(area.width.saturating_sub(2));
            let h = h.min(area.height.saturating_sub(2));
            let (x, y) = match corner {
                Corner::TopLeft => (0, 0),
                Corner::TopRight => (area.width.saturating_sub(w), 0),
                Corner::BottomLeft => (0, area.height.saturating_sub(h)),
                Corner::BottomRight => {
                    (area.width.saturating_sub(w), area.height.saturating_sub(h))
                }
            };
            Rect {
                x: area.x + x,
                y: area.y + y,
                width: w,
                height: h,
            }
        } else {
            area
        };

        let mut para = Paragraph::new(content);
        if self.wrap {
            para = para.wrap(Wrap { trim: false });
        }

        if self.show_border {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(self.border_fg));
            let block = if self.title.is_empty() {
                block
            } else {
                block.title(self.title.as_str())
            };
            para = para.block(block);
        }

        f.render_widget(para, render_area);
    }

    /// Quick static method: render a help card at bottom-right.
    #[allow(dead_code)]
    pub fn help_card(f: &mut Frame, area: Rect, entries: &[(&str, &str)], accent: Color) {
        let max_w = entries.iter().map(|(n, _)| n.len()).max().unwrap_or(10);
        let lines: Vec<_> = entries
            .iter()
            .map(|(n, d)| {
                ratatui::text::Line::from(vec![
                    ratatui::text::Span::styled(
                        format!("{n:<w$}", w = max_w),
                        Style::default()
                            .fg(accent)
                            .add_modifier(ratatui::style::Modifier::BOLD),
                    ),
                    ratatui::text::Span::raw(format!("  {d}")),
                ])
            })
            .collect();
        let h = lines.len() as u16 + 2;
        let w = (max_w + 30).min(area.width.saturating_sub(4) as usize) as u16;
        Self::new(" Help ")
            .border_color(accent)
            .overlay(Corner::BottomRight, w, h)
            .render(f, area, Text::from(lines));
    }
}

// ── ProgressBoard ──

#[derive(Default)]
#[allow(dead_code)]
pub struct ProgressBoard {
    pub label: String,
    pub progress: f32,
    pub visible: bool,
}

impl ProgressBoard {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            progress: 0.0,
            visible: false,
        }
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }
        let w = 30u16.min(area.width - 4);
        let bar_w = w.saturating_sub(2) as usize;
        let filled = (self.progress * bar_w as f32) as usize;
        let bar = format!(
            "[{}{}]",
            "█".repeat(filled),
            " ".repeat(bar_w.saturating_sub(filled))
        );
        let r = Rect {
            x: area.x + area.width - w - 2,
            y: area.y + 1,
            width: w,
            height: 3,
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(self.label.as_str())
            .border_style(Style::default().fg(Color::Cyan));
        f.render_widget(Paragraph::new(bar).block(block), r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boardstack_push() {
        let mut s = BoardStack::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let r1 = s.push(Corner::BottomRight, 30, 10, area);
        let r2 = s.push(Corner::BottomRight, 30, 8, area);
        // r2 should be above r1 (stacked)
        assert!(r2.y < r1.y);
    }

    #[test]
    fn test_boardstate_new() {
        let b = BoardState::new("Test", 30, 10, Corner::BottomRight);
        assert!(b.visible);
        assert_eq!(b.title, "Test");
    }

    #[test]
    fn test_listboard_nav() {
        let mut lb = ListBoard::new("Test");
        lb.set_items(vec!["a".into(), "b".into(), "c".into()]);
        assert_eq!(lb.current(), Some("a"));
        lb.select_next();
        assert_eq!(lb.current(), Some("b"));
        lb.select_prev();
        assert_eq!(lb.current(), Some("a"));
    }

    #[test]
    fn test_formboard_nav_and_values() {
        let mut form = FormBoard::new("Settings");
        form.add_field("Name", FieldValue::String("default".into()));
        form.add_field("Key", FieldValue::Password("secret".into()));
        form.add_field("Port", FieldValue::Integer(8080));
        form.add_field(
            "Theme",
            FieldValue::Enum {
                options: vec!["dark".into(), "light".into()],
                selected: 0,
            },
        );
        form.add_field("Debug", FieldValue::Boolean(false));
        assert_eq!(form.current_value(), Some("default".into()));
        form.next();
        assert_eq!(form.current_label(), Some("Key"));
        assert_eq!(form.current_value(), Some("******".into()));
        form.next();
        form.set_value("9000");
        assert_eq!(form.current_value(), Some("9000".into()));
        form.next();
        form.toggle();
        assert_eq!(form.current_value(), Some("light".into()));
        form.next();
        form.edit();
        assert_eq!(form.current_value(), Some("On".into()));
    }

    #[test]
    fn test_twopaneboard_nav() {
        let mut tp = TwoPaneBoard::new("Picker");
        tp.set_left(vec!["a".into(), "b".into()]);
        tp.set_right(vec!["a1".into(), "a2".into()]);
        assert_eq!(tp.current_left(), Some("a"));
        assert_eq!(tp.current_right(), Some("a1"));
        tp.select_left_next();
        assert_eq!(tp.current_left(), Some("b"));
        tp.focus_right();
        tp.select_right_next();
        assert_eq!(tp.current_right(), Some("a2"));
    }
}
