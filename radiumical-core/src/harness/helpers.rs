use crate::tools::{Tool, ToolContext};
use crate::types::{Message, MessageContent, Role, ToolCall, ToolResult};
use std::path::Path;
use std::time::Duration;

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
