//! Markdown → ratatui Lines converter using pulldown-cmark.
//! Table rendering handled by tui.rs (full-buffer pre-scan).
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};

const DIM: Color = Color::Rgb(100, 100, 110);

pub struct MarkdownRenderer {
    pub frame: usize,
}

impl MarkdownRenderer {
    pub fn new() -> Self { Self { frame: 0 } }
    pub fn tick_frame(&mut self) { self.frame += 1; }
}

// ── pulldown-cmark inline renderer (public for table cells) ──

pub fn render_inline(text: &str) -> Vec<Span<'static>> {
    if text.is_empty() { return vec![]; }
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
                Tag::Link { dest_url, .. } => { link_url = dest_url.to_string(); style = style.fg(Color::Cyan); }
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Emphasis => style = style.remove_modifier(Modifier::ITALIC),
                TagEnd::Strong => style = style.remove_modifier(Modifier::BOLD),
                TagEnd::CodeBlock => style = style.fg(Color::Reset),
                TagEnd::Link => {
                    if !link_url.is_empty() {
                        spans.push(Span::styled(format!(" ({link_url})"), Style::default().fg(DIM)));
                        link_url.clear();
                    }
                    style = style.fg(Color::Reset);
                }
                _ => {}
            },
            Event::Text(text) => spans.push(Span::styled(text.to_string(), style)),
            Event::Code(text) => spans.push(Span::styled(format!("`{text}`"), Style::default().fg(Color::Yellow))),
            Event::SoftBreak | Event::HardBreak => spans.push(Span::raw(" ")),
            _ => {}
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bold() {
        let spans = render_inline("hello **world**!");
        assert_eq!(spans.len(), 3);
        assert!(spans[1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_code() {
        let spans = render_inline("use `foo` bar");
        let text: String = spans.iter().map(|s| s.to_string()).collect();
        assert!(text.contains("`foo`"));
    }

    #[test]
    fn test_link() {
        let spans = render_inline("[text](url)");
        let text: String = spans.iter().map(|s| s.to_string()).collect();
        assert!(text.contains("text"));
    }

    #[test]
    fn test_nested() {
        let spans = render_inline("**bold *italic***");
        assert!(spans.len() > 1);
    }
}
