//! Syntax highlighting via syntect for code fences.
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style as SyntectStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

pub fn highlight_code(code: &str, lang: &str) -> Vec<Vec<Span<'static>>> {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let syntax = ss
        .find_syntax_by_token(lang)
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let theme = &ts.themes["base16-ocean.dark"];
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut rows: Vec<Vec<Span<'static>>> = Vec::new();
    for line in LinesWithEndings::from(code) {
        let line = line.trim_end_matches('\n');
        let ranges = highlighter.highlight_line(line, &ss).unwrap_or_default();
        let spans: Vec<Span> = ranges
            .into_iter()
            .map(|(style, text)| syntect_style_to_span(style, text))
            .collect();
        rows.push(spans);
    }
    rows
}

fn syntect_style_to_span(style: SyntectStyle, text: &str) -> Span<'static> {
    let mut s = Style::default();
    s = s.fg(Color::Rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
    ));
    if style.font_style.contains(FontStyle::BOLD) {
        s = s.add_modifier(ratatui::style::Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        s = s.add_modifier(ratatui::style::Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        s = s.add_modifier(ratatui::style::Modifier::UNDERLINED);
    }
    Span::styled(text.to_string(), s)
}

/// Quick check: is syntect available and can highlight?
#[allow(dead_code)]
pub fn available() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_rust() {
        let rows = highlight_code("fn main() {}", "rust");
        assert!(!rows.is_empty());
        let text: String = rows[0].iter().map(|s| s.to_string()).collect();
        assert!(text.contains("fn"));
    }

    #[test]
    fn test_highlight_unknown_lang_fallback() {
        let rows = highlight_code("hello world", "notareallang");
        assert_eq!(rows.len(), 1);
        let text: String = rows[0].iter().map(|s| s.to_string()).collect();
        assert_eq!(text, "hello world");
    }
}
