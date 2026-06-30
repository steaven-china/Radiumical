use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

use super::{
    text::strip_markdown, text::wrap_text_to_width, tool::format_tool_args,
    tool::strip_ansi_escapes, tool::wrapped_tool_result_lines, Block, BlockKind,
};
use crate::markdown::MarkdownRenderer;

const DIM: Color = Color::Rgb(100, 100, 110);
const BORDER: Color = Color::Rgb(80, 80, 90);

#[derive(Clone, Copy)]
enum DiffLineType {
    Added,
    Removed,
    Header,
    Normal,
}

fn diff_line_color(t: DiffLineType) -> Color {
    match t {
        DiffLineType::Added => Color::Rgb(80, 200, 80),
        DiffLineType::Removed => Color::Rgb(220, 80, 80),
        DiffLineType::Header => Color::Rgb(80, 180, 220),
        DiffLineType::Normal => BORDER,
    }
}

fn classify_diff_line(line: &str) -> DiffLineType {
    if line.starts_with('+') {
        DiffLineType::Added
    } else if line.starts_with('-') {
        DiffLineType::Removed
    } else {
        DiffLineType::Normal
    }
}

fn collect_diff_result_lines(result: &str, content_w: usize) -> Vec<(DiffLineType, String)> {
    let mut lines = Vec::new();
    let mut in_diff = false;

    for raw_line in result.lines() {
        if raw_line.contains('\x04') && raw_line.contains("diff:") {
            lines.push((DiffLineType::Header, "── Diff ──".to_string()));
            in_diff = true;
            continue;
        }

        let clean = strip_ansi_escapes(raw_line);
        let trimmed = clean.trim();

        if in_diff {
            if trimmed.is_empty() {
                continue;
            }
            let line_type = classify_diff_line(&clean);
            for w in wrap_text_to_width(&clean, content_w) {
                lines.push((line_type, w));
            }
        } else {
            for w in wrap_text_to_width(&clean, content_w) {
                lines.push((DiffLineType::Normal, w));
            }
        }
    }

    lines
}

// ── Pass 2: render blocks ──

impl Block {
    pub fn render(
        &self,
        _area_width: u16,
        _frame: usize,
        _markdown: &mut MarkdownRenderer,
        show_full: bool,
    ) -> Vec<Line<'static>> {
        match &self.kind {
            BlockKind::Logo => {
                // Breathing color: slow hue shift per line
                let breathe = |i: usize| -> Color {
                    let phase = (_frame as f32 + i as f32 * 0.3) * 0.05;
                    Color::Rgb(
                        (100.0 + 60.0 * phase.sin()) as u8,
                        (120.0 + 50.0 * (phase + 2.0).sin()) as u8,
                        (180.0 + 40.0 * (phase + 4.0).sin()) as u8,
                    )
                };
                self.source_lines
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        Line::from(Span::styled(s.clone(), Style::default().fg(breathe(i))))
                    })
                    .collect()
            }

            BlockKind::CodeFence { lang } => {
                let label = if lang.is_empty() {
                    "─".into()
                } else {
                    format!(" {lang} ")
                };
                let mut lines: Vec<Line> =
                    vec![Line::from(Span::styled(label, Style::default().fg(DIM)))];
                if self.source_lines.len() > 2 {
                    let code: String =
                        self.source_lines[1..self.source_lines.len().saturating_sub(1)].join("\n");
                    let rows = radiumical_core::highlight::highlight_code(&code, lang);
                    for row in rows {
                        lines.push(Line::from(row));
                    }
                }
                lines.push(Line::from(Span::styled(
                    "─".to_string(),
                    Style::default().fg(DIM),
                )));
                lines
            }

            BlockKind::Table {
                rows,
                widths,
                sep_idx,
            } => {
                let mut lines = Vec::new();
                let avail_width = (_area_width as usize).saturating_sub(4).max(1);
                let adjusted_widths = super::text::fit_table_widths(widths, avail_width);
                lines.push(border_line("┌", "┬", "┐", "─", &adjusted_widths));
                for (i, row) in rows.iter().enumerate() {
                    if *sep_idx == Some(i) {
                        lines.push(border_line("├", "┼", "┤", "─", &adjusted_widths));
                        continue;
                    }
                    // Wrap each cell to its column width and render multi-line rows
                    let mut cell_lines: Vec<Vec<String>> = Vec::new();
                    let mut max_lines = 1usize;
                    for (ci, cell) in row.iter().enumerate() {
                        let col_w = adjusted_widths.get(ci).copied().unwrap_or(3);
                        let stripped = strip_markdown(cell);
                        let wrapped = wrap_text_to_width(&stripped, col_w);
                        max_lines = max_lines.max(wrapped.len());
                        cell_lines.push(wrapped);
                    }
                    for li in 0..max_lines {
                        let mut spans = vec![
                            Span::raw("  "),
                            Span::styled("│", Style::default().fg(BORDER)),
                        ];
                        for (ci, cell_wrapped) in cell_lines.iter().enumerate() {
                            let col_w = adjusted_widths.get(ci).copied().unwrap_or(3);
                            let cell_text = cell_wrapped.get(li).map(|s| s.as_str()).unwrap_or("");
                            let cell_spans = _markdown.render_inline_cached(cell_text);
                            spans.push(Span::raw(" "));
                            let mut remaining = col_w;
                            for cs in cell_spans {
                                let cw = cs.width();
                                if cw <= remaining {
                                    spans.push(cs);
                                    remaining -= cw;
                                } else {
                                    break;
                                }
                            }
                            if remaining > 0 {
                                spans.push(Span::raw(" ".repeat(remaining)));
                            }
                            spans.push(Span::raw(" "));
                            if ci < adjusted_widths.len() - 1 {
                                spans.push(Span::styled("│", Style::default().fg(BORDER)));
                            }
                        }
                        spans.push(Span::styled("│", Style::default().fg(BORDER)));
                        lines.push(Line::from(spans));
                    }
                }
                lines.push(border_line("└", "┴", "┘", "─", &adjusted_widths));
                lines
            }

            BlockKind::Heading { level } => {
                let s = self.source_lines[0].trim();
                let rest = s.strip_prefix(&"#".repeat(*level)).unwrap_or(s);
                let rest = rest.strip_prefix(' ').unwrap_or(rest);
                let color = if *level <= 2 {
                    Color::Cyan
                } else {
                    Color::Blue
                };
                vec![Line::from(Span::styled(
                    rest.to_string(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ))]
            }

            BlockKind::ListItem => {
                let s = self.source_lines[0].trim();
                let rest = &s[2..];
                let mut spans = vec![Span::styled("  • ", Style::default().fg(Color::Green))];
                spans.extend(_markdown.render_inline_cached(rest));
                vec![Line::from(spans)]
            }

            BlockKind::OrderedItem { num } => {
                let s = self.source_lines[0].trim();
                let rest = s
                    .strip_prefix(&format!("{num}. "))
                    .or_else(|| s.strip_prefix(&format!("{num}) ")))
                    .unwrap_or("");
                let mut spans = vec![Span::styled(
                    format!("  {num}. "),
                    Style::default().fg(Color::Green),
                )];
                spans.extend(_markdown.render_inline_cached(rest));
                vec![Line::from(spans)]
            }

            BlockKind::Blockquote => {
                let s = self.source_lines[0].trim();
                let rest = &s[2..];
                let mut spans = vec![Span::styled("│ ", Style::default().fg(DIM))];
                spans.extend(_markdown.render_inline_cached(rest));
                vec![Line::from(spans)]
            }

            BlockKind::Reasoning => {
                let s = &self.source_lines[0];
                let raw = s[1..].trim_start_matches("[thinking] ").trim();
                let style = Style::default()
                    .fg(Color::Rgb(170, 170, 180))
                    .bg(Color::Rgb(35, 35, 42));
                if show_full {
                    raw.lines()
                        .map(|l| Line::from(Span::styled(format!("[thinking] {l}"), style)))
                        .collect()
                } else {
                    let preview: String = raw.chars().take(40).collect();
                    let dots = if raw.chars().count() > 40 { "…" } else { "" };
                    vec![Line::from(Span::styled(
                        format!("[thinking] {preview}{dots}"),
                        style,
                    ))]
                }
            }

            BlockKind::ToolCall {
                name,
                args,
                result,
                expanded,
                result_scroll,
            } => {
                let args_disp = format_tool_args(name, args);

                let result_text: String = result
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| l.replace('\t', "    "))
                    .collect::<Vec<_>>()
                    .join("\n");

                let box_w = tool_box_width(name, &args_disp, &result_text, _area_width as usize);
                let st = Style::default().fg(BORDER);

                let mut lines = Vec::new();
                lines.push(Line::from(Span::styled(box_top(name, box_w), st)));
                lines.push(Line::from(Span::styled(
                    box_args_line(&args_disp, box_w),
                    st,
                )));

                let hint_style = Style::default().fg(DIM);
                if !*expanded || result_text.is_empty() {
                    lines.push(Line::from(Span::styled(box_bottom(box_w), st)));
                    lines.push(Line::from(Span::styled(
                        "  [\u{25b8} click to expand]",
                        hint_style,
                    )));
                    return lines;
                }

                lines.push(Line::from(Span::styled(
                    box_sep("├── result ", box_w),
                    st,
                )));
                lines.extend(render_tool_result_lines(
                    &result_text,
                    box_w,
                    *result_scroll,
                ));
                lines.push(Line::from(Span::styled(box_bottom(box_w), st)));
                lines.push(Line::from(Span::styled(
                    "  [\u{25be} click to collapse]",
                    hint_style,
                )));
                lines
            }

            BlockKind::Text => {
                let raw = &self.source_lines[0];
                // Error styling: \x03 prefix
                if let Some(err) = raw.strip_prefix('\x03') {
                    return vec![Line::from(Span::styled(
                        err.to_string(),
                        Style::default().fg(Color::Red),
                    ))];
                }
                let leading = raw
                    .chars()
                    .take_while(|c| c.is_whitespace())
                    .collect::<String>();
                let s = raw.trim_start();
                if s.is_empty() {
                    return vec![Line::from("")];
                }
                // Diff highlighting
                if let Some(rest) = s.strip_prefix("+ ") {
                    return vec![Line::from(vec![
                        Span::raw(leading),
                        Span::styled(format!("+ {rest}"), Style::default().fg(Color::Green)),
                    ])];
                }
                if let Some(rest) = s.strip_prefix("- ") {
                    return vec![Line::from(vec![
                        Span::raw(leading),
                        Span::styled(format!("- {rest}"), Style::default().fg(Color::Red)),
                    ])];
                }
                // Truncate read_file output (lines matching "  NNN | ..."), Ctrl+O to expand
                if !show_full
                    && s.len() > 40
                    && s.get(7..9) == Some("| ")
                    && s[..6].chars().all(|c| c == ' ' || c.is_ascii_digit())
                {
                    let preview: String = s.chars().take(40).collect();
                    return vec![Line::from(vec![
                        Span::raw(leading),
                        Span::styled(format!("{preview}…"), Style::default().fg(Color::DarkGray)),
                    ])];
                }
                let mut spans = vec![Span::raw(leading)];
                if let Some(sample) = crate::markdown::maybe_color_sample(s) {
                    spans.push(sample);
                } else {
                    spans.extend(_markdown.render_inline_cached(s));
                }
                if spans.len() <= 1 {
                    vec![Line::from("")]
                } else {
                    vec![Line::from(spans)]
                }
            }
        }
    }
}

fn tool_box_width(name: &str, args: &str, result: &str, area_width: usize) -> usize {
    let mut w = name.width() + 7;
    w = w.max(args.width() + 6);
    for line in result.lines() {
        w = w.max(line.width() + 6);
    }
    w = w.max(56);
    w.min(area_width)
}

fn box_top(name: &str, width: usize) -> String {
    let name_w = name.width();
    let fill = width.saturating_sub(name_w + 7);
    let name = truncate_to_width(name, width.saturating_sub(7));
    format!("  ┌─ {name} {fill}┐", fill = "─".repeat(fill))
}

fn box_args_line(args: &str, width: usize) -> String {
    let inner = width.saturating_sub(6);
    let args = truncate_to_width(args, inner);
    let pad = inner.saturating_sub(args.width());
    format!("  │  {args}{pad}│", pad = " ".repeat(pad))
}

fn box_sep(label: &str, width: usize) -> String {
    let label_w = label.width();
    let fill = width.saturating_sub(label_w + 3);
    format!("  {label}{fill}┤", fill = "─".repeat(fill))
}

fn box_bottom(width: usize) -> String {
    let fill = width.saturating_sub(4);
    format!("  └{fill}┘", fill = "─".repeat(fill))
}

fn box_content_line(content: &str, width: usize, right: Option<char>) -> String {
    let content_w = width.saturating_sub(5).max(1);
    let visible = crate::layout::text::wrap_text_to_width(content, content_w)
        .into_iter()
        .next()
        .unwrap_or_default();
    let pad = content_w.saturating_sub(visible.width());
    match right {
        Some(c) => format!("  │ {visible}{}{c}", " ".repeat(pad)),
        None => format!("  │ {visible}{}", " ".repeat(pad)),
    }
}

fn render_tool_result_lines(
    result: &str,
    width: usize,
    result_scroll: usize,
) -> Vec<Line<'static>> {
    const MAX_RESULT_VIS: usize = 10;
    let content_w = width.saturating_sub(5).max(1);
    let is_diff = result.contains('\x04');

    let all_lines: Vec<(DiffLineType, String)> = if is_diff {
        collect_diff_result_lines(result, content_w)
    } else {
        wrapped_tool_result_lines(result, content_w)
            .into_iter()
            .map(|l| (DiffLineType::Normal, l))
            .collect()
    };

    let has_overflow = all_lines.len() > MAX_RESULT_VIS;
    let max_scroll = all_lines.len().saturating_sub(MAX_RESULT_VIS);
    let scroll = result_scroll.min(max_scroll);
    let visible = &all_lines[scroll..(scroll + MAX_RESULT_VIS).min(all_lines.len())];

    let sb_h = visible.len();
    let sb_thumb_h = ((MAX_RESULT_VIS as f32 / all_lines.len().max(MAX_RESULT_VIS) as f32).min(1.0)
        * sb_h as f32)
        .max(1.0) as usize;
    let sb_thumb_y = if max_scroll == 0 {
        0
    } else {
        ((scroll * (sb_h.saturating_sub(sb_thumb_h))) / max_scroll)
            .min(sb_h.saturating_sub(sb_thumb_h))
    };

    visible
        .iter()
        .enumerate()
        .map(|(i, (line_type, line))| {
            let right = if has_overflow
                && i >= sb_thumb_y
                && i < sb_thumb_y + sb_thumb_h
            {
                Some('█')
            } else {
                None
            };
            let color = diff_line_color(*line_type);
            Line::from(Span::styled(
                box_content_line(line, width, right),
                Style::default().fg(color),
            ))
        })
        .collect()
}

fn truncate_to_width(s: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = ch.to_string().width();
        if w + cw > max_width {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out
}

fn border_line(left: &str, mid: &str, right: &str, fill: &str, cols: &[usize]) -> Line<'static> {
    let mut s = String::from(left);
    for (i, &w) in cols.iter().enumerate() {
        s.push_str(&fill.repeat(w + 2));
        if i < cols.len() - 1 {
            s.push_str(mid);
        }
    }
    s.push_str(right);
    Line::from(Span::styled(format!("  {s}"), Style::default().fg(BORDER)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::measure_blocks;
    use crate::markdown::MarkdownRenderer;

    #[test]
    fn test_markdown_heading_list_blockquote() {
        let input = vec![
            "## Hello World".to_string(),
            "".to_string(),
            "This is **bold** and *italic* and `code`.".to_string(),
            "> A blockquote with a [link](https://example.com)".to_string(),
            "- List item with `inline` code".to_string(),
            "1. Ordered item".to_string(),
        ];

        let blocks = measure_blocks(&input, 80, false);
        let mut md = MarkdownRenderer::new();

        // Verify block kinds
        assert_eq!(
            blocks.len(),
            6,
            "should have 6 blocks (heading, blank, text, blockquote, list, ordered)"
        );
        assert!(matches!(blocks[0].kind, BlockKind::Heading { level: 2 }));
        assert!(matches!(blocks[1].kind, BlockKind::Text)); // blank line
        assert!(matches!(blocks[2].kind, BlockKind::Text));
        assert!(matches!(blocks[3].kind, BlockKind::Blockquote));
        assert!(matches!(blocks[4].kind, BlockKind::ListItem));
        assert!(matches!(blocks[5].kind, BlockKind::OrderedItem { .. }));

        // Render each block (should not panic)
        for block in &blocks {
            let lines = block.render(80, 0, &mut md, false);
            assert!(!lines.is_empty(), "block should produce at least one line");
        }
    }

    #[test]
    fn test_code_fence() {
        let input = vec![
            "```rust".to_string(),
            "fn main() {".to_string(),
            r#"    println!("hello");"#.to_string(),
            "}".to_string(),
            "```".to_string(),
        ];

        let blocks = measure_blocks(&input, 80, false);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0].kind, BlockKind::CodeFence { .. }));

        let mut md = MarkdownRenderer::new();
        let lines = blocks[0].render(80, 0, &mut md, false);
        assert!(lines.len() >= 3);
    }

    #[test]
    fn test_tool_call_box_lines_fit_width() {
        use unicode_width::UnicodeWidthStr;

        let mut md = MarkdownRenderer::new();
        let block = Block {
            kind: BlockKind::ToolCall {
                name: "read_file".into(),
                args: r#"{"path":"src/main.rs","start_line":1,"end_line":100}"#.into(),
                result: "line one\n\
                    line two is quite long and should wrap nicely inside the box without exceeding the width\n\
                    short\n\
                    another line\n\
                    yet more content here\n\
                    and even more lines to ensure overflow\n\
                    1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11"
                    .into(),
                expanded: true,
                result_scroll: 2,
            },
            source_lines: vec![],
            height: 10,
        };

        let area_width: u16 = 80;
        let lines = block.render(area_width, 0, &mut md, false);
        assert!(!lines.is_empty(), "tool call should render lines");
        for (i, line) in lines.iter().enumerate() {
            let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
            let w = text.width();
            assert!(
                w <= area_width as usize,
                "line {i} width {w} exceeds area width {area_width}: {text:?}"
            );
        }
    }
}
