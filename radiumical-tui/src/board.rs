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

#[allow(dead_code)]
#[derive(Default)]
pub struct BoardStack {
    boards: Vec<(Corner, u16, u16, u16)>, // (corner, x_offset, y_offset, height)
}

#[allow(dead_code)]
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

    pub fn clear(&mut self) {
        self.boards.clear();
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
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
            let new_w = (ow as i32 + dx).max(MIN_W as i32).min(MAX_W as i32) as u16;
            let new_h = (oh as i32 + dy).max(MIN_H as i32).min(MAX_H as i32) as u16;
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
        self.render(f, area, content);
    }

    /// Render with auto-stacking (no overlap with other boards at same corner).
    #[allow(dead_code)]
    pub fn render_stacked(&self, f: &mut Frame, area: Rect, content: Text, stack: &mut BoardStack) {
        if !self.visible {
            return;
        }
        let r = stack.push(self.corner, self.w, self.h, area);
        let mut para = Paragraph::new(content).wrap(Wrap { trim: false });
        if self.show_border {
            let block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(self.border_fg))
                .style(Style::default().bg(Color::Rgb(20, 20, 25)));
            let block = if self.title.is_empty() {
                block
            } else {
                block.title(self.title.as_str())
            };
            para = para.block(block);
        } else {
            para = para.style(Style::default().bg(Color::Rgb(20, 20, 25)));
        }
        f.render_widget(para, r);
    }

    /// Render using the persistent state.
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
                .border_style(Style::default().fg(self.border_fg))
                .style(Style::default().bg(Color::Rgb(20, 20, 25)));
            let block = if self.title.is_empty() {
                block
            } else {
                block.title(self.title.as_str())
            };
            para = para.block(block);
        } else {
            para = para.style(Style::default().bg(Color::Rgb(20, 20, 25)));
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

pub struct Toast {
    pub message: String,
    pub level: ToastLevel,
    pub expires: std::time::Instant,
}

#[derive(Debug, Clone, Copy)]
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

#[allow(dead_code)]
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

pub struct ProviderPicker {
    pub providers: Vec<ProviderSource>,
    pub models: Vec<String>,
    pub provider_selected: usize,
    pub model_selected: usize,
    pub focus_providers: bool,
    pub title: String,
    pub visible: bool,
    #[allow(dead_code)]
    pub w: u16,
    #[allow(dead_code)]
    pub h: u16,
    #[allow(dead_code)]
    pub corner: Corner,
}

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
            self.model_selected = (self.model_selected + self.models.len() - 1) % self.models.len();
        }
    }

    pub fn toggle_focus(&mut self) {
        self.focus_providers = !self.focus_providers;
    }

    pub fn toggle(&mut self, cmd_tx: &tokio::sync::mpsc::Sender<crate::tui::BackendCmd>) -> bool {
        self.visible = !self.visible;
        if self.visible {
            let _ = cmd_tx.blocking_send(crate::tui::BackendCmd::FetchProviders);
        }
        self.visible
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    #[allow(dead_code)]
    pub fn render_stacked(&self, f: &mut Frame, area: Rect, stack: &mut BoardStack) {
        if !self.visible {
            return;
        }
        const MIN_W: u16 = 24;
        const MIN_H: u16 = 6;
        let w = self.w.max(MIN_W).min(area.width.saturating_sub(2));
        let h = self.h.max(MIN_H).min(area.height.saturating_sub(2));
        let r = stack.push(self.corner, w, h, area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(80, 80, 90)))
            .style(Style::default().bg(Color::Rgb(20, 20, 25)))
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
                        format!(
                            "{prefix}{} ({}) [{}]",
                            source.name, source.api_type, key_mark
                        ),
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

    /// Render at a specific rect (from PanelManager layout).
    pub fn render_at(&self, f: &mut Frame, r: Rect) {
        use ratatui::widgets::Clear;
        if !self.visible {
            return;
        }
        f.render_widget(Clear, r);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Rgb(80, 80, 90)))
            .style(Style::default().bg(Color::Rgb(20, 20, 25)))
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
                        format!(
                            "{prefix}{} ({}) [{}]",
                            source.name, source.api_type, key_mark
                        ),
                        style,
                    ))
                })
                .collect()
        };
        let left_block = Block::default()
            .borders(Borders::RIGHT)
            .title(" Providers ");
        f.render_widget(
            Paragraph::new(Text::from(left_lines))
                .block(left_block)
                .wrap(Wrap { trim: false }),
            chunks[0],
        );

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
        f.render_widget(
            Paragraph::new(Text::from(right_lines))
                .block(right_block)
                .wrap(Wrap { trim: false }),
            chunks[1],
        );
    }
}

// ── ConfirmBoard: yes/no dialog ──

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

    #[allow(dead_code)]
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

    /// Render at a specific rect (from PanelManager layout).
    pub fn render_at(&self, f: &mut Frame, r: Rect) {
        if !self.visible {
            return;
        }
        use ratatui::widgets::Clear;
        // Dim background scrim
        f.render_widget(Clear, r);
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
            .border_style(Style::default().fg(Color::Yellow))
            .style(Style::default().bg(Color::Rgb(20, 20, 25)));
        let inner = block.inner(r);
        f.render_widget(block, r);
        f.render_widget(Paragraph::new(Text::from(lines)), inner);
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
    #[allow(dead_code)]
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
}

// ── ProgressBoard ──

#[derive(Default)]
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
}
