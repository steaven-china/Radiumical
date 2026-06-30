use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::panel::PanelSlot;

#[derive(Debug, Clone)]
pub struct McpServerStatus {
    pub name: String,
    pub alive: bool,
    pub tool_count: usize,
}

pub fn render(f: &mut Frame, slot: &PanelSlot, servers: &[McpServerStatus]) {
    let inner = Rect {
        x: slot.rect.x + 1,
        y: slot.rect.y + 1,
        width: slot.rect.width.saturating_sub(2),
        height: slot.rect.height.saturating_sub(2),
    };

    let mut lines: Vec<Line> = Vec::new();

    if servers.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No MCP servers configured.",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Configure in ~/.radi/mcp.json",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for server in servers {
            let (icon, status_color, status_text) = if server.alive {
                ("\u{25cf}", Color::Green, "online")
            } else {
                ("\u{2715}", Color::Red, "offline")
            };

            let line = Line::from(vec![
                Span::styled(format!(" {icon}  "), Style::default().fg(status_color)),
                Span::styled(
                    format!("{:<14}", server.name),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{} tools", server.tool_count),
                    Style::default().fg(Color::Rgb(130, 130, 140)),
                ),
                Span::raw("  "),
                Span::styled(status_text, Style::default().fg(status_color)),
            ]);
            lines.push(line);
        }
    }

    // Footer
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " health check: 5s interval",
        Style::default().fg(Color::DarkGray),
    )));

    f.render_widget(Paragraph::new(lines), inner);
}
