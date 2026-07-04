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
    pub enabled: bool,
}

pub fn render(f: &mut Frame, slot: &PanelSlot, servers: &[McpServerStatus], selected: usize) {
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
        for (i, server) in servers.iter().enumerate() {
            let is_sel = i == selected;
            let toggle_icon = if server.enabled {
                "\u{25cf}"
            } else {
                "\u{25cb}"
            };
            let toggle_color = if server.enabled {
                Color::Green
            } else {
                Color::DarkGray
            };

            let name_style = if !server.enabled {
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else if is_sel {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            };

            let cursor = if is_sel { "> " } else { "  " };

            let mut spans = vec![
                Span::styled(cursor, Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("{toggle_icon}  "),
                    Style::default().fg(toggle_color),
                ),
                Span::styled(format!("{:<14}", server.name), name_style),
            ];

            if server.enabled {
                let (status_icon, status_color, status_text) = if server.alive {
                    ("\u{25cf}", Color::Green, "online")
                } else {
                    ("\u{2715}", Color::Red, "offline")
                };
                spans.push(Span::styled(
                    format!("{} tools  ", server.tool_count),
                    Style::default().fg(Color::Rgb(130, 130, 140)),
                ));
                spans.push(Span::styled(status_icon, Style::default().fg(status_color)));
                spans.push(Span::styled(
                    format!(" {status_text}"),
                    Style::default().fg(status_color),
                ));
            } else {
                spans.push(Span::styled(
                    "disabled",
                    Style::default().fg(Color::DarkGray),
                ));
            }

            lines.push(Line::from(spans));
        }
    }

    // Footer
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Enter: toggle | \u{2191}\u{2193}: navigate | Esc: close",
        Style::default().fg(Color::DarkGray),
    )));

    f.render_widget(Paragraph::new(lines), inner);
}
