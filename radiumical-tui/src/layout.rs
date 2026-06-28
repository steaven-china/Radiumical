//! Two-pass output layout: measure → allocate → render.
//! Avoids per-line positioning drift — every block gets measured first,
//! then positioned with pre-calculated coordinates.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

const DIM: Color = Color::Rgb(100, 100, 110);
const BORDER: Color = Color::Rgb(80, 80, 90);

// ── Block types ──

#[derive(Debug, Clone, Hash)]
pub enum BlockKind {
    /// ASCII art logo (detected by █ characters)
    Logo,
    /// Code fence block
    CodeFence { lang: String },
    /// Table (buffered, fully measured)
    Table {
        rows: Vec<Vec<String>>,
        widths: Vec<usize>,
        sep_idx: Option<usize>,
    },
    /// Tool call box (collapsible)
    ToolCall {
        name: String,
        args: String,
        result: String,
        expanded: bool,
    },
    /// Regular text (markdown)
    Text,
    /// Heading
    Heading { level: usize },
    /// Unordered list item
    ListItem,
    /// Ordered list item
    OrderedItem { num: String },
    /// Blockquote
    Blockquote,
    /// Reasoning / thinking line
    Reasoning,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub kind: BlockKind,
    pub source_lines: Vec<String>,
    #[allow(dead_code)]
    pub width: usize,
    pub height: usize,
}

// ── Pass 1: measure ──

pub fn measure_blocks(output: &[String], area_width: u16, show_full_reasoning: bool) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut i = 0;

    while i < output.len() {
        let line = &output[i];
        let trimmed = line.trim();

        // Logo block: first line has █, include subsequent long lines (╚═╝, etc.)
        if line.contains('█') && line.len() > 30 {
            let start = i;
            i += 1;
            while i < output.len()
                && output[i].len() > 30
                && (output[i].contains('█') || output[i].contains('╚') || output[i].contains('╔'))
            {
                i += 1;
            }
            let source = output[start..i].to_vec();
            let w = source.iter().map(|s| s.chars().count()).max().unwrap_or(0);
            blocks.push(Block {
                kind: BlockKind::Logo,
                source_lines: source,
                width: w,
                height: i - start,
            });
            continue;
        }

        // Code fence open
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let lang = trimmed
                .trim_start_matches('`')
                .trim_start_matches('~')
                .trim()
                .to_string();
            let start = i;
            i += 1;
            while i < output.len() {
                let t = output[i].trim();
                if t.starts_with("```") || t.starts_with("~~~") {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let source = output[start..i].to_vec();
            let w = source.iter().map(|s| s.len()).max().unwrap_or(0);
            // Render guarantees at least 2 lines (label + footer). During streaming
            // the closing fence may be missing, so reserve space for it too.
            let height = if source.len() >= 2
                && (source.last().unwrap().trim().starts_with("```")
                    || source.last().unwrap().trim().starts_with("~~~"))
            {
                source.len().max(2)
            } else {
                source.len() + 1
            };
            blocks.push(Block {
                kind: BlockKind::CodeFence { lang },
                source_lines: source,
                width: w,
                height,
            });
            continue;
        }

        // Table
        if trimmed.starts_with('|') {
            let start = i;
            while i < output.len() && output[i].trim().starts_with('|') {
                i += 1;
            }
            let source = output[start..i].to_vec();
            let (rows, widths, sep_idx) = measure_table(&source, area_width);
            let avail_width = (area_width as usize).saturating_sub(4).max(1);
            let adjusted_widths = fit_table_widths(&widths, avail_width);
            let width = if widths.is_empty() {
                0
            } else {
                widths.iter().sum::<usize>() + widths.len() * 3 + 1
            };
            let sep_count = sep_idx.map(|_| 1).unwrap_or(0);
            let data_rows = rows.len() - sep_count;
            let mut extra_lines = 0usize;
            for (ri, row) in rows.iter().enumerate() {
                if sep_idx == Some(ri) {
                    continue;
                }
                let mut row_max_lines = 1usize;
                for (ci, cell) in row.iter().enumerate() {
                    let col_w = adjusted_widths.get(ci).copied().unwrap_or(3);
                    let stripped = strip_markdown(cell);
                    let wrapped = wrap_text_to_width(&stripped, col_w);
                    row_max_lines = row_max_lines.max(wrapped.len());
                }
                extra_lines += row_max_lines.saturating_sub(1);
            }
            let height = data_rows + 2 + sep_count + extra_lines;
            blocks.push(Block {
                kind: BlockKind::Table {
                    rows,
                    widths,
                    sep_idx,
                },
                source_lines: source,
                width,
                height,
            });
            continue;
        }

        // Tool call box
        if trimmed.starts_with('┌') && trimmed.contains('─') {
            let start = i;
            let name = trimmed
                .trim_start_matches('┌')
                .trim_start_matches('─')
                .trim_end_matches('─')
                .trim()
                .to_string();
            i += 1;
            let mut content_lines = Vec::new();
            while i < output.len() {
                let t = output[i].trim();
                if t.starts_with('└') {
                    i += 1;
                    break;
                }
                if let Some(body) = output[i].strip_prefix("  │  ") {
                    content_lines.push(body.to_string());
                } else if let Some(body) = output[i].strip_prefix("  │ ") {
                    content_lines.push(body.to_string());
                } else if let Some(body) = output[i].strip_prefix("│ ") {
                    content_lines.push(body.to_string());
                }
                i += 1;
            }
            let source = output[start..i].to_vec();
            let args = content_lines.first().cloned().unwrap_or_default();
            let result = if content_lines.len() > 1 {
                content_lines[1..].join("\n")
            } else {
                String::new()
            };
            blocks.push(Block {
                kind: BlockKind::ToolCall {
                    name,
                    args,
                    result,
                    expanded: false,
                },
                source_lines: source,
                width: 0,
                height: 1,
            });
            continue;
        }

        // Reasoning
        if line.starts_with("\x01") {
            let raw = line[1..].trim_start_matches("[思考] ").trim();
            let height = if show_full_reasoning {
                raw.lines().count().max(1)
            } else {
                1
            };
            blocks.push(Block {
                kind: BlockKind::Reasoning,
                source_lines: vec![line.clone()],
                width: line.len(),
                height,
            });
            i += 1;
            continue;
        }

        // Heading
        if let Some(rest) = trimmed.strip_prefix('#') {
            let level = rest.chars().take_while(|c| *c == '#').count() + 1;
            if level <= 6 && rest.as_bytes().get(level - 1) == Some(&b' ') {
                let w = rest[level..].chars().count();
                blocks.push(Block {
                    kind: BlockKind::Heading { level },
                    source_lines: vec![line.clone()],
                    width: w,
                    height: 1,
                });
                i += 1;
                continue;
            }
        }

        // Blockquote
        if let Some(rest) = trimmed.strip_prefix("> ") {
            let w = rest.chars().count() + 2;
            blocks.push(Block {
                kind: BlockKind::Blockquote,
                source_lines: vec![line.clone()],
                width: w,
                height: 1,
            });
            i += 1;
            continue;
        }

        // Unordered list
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            let w = trimmed[2..].chars().count() + 4;
            blocks.push(Block {
                kind: BlockKind::ListItem,
                source_lines: vec![line.clone()],
                width: w,
                height: 1,
            });
            i += 1;
            continue;
        }

        // Ordered list
        if let Some(rest) = trimmed.strip_prefix(|c: char| c.is_ascii_digit()) {
            if rest.starts_with(". ") || rest.starts_with(") ") {
                let num_end = trimmed.find(rest).unwrap_or(0);
                let num = trimmed[..num_end].to_string();
                let w = rest[2..].chars().count() + num.len() + 4;
                blocks.push(Block {
                    kind: BlockKind::OrderedItem { num },
                    source_lines: vec![line.clone()],
                    width: w,
                    height: 1,
                });
                i += 1;
                continue;
            }
        }

        // Regular text (including blank lines — must preserve spacing)
        let w = if trimmed.is_empty() { 0 } else { trimmed.len() };
        blocks.push(Block {
            kind: BlockKind::Text,
            source_lines: vec![line.clone()],
            width: w,
            height: 1,
        });
        i += 1;
    }

    blocks
}

fn measure_table(
    source: &[String],
    _area_width: u16,
) -> (Vec<Vec<String>>, Vec<usize>, Option<usize>) {
    let rows: Vec<Vec<String>> = source
        .iter()
        .map(|line| {
            line.trim()
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect()
        })
        .collect();
    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(1);
    let mut widths = vec![3; col_count];
    // Identify Markdown separator row: every non-empty cell contains only '-' and/or ':'
    let sep_idx = rows.iter().position(|r| {
        r.iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
    });
    for (ri, row) in rows.iter().enumerate() {
        if sep_idx == Some(ri) {
            continue;
        }
        for (ci, cell) in row.iter().enumerate() {
            if ci < col_count {
                let stripped = strip_markdown(cell);
                let w = stripped.width().max(3);
                widths[ci] = widths[ci].max(w);
            }
        }
    }
    (rows, widths, sep_idx)
}

fn strip_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_md_pair(&chars, i + 2, "**") {
                out.push_str(&chars[i + 2..end].iter().collect::<String>());
                i = end + 2;
                continue;
            }
        }
        if chars[i] == '*' && (i == 0 || chars[i - 1] != '*') {
            if let Some(end) = find_md_single(&chars, i + 1, '*') {
                out.push_str(&chars[i + 1..end].iter().collect::<String>());
                i = end + 1;
                continue;
            }
        }
        if chars[i] == '`' {
            if let Some(end) = find_md_single(&chars, i + 1, '`') {
                out.push_str(&chars[i + 1..end].iter().collect::<String>());
                i = end + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn find_md_pair(chars: &[char], start: usize, d: &str) -> Option<usize> {
    let d: Vec<char> = d.chars().collect();
    for i in start..chars.len().saturating_sub(1) {
        if chars[i] == d[0] && chars[i + 1] == d[1] {
            return Some(i);
        }
    }
    None
}

fn find_md_single(chars: &[char], start: usize, d: char) -> Option<usize> {
    chars[start..]
        .iter()
        .position(|&c| c == d)
        .map(|p| start + p)
}

// ── Table width fitting ──

fn fit_table_widths(widths: &[usize], avail: usize) -> Vec<usize> {
    let total: usize = widths.iter().sum::<usize>() + widths.len() * 3 + 1;
    if total <= avail || avail == 0 {
        return widths.to_vec();
    }
    let scale = avail as f32 / total as f32;
    let mut result: Vec<usize> = widths
        .iter()
        .map(|&w| ((w as f32 * scale).max(3.0) as usize).min(w))
        .collect();
    // Ensure we don't exceed avail after rounding
    let result_total = result.iter().sum::<usize>() + result.len() * 3 + 1;
    if result_total > avail && !result.is_empty() {
        let excess = result_total - avail;
        let max_idx = result
            .iter()
            .enumerate()
            .max_by_key(|(_, &w)| w)
            .map(|(i, _)| i)
            .unwrap_or(0);
        result[max_idx] = result[max_idx].saturating_sub(excess).max(3);
    }
    result
}

fn wrap_text_to_width(text: &str, max_width: usize) -> Vec<String> {
    if text.is_empty() || max_width == 0 {
        return vec!["".to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for word in text.split_whitespace() {
        let word_width = word.width();
        let space_width = if current.is_empty() { 0 } else { 1 };

        if word_width > max_width {
            if !current.is_empty() {
                lines.push(current);
                current = String::new();
                current_width = 0;
            }
            let mut w = String::new();
            let mut w_width = 0usize;
            for ch in word.chars() {
                let ch_w = ch.to_string().width();
                if w_width + ch_w > max_width {
                    if !w.is_empty() {
                        lines.push(w);
                    }
                    w = ch.to_string();
                    w_width = ch_w;
                } else {
                    w.push(ch);
                    w_width += ch_w;
                }
            }
            if !w.is_empty() {
                current = w;
                current_width = w_width;
            }
        } else if current_width + space_width + word_width > max_width {
            lines.push(current);
            current = word.to_string();
            current_width = word_width;
        } else {
            if !current.is_empty() {
                current.push(' ');
                current_width += 1;
            }
            current.push_str(word);
            current_width += word_width;
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push("".to_string());
    }
    lines
}

// ── Pass 2: render blocks ──

impl Block {
    pub fn render(
        &self,
        _area_width: u16,
        _frame: usize,
        _markdown: &mut crate::markdown::MarkdownRenderer,
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
                let adjusted_widths = fit_table_widths(widths, avail_width);
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
                let raw = s[1..].trim_start_matches("[思考] ").trim();
                let style = Style::default()
                    .fg(Color::Rgb(170, 170, 180))
                    .bg(Color::Rgb(35, 35, 42));
                if show_full {
                    raw.lines()
                        .map(|l| {
                            Line::from(Span::styled(
                                format!("[思考] {l}"),
                                style,
                            ))
                        })
                        .collect()
                } else {
                    let preview: String = raw.chars().take(40).collect();
                    let dots = if raw.chars().count() > 40 { "…" } else { "" };
                    vec![Line::from(Span::styled(
                        format!("[思考] {preview}{dots}"),
                        style,
                    ))]
                }
            }

            BlockKind::ToolCall {
                name,
                args,
                result,
                expanded,
            } => {
                let result_lines: Vec<String> = result
                    .lines()
                    .map(|l| l.replace('\t', "    "))
                    .collect();
                let max_content = args
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .count()
                    .max(result_lines.iter().map(|l| l.chars().count()).max().unwrap_or(0));
                let inner = max_content.max(56 - 4);

                let args_clean = if args.is_empty() {
                    String::new()
                } else {
                    let max_args = inner.saturating_sub(2);
                    let first: String =
                        args.lines().next().unwrap_or("").chars().take(max_args).collect();
                    let dots = if args.lines().next().unwrap_or("").chars().count() > max_args {
                        "…"
                    } else {
                        ""
                    };
                    format!("{first}{dots}").replace("\\\\", "\\")
                };

                let top_fill = inner.saturating_sub(name.len() + 3);
                let top = format!("  ┌─ {name} {}┐", "─".repeat(top_fill));
                if !*expanded {
                    return vec![Line::from(Span::styled(top, Style::default().fg(BORDER)))];
                }

                let mut lines = Vec::new();
                lines.push(Line::from(Span::styled(top, Style::default().fg(BORDER))));

                // args line with 2-space indent
                let args_pad = inner.saturating_sub(args_clean.chars().count() + 2);
                lines.push(Line::from(Span::styled(
                    format!("  │  {args_clean}{}│", " ".repeat(args_pad)),
                    Style::default().fg(BORDER),
                )));

                // result lines flush left (1-space indent), truncated to fit
                let max_line = inner.saturating_sub(1);
                for line in &result_lines {
                    let truncated: String = line.chars().take(max_line).collect();
                    let pad = inner.saturating_sub(truncated.chars().count() + 1);
                    lines.push(Line::from(Span::styled(
                        format!("  │ {truncated}{}│", " ".repeat(pad)),
                        Style::default().fg(BORDER),
                    )));
                }

                lines.push(Line::from(Span::styled(
                    format!("  └{}┘", "─".repeat(inner)),
                    Style::default().fg(BORDER),
                )));
                lines
            }

            BlockKind::Text => {
                let raw = &self.source_lines[0];
                let leading = raw.chars().take_while(|c| c.is_whitespace()).collect::<String>();
                let s = raw.trim_start();
                if s.is_empty() {
                    return vec![Line::from("")];
                }
                // Diff highlighting
                if let Some(rest) = s.strip_prefix("+ ") {
                    return vec![Line::from(vec![
                        Span::raw(leading),
                        Span::styled(
                            format!("+ {rest}"),
                            Style::default().fg(Color::Green),
                        ),
                    ])];
                }
                if let Some(rest) = s.strip_prefix("- ") {
                    return vec![Line::from(vec![
                        Span::raw(leading),
                        Span::styled(
                            format!("- {rest}"),
                            Style::default().fg(Color::Red),
                        ),
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
                        Span::styled(
                            format!("{preview}…"),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ])];
                }
                let mut spans = vec![Span::raw(leading)];
                spans.extend(_markdown.render_inline_cached(s));
                if spans.len() <= 1 {
                    vec![Line::from("")]
                } else {
                    vec![Line::from(spans)]
                }
            }
        }
    }
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

#[allow(dead_code)]
fn data_line(cells: &[String], cols: &[usize]) -> Line<'static> {
    let edge = Span::styled("│", Style::default().fg(BORDER));
    let mut spans = vec![Span::raw("  "), edge.clone()];
    for (i, cell) in cells.iter().enumerate() {
        let w = cols.get(i).copied().unwrap_or(cell.len());
        let cell_spans = crate::markdown::render_inline(cell);
        spans.push(Span::raw(" "));
        let mut remaining = w;
        let mut truncated = false;
        for cs in cell_spans {
            let cw = cs.width();
            if cw <= remaining {
                spans.push(cs);
                remaining -= cw;
            } else {
                // Truncate: take only what fits and append "…"
                let text: String = cs
                    .content
                    .as_ref()
                    .chars()
                    .take(remaining.saturating_sub(1))
                    .collect();
                if !text.is_empty() {
                    spans.push(Span::styled(text, cs.style));
                }
                spans.push(Span::raw("…"));
                truncated = true;
                break;
            }
        }
        if !truncated && remaining > 0 {
            spans.push(Span::raw(" ".repeat(remaining)));
        }
        spans.push(Span::raw(" "));
        if i < cols.len() - 1 {
            spans.push(edge.clone());
        }
    }
    spans.push(edge);
    Line::from(spans)
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_table_measure_and_render() {
        let input = vec![
            "| Name | Role | Status |".to_string(),
            "|------|------|--------|".to_string(),
            "| Alice | **Admin** | ✅ active |".to_string(),
            "| Bob | *Viewer* | ❌ inactive |".to_string(),
        ];

        let blocks = measure_blocks(&input, 80, false);
        assert_eq!(blocks.len(), 1);
        let block = &blocks[0];

        match &block.kind {
            BlockKind::Table {
                rows,
                widths,
                sep_idx: _,
            } => {
                assert_eq!(rows.len(), 4, "4 table rows");
                assert_eq!(widths.len(), 3, "3 columns");
                // Check that widths account for markdown stripping
                assert!(widths[0] >= 5, "Name column should be >= width 5 (Alice)");
                assert!(
                    widths[1] >= 4,
                    "Role column should be >= width 4 (Viewer stripped)"
                );
                println!("test_table widths: {:?}", widths);
            }
            _ => panic!("expected Table block"),
        }

        let mut md = MarkdownRenderer::new();
        let lines = block.render(80, 0, &mut md, false);
        // Should have: top border + header + sep border + 2 data rows + bottom border = 6 lines
        assert_eq!(
            lines.len(),
            6,
            "table should render 6 lines (borders + header + sep + 2 data)"
        );

        // Verify border characters
        let top = &lines[0].spans[0].content;
        assert!(
            top.contains('┌'),
            "top border should start with ┌, got: {top}"
        );
        let bottom = &lines[5].spans[0].content;
        assert!(
            bottom.contains('└'),
            "bottom border should start with └, got: {bottom}"
        );

        // Verify data is present (index 3 = Alice row)
        let alice_line = &lines[3]
            .spans
            .iter()
            .map(|s| &*s.content)
            .collect::<String>();
        assert!(alice_line.contains("Alice"), "should contain Alice");
        assert!(
            alice_line.contains("Admin"),
            "should contain Admin (bold stripped in border but rendered in data)"
        );
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
    fn test_logo_block_detected() {
        let input = vec![
            "██████╗  █████╗ ██████╗ ██╗██╗   ██╗███╗   ███╗██╗ ██████╗ █████╗ ██╗     "
                .to_string(),
            "██╔══██╗██╔══██╗██╔══██╗██║██║   ██║████╗ ████║██║██╔════╝██╔══██╗██║     "
                .to_string(),
        ];

        let blocks = measure_blocks(&input, 80, false);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0].kind, BlockKind::Logo));
    }

    #[test]
    fn test_reasoning_block() {
        let input = vec!["\x01[思考] Analyzing code structure...".to_string()];

        let blocks = measure_blocks(&input, 80, false);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0].kind, BlockKind::Reasoning));
        assert_eq!(blocks[0].height, 1, "collapsed reasoning height should be 1");

        let mut md = MarkdownRenderer::new();
        let lines = blocks[0].render(80, 0, &mut md, false);
        let content = &lines[0].spans[0].content;
        assert!(content.contains("[思考]"), "should contain [思考]");

        // Expanded multi-line reasoning
        let multi = vec!["\x01[思考] line one\nline two\nline three".to_string()];
        let blocks_full = measure_blocks(&multi, 80, true);
        assert_eq!(blocks_full.len(), 1);
        assert_eq!(blocks_full[0].height, 3, "expanded reasoning height should match line count");
        let lines_full = blocks_full[0].render(80, 0, &mut md, true);
        assert_eq!(lines_full.len(), 3, "expanded reasoning should render 3 lines");
    }

    #[test]
    fn test_wrap_cjk() {
        let w1 = wrap_text_to_width("持久记忆与上下文", 16);
        println!("wrap(16): {:?}", w1);
        assert_eq!(w1.len(), 1, "should fit in 16 cols");
        let w2 = wrap_text_to_width("持久记忆与上下文", 12);
        println!("wrap(12): {:?}", w2);
        let w3 = wrap_text_to_width("持久记忆与上下文", 10);
        println!("wrap(10): {:?}", w3);
        let s = strip_markdown("持久记忆与上下文");
        println!("strip: '{}' width={}", s, s.width());
        let input = vec![
            "| 能力 | 说明 |".to_string(),
            "|------|------|".to_string(),
            "| 持久记忆与上下文 | 跨会话的记忆系统（核心/次要/短期三层） |".to_string(),
        ];
        let (rows, widths, _sep_idx) = measure_table(&input, 80);
        println!("rows[2][0]='{}' w={}", rows[2][0], rows[2][0].width());
        println!("widths: {:?}", widths);
        let fitted = fit_table_widths(&widths, 76);
        println!("fitted(76): {:?}", fitted);
    }

    #[test]
    fn test_help_content_blocks_not_eaten() {
        // Simulate the exact output produced by App::show_help()
        let output = vec![
            "  Commands:".to_string(),
            "  /help      Show this help".to_string(),
            "  /plan      Read-only mode".to_string(),
            "  /exec      Write mode".to_string(),
            "  /auto      Full auto mode".to_string(),
            "  /review    Self-review changes".to_string(),
            "  /tools     List available tools".to_string(),
            "  /settings  Show configuration".to_string(),
            "  /models    Model picker panel".to_string(),
            "  /model     Switch model".to_string(),
            "  /session   Save/load sessions".to_string(),
            "  /cod on/off Chain of Draft experimental".to_string(),
            "  /debug     Debug info".to_string(),
            "  /end       Jump to bottom".to_string(),
            "  /clear     Clear screen".to_string(),
            "  /exit      Quit".to_string(),
            "".to_string(),
            "  Keys:".to_string(),
            "  PgUp/PgDn  Scroll | Up/Down  History".to_string(),
            "  Ctrl+W     Del word | Shift+Enter  Newline".to_string(),
            "  End        Jump to bottom (empty input)".to_string(),
            "  Mouse drag Scroll | PgUp/PgDn Scroll".to_string(),
            "  Ctrl+C     Quit".to_string(),
            "".to_string(),
        ];

        let blocks = measure_blocks(&output, 80, false);
        let total_h: usize = blocks.iter().map(|b| b.height).sum();
        // For text-only output, total block height must equal raw line count
        assert_eq!(
            total_h,
            output.len(),
            "block height sum ({}) must equal output line count ({})",
            total_h,
            output.len()
        );

        // Every line must produce exactly one block (no accidental merging)
        assert_eq!(
            blocks.len(),
            output.len(),
            "each line should be its own block, got {} blocks for {} lines",
            blocks.len(),
            output.len()
        );

        // Simulate draw_output viewport logic (stick_to_bottom)
        let vis = 15usize; // small terminal
        let total = output.len();
        let start = total.saturating_sub(vis);
        let end = (start + vis).min(total);

        let mut rendered = 0usize;
        let mut line_offset = 0usize;
        for block in &blocks {
            let block_end = line_offset + block.height;
            if block_end > start && line_offset < end {
                let skip = if line_offset < start {
                    start - line_offset
                } else {
                    0
                };
                let take = vis.saturating_sub(rendered);
                let all_lines =
                    block.render(80, 0, &mut crate::markdown::MarkdownRenderer::new(), false);
                let block_start = skip.min(all_lines.len());
                let block_end_idx = (block_start + take).min(all_lines.len());
                let block_lines = all_lines[block_start..block_end_idx].to_vec();
                rendered += block_lines.len();
            }
            line_offset = block_end;
            if rendered >= vis {
                break;
            }
        }

        // We should have rendered exactly 'vis' lines (or all remaining if fewer)
        let expected = vis.min(total.saturating_sub(start));
        assert_eq!(
            rendered, expected,
            "viewport should render {} lines but got {}",
            expected, rendered
        );

        // Specifically: render every text block and make sure nothing disappears
        let mut md = crate::markdown::MarkdownRenderer::new();
        for (i, block) in blocks.iter().enumerate() {
            let lines = block.render(80, 0, &mut md, false);
            assert!(
                !lines.is_empty(),
                "block {} (source: {:?}) should render at least one line",
                i,
                block.source_lines
            );
        }
    }

    #[test]
    fn test_render_inline_pipe_preserves_text() {
        // Keys lines contain '|' — make sure pulldown-cmark doesn't eat them
        let spans = crate::markdown::render_inline("PgUp/PgDn  Scroll | Up/Down  History");
        let text: String = spans.iter().map(|s| s.to_string()).collect();
        assert!(
            text.contains("PgUp/PgDn"),
            "render_inline ate text before pipe: {}",
            text
        );
        assert!(
            text.contains("History"),
            "render_inline ate text after pipe: {}",
            text
        );
        assert!(
            text.contains("|"),
            "render_inline dropped pipe character: {}",
            text
        );

        let spans2 = crate::markdown::render_inline("Mouse drag Scroll | PgUp/PgDn Scroll");
        let text2: String = spans2.iter().map(|s| s.to_string()).collect();
        assert!(
            text2.contains("Mouse drag"),
            "render_inline ate 'Mouse drag': {}",
            text2
        );
        assert!(
            text2.contains("Scroll"),
            "render_inline ate 'Scroll': {}",
            text2
        );
    }

    #[test]
    fn test_strip_markdown() {
        assert_eq!(strip_markdown("**bold**"), "bold");
        assert_eq!(strip_markdown("*italic*"), "italic");
        assert_eq!(strip_markdown("`code`"), "code");
        assert_eq!(strip_markdown("**`nested`**"), "`nested`"); // bold wraps code
        assert_eq!(strip_markdown("plain"), "plain");
        assert_eq!(strip_markdown("✅ active"), "✅ active");
    }
}
