//! Timeline panel — visual checkpoint history with rollback/diff actions.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

pub fn render_timeline_panel(
    f: &mut Frame,
    slot: &crate::panel::PanelSlot,
    items: &[radiumical_core::checkpoint::Checkpoint],
    selected: usize,
    diff: Option<&str>,
) {
    let r = slot.rect;
    let inner = Rect {
        x: r.x + 1,
        y: r.y + 1,
        width: r.width.saturating_sub(2),
        height: r.height.saturating_sub(2),
    };

    let diff_h = if diff.is_some() {
        (inner.height / 3).max(5)
    } else {
        0
    };
    let list_h = inner.height.saturating_sub(diff_h);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(list_h), Constraint::Length(diff_h)])
        .split(inner);

    // ── Timeline list ──
    let mut lines = Vec::new();
    if items.is_empty() {
        lines.push(Line::from("No checkpoints yet."));
        lines.push(Line::from("Mutating tool calls will create them automatically."));
    } else {
        lines.push(Line::from(Span::styled(
            " ↑/↓ select  Enter diff  r rollback  Esc close ",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));

        for (i, cp) in items.iter().enumerate() {
            let time = cp
                .created
                .format("%H:%M")
                .to_string();
            let marker = if i == selected { "●" } else { "○" };
            let branch_info = cp
                .branch
                .as_deref()
                .or(cp.commit.as_deref())
                .map(|s| format!(" [{:.7}]", s))
                .unwrap_or_default();
            let msg = cp
                .message
                .strip_prefix("[radiumical] checkpoint: ")
                .unwrap_or(&cp.message);

            let style = if i == selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            lines.push(Line::from(vec![
                Span::styled(format!("{marker} "), style),
                Span::styled(format!("{time} "), style),
                Span::styled(msg.to_string(), style),
                Span::styled(branch_info, Style::default().fg(Color::DarkGray)),
            ]));
            // Vertical connector, except for last item.
            if i < items.len().saturating_sub(1) {
                lines.push(Line::from("  │"));
            }
        }
    }

    let list_para = Paragraph::new(Text::from(lines))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_type(BorderType::Plain)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(list_para, chunks[0]);

    // ── Diff preview ──
    if let Some(diff_text) = diff {
        let diff_lines: Vec<Line> = diff_text
            .lines()
            .map(|line| {
                let style = if line.starts_with('+') {
                    Style::default().fg(Color::Green)
                } else if line.starts_with('-') {
                    Style::default().fg(Color::Red)
                } else if line.starts_with("@@") || line.starts_with("diff --git") {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::Gray)
                };
                Line::from(Span::styled(line.to_string(), style))
            })
            .collect();
        let diff_para = Paragraph::new(Text::from(diff_lines))
            .block(
                Block::default()
                    .title(" Diff ")
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .wrap(ratatui::widgets::Wrap { trim: false });
        f.render_widget(diff_para, chunks[1]);
    }
}
