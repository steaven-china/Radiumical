//! Plan / orchestration panel: shows task list with status icons and a
//! progress bar.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use radiumical_core::orchestrator::TaskStatus;

use crate::panel::PanelSlot;

/// A single plan task with an id, title, and execution status.
#[derive(Debug, Clone)]
pub struct PlanTask {
    pub id: u32,
    pub title: String,
    pub status: TaskStatus,
}

/// Render the plan panel showing the task list and an overall progress bar.
pub fn render_plan_panel(f: &mut Frame, slot: &PanelSlot, title: &str, tasks: &[PlanTask]) {
    let inner = Rect {
        x: slot.rect.x + 1,
        y: slot.rect.y + 1,
        width: slot.rect.width.saturating_sub(2),
        height: slot.rect.height.saturating_sub(2),
    };

    if tasks.is_empty() {
        let empty = Paragraph::new("  No plan active.\n  Use orchestrate tool to create one.")
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: false });
        f.render_widget(empty, inner);
        return;
    }

    let total = tasks.len();
    let done = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Done)
        .count();
    let pct = (done * 100).checked_div(total).unwrap_or(0);

    let mut lines: Vec<Line> = Vec::new();

    let title_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    lines.push(Line::from(vec![Span::styled(
        format!("  {title}"),
        title_style,
    )]));
    lines.push(Line::from(""));

    for task in tasks {
        let icon = task.status.icon();
        let (icon_fg, title_fg) = match task.status {
            TaskStatus::Done => (Color::Green, Color::Rgb(120, 120, 130)),
            TaskStatus::Active => (Color::Yellow, Color::White),
            TaskStatus::Pending => (Color::DarkGray, Color::Rgb(140, 140, 150)),
            TaskStatus::Blocked => (Color::Red, Color::Rgb(180, 120, 120)),
            TaskStatus::Skipped => (Color::Rgb(100, 100, 110), Color::Rgb(90, 90, 100)),
        };
        let display_title = if task.title.chars().count() > 28 {
            format!("{}…", task.title.chars().take(27).collect::<String>())
        } else {
            task.title.clone()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {icon} "), Style::default().fg(icon_fg)),
            Span::styled(
                format!("{}. {}", task.id, display_title),
                Style::default().fg(title_fg),
            ),
        ]));
    }

    lines.push(Line::from(""));

    let bar_w = inner.width.saturating_sub(8) as usize;
    let filled = (bar_w * pct) / 100;
    let empty = bar_w.saturating_sub(filled);
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {}/{} ", done, total),
            Style::default().fg(Color::Rgb(130, 130, 140)),
        ),
        Span::styled(bar, Style::default().fg(Color::Green)),
        Span::styled(
            format!(" {}%", pct),
            Style::default().fg(Color::Rgb(130, 130, 140)),
        ),
    ]));

    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(para, inner);
}
