/// Strip ANSI escape sequences (including cursor movement and erase) from a
/// string, returning plain text. Used for tool results where control sequences
/// would otherwise leave stray characters on screen.
pub fn strip_ansi_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Skip ESC [ ... letter or ESC ] ... BEL or ESC ( letter etc.
            if chars.next_if_eq(&'[').is_some() {
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                // Skip the next char as part of a non-CSI escape.
                chars.next();
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Wrap tool result lines to the available content width and return the total
/// number of visual lines (used both for block height and rendering).
pub fn wrapped_tool_result_lines(result: &str, content_width: usize) -> Vec<String> {
    let cleaned = strip_ansi_escapes(result);
    // Normalize diff marker so height calculation matches the renderer.
    let normalized = if cleaned.contains('\x04') {
        cleaned.replace("\x04diff:", "── Diff ──")
    } else {
        cleaned
    };
    normalized
        .lines()
        .filter(|l| !l.trim().is_empty())
        .flat_map(|l| crate::layout::text::wrap_text_to_width(l, content_width.max(1)))
        .collect()
}

pub(crate) fn format_read_file_path(path: &str, start: Option<u64>, end: Option<u64>) -> String {
    let path = path.replace("\\\\", "\\");
    let range = match (start, end) {
        (Some(s), Some(e)) => format!("[{s}-{e}]"),
        (Some(s), None) => format!("[{s}]"),
        _ => String::new(),
    };
    if range.is_empty() {
        path
    } else {
        format!("{path} {range}")
    }
}

/// Extract the path value from a malformed read_file JSON args string
/// (e.g. with unescaped backslashes) without requiring a regex dependency.
pub(crate) fn extract_read_file_path(args: &str) -> Option<String> {
    let key = "\"path\"";
    let start = args.find(key)? + key.len();
    let rest = &args[start..];
    let colon = rest.find(':')?;
    let rest = &rest[colon + 1..];
    let first_quote = rest.find('"')?;
    let rest = &rest[first_quote + 1..];
    let end_quote = rest.find('"')?;
    Some(rest[..end_quote].replace("\\\\", "\\"))
}

/// Parse a tool's JSON arguments into a readable `key: value, key: value` string.
/// Falls back to the raw string if parsing fails. Special-cases `read_file` to
/// show `path[start_line[-end_line]]` compactly.
pub(crate) fn format_tool_args(name: &str, args: &str) -> String {
    if args.is_empty() {
        return String::new();
    }
    let v = match serde_json::from_str::<serde_json::Value>(args) {
        Ok(v) => v,
        Err(_) => {
            // not JSON — try to salvage a read_file path, otherwise show raw
            if name.split_whitespace().next() == Some("read_file") {
                if let Some(path) = extract_read_file_path(args) {
                    return path;
                }
            }
            return args.replace("\\\\", "\\");
        }
    };

    if name.split_whitespace().next() == Some("read_file") {
        // Try to extract path even if JSON is malformed (e.g. unescaped Windows backslashes).
        if let Some(obj) = v.as_object() {
            let path = obj.get("path").and_then(|p| p.as_str()).unwrap_or("");
            let start = obj.get("start_line").and_then(|n| n.as_u64());
            let end = obj.get("end_line").and_then(|n| n.as_u64());
            return format_read_file_path(path, start, end);
        } else if let Some(path) = extract_read_file_path(args) {
            return path;
        }
    }

    if name.split_whitespace().next() == Some("tree") {
        if let Some(obj) = v.as_object() {
            if let Some(path) = obj.get("path").and_then(|p| p.as_str()) {
                return path.replace("\\\\", "\\");
            }
        } else if let Some(path) = extract_read_file_path(args) {
            return path;
        }
    }

    if let Some(obj) = v.as_object() {
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        let pairs: Vec<String> = keys
            .into_iter()
            .map(|k| {
                let val = &obj[k];
                let s = match val {
                    serde_json::Value::String(s) => s.replace("\\\\", "\\"),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Null => "null".into(),
                    other => other.to_string(),
                };
                format!("{k}: {s}")
            })
            .collect();
        let joined = pairs.join(", ");
        // truncate to ~60 chars
        let chars: Vec<char> = joined.chars().collect();
        if chars.len() > 60 {
            let mut t: String = chars[..60].iter().collect();
            t.push('…');
            t
        } else {
            joined
        }
    } else {
        args.replace("\\\\", "\\")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tool_args_read_file() {
        assert_eq!(
            format_tool_args("read_file", r#"{"path":"src/main.rs"}"#),
            "src/main.rs"
        );
        assert_eq!(
            format_tool_args("read_file", r#"{"path":"src/main.rs","start_line":10}"#),
            "src/main.rs [10]"
        );
        assert_eq!(
            format_tool_args(
                "read_file",
                r#"{"path":"src/main.rs","start_line":10,"end_line":50}"#
            ),
            "src/main.rs [10-50]"
        );
        // Malformed JSON with unescaped Windows backslashes
        assert_eq!(
            format_tool_args("read_file", r#"{"path": "D:\Radiumical\Cargo.toml"}"#),
            "D:\\Radiumical\\Cargo.toml"
        );
        // Header may include batch suffix
        assert_eq!(
            format_tool_args("read_file (2/2)", r#"{"path":"src/main.rs"}"#),
            "src/main.rs"
        );
        // Non-read_file tools keep generic key:value formatting (sorted keys)
        assert_eq!(
            format_tool_args("write_file", r#"{"path":"x","content":"y"}"#),
            "content: y, path: x"
        );
    }
}
