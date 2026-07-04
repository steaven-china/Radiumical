use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::panel::PanelSlot;

pub fn render(f: &mut Frame, slot: &PanelSlot) {
    let inner = Rect {
        x: slot.rect.x + 1,
        y: slot.rect.y + 1,
        width: slot.rect.width.saturating_sub(2),
        height: slot.rect.height.saturating_sub(2),
    };

    let agents = radiumical_core::subagent::list_all();

    let mut lines: Vec<Line> = Vec::new();

    if agents.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No sub-agents running.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for agent in &agents {
            let (icon, status_color) = if agent.done {
                if agent.success {
                    ("\u{2713}", Color::Green)
                } else {
                    ("\u{274c}", Color::Red)
                }
            } else {
                ("\u{23f3}", Color::Yellow)
            };
            let role = agent.agent.as_deref().unwrap_or("coder");
            let status_text = if agent.done {
                if agent.success { "done" } else { "failed" }
            } else {
                "running"
            };

            let line = Line::from(vec![
                Span::styled(format!(" {icon}  "), Style::default().fg(status_color)),
                Span::styled(
                    format!("{:<12}", agent.id),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<10}", status_text),
                    Style::default().fg(status_color),
                ),
                Span::styled(
                    format!("({role})"),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            lines.push(line);

            // Show task description (truncated)
            let task_display: String = agent.task.chars().take(slot.rect.width as usize - 6).collect();
            let task_suffix = if agent.task.chars().count() > slot.rect.width as usize - 6 {
                "…"
            } else {
                ""
            };
            lines.push(Line::from(Span::styled(
                format!("      {task_display}{task_suffix}"),
                Style::default().fg(Color::Rgb(130, 130, 140)),
            )));
        }
    }

    // Footer
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Enter: view result  Esc: close",
        Style::default().fg(Color::DarkGray),
    )));

    f.render_widget(Paragraph::new(lines), inner);
}
