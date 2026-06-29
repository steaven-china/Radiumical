//! Source-code plugin API — static analysis plugins for codebases.
//!
//! A source plugin receives a workspace-relative file path and returns
//! structured diagnostics, symbols, or summaries. Built-in examples include
//! LSP-based diagnostics and simple regex/AST scanners.

use std::path::Path;
use std::sync::Arc;

use super::{Plugin, PluginRegistry};

/// Severity of a source plugin finding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A single finding from a source plugin.
#[derive(Debug, Clone)]
pub struct Finding {
    pub line: usize,
    pub column: usize,
    pub severity: Severity,
    pub message: String,
    pub code: Option<String>,
}

/// Summary returned by a source plugin for a file or directory.
#[derive(Debug, Clone, Default)]
pub struct SourceSummary {
    pub language: Option<String>,
    pub symbol_count: usize,
    pub findings: Vec<Finding>,
    pub notes: Vec<String>,
}

/// Trait for plugins that analyze source code files.
pub trait SourcePlugin: Plugin {
    /// Human-readable description of what this plugin analyzes.
    fn description(&self) -> &str;

    /// Return true if this plugin can analyze the given relative path.
    fn can_handle(&self, relative_path: &Path) -> bool;

    /// Analyze a single file. `workspace` is the absolute project root;
    /// `relative_path` is the file path relative to that root.
    fn analyze(
        &self,
        workspace: &Path,
        relative_path: &Path,
    ) -> anyhow::Result<SourceSummary>;
}

impl PluginRegistry {
    /// Returns true if there are no plugins registered.
    pub fn has_no_plugins(&self) -> bool {
        self.is_empty()
    }
}

/// Dedicated registry for source plugins. Kept alongside the generic plugin
/// registry because source plugins need to be queried by file type.
#[derive(Default, Clone)]
pub struct SourcePluginRegistry {
    plugins: Vec<Arc<dyn SourcePlugin>>,
}

impl SourcePluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    pub fn register(&mut self, plugin: Box<dyn SourcePlugin>) {
        self.plugins.push(Arc::from(plugin));
    }

    /// Analyze a file with every plugin that can handle it.
    pub fn analyze(
        &self,
        workspace: &Path,
        relative_path: &Path,
    ) -> anyhow::Result<SourceSummary> {
        let mut merged = SourceSummary {
            language: language_for(relative_path_str(relative_path)),
            ..Default::default()
        };

        for plugin in &self.plugins {
            if !plugin.can_handle(relative_path) {
                continue;
            }
            match plugin.analyze(workspace, relative_path) {
                Ok(summary) => {
                    merged.symbol_count += summary.symbol_count;
                    merged.findings.extend(summary.findings);
                    merged.notes.extend(summary.notes);
                }
                Err(e) => {
                    merged.notes.push(format!(
                        "{} failed: {e}",
                        plugin.name()
                    ));
                }
            }
        }

        merged.findings.sort_by(|a, b| {
            a.severity
                .cmp(&b.severity)
                .then_with(|| a.line.cmp(&b.line))
                .then_with(|| a.column.cmp(&b.column))
        });
        Ok(merged)
    }
}

fn relative_path_str(p: &Path) -> &str {
    p.to_str().unwrap_or("")
}

fn language_for(path: &str) -> Option<String> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "rs" => Some("rust".into()),
        "py" => Some("python".into()),
        "js" | "ts" | "tsx" => Some("javascript".into()),
        "go" => Some("go".into()),
        "c" | "cpp" | "h" | "hpp" => Some("c/c++".into()),
        "java" => Some("java".into()),
        _ => None,
    }
}

/// Built-in regex-based linter plugin (example implementation).
pub struct RegexLinter;

impl Plugin for RegexLinter {
    fn id(&self) -> &str {
        "regex_linter"
    }
    fn name(&self) -> &str {
        "Regex Linter"
    }
}

impl SourcePlugin for RegexLinter {
    fn description(&self) -> &str {
        "Finds TODO/FIXME/XXX markers and trailing whitespace in source files."
    }

    fn can_handle(&self, relative_path: &Path) -> bool {
        language_for(relative_path.to_str().unwrap_or("")).is_some()
    }

    fn analyze(
        &self,
        workspace: &Path,
        relative_path: &Path,
    ) -> anyhow::Result<SourceSummary> {
        let full = workspace.join(relative_path);
        let text = std::fs::read_to_string(&full)?;
        let mut findings = Vec::new();
        let mut notes = Vec::new();

        for (i, line) in text.lines().enumerate() {
            let line_num = i + 1;
            for marker in ["TODO", "FIXME", "XXX"] {
                if let Some(pos) = line.find(marker) {
                    findings.push(Finding {
                        line: line_num,
                        column: pos,
                        severity: Severity::Info,
                        message: format!("Found {marker} marker"),
                        code: Some(line.trim().to_string()),
                    });
                }
            }
            if line.ends_with(' ') || line.ends_with('\t') {
                findings.push(Finding {
                    line: line_num,
                    column: line.len(),
                    severity: Severity::Warning,
                    message: "Trailing whitespace".into(),
                    code: None,
                });
            }
        }

        notes.push(format!(
            "RegexLinter scanned {} lines",
            text.lines().count()
        ));
        Ok(SourceSummary {
            language: language_for(relative_path.to_str().unwrap_or("")),
            symbol_count: 0,
            findings,
            notes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_dir() -> std::path::PathBuf {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("radium_plugin_test_{t:x}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cleanup(dir: &std::path::Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_regex_linter_finds_markers_and_trailing_whitespace() {
        let dir = tmp_dir();
        let path = dir.join("main.rs");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"fn main() {\n    // TODO: fix this \n}\n")
            .unwrap();

        let linter = RegexLinter;
        let summary = linter
            .analyze(&dir, std::path::Path::new("main.rs"))
            .unwrap();

        assert!(summary.language.as_deref() == Some("rust"));
        let todo = summary
            .findings
            .iter()
            .any(|f| f.message.contains("TODO"));
        let trailing = summary
            .findings
            .iter()
            .any(|f| f.message.contains("Trailing whitespace"));
        assert!(todo, "should find TODO marker");
        assert!(trailing, "should find trailing whitespace");
        cleanup(&dir);
    }

    #[test]
    fn test_source_plugin_registry_merges_plugins() {
        let dir = tmp_dir();
        let path = dir.join("lib.rs");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"// FIXME: hack\n").unwrap();

        let mut reg = SourcePluginRegistry::new();
        reg.register(Box::new(RegexLinter));
        let summary = reg
            .analyze(&dir, std::path::Path::new("lib.rs"))
            .unwrap();

        assert!(summary.findings.iter().any(|f| f.message.contains("FIXME")));
        cleanup(&dir);
    }

    #[test]
    fn test_language_detection() {
        assert_eq!(language_for("src/main.rs"), Some("rust".into()));
        assert_eq!(language_for("app.py"), Some("python".into()));
        assert_eq!(language_for("README.md"), None);
    }
}
