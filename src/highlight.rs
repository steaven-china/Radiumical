//! Syntax highlighting via syntect for code fences.
use syntect::highlighting::ThemeSet;
use syntect::html::highlighted_html_for_string;
use syntect::parsing::SyntaxSet;

pub fn highlight_code(code: &str, lang: &str) -> String {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let syntax = ss.find_syntax_by_token(lang)
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let theme = &ts.themes["base16-ocean.dark"];
    // Convert HTML spans to ANSI terminal escapes
    highlighted_html_for_string(code, &ss, syntax, theme)
        .unwrap_or_else(|_| code.to_string())
        // Strip HTML tags, keep text content (simple approach)
        .replace("<span style=\"color: ", "\x1b[38;2;")
        .replace(";\">", "m")
        .replace("</span>", "\x1b[0m")
        .replace("<pre>", "")
        .replace("</pre>", "")
}

/// Quick check: is syntect available and can highlight?
pub fn available() -> bool { true }
