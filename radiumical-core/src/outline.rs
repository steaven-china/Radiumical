//! Workspace outline — a compact map of file paths and their public symbols.
//!
//! Used to prime the LLM with project structure before it reads specific files.
//! Format consumed by the LLM is plain text; internal cache is JSON.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const OUTLINE_FILE: &str = ".radi/outline.json";
const MAX_OUTLINE_CHARS: usize = 12_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineItem {
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineEntry {
    pub path: String,
    pub language: String,
    pub items: Vec<OutlineItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceOutline {
    pub entries: Vec<OutlineEntry>,
    pub generated_at: u64,
}

impl WorkspaceOutline {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Render the outline as a concise string for the LLM.
    pub fn format(&self, max_chars: usize) -> String {
        let mut out = String::from("## Workspace Outline\n\n");
        for entry in &self.entries {
            if out.len() > max_chars {
                out.push_str("\n... (outline truncated)\n");
                break;
            }
            out.push_str(&format!("{} ({}):\n", entry.path, entry.language));
            for item in &entry.items {
                if out.len() > max_chars {
                    break;
                }
                let sig = item
                    .signature
                    .as_deref()
                    .map(|s| format!(" {s}"))
                    .unwrap_or_default();
                out.push_str(&format!("  {} {}{}\n", item.kind, item.name, sig));
            }
            out.push('\n');
        }
        out
    }
}

/// Generate or load a cached workspace outline.
pub fn load_or_generate(workspace: &Path) -> Result<WorkspaceOutline> {
    let cache_path = workspace.join(OUTLINE_FILE);

    let source_files = collect_source_files(workspace)?;
    let mtimes: HashMap<PathBuf, u64> = source_files
        .iter()
        .filter_map(|p| {
            let modified = fs::metadata(p).ok()?.modified().ok()?;
            let secs = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
            Some((p.clone(), secs))
        })
        .collect();

    if let Ok(text) = fs::read_to_string(&cache_path) {
        if let Ok(cached) = serde_json::from_str::<WorkspaceOutline>(&text) {
            let mut stale = false;
            for entry in &cached.entries {
                let path = workspace.join(&entry.path);
                let cached_mtime = entry.modified.unwrap_or(0);
                if mtimes.get(&path).copied().unwrap_or(0) != cached_mtime {
                    stale = true;
                    break;
                }
            }
            if !stale && cached.entries.len() == source_files.len() {
                return Ok(cached);
            }
        }
    }

    let mut entries = Vec::new();
    for path in source_files {
        let modified = mtimes.get(&path).copied();
        let relative = path
            .strip_prefix(workspace)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        let language = language_for(&relative);
        let items = extract_items(&path, &language)?;
        if !items.is_empty() {
            entries.push(OutlineEntry {
                path: relative,
                language,
                items,
                modified,
            });
        }
    }

    let outline = WorkspaceOutline {
        entries,
        generated_at: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };

    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&outline)?;
    fs::write(&cache_path, json)?;

    Ok(outline)
}

fn collect_source_files(workspace: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![workspace.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir)
            .with_context(|| format!("read directory {}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if path.is_dir() {
                if name.starts_with('.')
                    || name == "target"
                    || name == "node_modules"
                    || name == "dist"
                    || name == "build"
                {
                    continue;
                }
                stack.push(path);
            } else if path.is_file() {
                if is_source_file(&path) {
                    files.push(path);
                }
            }
        }
    }

    files.sort();
    Ok(files)
}

fn is_source_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with('.') {
        return false;
    }
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("rs" | "py" | "js" | "ts" | "tsx" | "go" | "java" | "c" | "cpp" | "h" | "hpp")
    )
}

fn language_for(path: &str) -> String {
    match path.rsplit('.').next() {
        Some("rs") => "rust",
        Some("py") => "python",
        Some("js") => "javascript",
        Some("ts") => "typescript",
        Some("tsx") => "typescript-tsx",
        Some("go") => "go",
        Some("java") => "java",
        Some("c") => "c",
        Some("cpp" | "hpp") => "cpp",
        Some("h") => "c-header",
        _ => "text",
    }
    .into()
}

fn extract_items(path: &Path, language: &str) -> Result<Vec<OutlineItem>> {
    let text = fs::read_to_string(path)?;
    let lang: &str = &language;
    Ok(match lang {
        "rust" => parse_rust(&text),
        "python" => parse_python(&text),
        "javascript" | "typescript" | "typescript-tsx" => parse_js_ts(&text),
        "go" => parse_go(&text),
        "java" => parse_java(&text),
        "c" | "cpp" | "c-header" => parse_c_family(&text),
        _ => Vec::new(),
    })
}

// ── Simple line-based parsers (good enough for navigation) ──

fn parse_rust(text: &str) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    for raw in text.lines() {
        let line = strip_line_comment(raw, "//");
        if line.trim().is_empty() {
            continue;
        }

        // mod declaration
        if let Some(m) = line.trim().strip_prefix("mod ") {
            let name = m.trim_end_matches(';').split_whitespace().next().unwrap_or("");
            if !name.is_empty() && !name.starts_with('{') {
                items.push(OutlineItem {
                    kind: "mod".into(),
                    name: name.into(),
                    signature: None,
                });
                continue;
            }
        }

        // use statement
        if line.trim().starts_with("use ") {
            let sig = line.trim().trim_end_matches(';').to_string();
            items.push(OutlineItem {
                kind: "use".into(),
                name: sig.clone(),
                signature: Some(sig),
            });
            continue;
        }

        // impl block
        if line.trim().starts_with("impl") {
            let sig = extract_signature(&line);
            items.push(OutlineItem {
                kind: "impl".into(),
                name: sig.clone(),
                signature: Some(sig),
            });
            continue;
        }

        // pub / fn / struct / enum / trait / type / const / static
        let trimmed = line.trim_start();
        let (visibility, rest) = if trimmed.starts_with("pub ") {
            ("pub", trimmed[4..].trim_start())
        } else {
            ("", trimmed)
        };

        if let Some(item) = parse_decl(rest, "fn", "fn", extract_fn_name) {
            items.push(item);
        } else if let Some(item) = parse_decl(rest, "struct", "struct", extract_type_name) {
            items.push(item);
        } else if let Some(item) = parse_decl(rest, "enum", "enum", extract_type_name) {
            items.push(item);
        } else if let Some(item) = parse_decl(rest, "trait", "trait", extract_type_name) {
            items.push(item);
        } else if let Some(item) = parse_decl(rest, "type", "type", extract_type_name) {
            items.push(item);
        } else if let Some(item) = parse_decl(rest, "const", "const", extract_const_name) {
            items.push(item);
        } else if let Some(item) = parse_decl(rest, "static", "static", extract_const_name) {
            items.push(item);
        }
    }
    items
}

fn parse_python(text: &str) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    for raw in text.lines() {
        let line = strip_line_comment(raw, "#");
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("def ") {
            if let Some(name) = rest.split('(').next() {
                items.push(OutlineItem {
                    kind: "def".into(),
                    name: name.trim().into(),
                    signature: Some(extract_signature(&line)),
                });
            }
        } else if let Some(rest) = trimmed.strip_prefix("class ") {
            if let Some(name) = rest.split(':').next().and_then(|s| s.split('(').next()) {
                items.push(OutlineItem {
                    kind: "class".into(),
                    name: name.trim().into(),
                    signature: None,
                });
            }
        }
    }
    items
}

fn parse_js_ts(text: &str) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    for raw in text.lines() {
        let line = strip_line_comment(raw, "//");
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("function ") {
            if let Some(name) = rest.split('(').next() {
                items.push(OutlineItem {
                    kind: "function".into(),
                    name: name.trim().into(),
                    signature: Some(extract_signature(&line)),
                });
            }
        } else if let Some(rest) = trimmed.strip_prefix("class ") {
            if let Some(name) = rest.split(' ').next() {
                items.push(OutlineItem {
                    kind: "class".into(),
                    name: name.trim_end_matches('{').trim().into(),
                    signature: None,
                });
            }
        } else if trimmed.starts_with("export ") {
            // export function/class/const/let
            let inner = trimmed[7..].trim_start();
            if let Some(rest) = inner.strip_prefix("function ") {
                if let Some(name) = rest.split('(').next() {
                    items.push(OutlineItem {
                        kind: "function".into(),
                        name: name.trim().into(),
                        signature: Some(extract_signature(&line)),
                    });
                }
            } else if let Some(rest) = inner.strip_prefix("class ") {
                if let Some(name) = rest.split(' ').next() {
                    items.push(OutlineItem {
                        kind: "class".into(),
                        name: name.trim_end_matches('{').trim().into(),
                        signature: None,
                    });
                }
            } else if let Some(rest) = inner.strip_prefix("const ")
                .or_else(|| inner.strip_prefix("let "))
                .or_else(|| inner.strip_prefix("var "))
            {
                if let Some(name) = rest.split('=').next() {
                    items.push(OutlineItem {
                        kind: "const".into(),
                        name: name.trim().into(),
                        signature: None,
                    });
                }
            }
        }
    }
    items
}

fn parse_go(text: &str) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    for raw in text.lines() {
        let line = strip_line_comment(raw, "//");
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("func ") {
            let sig = extract_signature(&line);
            let name = sig
                .split('(')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .last()
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                items.push(OutlineItem {
                    kind: "func".into(),
                    name,
                    signature: Some(sig),
                });
            }
        } else if let Some(rest) = trimmed.strip_prefix("type ") {
            if let Some(name) = rest.split_whitespace().next() {
                items.push(OutlineItem {
                    kind: "type".into(),
                    name: name.into(),
                    signature: None,
                });
            }
        }
    }
    items
}

fn parse_java(text: &str) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    for raw in text.lines() {
        let line = strip_line_comment(raw, "//");
        let trimmed = line.trim_start();
        if trimmed.starts_with("public ") || trimmed.starts_with("private ") || trimmed.starts_with("protected ") {
            let rest = trimmed.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
            if rest.contains('(') {
                if let Some(name) = rest.split('(').next().and_then(|s| s.split_whitespace().last()) {
                    items.push(OutlineItem {
                        kind: "method".into(),
                        name: name.into(),
                        signature: Some(extract_signature(&line)),
                    });
                }
            } else if rest.starts_with("class ") || rest.starts_with("interface ") {
                let kind = if rest.starts_with("class ") { "class" } else { "interface" };
                if let Some(name) = rest[kind.len() + 1..].split_whitespace().next() {
                    items.push(OutlineItem {
                        kind: kind.into(),
                        name: name.into(),
                        signature: None,
                    });
                }
            }
        }
    }
    items
}

fn parse_c_family(text: &str) -> Vec<OutlineItem> {
    let mut items = Vec::new();
    for raw in text.lines() {
        let line = strip_line_comment(raw, "//");
        let trimmed = line.trim_start();
        if trimmed.starts_with("typedef ") {
            let rest = trimmed[8..].trim_start();
            if rest.starts_with("struct ") || rest.starts_with("enum ") {
                if let Some(name) = rest.split_whitespace().nth(1) {
                    items.push(OutlineItem {
                        kind: "typedef".into(),
                        name: name.trim_end_matches(';').into(),
                        signature: None,
                    });
                }
            }
        } else if trimmed.starts_with("struct ") {
            if let Some(name) = trimmed[7..].split_whitespace().next() {
                items.push(OutlineItem {
                    kind: "struct".into(),
                    name: name.trim_end_matches('{').into(),
                    signature: None,
                });
            }
        } else if trimmed.starts_with("enum ") {
            if let Some(name) = trimmed[5..].split_whitespace().next() {
                items.push(OutlineItem {
                    kind: "enum".into(),
                    name: name.trim_end_matches('{').into(),
                    signature: None,
                });
            }
        } else if trimmed.contains('(') && !trimmed.starts_with("#") {
            if let Some(name) = trimmed.split('(').next().and_then(|s| s.split_whitespace().last()) {
                items.push(OutlineItem {
                    kind: "function".into(),
                    name: name.into(),
                    signature: Some(extract_signature(&line)),
                });
            }
        }
    }
    items
}

// ── Helpers ──

fn strip_line_comment(line: &str, marker: &str) -> String {
    if let Some(idx) = line.find(marker) {
        line[..idx].to_string()
    } else {
        line.to_string()
    }
}

fn extract_signature(line: &str) -> String {
    line.trim()
        .trim_end_matches(';')
        .trim_end_matches('{')
        .trim()
        .to_string()
}

fn parse_decl(
    rest: &str,
    keyword: &str,
    kind: &str,
    name_extractor: fn(&str) -> Option<String>,
) -> Option<OutlineItem> {
    let prefix = format!("{keyword} ");
    if !rest.starts_with(&prefix) {
        return None;
    }
    let after = &rest[prefix.len()..];
    let name = name_extractor(after)?;
    Some(OutlineItem {
        kind: kind.into(),
        name,
        signature: None,
    })
}

fn extract_fn_name(after: &str) -> Option<String> {
    after.split('(').next().map(|s| s.trim().to_string())
}

fn extract_type_name(after: &str) -> Option<String> {
    after
        .split_whitespace()
        .next()
        .map(|s| s.trim_end_matches('{').trim_end_matches(';').to_string())
}

fn extract_const_name(after: &str) -> Option<String> {
    after
        .split_whitespace()
        .next()
        .map(|s| s.trim_end_matches(':').to_string())
}

/// Convenience: load outline and format it for the LLM, bounded by char limit.
pub fn formatted_outline(workspace: &Path) -> String {
    match load_or_generate(workspace) {
        Ok(o) => o.format(MAX_OUTLINE_CHARS),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rust() {
        let text = r#"
// comment
pub fn foo(a: i32) -> i32 { a }
pub struct Bar;
mod baz;
        "#;
        let items = parse_rust(text);
        assert!(items.iter().any(|i| i.name == "foo" && i.kind == "fn"));
        assert!(items.iter().any(|i| i.name == "Bar" && i.kind == "struct"));
        assert!(items.iter().any(|i| i.name == "baz" && i.kind == "mod"));
    }

    #[test]
    fn test_parse_python() {
        let text = "def hello():\n    pass\nclass World:\n    pass";
        let items = parse_python(text);
        assert!(items.iter().any(|i| i.name == "hello" && i.kind == "def"));
        assert!(items.iter().any(|i| i.name == "World" && i.kind == "class"));
    }

    #[test]
    fn test_workspace_outline_format() {
        let outline = WorkspaceOutline {
            entries: vec![OutlineEntry {
                path: "src/main.rs".into(),
                language: "rust".into(),
                items: vec![OutlineItem {
                    kind: "fn".into(),
                    name: "main".into(),
                    signature: Some("fn main()".into()),
                }],
                modified: None,
            }],
            generated_at: 0,
        };
        let text = outline.format(1000);
        assert!(text.contains("src/main.rs"));
        assert!(text.contains("fn main"));
    }
}
