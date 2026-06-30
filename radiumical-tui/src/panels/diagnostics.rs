use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DiagnosticItem {
    pub file: String,
    pub line: u32,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

#[allow(dead_code)]
pub fn parse_diagnostics(raw: &str) -> Vec<DiagnosticItem> {
    let mut items = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // cargo check format: file:line:col: severity: message
        // or: file:line: severity: message
        if let Some((file_loc, rest)) = trimmed.split_once(": ") {
            let severity = if rest.starts_with("error") {
                DiagnosticSeverity::Error
            } else if rest.starts_with("warning") {
                DiagnosticSeverity::Warning
            } else {
                DiagnosticSeverity::Info
            };
            let parts: Vec<&str> = file_loc.splitn(3, ':').collect();
            let file = parts.first().copied().unwrap_or("").to_string();
            let line_num = parts
                .get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            items.push(DiagnosticItem {
                file,
                line: line_num,
                severity,
                message: rest.to_string(),
            });
        }
    }
    items
}

#[allow(dead_code)]
pub fn render_diagnostics_panel(
    f: &mut Frame,
    area: Rect,
    diagnostics: &[DiagnosticItem],
    scroll: usize,
) {
    let mut lines: Vec<Line> = Vec::new();

    if diagnostics.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No diagnostics. Run /lint to check.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let errors = diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .count();
        let warnings = diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .count();
        lines.push(Line::from(Span::styled(
            format!("  {} errors, {} warnings", errors, warnings),
            Style::default().fg(if errors > 0 { Color::Red } else { Color::Yellow }),
        )));
        lines.push(Line::from(""));

        for item in diagnostics {
            let (icon, color) = match item.severity {
                DiagnosticSeverity::Error => ("✗", Color::Red),
                DiagnosticSeverity::Warning => ("⚠", Color::Yellow),
                DiagnosticSeverity::Info => ("ℹ", Color::Cyan),
            };
            let loc = if item.line > 0 {
                format!("{}:{}", item.file, item.line)
            } else {
                item.file.clone()
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(icon, Style::default().fg(color)),
                Span::raw(" "),
                Span::styled(loc, Style::default().fg(Color::Rgb(120, 120, 140))),
                Span::raw(" "),
                Span::styled(&item.message, Style::default().fg(Color::Rgb(180, 180, 190))),
            ]));
        }
    }

    let visible = area.height as usize;
    let start = scroll.min(lines.len().saturating_sub(visible).max(0));
    let end = (start + visible).min(lines.len());
    let visible_lines: Vec<Line> = lines[start..end].to_vec();

    f.render_widget(Paragraph::new(visible_lines), area);
}
