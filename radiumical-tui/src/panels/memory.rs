use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

#[derive(Debug, Clone, Default)]
pub struct MemoryPanelState {
    pub tier_idx: usize,
    pub entry_idx: usize,
}

impl MemoryPanelState {
    pub fn select_next(&mut self, memory: &radiumical_core::memory::Memory) {
        let tier_entries = self.current_tier_entries(memory);
        if !tier_entries.is_empty() {
            self.entry_idx = (self.entry_idx + 1) % tier_entries.len();
        }
    }

    pub fn select_prev(&mut self, memory: &radiumical_core::memory::Memory) {
        let tier_entries = self.current_tier_entries(memory);
        if !tier_entries.is_empty() {
            self.entry_idx = (self.entry_idx + tier_entries.len() - 1) % tier_entries.len();
        }
    }

    pub fn next_tier(&mut self, memory: &radiumical_core::memory::Memory) {
        self.tier_idx = (self.tier_idx + 1) % 3;
        self.entry_idx = 0;
        let _ = memory;
    }

    pub fn current_tier_name(&self) -> &'static str {
        match self.tier_idx {
            0 => "core",
            1 => "mino",
            _ => "short",
        }
    }

    fn current_tier_entries<'a>(
        &self,
        memory: &'a radiumical_core::memory::Memory,
    ) -> &'a [radiumical_core::memory::MemoryEntry] {
        match self.tier_idx {
            0 => &memory.core,
            1 => &memory.mino,
            _ => &memory.short,
        }
    }

    pub fn selected_entry_index(&self) -> Option<usize> {
        Some(self.entry_idx)
    }
}

pub fn render_memory_panel(
    f: &mut Frame,
    area: Rect,
    memory: &radiumical_core::memory::Memory,
    state: &MemoryPanelState,
) {
    let tiers = [
        ("Core", &memory.core as &[radiumical_core::memory::MemoryEntry]),
        ("Mino", &memory.mino),
        ("Short", &memory.short),
    ];

    let mut lines: Vec<Line> = Vec::new();

    for (ti, (name, entries)) in tiers.iter().enumerate() {
        let is_active = ti == state.tier_idx;
        let header_style = if is_active {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(120, 120, 140))
        };
        lines.push(Line::from(Span::styled(
            format!("  {} ({}):", name, entries.len()),
            header_style,
        )));
        if entries.is_empty() {
            lines.push(Line::from(Span::styled(
                "    (empty)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for (ei, entry) in entries.iter().enumerate() {
                let selected = is_active && ei == state.entry_idx;
                let style = if selected {
                    Style::default()
                        .bg(Color::Rgb(50, 50, 60))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(180, 180, 190))
                };
                let prefix = if selected { "  > " } else { "    " };
                let ts = if entry.timestamp.is_empty() {
                    String::new()
                } else {
                    format!("[{}] ", entry.timestamp)
                };
                lines.push(Line::from(Span::styled(
                    format!("{}{}{}", prefix, ts, entry.content),
                    style,
                )));
            }
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(Span::styled(
        "  Tab: switch tier | Del: remove",
        Style::default().fg(Color::DarkGray),
    )));

    let visible = area.height as usize;
    let visible_lines: Vec<Line> = lines.into_iter().take(visible).collect();

    f.render_widget(Paragraph::new(visible_lines), area);
}
