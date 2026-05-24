//! Two-pass output layout: measure → allocate → render.
//! Avoids per-line positioning drift — every block gets measured first,
//! then positioned with pre-calculated coordinates.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

const DIM: Color = Color::Rgb(100, 100, 110);
const BORDER: Color = Color::Rgb(80, 80, 90);

// ── Block types ──

#[derive(Debug, Clone)]
pub enum BlockKind {
    /// ASCII art logo (detected by █ characters)
    Logo,
    /// Code fence block
    CodeFence { lang: String },
    /// Table (buffered, fully measured)
    Table { rows: Vec<Vec<String>>, widths: Vec<usize> },
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

pub fn measure_blocks(output: &[String]) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut i = 0;

    while i < output.len() {
        let line = &output[i];
        let trimmed = line.trim();

        // Logo block: first line has █, include subsequent long lines (╚═╝, etc.)
        if line.contains('█') && line.len() > 30 {
            let start = i;
            i += 1;
            while i < output.len() && output[i].len() > 30
                && (output[i].contains('█') || output[i].contains('╚') || output[i].contains('╔'))
            {
                i += 1;
            }
            let source = output[start..i].to_vec();
            let w = source.iter().map(|s| s.chars().count()).max().unwrap_or(0);
            blocks.push(Block { kind: BlockKind::Logo, source_lines: source, width: w, height: i - start });
            continue;
        }

        // Code fence open
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let lang = trimmed.trim_start_matches('`').trim_start_matches('~').trim().to_string();
            let start = i;
            i += 1;
            while i < output.len() {
                let t = output[i].trim();
                if t.starts_with("```") || t.starts_with("~~~") { i += 1; break; }
                i += 1;
            }
            let source = output[start..i].to_vec();
            let w = source.iter().map(|s| s.len()).max().unwrap_or(0);
            blocks.push(Block { kind: BlockKind::CodeFence { lang }, source_lines: source, width: w, height: i - start });
            continue;
        }

        // Table
        if trimmed.starts_with('|') {
            let start = i;
            while i < output.len() && output[i].trim().starts_with('|') { i += 1; }
            let source = output[start..i].to_vec();
            let (rows, widths) = measure_table(&source);
            let total_w = widths.iter().sum::<usize>() + widths.len() * 3 + 1;
            blocks.push(Block { kind: BlockKind::Table { rows, widths }, source_lines: source, width: total_w, height: i - start + 2 });
            continue;
        }

        // Reasoning
        if line.starts_with("\x01") {
            blocks.push(Block { kind: BlockKind::Reasoning, source_lines: vec![line.clone()], width: line.len(), height: 1 });
            i += 1;
            continue;
        }

        // Heading
        if let Some(rest) = trimmed.strip_prefix('#') {
            let level = rest.chars().take_while(|c| *c == '#').count() + 1;
            if level <= 6 && rest.as_bytes().get(level - 1) == Some(&b' ') {
                let w = rest[level..].chars().count();
                blocks.push(Block { kind: BlockKind::Heading { level }, source_lines: vec![line.clone()], width: w, height: 1 });
                i += 1;
                continue;
            }
        }

        // Blockquote
        if let Some(rest) = trimmed.strip_prefix("> ") {
            let w = rest.chars().count() + 2;
            blocks.push(Block { kind: BlockKind::Blockquote, source_lines: vec![line.clone()], width: w, height: 1 });
            i += 1;
            continue;
        }

        // Unordered list
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            let w = trimmed[2..].chars().count() + 4;
            blocks.push(Block { kind: BlockKind::ListItem, source_lines: vec![line.clone()], width: w, height: 1 });
            i += 1;
            continue;
        }

        // Ordered list
        if let Some(rest) = trimmed.strip_prefix(|c: char| c.is_ascii_digit()) {
            if rest.starts_with(". ") || rest.starts_with(") ") {
                let num_end = trimmed.find(rest).unwrap_or(0);
                let num = trimmed[..num_end].to_string();
                let w = rest[2..].chars().count() + num.len() + 4;
                blocks.push(Block { kind: BlockKind::OrderedItem { num }, source_lines: vec![line.clone()], width: w, height: 1 });
                i += 1;
                continue;
            }
        }

        // Regular text (including blank lines — must preserve spacing)
        let w = if trimmed.is_empty() { 0 } else { trimmed.len() };
        blocks.push(Block { kind: BlockKind::Text, source_lines: vec![line.clone()], width: w, height: 1 });
        i += 1;
    }

    blocks
}

fn measure_table(source: &[String]) -> (Vec<Vec<String>>, Vec<usize>) {
    let rows: Vec<Vec<String>> = source.iter()
        .map(|line| line.trim().trim_matches('|').split('|').map(|c| c.trim().to_string()).collect())
        .collect();
    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(1);
    let mut widths = vec![3; col_count];
    for row in &rows {
        for (ci, cell) in row.iter().enumerate() {
            if ci < col_count {
                let stripped = strip_markdown(cell);
                let w = stripped.chars().count().max(3).min(50);
                widths[ci] = widths[ci].max(w);
            }
        }
    }
    (rows, widths)
}

fn strip_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_md_pair(&chars, i + 2, "**") {
                out.push_str(&chars[i + 2..end].iter().collect::<String>());
                i = end + 2; continue;
            }
        }
        if chars[i] == '*' && (i == 0 || chars[i - 1] != '*') {
            if let Some(end) = find_md_single(&chars, i + 1, '*') {
                out.push_str(&chars[i + 1..end].iter().collect::<String>());
                i = end + 1; continue;
            }
        }
        if chars[i] == '`' {
            if let Some(end) = find_md_single(&chars, i + 1, '`') {
                out.push_str(&chars[i + 1..end].iter().collect::<String>());
                i = end + 1; continue;
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
        if chars[i] == d[0] && chars[i + 1] == d[1] { return Some(i); }
    }
    None
}

fn find_md_single(chars: &[char], start: usize, d: char) -> Option<usize> {
    chars[start..].iter().position(|&c| c == d).map(|p| start + p)
}

// ── Pass 2: render blocks ──

impl Block {
    pub fn render(&self, _area_width: u16, _frame: usize, _markdown: &mut crate::markdown::MarkdownRenderer, show_full: bool) -> Vec<Line<'static>> {
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
                self.source_lines.iter().enumerate().map(|(i, s)| {
                    Line::from(Span::styled(s.clone(), Style::default().fg(breathe(i))))
                }).collect()
            }

            BlockKind::CodeFence { lang } => {
                let label = if lang.is_empty() { "─".into() } else { format!(" {lang} ") };
                let mut lines: Vec<Line> = vec![Line::from(Span::styled(label, Style::default().fg(DIM)))];
                let code: String = self.source_lines[1..self.source_lines.len()-1].join("\n");
                let highlighted = crate::highlight::highlight_code(&code, lang);
                for line in highlighted.lines() {
                    lines.push(Line::from(Span::styled(line.to_string(), Style::default().fg(Color::Rgb(180, 180, 190)))));
                }
                lines.push(Line::from(Span::styled("─".to_string(), Style::default().fg(DIM))));
                lines
            }

            BlockKind::Table { rows, widths } => {
                let mut lines = Vec::new();
                lines.push(border_line("┌", "┬", "┐", "─", widths));
                let sep_idx = rows.iter().position(|r| r.iter().all(|c| c.is_empty())).unwrap_or(1).min(1);
                for (i, row) in rows.iter().enumerate() {
                    if i == sep_idx && i > 0 {
                        lines.push(border_line("├", "┼", "┤", "─", widths));
                    }
                    lines.push(data_line(row, widths));
                }
                lines.push(border_line("└", "┴", "┘", "─", widths));
                lines
            }

            BlockKind::Heading { level } => {
                let s = self.source_lines[0].trim();
                let rest = s.strip_prefix(&"#".repeat(*level)).unwrap_or(s);
                let rest = rest.strip_prefix(' ').unwrap_or(rest);
                let color = if *level <= 2 { Color::Cyan } else { Color::Blue };
                vec![Line::from(Span::styled(rest.to_string(), Style::default().fg(color).add_modifier(Modifier::BOLD)))]
            }

            BlockKind::ListItem => {
                let s = self.source_lines[0].trim();
                let rest = &s[2..];
                let mut spans = vec![Span::styled("  • ", Style::default().fg(Color::Green))];
                spans.extend(crate::markdown::render_inline(rest));
                vec![Line::from(spans)]
            }

            BlockKind::OrderedItem { num } => {
                let s = self.source_lines[0].trim();
                let rest = s.strip_prefix(&format!("{num}. ")).or_else(|| s.strip_prefix(&format!("{num}) "))).unwrap_or("");
                let mut spans = vec![Span::styled(format!("  {num}. "), Style::default().fg(Color::Green))];
                spans.extend(crate::markdown::render_inline(rest));
                vec![Line::from(spans)]
            }

            BlockKind::Blockquote => {
                let s = self.source_lines[0].trim();
                let rest = &s[2..];
                let mut spans = vec![Span::styled("│ ", Style::default().fg(DIM))];
                spans.extend(crate::markdown::render_inline(rest));
                vec![Line::from(spans)]
            }

            BlockKind::Reasoning => {
                let s = &self.source_lines[0];
                let raw = s[1..].trim_start_matches("[思考] ").trim();
                let display = if show_full { format!("[思考] {raw}") } else { let preview: String = raw.chars().take(40).collect(); let dots = if raw.chars().count() > 40 { "…" } else { "" }; format!("[思考] {preview}{dots}") };
                vec![Line::from(Span::styled(display, Style::default().fg(Color::Rgb(170, 170, 180)).bg(Color::Rgb(35, 35, 42))))]
            }

            BlockKind::Text => {
                let s = self.source_lines[0].trim();
                if s.is_empty() { return vec![Line::from("")]; }
                // Diff highlighting
                if let Some(rest) = s.strip_prefix("+ ") {
                    return vec![Line::from(Span::styled(format!("+ {rest}"), Style::default().fg(Color::Green)))];
                }
                if let Some(rest) = s.strip_prefix("- ") {
                    return vec![Line::from(Span::styled(format!("- {rest}"), Style::default().fg(Color::Red)))];
                }
                // Truncate read_file output (lines matching "  NNN | ..."), Ctrl+O to expand
                if !show_full && s.len() > 40 && s.get(7..9) == Some("| ") && s[..6].chars().all(|c| c == ' ' || c.is_ascii_digit()) {
                    let preview: String = s.chars().take(40).collect();
                    return vec![Line::from(Span::styled(format!("{preview}…"), Style::default().fg(Color::DarkGray)))];
                }
                let spans = crate::markdown::render_inline(s);
                if spans.is_empty() { vec![Line::from("")] } else { vec![Line::from(spans)] }
            }
        }
    }
}

fn border_line(left: &str, mid: &str, right: &str, fill: &str, cols: &[usize]) -> Line<'static> {
    let mut s = String::from(left);
    for (i, &w) in cols.iter().enumerate() {
        s.push_str(&fill.repeat(w + 2));
        if i < cols.len() - 1 { s.push_str(mid); }
    }
    s.push_str(right);
    Line::from(Span::styled(format!("  {s}"), Style::default().fg(BORDER)))
}

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
                let text: String = cs.content.as_ref().chars().take(remaining.saturating_sub(1)).collect();
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
        if i < cols.len() - 1 { spans.push(edge.clone()); }
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

        let blocks = measure_blocks(&input);
        let mut md = MarkdownRenderer::new();

        // Verify block kinds
        assert_eq!(blocks.len(), 6, "should have 6 blocks (heading, blank, text, blockquote, list, ordered)");
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

        let blocks = measure_blocks(&input);
        assert_eq!(blocks.len(), 1);
        let block = &blocks[0];

        match &block.kind {
            BlockKind::Table { rows, widths } => {
                assert_eq!(rows.len(), 4, "4 table rows");
                assert_eq!(widths.len(), 3, "3 columns");
                // Check that widths account for markdown stripping
                assert!(widths[0] >= 5, "Name column should be >= width 5 (Alice)");
                assert!(widths[1] >= 4, "Role column should be >= width 4 (Viewer stripped)");
            }
            _ => panic!("expected Table block"),
        }

        let mut md = MarkdownRenderer::new();
        let lines = block.render(80, 0, &mut md, false);
        // Should have: top border + header + sep border + 2 data rows + bottom border = 7 lines
        assert_eq!(lines.len(), 7, "table should render 7 lines (borders + header + sep + 2 data)");

        // Verify border characters
        let top = &lines[0].spans[0].content;
        assert!(top.contains('┌'), "top border should start with ┌, got: {top}");
        let bottom = &lines[6].spans[0].content;
        assert!(bottom.contains('└'), "bottom border should start with └, got: {bottom}");

        // Verify data is present (index 4 = Alice row, index 3 is separator dashes)
        let alice_line = &lines[4].spans.iter().map(|s| &*s.content).collect::<String>();
        assert!(alice_line.contains("Alice"), "should contain Alice");
        assert!(alice_line.contains("Admin"), "should contain Admin (bold stripped in border but rendered in data)");
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

        let blocks = measure_blocks(&input);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0].kind, BlockKind::CodeFence { .. }));

        let mut md = MarkdownRenderer::new();
        let lines = blocks[0].render(80, 0, &mut md, false);
        // label + 3 code lines + closing = 5
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn test_logo_block_detected() {
        let input = vec![
            "██████╗  █████╗ ██████╗ ██╗██╗   ██╗███╗   ███╗██╗ ██████╗ █████╗ ██╗     ".to_string(),
            "██╔══██╗██╔══██╗██╔══██╗██║██║   ██║████╗ ████║██║██╔════╝██╔══██╗██║     ".to_string(),
        ];

        let blocks = measure_blocks(&input);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0].kind, BlockKind::Logo));
    }

    #[test]
    fn test_reasoning_block() {
        let input = vec![
            "\x01[思考] Analyzing code structure...".to_string(),
        ];

        let blocks = measure_blocks(&input);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0].kind, BlockKind::Reasoning));

        let mut md = MarkdownRenderer::new();
        let lines = blocks[0].render(80, 0, &mut md, false);
        let content = &lines[0].spans[0].content;
        assert!(content.contains("[思考]"), "should contain [思考]");
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
