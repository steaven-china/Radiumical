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
