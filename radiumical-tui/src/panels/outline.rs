use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

#[allow(dead_code)]
pub fn render_outline_panel(
    f: &mut Frame,
    area: Rect,
    outline: &radiumical_core::outline::WorkspaceOutline,
    scroll: usize,
) {
    let mut lines: Vec<Line> = Vec::new();
    for entry in &outline.entries {
        let path_line = Line::from(Span::styled(
            format!("  {} ", entry.path),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        lines.push(path_line);
        for item in &entry.items {
            let kind_style = match item.kind.as_str() {
                "fn" | "func" | "function" | "def" | "method" => Color::Green,
                "struct" | "class" | "enum" | "trait" | "type" | "interface" => Color::Yellow,
                "mod" | "use" => Color::DarkGray,
                "const" | "static" => Color::Magenta,
                _ => Color::Rgb(160, 160, 170),
            };
            let sig = item
                .signature
                .as_deref()
                .map(|s| format!(" {s}"))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(format!("{:<8}", item.kind), Style::default().fg(kind_style)),
                Span::styled(
                    format!("{}{}", item.name, sig),
                    Style::default().fg(Color::Rgb(180, 180, 190)),
                ),
            ]));
        }
        lines.push(Line::from(""));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No outline data. Run /outline to generate.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let visible = area.height as usize;
    let start = scroll.min(lines.len().saturating_sub(visible).max(0));
    let end = (start + visible).min(lines.len());
    let visible_lines: Vec<Line> = lines[start..end].to_vec();

    f.render_widget(Paragraph::new(visible_lines), area);
}
