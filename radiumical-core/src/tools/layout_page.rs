//! Page layout DSL — compact notation for structured terminal output.
//!
//! # DSL syntax
//!
//! ## Grid
//! ```text
//! grid 2x3
//! Header A | Header B | Header C
//! Cell 1   | Cell 2   | Cell 3
//! ```
//!
//! ## Split (horizontal)
//! ```text
//! split 60 40
//! Left content
//! Right content
//! ```
//!
//! ## Rows (vertical stack)
//! ```text
//! rows
//! First line
//! ---
//! Second line
//! ```
//!
//! ## Box
//! ```text
//! box Title
//! Content goes here
//! ```
//!
//! ## Columns (side-by-side with separator)
//! ```text
//! cols 3
//! Column 1 text
//! |||
//! Column 2 text
//! |||
//! Column 3 text
//! ```
//!
//! ## Table (auto-width)
//! ```text
//! table
//! Name | Age | City
//! Alice | 30 | Beijing
//! Bob | 25 | Shanghai
//! ```

use std::fmt;

// ── Width helper (ASCII-only, avoids crate dependency) ──

fn char_width(ch: char) -> usize {
    // CJK and fullwidth chars count as 2, others as 1.
    let cp = ch as u32;
    if (0x1100..=0x115F).contains(&cp)
        || (0x2E80..=0x303E).contains(&cp)
        || (0x3040..=0x33BF).contains(&cp)
        || (0x3400..=0x4DBF).contains(&cp)
        || (0x4E00..=0x9FFF).contains(&cp)
        || (0xA000..=0xA4CF).contains(&cp)
        || (0xAC00..=0xD7AF).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0xFE30..=0xFE6F).contains(&cp)
        || (0xFF00..=0xFF60).contains(&cp)
        || (0xFFE0..=0xFFE6).contains(&cp)
        || (0x20000..=0x2FA1F).contains(&cp)
    {
        2
    } else {
        1
    }
}

fn str_display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

// ── Types ──

#[derive(Debug, Clone, PartialEq)]
pub enum Layout {
    /// Grid: rows × cols, cells in row-major order.
    Grid {
        rows: usize,
        cols: usize,
        cells: Vec<String>,
    },
    /// Horizontal split: widths as percentages (sum to 100).
    Split { widths: Vec<u8>, panes: Vec<String> },
    /// Vertical stack of blocks separated by `---`.
    Rows { blocks: Vec<String> },
    /// Side-by-side columns separated by `|||`.
    Cols { columns: Vec<String> },
    /// Bordered box with optional title.
    Box {
        title: Option<String>,
        content: String,
    },
    /// Table with header row + data rows.
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
}

// ── Parser ──

pub fn parse(input: &str) -> Result<Layout, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("empty layout spec".into());
    }

    let first_line = input.lines().next().unwrap_or("").trim();

    if first_line.starts_with("grid ") {
        parse_grid(input)
    } else if first_line.starts_with("split ") {
        parse_split(input)
    } else if first_line == "rows" {
        parse_rows(input)
    } else if first_line.starts_with("cols") {
        parse_cols(input)
    } else if first_line.starts_with("box") {
        parse_box(input)
    } else if first_line == "table" || first_line.starts_with("table ") {
        parse_table(input)
    } else {
        Err(format!("unknown layout directive: '{first_line}'"))
    }
}

fn parse_grid(input: &str) -> Result<Layout, String> {
    let first = input.lines().next().unwrap().trim();
    let dims = first.strip_prefix("grid").unwrap().trim();
    let (rows, cols) = parse_dims(dims)?;

    let cells: Vec<String> = input
        .lines()
        .skip(1)
        .flat_map(|line| {
            line.split('|')
                .map(|s| s.trim().to_string())
                .collect::<Vec<_>>()
        })
        .collect();

    if cells.len() < rows * cols {
        return Err(format!(
            "grid {rows}x{cols} needs {} cells, got {}",
            rows * cols,
            cells.len()
        ));
    }

    Ok(Layout::Grid {
        rows,
        cols,
        cells: cells.into_iter().take(rows * cols).collect(),
    })
}

fn parse_split(input: &str) -> Result<Layout, String> {
    let first = input.lines().next().unwrap().trim();
    let dims = first.strip_prefix("split").unwrap().trim();
    let widths: Vec<u8> = dims
        .split_whitespace()
        .map(|s| {
            s.parse::<u8>()
                .map_err(|e| format!("invalid width '{s}': {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let sum: u8 = widths.iter().sum();
    if sum != 100 {
        return Err(format!("split widths must sum to 100, got {sum}"));
    }

    let body = input.lines().skip(1).collect::<Vec<_>>().join("\n");
    let panes: Vec<String> = body.split("|||").map(|s| s.trim().to_string()).collect();

    if panes.len() != widths.len() {
        return Err(format!(
            "split needs {} panes (|||-separated), got {}",
            widths.len(),
            panes.len()
        ));
    }

    Ok(Layout::Split { widths, panes })
}

fn parse_rows(input: &str) -> Result<Layout, String> {
    let blocks: Vec<String> = input
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n")
        .split("\n---\n")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if blocks.is_empty() {
        return Err("rows needs at least one block (separated by ---)".into());
    }

    Ok(Layout::Rows { blocks })
}

fn parse_cols(input: &str) -> Result<Layout, String> {
    let first = input.lines().next().unwrap().trim();
    let n_str = first.strip_prefix("cols").unwrap().trim();
    let n: usize = if n_str.is_empty() {
        0
    } else {
        n_str
            .parse()
            .map_err(|e| format!("invalid col count '{n_str}': {e}"))?
    };

    let body = input.lines().skip(1).collect::<Vec<_>>().join("\n");
    let columns: Vec<String> = body.split("|||").map(|s| s.trim().to_string()).collect();

    let n = if n == 0 { columns.len() } else { n };
    if columns.len() != n {
        return Err(format!(
            "cols {n} needs {n} columns (|||-separated), got {}",
            columns.len()
        ));
    }

    Ok(Layout::Cols { columns })
}

fn parse_box(input: &str) -> Result<Layout, String> {
    let first = input.lines().next().unwrap().trim();
    let title_str = first.strip_prefix("box").unwrap().trim();
    let title = if title_str.is_empty() {
        None
    } else {
        Some(title_str.to_string())
    };
    let content = input
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    Ok(Layout::Box { title, content })
}

fn parse_table(input: &str) -> Result<Layout, String> {
    let data_lines: Vec<&str> = input
        .lines()
        .skip(1)
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && *l != "---" && !l.starts_with("---"))
        .collect();

    if data_lines.is_empty() {
        return Err("table needs at least a header row".into());
    }

    let headers: Vec<String> = data_lines[0]
        .split('|')
        .map(|s| s.trim().to_string())
        .collect();

    let rows: Vec<Vec<String>> = data_lines[1..]
        .iter()
        .map(|line| line.split('|').map(|s| s.trim().to_string()).collect())
        .collect();

    Ok(Layout::Table { headers, rows })
}

fn parse_dims(s: &str) -> Result<(usize, usize), String> {
    let parts: Vec<&str> = s.split('x').collect();
    if parts.len() != 2 {
        return Err(format!("expected NxM, got '{s}'"));
    }
    let r: usize = parts[0]
        .parse()
        .map_err(|e| format!("invalid rows '{}': {}", parts[0], e))?;
    let c: usize = parts[1]
        .parse()
        .map_err(|e| format!("invalid cols '{}': {}", parts[1], e))?;
    Ok((r, c))
}

// ── Renderer ──

pub fn render(layout: &Layout, width: usize) -> String {
    let width = width.max(10);
    match layout {
        Layout::Grid { rows, cols, cells } => render_grid(*rows, *cols, cells, width),
        Layout::Split { widths, panes } => render_split(widths, panes, width),
        Layout::Rows { blocks } => render_rows(blocks, width),
        Layout::Cols { columns } => render_cols(columns, width),
        Layout::Box { title, content } => render_box(title.as_deref(), content, width),
        Layout::Table { headers, rows } => render_table(headers, rows, width),
    }
}

fn render_grid(rows: usize, cols: usize, cells: &[String], width: usize) -> String {
    let col_w = width.saturating_sub(cols + 1) / cols.max(1);
    let mut out = String::new();

    for r in 0..rows {
        if r == 0 {
            out.push('┌');
            for c in 0..cols {
                if c > 0 {
                    out.push('┬');
                }
                out.push_str(&"─".repeat(col_w));
            }
            out.push_str("┐\n");
        }

        // Content row
        out.push('│');
        for c in 0..cols {
            if c > 0 {
                out.push('│');
            }
            let idx = r * cols + c;
            let text = cells.get(idx).map(|s| s.as_str()).unwrap_or("");
            let display_w = str_display_width(text);
            let pad = col_w.saturating_sub(display_w);
            out.push_str(text);
            out.push_str(&" ".repeat(pad));
        }
        out.push_str("│\n");

        // Separator or bottom
        if r < rows - 1 {
            out.push('├');
            for c in 0..cols {
                if c > 0 {
                    out.push('┼');
                }
                out.push_str(&"─".repeat(col_w));
            }
            out.push_str("┤\n");
        } else {
            out.push('└');
            for c in 0..cols {
                if c > 0 {
                    out.push('┴');
                }
                out.push_str(&"─".repeat(col_w));
            }
            out.push_str("┘\n");
        }
    }

    out
}

fn render_split(widths: &[u8], panes: &[String], width: usize) -> String {
    let total: u8 = widths.iter().sum();
    let pane_widths: Vec<usize> = widths
        .iter()
        .map(|w| {
            let pw = (*w as f64 / total as f64 * width as f64) as usize;
            pw.max(1)
        })
        .collect();

    // Wrap each pane
    let wrapped_panes: Vec<Vec<String>> = panes
        .iter()
        .zip(&pane_widths)
        .map(|(pane, &pw)| wrap_lines(pane, pw))
        .collect();

    let max_lines = wrapped_panes.iter().map(|p| p.len()).max().unwrap_or(0);
    let mut out = String::new();

    for i in 0..max_lines {
        for (pi, pane) in wrapped_panes.iter().enumerate() {
            if pi > 0 {
                out.push_str(" │ ");
            }
            let line = pane.get(i).map(|s| s.as_str()).unwrap_or("");
            let display_w = str_display_width(line);
            let pw = pane_widths[pi];
            let pad = pw.saturating_sub(display_w);
            out.push_str(line);
            out.push_str(&" ".repeat(pad));
        }
        out.push('\n');
    }

    out
}

fn render_rows(blocks: &[String], width: usize) -> String {
    let mut out = String::new();
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            out.push_str(&format!("  {}\n", "─".repeat(width.saturating_sub(2))));
        }
        for line in block.lines() {
            out.push_str(&format!("  {line}\n"));
        }
    }
    out
}

fn render_cols(columns: &[String], width: usize) -> String {
    let col_w = width.saturating_sub(columns.len() * 3) / columns.len().max(1);
    let wrapped: Vec<Vec<String>> = columns.iter().map(|c| wrap_lines(c, col_w)).collect();
    let max_lines = wrapped.iter().map(|p| p.len()).max().unwrap_or(0);
    let mut out = String::new();

    for i in 0..max_lines {
        for (ci, col) in wrapped.iter().enumerate() {
            if ci > 0 {
                out.push_str(" │ ");
            }
            let line = col.get(i).map(|s| s.as_str()).unwrap_or("");
            let display_w = str_display_width(line);
            let pad = col_w.saturating_sub(display_w);
            out.push_str(line);
            out.push_str(&" ".repeat(pad));
        }
        out.push('\n');
    }

    out
}

fn render_box(title: Option<&str>, content: &str, width: usize) -> String {
    let inner_w = width.saturating_sub(4);
    let mut out = String::new();

    // Top border
    if let Some(t) = title {
        let t_display = truncate_to_width(t, inner_w.saturating_sub(2));
        let fill = inner_w.saturating_sub(t_display.len() + 2);
        out.push_str(&format!("  ┌─ {t_display} {}┐\n", "─".repeat(fill)));
    } else {
        out.push_str(&format!("  ┌{}┐\n", "─".repeat(inner_w)));
    }

    // Content
    for line in content.lines() {
        for wrapped in wrap_lines(line, inner_w) {
            let display_w = str_display_width(&wrapped);
            let pad = inner_w.saturating_sub(display_w);
            out.push_str(&format!("  │ {wrapped}{}│\n", " ".repeat(pad)));
        }
    }

    // Bottom border
    out.push_str(&format!("  └{}┘\n", "─".repeat(inner_w)));
    out
}

fn render_table(headers: &[String], rows: &[Vec<String>], width: usize) -> String {
    let cols = headers.len().max(1);
    let col_w = width.saturating_sub(cols + 1) / cols;

    let mut out = String::new();

    // Header
    out.push('│');
    for (i, h) in headers.iter().enumerate() {
        if i > 0 {
            out.push('│');
        }
        let t = truncate_to_width(h, col_w);
        let pad = col_w.saturating_sub(str_display_width(&t));
        out.push_str(&t);
        out.push_str(&" ".repeat(pad));
    }
    out.push_str("│\n");

    // Separator
    out.push('├');
    for i in 0..cols {
        if i > 0 {
            out.push('┼');
        }
        out.push_str(&"─".repeat(col_w));
    }
    out.push_str("┤\n");

    // Data rows
    for row in rows {
        out.push('│');
        for i in 0..cols {
            if i > 0 {
                out.push('│');
            }
            let text = row.get(i).map(|s| s.as_str()).unwrap_or("");
            let t = truncate_to_width(text, col_w);
            let pad = col_w.saturating_sub(str_display_width(&t));
            out.push_str(&t);
            out.push_str(&" ".repeat(pad));
        }
        out.push_str("│\n");
    }

    out
}

// ── Helpers ──

fn wrap_lines(text: &str, max_w: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for line in text.split('\n') {
        if str_display_width(line) <= max_w {
            lines.push(line.to_string());
        } else {
            let mut buf = String::new();
            let mut w = 0;
            for ch in line.chars() {
                let cw = char_width(ch);
                if w + cw > max_w && !buf.is_empty() {
                    lines.push(buf.clone());
                    buf.clear();
                    w = 0;
                }
                buf.push(ch);
                w += cw;
            }
            if !buf.is_empty() {
                lines.push(buf);
            }
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn truncate_to_width(s: &str, max_w: usize) -> String {
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = char_width(ch);
        if w + cw > max_w.saturating_sub(1) {
            out.push('…');
            break;
        }
        out.push(ch);
        w += cw;
    }
    out
}

impl fmt::Display for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", render(self, 80))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_grid() {
        let spec = "grid 2x3\nA | B | C\nD | E | F";
        let layout = parse(spec).unwrap();
        match layout {
            Layout::Grid { rows, cols, cells } => {
                assert_eq!(rows, 2);
                assert_eq!(cols, 3);
                assert_eq!(cells.len(), 6);
                assert_eq!(cells[0], "A");
                assert_eq!(cells[5], "F");
            }
            _ => panic!("expected Grid"),
        }
    }

    #[test]
    fn test_parse_table() {
        let spec = "table\nName | Age\nAlice | 30\nBob | 25";
        let layout = parse(spec).unwrap();
        match layout {
            Layout::Table { headers, rows } => {
                assert_eq!(headers.len(), 2);
                assert_eq!(rows.len(), 2);
                assert_eq!(headers[0], "Name");
                assert_eq!(rows[0][1], "30");
            }
            _ => panic!("expected Table"),
        }
    }

    #[test]
    fn test_parse_box() {
        let spec = "box My Title\nHello world";
        let layout = parse(spec).unwrap();
        match layout {
            Layout::Box { title, content } => {
                assert_eq!(title.as_deref(), Some("My Title"));
                assert_eq!(content, "Hello world");
            }
            _ => panic!("expected Box"),
        }
    }

    #[test]
    fn test_parse_split() {
        let spec = "split 60 40\nLeft content\n|||\nRight content";
        let layout = parse(spec).unwrap();
        match layout {
            Layout::Split { widths, panes } => {
                assert_eq!(widths, vec![60, 40]);
                assert_eq!(panes.len(), 2);
            }
            _ => panic!("expected Split"),
        }
    }

    #[test]
    fn test_parse_rows() {
        let spec = "rows\nFirst\n---\nSecond";
        let layout = parse(spec).unwrap();
        match layout {
            Layout::Rows { blocks } => {
                assert_eq!(blocks.len(), 2);
                assert_eq!(blocks[0], "First");
                assert_eq!(blocks[1], "Second");
            }
            _ => panic!("expected Rows"),
        }
    }

    #[test]
    fn test_parse_cols() {
        let spec = "cols 2\nLeft\n|||\nRight";
        let layout = parse(spec).unwrap();
        match layout {
            Layout::Cols { columns } => {
                assert_eq!(columns.len(), 2);
            }
            _ => panic!("expected Cols"),
        }
    }

    #[test]
    fn test_render_grid() {
        let layout = Layout::Grid {
            rows: 2,
            cols: 2,
            cells: vec!["A".into(), "B".into(), "C".into(), "D".into()],
        };
        let output = render(&layout, 20);
        assert!(output.contains('A'));
        assert!(output.contains('D'));
        assert!(output.contains('┌'));
        assert!(output.contains('└'));
    }

    #[test]
    fn test_render_box() {
        let layout = Layout::Box {
            title: Some("Test".into()),
            content: "Hello\nWorld".into(),
        };
        let output = render(&layout, 40);
        assert!(output.contains("Test"));
        assert!(output.contains("Hello"));
        assert!(output.contains("World"));
    }

    #[test]
    fn test_unknown_directive() {
        let result = parse("foobar 123");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown"));
    }
}
