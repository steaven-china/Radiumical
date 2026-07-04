//! Pure-text layout helpers: word-wrapping to a column width, Markdown inline
//! stripping, and proportional table-column width fitting.

use unicode_width::UnicodeWidthStr;

// ── Table width fitting ──

/// Scale `widths` proportionally so their sum (with padding) fits within `avail`.
pub fn fit_table_widths(widths: &[usize], avail: usize) -> Vec<usize> {
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

/// Remove Markdown inline markers (`**`, `*`, `` ` ``) from `text`, returning
/// the plain content.
pub fn strip_markdown(text: &str) -> String {
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

pub(crate) fn find_md_pair(chars: &[char], start: usize, d: &str) -> Option<usize> {
    let d: Vec<char> = d.chars().collect();
    (start..chars.len().saturating_sub(1)).find(|&i| chars[i] == d[0] && chars[i + 1] == d[1])
}

pub(crate) fn find_md_single(chars: &[char], start: usize, d: char) -> Option<usize> {
    chars[start..]
        .iter()
        .position(|&c| c == d)
        .map(|p| start + p)
}

/// Word-wrap `text` into lines that do not exceed `max_width` display columns
/// (Unicode-width-aware).
pub fn wrap_text_to_width(text: &str, max_width: usize) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_markdown() {
        assert_eq!(strip_markdown("**bold**"), "bold");
        assert_eq!(strip_markdown("*italic*"), "italic");
        assert_eq!(strip_markdown("`code`"), "code");
        assert_eq!(strip_markdown("**`nested`**"), "`nested`"); // bold wraps code
        assert_eq!(strip_markdown("plain"), "plain");
        assert_eq!(strip_markdown("✅ active"), "✅ active");
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
    }
}
