//! Message sanitization for providers that require strict tool_call/tool_result pairing.

use super::message::{Message, Role, ToolCall};

/// Sanitize messages for providers that strictly require tool_calls → tool_results pairing.
///
/// DeepSeek (and some other providers) return HTTP 400 if an assistant message with
/// `tool_calls` is not immediately followed by tool result messages for each call_id.
/// This function fixes orphaned tool_calls by either:
/// - Preserving correctly paired sequences
/// - Removing tool_calls from assistant messages whose results are missing
pub fn sanitize_tool_messages(messages: &mut Vec<Message>) {
    // Collect all tool_call_ids that have corresponding tool results.
    let result_ids: std::collections::HashSet<String> = messages
        .iter()
        .filter(|m| m.role == Role::Tool)
        .filter_map(|m| m.tool_call_id.clone())
        .collect();

    for msg in messages.iter_mut() {
        if msg.role == Role::Assistant {
            if let Some(calls) = &msg.tool_calls {
                // Check if ALL calls in this message have results.
                let all_present = calls.iter().all(|c| result_ids.contains(&c.id));
                if !all_present {
                    // Remove orphaned tool_calls — keep only those with results.
                    let kept: Vec<ToolCall> = calls
                        .iter()
                        .filter(|c| result_ids.contains(&c.id))
                        .cloned()
                        .collect();
                    if kept.is_empty() {
                        msg.tool_calls = None;
                    } else {
                        msg.tool_calls = Some(kept);
                    }
                }
            }
        }
    }

    // Remove orphan tool result messages (no matching tool_call).
    let call_ids: std::collections::HashSet<String> = messages
        .iter()
        .filter(|m| m.role == Role::Assistant)
        .filter_map(|m| m.tool_calls.as_ref())
        .flatten()
        .map(|c| c.id.clone())
        .collect();

    messages.retain(|m| {
        if m.role == Role::Tool {
            m.tool_call_id
                .as_ref()
                .map(|id| call_ids.contains(id))
                .unwrap_or(false)
        } else {
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::message::{FunctionCall, MessageContent};
    use super::*;

    fn assistant_with_calls(ids: &[&str]) -> Message {
        Message {
            role: Role::Assistant,
            content: MessageContent::Text("".into()),
            tool_calls: Some(
                ids.iter()
                    .map(|id| ToolCall {
                        id: id.to_string(),
                        call_type: "function".into(),
                        function: FunctionCall {
                            name: "test_tool".into(),
                            arguments: "{}".into(),
                        },
                    })
                    .collect(),
            ),
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        }
    }

    fn tool_result(id: &str) -> Message {
        Message {
            role: Role::Tool,
            content: MessageContent::Text("ok".into()),
            tool_calls: None,
            tool_call_id: Some(id.to_string()),
            name: Some("test_tool".into()),
            reasoning_content: None,
        }
    }

    fn user_msg(text: &str) -> Message {
        Message {
            role: Role::User,
            content: MessageContent::Text(text.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        }
    }

    fn assistant_msg(text: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: MessageContent::Text(text.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        }
    }

    #[test]
    fn test_sanitize_ok_when_paired() {
        let mut msgs = vec![
            user_msg("hi"),
            assistant_with_calls(&["c1"]),
            tool_result("c1"),
            assistant_msg("done"),
        ];
        sanitize_tool_messages(&mut msgs);
        assert_eq!(msgs.len(), 4);
        assert!(msgs[1].tool_calls.is_some());
        assert_eq!(msgs[2].role, Role::Tool);
    }

    #[test]
    fn test_sanitize_removes_orphan_calls() {
        let mut msgs = vec![
            user_msg("hi"),
            assistant_with_calls(&["orphan"]),
            assistant_msg("I continued without waiting"),
        ];
        sanitize_tool_messages(&mut msgs);
        assert!(msgs[1].tool_calls.is_none());
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn test_sanitize_removes_orphan_result() {
        let mut msgs = vec![
            user_msg("hi"),
            assistant_msg("hello"),
            tool_result("no_match"),
            assistant_msg("done"),
        ];
        sanitize_tool_messages(&mut msgs);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[2].role, Role::Assistant);
    }

    #[test]
    fn test_sanitize_keeps_partial_pairs() {
        let mut msgs = vec![
            user_msg("hi"),
            assistant_with_calls(&["c1", "orphan"]),
            tool_result("c1"),
            assistant_msg("done"),
        ];
        sanitize_tool_messages(&mut msgs);
        let calls = msgs[1].tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "c1");
    }

    #[test]
    fn test_sanitize_preserves_system_and_user() {
        let mut msgs = vec![
            Message {
                role: Role::System,
                content: MessageContent::Text("sys".into()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
                reasoning_content: None,
            },
            user_msg("hi"),
            assistant_with_calls(&["c1", "c2"]),
            tool_result("c1"),
            tool_result("c2"),
            assistant_msg("all done"),
        ];
        sanitize_tool_messages(&mut msgs);
        assert_eq!(msgs.len(), 6);
        assert_eq!(msgs[0].role, Role::System);
        assert_eq!(msgs[1].role, Role::User);
    }

    #[test]
    fn test_sanitize_deepseek_scenario() {
        let mut msgs = vec![
            Message {
                role: Role::System,
                content: MessageContent::Text("You are helpful.".into()),
                tool_calls: None,
                tool_call_id: None,
                name: None,
                reasoning_content: None,
            },
            user_msg("read the file"),
            assistant_with_calls(&["call_1"]),
            Message {
                role: Role::System,
                content: MessageContent::Text(
                    "[Context compressed: 2 older messages summarised]".into(),
                ),
                tool_calls: None,
                tool_call_id: None,
                name: None,
                reasoning_content: None,
            },
            user_msg("now fix the bug"),
            assistant_with_calls(&["call_2"]),
            tool_result("call_2"),
            assistant_msg("Fixed."),
        ];
        sanitize_tool_messages(&mut msgs);
        let orphan_asst = msgs.iter().find(|m| {
            m.role == Role::Assistant
                && matches!(&m.content, MessageContent::Text(s) if s.is_empty())
                && m.tool_calls.as_ref().map_or(true, |c| c.is_empty())
        });
        assert!(
            orphan_asst.is_some(),
            "orphan assistant should have tool_calls cleared"
        );
        let paired_asst = msgs.iter().find(|m| {
            m.role == Role::Assistant
                && m.tool_calls
                    .as_ref()
                    .map_or(false, |c| c.iter().any(|tc| tc.id == "call_2"))
        });
        assert!(paired_asst.is_some(), "call_2 should still be paired");
        assert!(!msgs
            .iter()
            .any(|m| m.role == Role::Tool && m.tool_call_id.as_deref() == Some("call_1")));
    }

    #[test]
    fn test_sanitize_empty() {
        let mut msgs = vec![];
        sanitize_tool_messages(&mut msgs);
        assert!(msgs.is_empty());
    }
}
