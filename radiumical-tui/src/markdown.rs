//! Markdown → ratatui Lines converter using pulldown-cmark.
//! Table rendering handled by tui.rs (full-buffer pre-scan).
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use std::collections::HashMap;

const DIM: Color = Color::Rgb(100, 100, 110);

const INLINE_CACHE_CAP: usize = 512;

pub struct MarkdownRenderer {
    pub frame: usize,
    inline_cache: HashMap<String, Vec<Span<'static>>>,
    inline_cache_order: Vec<String>,
}

impl MarkdownRenderer {
    pub fn new() -> Self {
        Self {
            frame: 0,
            inline_cache: HashMap::new(),
            inline_cache_order: Vec::new(),
        }
    }
    pub fn tick_frame(&mut self) {
        self.frame += 1;
    }

    pub fn render_inline_cached(&mut self, text: &str) -> Vec<Span<'static>> {
        if let Some(spans) = self.inline_cache.get(text) {
            return spans.clone();
        }
        let spans = render_inline(text);
        if self.inline_cache_order.len() >= INLINE_CACHE_CAP {
            if let Some(oldest) = self.inline_cache_order.first().cloned() {
                self.inline_cache.remove(&oldest);
            }
            self.inline_cache_order.remove(0);
        }
        self.inline_cache_order.push(text.to_string());
        self.inline_cache.insert(text.to_string(), spans.clone());
        spans
    }
}

// ── pulldown-cmark inline renderer (public for table cells) ──

pub fn render_inline(text: &str) -> Vec<Span<'static>> {
    if text.is_empty() {
        return vec![];
    }
    // First, handle ANSI and HTML color spans that pulldown-cmark ignores.
    if let Some(spans) = try_parse_color_spans(text) {
        return spans;
    }

    let parser = Parser::new_ext(text, Options::all());
    let mut spans = Vec::new();
    let mut style = Style::default();
    let mut link_url = String::new();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Emphasis => style = style.add_modifier(Modifier::ITALIC),
                Tag::Strong => style = style.add_modifier(Modifier::BOLD),
                Tag::CodeBlock(_) => style = style.fg(Color::Yellow),
                Tag::Link { dest_url, .. } => {
                    link_url = dest_url.to_string();
                    style = style.fg(Color::Cyan);
                }
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Emphasis => style = style.remove_modifier(Modifier::ITALIC),
                TagEnd::Strong => style = style.remove_modifier(Modifier::BOLD),
                TagEnd::CodeBlock => style = style.fg(Color::Reset),
                TagEnd::Link => {
                    if !link_url.is_empty() {
                        spans.push(Span::styled(
                            format!(" ({link_url})"),
                            Style::default().fg(DIM),
                        ));
                        link_url.clear();
                    }
                    style = style.fg(Color::Reset);
                }
                _ => {}
            },
            Event::Text(text) => spans.push(Span::styled(text.to_string(), style)),
            Event::Code(text) => spans.push(Span::styled(
                format!("`{text}`"),
                Style::default().fg(Color::Yellow),
            )),
            Event::SoftBreak | Event::HardBreak => spans.push(Span::raw(" ")),
            _ => {}
        }
    }
    spans
}

// ── ANSI / HTML color helpers ──

/// If the text contains ANSI SGR codes or simple HTML color tags, parse it
/// directly into styled spans instead of going through pulldown-cmark.
fn try_parse_color_spans(text: &str) -> Option<Vec<Span<'static>>> {
    if text.contains('\x1b') || text.contains("<span") || text.contains("<font") {
        Some(parse_ansi_and_html(text))
    } else {
        None
    }
}

/// Parse a string that may contain ANSI SGR escapes and/or HTML color spans.
fn parse_ansi_and_html(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut style = Style::default();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if !current.is_empty() {
                spans.push(Span::styled(current.clone(), style));
                current.clear();
            }
            // Parse ANSI escape sequence: ESC [ ... m
            if chars.next_if_eq(&'[').is_some() {
                let mut seq = String::new();
                while let Some(&next) = chars.peek() {
                    if next == 'm' {
                        chars.next();
                        break;
                    }
                    seq.push(next);
                    chars.next();
                }
                style = apply_sgr(style, &seq);
            } else {
                // Unsupported escape, skip one more char if any.
                chars.next();
            }
            continue;
        }

        if ch == '<' {
            // Try to parse <span style="color: #rrggbb"> ... </span>
            // or <font color="..."> ... </font>
            let rest: String = chars.clone().collect();
            if let Some((tag_len, color)) = parse_html_color_open(&rest) {
                if !current.is_empty() {
                    spans.push(Span::styled(current.clone(), style));
                    current.clear();
                }
                style = style.fg(color);
                // Consume the opening tag.
                for _ in 0..tag_len {
                    chars.next();
                }
                continue;
            }
            if rest.starts_with("/span>") || rest.starts_with("/font>") {
                if !current.is_empty() {
                    spans.push(Span::styled(current.clone(), style));
                    current.clear();
                }
                style = style.fg(Color::Reset);
                let close_len = if rest.starts_with("/span>") { 6 } else { 7 };
                for _ in 0..close_len {
                    chars.next();
                }
                continue;
            }
        }

        current.push(ch);
    }

    if !current.is_empty() {
        spans.push(Span::styled(current, style));
    }

    spans
}

/// Parse an opening HTML color tag and return (tag_length, color).
fn parse_html_color_open(rest: &str) -> Option<(usize, Color)> {
    let rest_lower = rest.to_lowercase();

    // <span style="color: #rrggbb">
    if rest_lower.starts_with("span style=") {
        let close = rest.find('>')?;
        let inside = &rest["span style=".len()..close];
        let quote = inside.chars().next()?;
        let inside = &inside[1..];
        let value_end = inside.find(quote)?;
        let value = &inside[..value_end];
        if let Some(color_pos) = value.to_lowercase().find("color") {
            let after = value.as_bytes().get(color_pos + 5..)?;
            let after_str = std::str::from_utf8(after).ok()?;
            let color_str = after_str.trim_start_matches(':').trim_start();
            let color = parse_color(color_str)?;
            return Some((close + 1, color));
        }
    }

    // <font color="...">
    if rest_lower.starts_with("font color=") {
        let close = rest.find('>')?;
        let inside = &rest["font color=".len()..close];
        let quote = inside.chars().next()?;
        let inside = &inside[1..];
        let value_end = inside.find(quote)?;
        let color_str = &inside[..value_end];
        let color = parse_color(color_str.trim())?;
        return Some((close + 1, color));
    }

    None
}

/// Parse a color string into ratatui Color.
fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("red") {
        return Some(Color::Red);
    }
    if s.eq_ignore_ascii_case("green") {
        return Some(Color::Green);
    }
    if s.eq_ignore_ascii_case("blue") {
        return Some(Color::Blue);
    }
    if s.eq_ignore_ascii_case("yellow") {
        return Some(Color::Yellow);
    }
    if s.eq_ignore_ascii_case("cyan") {
        return Some(Color::Cyan);
    }
    if s.eq_ignore_ascii_case("magenta") || s.eq_ignore_ascii_case("purple") {
        return Some(Color::Magenta);
    }
    if s.eq_ignore_ascii_case("white") {
        return Some(Color::White);
    }
    if s.eq_ignore_ascii_case("black") {
        return Some(Color::Black);
    }
    if s.eq_ignore_ascii_case("gray") || s.eq_ignore_ascii_case("grey") {
        return Some(Color::Gray);
    }

    // Hex: #rrggbb or #rgb
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }

    // rgb(r,g,b)
    if s.starts_with("rgb(") && s.ends_with(')') {
        let inner = &s[4..s.len() - 1];
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
        if parts.len() == 3 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                parts[0].parse::<u8>(),
                parts[1].parse::<u8>(),
                parts[2].parse::<u8>(),
            ) {
                return Some(Color::Rgb(r, g, b));
            }
        }
    }

    None
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    let hex = hex.trim();
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        return Some(Color::Rgb(r, g, b));
    }
    if hex.len() == 3 {
        let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
        let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
        let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
        return Some(Color::Rgb(r, g, b));
    }
    None
}

/// Apply an ANSI SGR sequence to a base style.
fn apply_sgr(mut style: Style, seq: &str) -> Style {
    let codes: Vec<u8> = seq
        .split(';')
        .filter_map(|p| p.parse().ok())
        .collect();
    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            22 => style = style.remove_modifier(Modifier::BOLD),
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            30 => style = style.fg(Color::Black),
            31 => style = style.fg(Color::Red),
            32 => style = style.fg(Color::Green),
            33 => style = style.fg(Color::Yellow),
            34 => style = style.fg(Color::Blue),
            35 => style = style.fg(Color::Magenta),
            36 => style = style.fg(Color::Cyan),
            37 => style = style.fg(Color::Gray),
            39 => style = style.fg(Color::Reset),
            40 => style = style.bg(Color::Black),
            41 => style = style.bg(Color::Red),
            42 => style = style.bg(Color::Green),
            43 => style = style.bg(Color::Yellow),
            44 => style = style.bg(Color::Blue),
            45 => style = style.bg(Color::Magenta),
            46 => style = style.bg(Color::Cyan),
            47 => style = style.bg(Color::Gray),
            49 => style = style.bg(Color::Reset),
            90 => style = style.fg(Color::DarkGray),
            91 => style = style.fg(Color::LightRed),
            92 => style = style.fg(Color::LightGreen),
            93 => style = style.fg(Color::LightYellow),
            94 => style = style.fg(Color::LightBlue),
            95 => style = style.fg(Color::LightMagenta),
            96 => style = style.fg(Color::LightCyan),
            97 => style = style.fg(Color::White),
            38 => {
                // Foreground truecolor
                if i + 4 < codes.len() && codes[i + 1] == 2 {
                    style = style.fg(Color::Rgb(codes[i + 2], codes[i + 3], codes[i + 4]));
                    i += 4;
                }
            }
            48 => {
                // Background truecolor
                if i + 4 < codes.len() && codes[i + 1] == 2 {
                    style = style.bg(Color::Rgb(codes[i + 2], codes[i + 3], codes[i + 4]));
                    i += 4;
                }
            }
            _ => {}
        }
        i += 1;
    }
    style
}

/// Render a hex color sample inline if the text is exactly a hex color.
pub fn maybe_color_sample(text: &str) -> Option<Span<'static>> {
    let trimmed = text.trim();
    if let Some(color) = parse_color(trimmed) {
        return Some(Span::styled(
            format!("■ {trimmed}"),
            Style::default().fg(color),
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bold() {
        let mut md = MarkdownRenderer::new();
        let spans = md.render_inline_cached("hello **world**!");
        assert_eq!(spans.len(), 3);
        assert!(spans[1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_code() {
        let mut md = MarkdownRenderer::new();
        let spans = md.render_inline_cached("use `foo` bar");
        let text: String = spans.iter().map(|s| s.to_string()).collect();
        assert!(text.contains("`foo`"));
    }

    #[test]
    fn test_link() {
        let mut md = MarkdownRenderer::new();
        let spans = md.render_inline_cached("[text](url)");
        let text: String = spans.iter().map(|s| s.to_string()).collect();
        assert!(text.contains("text"));
    }

    #[test]
    fn test_nested() {
        let mut md = MarkdownRenderer::new();
        let spans = md.render_inline_cached("**bold *italic***");
        assert!(spans.len() > 1);
    }

    #[test]
    fn test_inline_cache_reuses() {
        let mut md = MarkdownRenderer::new();
        let a = md.render_inline_cached("hello **world**");
        let b = md.render_inline_cached("hello **world**");
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn test_ansi_color() {
        let spans = render_inline("\x1b[31mred\x1b[0m text");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].style.fg, Some(Color::Red));
        assert_eq!(spans[0].content, "red");
    }

    #[test]
    fn test_html_color_span() {
        let spans = render_inline("<span style=\"color: #ff0000\">red</span> text");
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].style.fg, Some(Color::Rgb(255, 0, 0)));
        assert_eq!(spans[0].content, "red");
    }

    #[test]
    fn test_color_sample() {
        let span = maybe_color_sample("#00ff00").unwrap();
        assert_eq!(span.style.fg, Some(Color::Rgb(0, 255, 0)));
    }
}
