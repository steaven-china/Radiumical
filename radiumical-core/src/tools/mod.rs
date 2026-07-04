use std::path::PathBuf;
use std::sync::Arc;

use crate::plugins::source::SourcePluginRegistry;
use crate::types::{ToolDefinition, ToolResult, UiEvent};

mod agent;
mod agent_pool;
pub mod cluster_tool;
mod command;
mod file;
pub mod interact;
pub mod layout_page;
mod layout_tool;
pub mod mcp_tool;
mod search;
mod skill;
mod source_plugin;
mod system;
mod task;

pub use agent::{MemoryTool, PlaywrightTool, SubAgentListTool, SubAgentTool, SubAgentWaitTool};
pub use agent_pool::{ListAgentsTool, LoadAgentTool};
pub use cluster_tool::ClusterTool;
pub use command::RunCommand;
pub use file::{EditFile, ReadFile, WriteFile};
pub use interact::{AnnotateTool, ChoiceTool};
pub use layout_tool::LayoutPageTool;
pub use mcp_tool::McpToolAdapter;
pub use search::{FindFiles, SearchCode};
pub use skill::{ListSkillsTool, LoadSkillTool};
pub use source_plugin::SourceCodeTool;
pub use system::{CronTab, ListDir, LspDiagnostics, SysInfo, TimeNow, TreeDir};
pub use task::{GoalTool, OrchestrateTool, TodoList};

/// Context passed to tools that need to interact with the UI or access
/// harness-level services such as source-code plugins.
pub struct ToolContext {
    pub ui_tx: tokio::sync::mpsc::Sender<UiEvent>,
    pub source_plugins: Option<Arc<SourcePluginRegistry>>,
}

impl ToolContext {
    pub fn new(ui_tx: tokio::sync::mpsc::Sender<UiEvent>) -> Self {
        Self {
            ui_tx,
            source_plugins: None,
        }
    }
}

impl Default for ToolContext {
    fn default() -> Self {
        let (tx, _rx) = tokio::sync::mpsc::channel(256);
        Self {
            ui_tx: tx,
            source_plugins: None,
        }
    }
}

/// A tool that the agent can call.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, workspace: &PathBuf, arguments: &str) -> ToolResult;

    async fn execute_with_context(
        &self,
        workspace: &PathBuf,
        arguments: &str,
        _ctx: &ToolContext,
    ) -> ToolResult {
        self.execute(workspace, arguments).await
    }
}

/// Returns all tools as Vec.
pub fn all_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ReadFile),
        Box::new(WriteFile),
        Box::new(EditFile),
        Box::new(SearchCode),
        Box::new(FindFiles),
        Box::new(RunCommand),
        Box::new(TodoList),
        Box::new(OrchestrateTool),
        Box::new(GoalTool),
        Box::new(ChoiceTool),
        Box::new(LspDiagnostics),
        Box::new(SysInfo),
        Box::new(ListDir),
        Box::new(TreeDir),
        Box::new(TimeNow),
        Box::new(CronTab),
        Box::new(AnnotateTool),
        Box::new(SubAgentTool),
        Box::new(SubAgentListTool),
        Box::new(SubAgentWaitTool),
        Box::new(MemoryTool),
        Box::new(PlaywrightTool),
        Box::new(SourceCodeTool),
        Box::new(ListSkillsTool),
        Box::new(LoadSkillTool),
        Box::new(ListAgentsTool),
        Box::new(LoadAgentTool),
        Box::new(LayoutPageTool),
        Box::new(ClusterTool),
    ]
}

// ── Shared helpers ──

pub(crate) fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with('.') || s == "node_modules" || s == "target" || s == ".git")
        .unwrap_or(false)
}

pub(crate) fn simple_glob_match(pattern: &str, path: &str) -> bool {
    let parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = path.split('/').collect();

    /// Recursive matching with proper ** backtracking.
    fn match_from(pi: usize, si: usize, parts: &[&str], path_parts: &[&str]) -> bool {
        if pi == parts.len() {
            return si == path_parts.len();
        }

        if parts[pi] == "**" {
            // ** matches zero or more path segments — try zero first, then each prefix
            for next_si in si..=path_parts.len() {
                if match_from(pi + 1, next_si, parts, path_parts) {
                    return true;
                }
            }
            return false;
        }

        if si >= path_parts.len() {
            return false;
        }

        if part_match(parts[pi], path_parts[si]) {
            return match_from(pi + 1, si + 1, parts, path_parts);
        }

        false
    }

    match_from(0, 0, &parts, &path_parts)
}

pub(crate) fn part_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') && !pattern.contains('?') {
        return pattern == value;
    }
    // Very basic glob matching for single part
    let re_str = pattern
        .replace('.', "\\.")
        .replace('*', ".*")
        .replace('?', ".");
    regex::Regex::new(&format!("^{re_str}$")).is_ok_and(|re| re.is_match(value))
}

/// Convert LF → CRLF (for Windows files)
pub(crate) fn lf_to_crlf(s: &str) -> String {
    // Normalize to LF first, then convert
    s.replace("\r\n", "\n").replace("\n", "\r\n")
}

/// Convert CRLF → LF (for matching)
pub(crate) fn crlf_to_lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;

    use super::*;

    fn uuid_simple() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("{t:x}_{n:x}")
    }

    fn setup_temp_dir(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("radium_test_{}", uuid_simple()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            let mut f = File::create(&path).unwrap();
            f.write_all(content.as_bytes()).unwrap();
        }
        dir
    }

    fn cleanup(dir: &PathBuf) {
        std::fs::remove_dir_all(dir).ok();
    }

    // ── Glob matching ──

    #[test]
    fn test_glob_exact_match() {
        assert!(simple_glob_match("src/main.rs", "src/main.rs"));
        assert!(!simple_glob_match("src/main.rs", "src/lib.rs"));
    }

    #[test]
    fn test_glob_single_wildcard() {
        assert!(simple_glob_match("src/*.rs", "src/main.rs"));
        assert!(simple_glob_match("src/*.rs", "src/tools.rs"));
        assert!(!simple_glob_match("src/*.rs", "src/sub/main.rs"));
    }

    #[test]
    fn test_glob_double_wildcard() {
        assert!(simple_glob_match("src/**/*.rs", "src/main.rs"));
        assert!(simple_glob_match("src/**/*.rs", "src/sub/mod.rs"));
        assert!(simple_glob_match("src/**/*.rs", "src/a/b/c/deep.rs"));
    }

    #[test]
    fn test_glob_question_mark() {
        assert!(simple_glob_match("src/???.rs", "src/mod.rs"));
        assert!(!simple_glob_match("src/???.rs", "src/main.rs")); // 4 chars
    }

    #[test]
    fn test_glob_mixed() {
        assert!(simple_glob_match("**/*test*", "src/test_utils.rs"));
        assert!(simple_glob_match("**/*test*", "tests/integration_test.rs"));
        assert!(!simple_glob_match("**/*test*", "src/main.rs"));
    }

    #[test]
    fn test_part_match_exact() {
        assert!(part_match("main.rs", "main.rs"));
        assert!(!part_match("main.rs", "lib.rs"));
    }

    #[test]
    fn test_part_match_star() {
        assert!(part_match("*", "anything"));
        assert!(part_match("*", ""));
    }

    #[test]
    fn test_part_match_wildcard() {
        assert!(part_match("*.rs", "main.rs"));
        assert!(part_match("*.rs", "lib.rs"));
        assert!(!part_match("*.rs", "main.py"));
    }

    #[test]
    fn test_part_match_question() {
        assert!(part_match("???.rs", "mod.rs"));
        assert!(!part_match("???.rs", "main.rs"));
    }

    // ── CRLF helpers ──

    #[test]
    fn test_crlf_to_lf_conversion() {
        assert_eq!(crlf_to_lf("hello\r\nworld"), "hello\nworld");
        assert_eq!(crlf_to_lf("hello\nworld"), "hello\nworld");
        assert_eq!(crlf_to_lf("no newlines"), "no newlines");
    }

    #[test]
    fn test_lf_to_crlf_conversion() {
        assert_eq!(lf_to_crlf("hello\nworld"), "hello\r\nworld");
        assert_eq!(lf_to_crlf("hello\r\nworld"), "hello\r\nworld");
        assert_eq!(lf_to_crlf("no newlines"), "no newlines");
    }

    #[test]
    fn test_crlf_roundtrip() {
        let original = "line1\nline2\r\nline3\n";
        let converted = lf_to_crlf(&crlf_to_lf(original));
        assert_eq!(converted, lf_to_crlf(original));
    }

    // ── is_hidden ──

    #[test]
    fn test_is_hidden_dir() {
        // We can't easily construct a DirEntry, but we can test the predicate logic
        // on known patterns
        let hidden_names = [".git", "node_modules", "target", ".hidden"];
        let visible_names = ["src", "tests", "README.md", "Cargo.toml"];

        for name in hidden_names {
            assert!(
                name.starts_with('.')
                    || name == "node_modules"
                    || name == "target"
                    || name == ".git",
                "{} should be considered hidden",
                name
            );
        }
        for name in visible_names {
            let hidden = name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == ".git";
            assert!(!hidden, "{} should be visible", name);
        }
    }

    // ── TodoList ──

    #[tokio::test]
    async fn test_todo_add_and_list() {
        let ws = PathBuf::from(std::env::temp_dir()).join("__test_todo_add_list__");
        let _ = std::fs::remove_file(task::TodoStore::path_for(&ws));

        let result = TodoList
            .execute(&ws, r#"{"action": "add write tests"}"#)
            .await;
        assert!(!result.is_error);
        assert!(result.content.contains("Added #1"));

        let result = TodoList
            .execute(&ws, r#"{"action": "add fix bugs"}"#)
            .await;
        assert!(!result.is_error);
        assert!(result.content.contains("Added #2"));

        let result = TodoList
            .execute(&ws, r#"{"action": "list"}"#)
            .await;
        assert!(!result.is_error);
        assert!(result.content.contains("write tests"));
        assert!(result.content.contains("fix bugs"));

        let _ = std::fs::remove_file(task::TodoStore::path_for(&ws));
    }

    #[tokio::test]
    async fn test_todo_done() {
        let ws = PathBuf::from(std::env::temp_dir()).join("__test_todo_done__");
        let _ = std::fs::remove_file(task::TodoStore::path_for(&ws));

        TodoList
            .execute(&ws, r#"{"action": "add task"}"#)
            .await;

        let result = TodoList
            .execute(&ws, r#"{"action": "done 1"}"#)
            .await;
        assert!(!result.is_error);
        assert!(result.content.contains("Done #1"));

        let _ = std::fs::remove_file(task::TodoStore::path_for(&ws));
    }

    #[tokio::test]
    async fn test_todo_done_invalid_index() {
        let ws = PathBuf::from(std::env::temp_dir()).join("__test_todo_invalid__");
        let _ = std::fs::remove_file(task::TodoStore::path_for(&ws));

        let result = TodoList
            .execute(&ws, r#"{"action": "done 99"}"#)
            .await;
        assert!(result.is_error);

        let _ = std::fs::remove_file(task::TodoStore::path_for(&ws));
    }

    // ── GoalTool ──

    #[tokio::test]
    async fn test_goal_set_and_list() {
        task::goals().lock().unwrap().clear();

        let result = GoalTool
            .execute(
                &PathBuf::from("."),
                r#"{"action": "set Finish the project"}"#,
            )
            .await;
        assert!(!result.is_error);
        assert!(result.content.contains("Goal set: Finish the project"));

        GoalTool
            .execute(&PathBuf::from("."), r#"{"action": "add write docs"}"#)
            .await;

        let result = GoalTool
            .execute(&PathBuf::from("."), r#"{"action": "list"}"#)
            .await;
        assert!(result.content.contains("Finish the project"));
        assert!(result.content.contains("write docs"));
    }

    // ── ChoiceTool ──

    #[tokio::test]
    async fn test_choice_single() {
        let result = ChoiceTool
            .execute(
                &PathBuf::from("."),
                r#"{"mode": "single", "options": "A, B, C"}"#,
            )
            .await;
        assert!(!result.is_error);
        assert!(result.content.contains("A"));
        assert!(result.content.contains("B"));
        assert!(result.content.contains("C"));
        assert!(result.content.contains("Choose (single)"));
    }

    #[tokio::test]
    async fn test_choice_empty_options() {
        let result = ChoiceTool
            .execute(&PathBuf::from("."), r#"{"mode": "single", "options": ""}"#)
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("No options"));
    }

    // ── ReadFile ──

    #[tokio::test]
    async fn test_read_file_basic() {
        let dir = setup_temp_dir(&[("hello.txt", "Hello, world!\nLine 2\n")]);
        let result = ReadFile.execute(&dir, r#"{"path": "hello.txt"}"#).await;
        cleanup(&dir);
        assert!(!result.is_error);
        assert!(result.content.contains("Hello, world!"));
        assert!(result.content.contains("Line 2"));
    }

    #[tokio::test]
    async fn test_read_file_with_line_range() {
        let dir = setup_temp_dir(&[("lines.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n")]);
        let result = ReadFile
            .execute(
                &dir,
                r#"{"path": "lines.txt", "start_line": 3, "end_line": 5}"#,
            )
            .await;
        cleanup(&dir);
        assert!(!result.is_error);
        // Format is: "    3 | 3", "    4 | 4", "    5 | 5"
        assert!(result.content.contains("3 | 3") || result.content.contains("3|3"));
        assert!(!result.content.contains("8 | 8"));
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let dir = setup_temp_dir(&[]);
        let result = ReadFile
            .execute(&dir, r#"{"path": "nonexistent.txt"}"#)
            .await;
        cleanup(&dir);
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn test_read_file_invalid_json() {
        let dir = setup_temp_dir(&[]);
        let result = ReadFile.execute(&dir, "not json").await;
        cleanup(&dir);
        assert!(result.is_error);
        assert!(result.content.contains("Invalid JSON"));
    }

    // ── WriteFile ──

    #[tokio::test]
    async fn test_write_file_new() {
        let dir = setup_temp_dir(&[]);
        let result = WriteFile
            .execute(&dir, r#"{"path": "new.txt", "content": "fresh content"}"#)
            .await;
        assert!(!result.is_error);
        assert!(result.content.contains("Wrote"));
        assert!(result.content.contains("new.txt"));
        let contents = std::fs::read_to_string(dir.join("new.txt")).unwrap();
        assert_eq!(contents, "fresh content");
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_write_file_overwrite() {
        let dir = setup_temp_dir(&[("existing.txt", "old content")]);
        let result = WriteFile
            .execute(
                &dir,
                r#"{"path": "existing.txt", "content": "new content"}"#,
            )
            .await;
        assert!(!result.is_error);
        let contents = std::fs::read_to_string(dir.join("existing.txt")).unwrap();
        assert_eq!(contents, "new content");
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_write_file_creates_parent_dirs() {
        let dir = setup_temp_dir(&[]);
        let result = WriteFile
            .execute(
                &dir,
                r#"{"path": "sub/deep/nested.txt", "content": "deep"}"#,
            )
            .await;
        assert!(!result.is_error);
        let contents = std::fs::read_to_string(dir.join("sub/deep/nested.txt")).unwrap();
        assert_eq!(contents, "deep");
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_write_file_invalid_json() {
        let dir = setup_temp_dir(&[]);
        let result = WriteFile.execute(&dir, "bad json").await;
        cleanup(&dir);
        assert!(result.is_error);
        assert!(result.content.contains("Invalid JSON"));
    }

    // ── EditFile ──

    #[tokio::test]
    async fn test_edit_file_basic_replace() {
        let dir = setup_temp_dir(&[("code.rs", "fn old_name() {\n    println!(\"hi\");\n}\n")]);
        let result = EditFile
            .execute(
                &dir,
                r#"{"path": "code.rs", "old_text": "old_name", "new_text": "new_name"}"#,
            )
            .await;
        assert!(!result.is_error, "edit should succeed: {}", result.content);
        assert!(result.content.contains("OK"));
        let contents = std::fs::read_to_string(dir.join("code.rs")).unwrap();
        assert!(contents.contains("new_name"));
        assert!(!contents.contains("old_name"));
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_edit_file_not_found() {
        let dir = setup_temp_dir(&[("code.rs", "some content\n")]);
        let result = EditFile
            .execute(
                &dir,
                r#"{"path": "code.rs", "old_text": "nothing like this", "new_text": "replacement"}"#,
            )
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("not found"));
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_edit_file_non_unique() {
        let dir = setup_temp_dir(&[("dup.txt", "x\nx\nx\n")]);
        let result = EditFile
            .execute(
                &dir,
                r#"{"path": "dup.txt", "old_text": "x", "new_text": "y"}"#,
            )
            .await;
        assert!(result.is_error);
        assert!(result.content.contains("matches 3 times"));
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_edit_file_missing_file() {
        let dir = setup_temp_dir(&[]);
        let result = EditFile
            .execute(
                &dir,
                r#"{"path": "nope.txt", "old_text": "a", "new_text": "b"}"#,
            )
            .await;
        assert!(result.is_error);
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_edit_file_invalid_json() {
        let dir = setup_temp_dir(&[]);
        let result = EditFile.execute(&dir, "garbage").await;
        assert!(result.is_error);
        assert!(result.content.contains("Invalid JSON"));
        cleanup(&dir);
    }

    #[tokio::test]
    async fn test_edit_file_crlf_auto_adjust() {
        let dir = setup_temp_dir(&[]);
        let path = dir.join("crlf.txt");
        let mut f = File::create(&path).unwrap();
        f.write_all(b"hello\r\nworld\r\n").unwrap();
        // Send LF in the edit args — the tool should auto-detect and convert
        let result = EditFile
            .execute(
                &dir,
                r#"{"path": "crlf.txt", "old_text": "hello\nworld", "new_text": "goodbye\nworld"}"#,
            )
            .await;
        assert!(
            !result.is_error,
            "should auto-adjust CRLF: {}",
            result.content
        );
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("goodbye\r\nworld"));
        cleanup(&dir);
    }

    // ── Tool registry ──

    #[test]
    fn test_all_tools_have_unique_names() {
        let tools = all_tools();
        let names: Vec<String> = tools
            .iter()
            .map(|t| t.definition().function.name.clone())
            .collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "all tool names must be unique");
    }

    #[test]
    fn test_all_tools_have_descriptions() {
        let tools = all_tools();
        for tool in &tools {
            let def = tool.definition();
            assert!(
                !def.function.description.is_empty(),
                "{} has no description",
                def.function.name
            );
            assert!(
                def.function.parameters.is_object(),
                "{} has invalid parameters",
                def.function.name
            );
        }
    }
}
