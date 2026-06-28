//! Two-pass output layout: measure → allocate → render.
//! Avoids per-line positioning drift — every block gets measured first,
//! then positioned with pre-calculated coordinates.

use unicode_width::UnicodeWidthStr;

mod render;
mod text;
mod tool;

#[allow(unused_imports)]
pub use text::{fit_table_widths, strip_markdown, wrap_text_to_width};
pub use tool::wrapped_tool_result_lines;

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
    /// Tool call box (collapsible, result scrollable)
    ToolCall {
        name: String,
        args: String,
        result: String,
        expanded: bool,
        result_scroll: usize,
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

        // Logo block: first line has █ and looks like ASCII art.
        // Reject lines that contain XML-like tags (e.g. <environment_details>)
        // to avoid treating structured metadata as the logo.
        if line.contains('█')
            && line.len() > 30
            && !line.contains('<')
            && !line.contains('>')
        {
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
            let adjusted_widths = text::fit_table_widths(&widths, avail_width);
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
                    let stripped = text::strip_markdown(cell);
                    let wrapped = text::wrap_text_to_width(&stripped, col_w);
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
                .trim_end_matches('┐')
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
                    result_scroll: 0,
                },
                source_lines: source,
                width: 0,
                height: 3,
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
                let stripped = text::strip_markdown(cell);
                let w = stripped.width().max(3);
                widths[ci] = widths[ci].max(w);
            }
        }
    }
    (rows, widths, sep_idx)
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::MarkdownRenderer;

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
}
