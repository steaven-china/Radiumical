//! Small helpers used by the agent harness to build conversation messages
//! and execute tools with a timeout.
//!
//! These functions are intentionally thin: they convert raw strings and tool
//! results into the `Message` types expected by the provider layer, and wrap
//! tool execution in a Tokio timeout so the harness cannot hang forever.

use crate::tools::{Tool, ToolContext};
use crate::types::{Message, MessageContent, Role, ToolCall, ToolResult};
use std::path::Path;
use std::time::Duration;

/// Build a user-role `Message` from plain text.
pub fn user_msg(text: &str) -> Message {
    Message {
        role: Role::User,
        content: MessageContent::Text(text.to_string()),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        reasoning_content: None,
    }
}

/// Build an assistant-role `Message` from text, optional tool calls, and reasoning.
pub fn assistant_msg(text: &str, calls: Option<Vec<ToolCall>>, reasoning: &str) -> Message {
    Message {
        role: Role::Assistant,
        content: MessageContent::Text(text.to_string()),
        tool_calls: calls,
        tool_call_id: None,
        name: None,
        reasoning_content: if reasoning.is_empty() {
            None
        } else {
            Some(reasoning.to_string())
        },
    }
}

/// Build a tool-role `Message` from a tool call and its result.
///
/// If the tool call lacks an id, a synthetic id based on the function name is
/// generated so the provider message is well-formed.
pub fn tool_result_msg(tc: &ToolCall, result: ToolResult) -> Message {
    let call_id = if tc.id.is_empty() {
        format!("call_{}", tc.function.name)
    } else {
        tc.id.clone()
    };
    Message {
        role: Role::Tool,
        content: MessageContent::Text(result.content),
        tool_calls: None,
        tool_call_id: Some(call_id),
        name: Some(tc.function.name.clone()),
        reasoning_content: None,
    }
}

/// Execute a tool with a hard timeout.
///
/// If the tool does not complete within `timeout`, a `ToolResult` with
/// `is_error: true` is returned instead of panicking or hanging.
pub async fn exec_with_timeout(
    tool: &dyn Tool,
    workspace: &Path,
    arguments: &str,
    timeout: Duration,
    ctx: &ToolContext,
) -> ToolResult {
    let name = tool.definition().function.name.clone();
    let ws = workspace.to_path_buf();
    let args = arguments.to_string();
    match tokio::time::timeout(timeout, tool.execute_with_context(&ws, &args, ctx)).await {
        Ok(r) => r,
        Err(_) => ToolResult {
            tool_call_id: String::new(),
            content: format!("Tool '{name}' timed out after {}s.", timeout.as_secs()),
            is_error: true,
        },
    }
}
