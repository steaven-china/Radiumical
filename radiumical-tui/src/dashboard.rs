//! Dashboard — encapsulated nav hub with categories + items.
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

pub struct Dashboard {
    pub visible: bool,
    pub cat_idx: usize,
    pub item_idx: usize,
    pub in_items: bool, // true = navigating items, false = navigating categories
    sections: Vec<(&'static str, &'static [&'static str])>,
}

#[derive(Debug, Clone)]
pub enum DashAction {
    ShowModels,
    ShowSettings,
    ShowHelp,
    ToggleThinking,
    SessionNew,
    SessionSave,
    SessionLoad,
    SessionList,
    SessionDelete,
    Diagnostics,
    ShowTools,
    About,
}

impl Dashboard {
    pub fn new() -> Self {
        Self {
            visible: false,
            cat_idx: 0,
            item_idx: 0,
            in_items: false,
            sections: vec![
                ("Navigate", &["Models", "Settings", "Help"] as &[&str]),
                ("Session", &["New", "Save", "Load", "List", "Delete"]),
                ("Debug", &["Diagnostics", "Tools", "Thinking"]),
                ("View", &["About"]),
            ],
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn up(&mut self) {
        if self.in_items {
            self.item_idx = self.item_idx.saturating_sub(1);
        } else {
            self.cat_idx = self.cat_idx.saturating_sub(1);
        }
    }

    pub fn down(&mut self) {
        if self.in_items {
            let max = self
                .sections
                .get(self.cat_idx)
                .map(|(_, items)| items.len().saturating_sub(1))
                .unwrap_or(0);
            self.item_idx = (self.item_idx + 1).min(max);
        } else {
            self.cat_idx = (self.cat_idx + 1).min(self.sections.len().saturating_sub(1));
        }
    }

    pub fn left(&mut self) {
        if self.in_items {
            self.in_items = false;
            self.item_idx = 0;
        }
    }

    pub fn right(&mut self) {
        if !self.in_items {
            self.in_items = true;
            self.item_idx = 0;
        }
    }

    pub fn selected_action(&self) -> Option<DashAction> {
        if !self.in_items {
            return None;
        }
        let (cat, items) = self.sections.get(self.cat_idx)?;
        let item = items.get(self.item_idx)?;
        Some(match (*cat, *item) {
            ("Navigate", "Models") => DashAction::ShowModels,
            ("Navigate", "Settings") => DashAction::ShowSettings,
            ("Navigate", "Help") => DashAction::ShowHelp,
            ("Session", "New") => DashAction::SessionNew,
            ("Session", "Save") => DashAction::SessionSave,
            ("Session", "Load") => DashAction::SessionLoad,
            ("Session", "List") => DashAction::SessionList,
            ("Session", "Delete") => DashAction::SessionDelete,
            ("Debug", "Diagnostics") => DashAction::Diagnostics,
            ("Debug", "Tools") => DashAction::ShowTools,
            ("Debug", "Thinking") => DashAction::ToggleThinking,
            ("View", "About") => DashAction::About,
            _ => return None,
        })
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }
        let w = (area.width as f32 * 0.65) as u16;
        let h = (area.height as f32 * 0.65) as u16;
        let x = (area.width - w) / 2;
        let y = (area.height - h) / 2;
        let r = Rect {
            x: area.x + x,
            y: area.y + y,
            width: w,
            height: h,
        };

        f.render_widget(Clear, r);

        // Outer frame
        let outer = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Dashboard ")
            .border_style(Style::default().fg(Color::Cyan));
        f.render_widget(outer, r);
        let inner = Rect {
            x: r.x + 1,
            y: r.y + 1,
            width: r.width - 2,
            height: r.height - 2,
        };

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(inner);

        // Left: categories
        let nav_lines: Vec<Line> = self
            .sections
            .iter()
            .enumerate()
            .map(|(i, (cat, _))| {
                let selected = i == self.cat_idx && !self.in_items;
                let prefix = if selected { "* " } else { "  " };
                let style = if selected {
                    Style::default()
                        .bg(Color::Rgb(50, 50, 60))
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else if i == self.cat_idx && self.in_items {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Rgb(160, 160, 170))
                };
                Line::from(Span::styled(format!("{prefix}{cat}"), style))
            })
            .collect();
        let nav_block = Block::default()
            .borders(Borders::RIGHT)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::DarkGray));
        f.render_widget(
            Paragraph::new(Text::from(nav_lines)).block(nav_block),
            chunks[0],
        );

        // Right: items for selected category
        if let Some((cat_name, items)) = self.sections.get(self.cat_idx) {
            let item_lines: Vec<Line> = items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let selected = self.in_items && i == self.item_idx;
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
            let detail_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(format!(" {cat_name} "))
                .border_style(Style::default().fg(Color::Cyan));
            f.render_widget(
                Paragraph::new(Text::from(item_lines)).block(detail_block),
                chunks[1],
            );
        }
    }
}
