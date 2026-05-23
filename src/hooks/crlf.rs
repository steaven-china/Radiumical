//! CRLF normalizer as a ToolHook.
//! Auto-converts old_text/new_text line endings to match the target file.
use crate::pipeline::ToolHook;
use crate::types::{ToolCall, ToolResult};
use std::path::PathBuf;

pub struct CRLFNormalizer;

impl CRLFNormalizer {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ToolHook for CRLFNormalizer {
    fn after(&self, call: &ToolCall, mut result: ToolResult, workspace: &PathBuf) -> ToolResult {
        if call.function.name != "edit_file" || !result.is_error {
            return result;
        }

        // If edit_file failed, try with opposite line endings
        let args: serde_json::Value = match serde_json::from_str(&call.function.arguments) {
            Ok(v) => v,
            Err(_) => return result,
        };

        let path_str = args["path"].as_str().unwrap_or("");
        let old_text = args["old_text"].as_str().unwrap_or("");
        let new_text = args["new_text"].as_str().unwrap_or("");
        let full_path = workspace.join(path_str);

        let raw = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(_) => return result,
        };

        // Normalize both sides to LF, then try matching.
        let is_crlf = raw.contains("\r\n");
        let raw_lf = crlf_to_lf(&raw);
        let old_lf = crlf_to_lf(old_text);
        let new_lf = crlf_to_lf(new_text);

        if raw_lf.matches(&old_lf).count() == 1 {
            let new_content_lf = raw_lf.replacen(&old_lf, &new_lf, 1);
            // Restore original line endings
            let new_content = if is_crlf {
                lf_to_crlf(&new_content_lf)
            } else {
                new_content_lf
            };
            if std::fs::write(&full_path, &new_content).is_ok() {
                result.content = format!(
                    "Edited {} (auto-adjusted line endings). Replaced 1 occurrence.",
                    path_str
                );
                result.is_error = false;
            }
        }

        result
    }
}

fn lf_to_crlf(s: &str) -> String {
    s.replace("\r\n", "\n").replace("\n", "\r\n")
}

fn crlf_to_lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionCall, ToolCall, ToolResult};

    fn make_edit_call(old: &str, new: &str) -> ToolCall {
        ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "edit_file".into(),
                arguments: serde_json::json!({
                    "path": "test.txt",
                    "old_text": old,
                    "new_text": new
                }).to_string(),
            },
        }
    }

    #[test]
    fn test_crlf_normalizer_ignores_non_edit() {
        let normalizer = CRLFNormalizer::new();
        let call = ToolCall {
            id: "c1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: "{}".into(),
            },
        };
        let result = ToolResult {
            tool_call_id: "c1".into(),
            content: "some content".into(),
            is_error: false,
        };
        let out = normalizer.after(&call, result.clone(), &std::path::PathBuf::from("."));
        assert_eq!(out.content, result.content);
    }

    #[test]
    fn test_crlf_normalizer_ignores_success() {
        let normalizer = CRLFNormalizer::new();
        let call = make_edit_call("hello", "world");
        let result = ToolResult {
            tool_call_id: "call_1".into(),
            content: "OK".into(),
            is_error: false,
        };
        let out = normalizer.after(&call, result.clone(), &std::path::PathBuf::from("."));
        assert_eq!(out.content, result.content);
    }

    #[test]
    fn test_crlf_normalizer_tries_lf_fallback() {
        use std::io::Write;

        let dir = std::env::temp_dir().join("radium_test_crlf");
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("test.txt");

        // Write a CRLF file with "hello\r\nworld"
        let mut f = std::fs::File::create(&file_path).unwrap();
        f.write_all(b"hello\r\nworld\r\n").unwrap();

        let normalizer = CRLFNormalizer::new();
        // LLM sends LF old_text but file is CRLF
        let call = make_edit_call("hello\nworld", "goodbye\nworld");
        let result = ToolResult {
            tool_call_id: "call_1".into(),
            content: "old_text not found".into(),
            is_error: true,
        };
        let out = normalizer.after(&call, result, &dir);
        assert!(!out.is_error, "should have auto-adjusted line endings");
        assert!(out.content.contains("auto-adjusted"));

        let contents = std::fs::read_to_string(&file_path).unwrap();
        assert!(contents.contains("goodbye\r\nworld"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
