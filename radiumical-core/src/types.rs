use std::borrow::Cow;
use serde::{Deserialize, Serialize};

use crate::providers::ProviderSource;
use crate::session::SessionItem;

// ── Chat types ──

/// Threshold for lz4 transparent compression (1 KB).
const COMPRESS_THRESHOLD: usize = 1024;

/// Magic prefix indicating lz4-compressed text content.
const LZ4_PREFIX: &str = "\x00lz4:";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

impl MessageContent {
    pub fn text(&self) -> Cow<'_, str> {
        match self {
            MessageContent::Text(s) => {
                if s.starts_with(LZ4_PREFIX) {
                    match decompress_text(s) {
                        Some(decompressed) => Cow::Owned(decompressed),
                        None => Cow::Borrowed(""),
                    }
                } else {
                    Cow::Borrowed(s.as_str())
                }
            }
            MessageContent::Parts(_) => Cow::Borrowed(""),
        }
    }

    /// Get raw text without decompression (for callers that handle compression themselves).
    pub fn raw_str(&self) -> &str {
        match self {
            MessageContent::Text(s) => s.as_str(),
            MessageContent::Parts(_) => "",
        }
    }

    /// Create Text, compressing with lz4 if > 1KB.
    pub fn from_text(text: String) -> Self {
        if text.len() > COMPRESS_THRESHOLD {
            if let Some(compressed) = compress_text(&text) {
                return MessageContent::Text(compressed);
            }
        }
        MessageContent::Text(text)
    }

    /// Return raw text without decompression (for serialization).
    pub fn raw_text(&self) -> &str {
        match self {
            MessageContent::Text(s) => s.as_str(),
            MessageContent::Parts(_) => "",
        }
    }

    /// Return true if this content is lz4-compressed.
    pub fn is_compressed(&self) -> bool {
        matches!(self, MessageContent::Text(s) if s.starts_with(LZ4_PREFIX))
    }
}

/// Compress text with lz4, returning prefixed string. Returns None on failure.
pub fn compress_text(text: &str) -> Option<String> {
    let compressed = lz4_flex::compress_prepend_size(text.as_bytes());
    let encoded = base64_encode(&compressed);
    Some(format!("{LZ4_PREFIX}{encoded}"))
}

/// Decompress lz4-prefixed text. Returns None on failure.
pub fn decompress_text(s: &str) -> Option<String> {
    let encoded = s.strip_prefix(LZ4_PREFIX)?;
    let compressed = base64_decode(encoded)?;
    let bytes = lz4_flex::decompress_size_prepended(&compressed).ok()?;
    String::from_utf8(bytes).ok()
}

/// Minimal base64 encode (no external dep needed for this use case).
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() * 4 / 3) + 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 {
            return None;
        }
        let a = val(chunk[0])? as u32;
        let b = val(chunk[1])? as u32;
        let c = if chunk.len() > 2 && chunk[2] != b'=' {
            val(chunk[2])? as u32
        } else {
            0
        };
        let d = if chunk.len() > 3 && chunk[3] != b'=' {
            val(chunk[3])? as u32
        } else {
            0
        };
        let triple = (a << 18) | (b << 12) | (c << 6) | d;
        out.push((triple >> 16) as u8);
        if chunk.len() > 2 && chunk[2] != b'=' {
            out.push((triple >> 8) as u8);
        }
        if chunk.len() > 3 && chunk[3] != b'=' {
            out.push(triple as u8);
        }
    }
    Some(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── sanitize_tool_messages tests ──

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
        // assistant has tool_calls but NO tool result follows
        let mut msgs = vec![
            user_msg("hi"),
            assistant_with_calls(&["orphan"]),
            assistant_msg("I continued without waiting"),
        ];
        sanitize_tool_messages(&mut msgs);
        // orphan tool_calls should be removed
        assert!(msgs[1].tool_calls.is_none());
        // orphan tool result messages should also be gone (none in this case)
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn test_sanitize_removes_orphan_result() {
        // tool result with no matching tool_call
        let mut msgs = vec![
            user_msg("hi"),
            assistant_msg("hello"),
            tool_result("no_match"),
            assistant_msg("done"),
        ];
        sanitize_tool_messages(&mut msgs);
        // orphan tool result should be removed
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[2].role, Role::Assistant);
    }

    #[test]
    fn test_sanitize_keeps_partial_pairs() {
        // assistant has 2 calls, only 1 has result
        let mut msgs = vec![
            user_msg("hi"),
            assistant_with_calls(&["c1", "orphan"]),
            tool_result("c1"),
            assistant_msg("done"),
        ];
        sanitize_tool_messages(&mut msgs);
        // c1 should be kept, orphan removed
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
        // Simulate DeepSeek 400: context compression dropped tool results
        // but left assistant message with tool_calls
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
            // MISSING: tool_result("call_1") — compression dropped it
            Message {
                role: Role::System,
                content: MessageContent::Text("[Context compressed: 2 older messages summarised]".into()),
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
        // call_1 orphan → tool_calls should be None on that assistant
        let orphan_asst = msgs.iter().find(|m| {
            m.role == Role::Assistant
                && matches!(&m.content, MessageContent::Text(s) if s.is_empty())
                && m.tool_calls.as_ref().map_or(true, |c| c.is_empty())
        });
        assert!(orphan_asst.is_some(), "orphan assistant should have tool_calls cleared");
        // call_2 still paired → kept
        let paired_asst = msgs.iter().find(|m| {
            m.role == Role::Assistant
                && m.tool_calls.as_ref().map_or(false, |c| c.iter().any(|tc| tc.id == "call_2"))
        });
        assert!(paired_asst.is_some(), "call_2 should still be paired");
        // No orphan tool results
        assert!(!msgs.iter().any(|m| m.role == Role::Tool && m.tool_call_id.as_deref() == Some("call_1")));
    }

    #[test]
    fn test_sanitize_empty() {
        let mut msgs = vec![];
        sanitize_tool_messages(&mut msgs);
        assert!(msgs.is_empty());
    }

    // ── lz4 compression tests ──

    #[test]
    fn compress_decompress_small() {
        let input = "Hello, World!";
        let compressed = compress_text(input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn compress_decompress_exactly_1kb() {
        let input = "x".repeat(1024);
        let compressed = compress_text(&input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn compress_decompress_large() {
        let input = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(200);
        assert!(input.len() > 10_000);
        let compressed = compress_text(&input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn compress_decompress_unicode() {
        let input = "😀🎉💻🚀✨αβγδελΩΣΠ".repeat(50);
        let compressed = compress_text(&input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn compress_decompress_cjk() {
        let input = "日本語テスト文字列です。这是中文测试文本。한국어 테스트 문자열입니다.".repeat(80);
        assert!(input.len() > 1024);
        let compressed = compress_text(&input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn compress_decompress_newlines_and_special() {
        let input = "line1\nline2\r\nline3\tindented\n\n".repeat(100);
        let compressed = compress_text(&input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn compress_decompress_repeated_patterns() {
        let input = "ABCD".repeat(5000);
        let compressed = compress_text(&input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn compress_decompress_null_bytes() {
        let input = "\x00hello\x00world\x00".repeat(200);
        let compressed = compress_text(&input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn decompress_invalid_returns_none() {
        assert!(decompress_text("not compressed").is_none());
        assert!(decompress_text(&format!("{LZ4_PREFIX}!!!bad_base64!!!")).is_none());
        assert!(decompress_text(&format!("{LZ4_PREFIX}{}", base64_encode(b"garbage"))).is_none());
    }

    // ── MessageContent::from_text auto-compression threshold ──

    #[test]
    fn from_text_empty() {
        let mc = MessageContent::from_text(String::new());
        assert!(!mc.is_compressed());
        assert_eq!(mc.text(), "");
    }

    #[test]
    fn from_text_small_no_compress() {
        let input = "Hello, World!";
        let mc = MessageContent::from_text(input.to_string());
        assert!(!mc.is_compressed());
        assert_eq!(mc.text(), input);
    }

    #[test]
    fn from_text_exactly_at_threshold_no_compress() {
        let input = "x".repeat(COMPRESS_THRESHOLD);
        let mc = MessageContent::from_text(input.clone());
        assert!(!mc.is_compressed());
        assert_eq!(mc.text(), input);
    }

    #[test]
    fn from_text_just_above_threshold_compresses() {
        let input = "x".repeat(COMPRESS_THRESHOLD + 1);
        let mc = MessageContent::from_text(input.clone());
        assert!(mc.is_compressed());
        assert_eq!(mc.text(), input);
    }

    #[test]
    fn from_text_large_compresses() {
        let input = "Some large content. ".repeat(200);
        assert!(input.len() > COMPRESS_THRESHOLD);
        let mc = MessageContent::from_text(input.clone());
        assert!(mc.is_compressed());
        assert_eq!(mc.text(), input);
    }

    #[test]
    fn from_text_cjk_above_threshold_compresses() {
        let input = "中文测试".repeat(200);
        assert!(input.len() > COMPRESS_THRESHOLD);
        let mc = MessageContent::from_text(input.clone());
        assert!(mc.is_compressed());
        assert_eq!(mc.text(), input);
    }

    // ── MessageContent::text() ──

    #[test]
    fn text_uncompressed_returns_borrowed() {
        let input = "hello world".to_string();
        let mc = MessageContent::Text(input.clone());
        let cow = mc.text();
        assert!(matches!(cow, Cow::Borrowed(_)));
        assert_eq!(&*cow, "hello world");
    }

    #[test]
    fn text_compressed_returns_owned() {
        let input = "x".repeat(2000);
        let mc = MessageContent::from_text(input.clone());
        assert!(mc.is_compressed());
        let cow = mc.text();
        assert!(matches!(cow, Cow::Owned(_)));
        assert_eq!(&*cow, input);
    }

    #[test]
    fn text_parts_returns_empty() {
        let mc = MessageContent::Parts(vec![ContentPart::Text {
            text: "ignored".into(),
        }]);
        assert_eq!(mc.text(), "");
    }

    #[test]
    fn text_compressed_invalid_returns_empty() {
        let mc = MessageContent::Text(format!("{LZ4_PREFIX}bad"));
        assert_eq!(mc.text(), "");
    }

    // ── MessageContent::raw_str() vs text() ──

    #[test]
    fn raw_str_uncompressed_equals_text() {
        let input = "hello world".to_string();
        let mc = MessageContent::Text(input.clone());
        assert_eq!(mc.raw_str(), "hello world");
        assert_eq!(mc.raw_str(), mc.text().as_ref());
    }

    #[test]
    fn raw_str_compressed_differs_from_text() {
        let input = "x".repeat(2000);
        let mc = MessageContent::from_text(input);
        let raw = mc.raw_str();
        assert!(raw.starts_with(LZ4_PREFIX));
        assert_ne!(raw, mc.text().as_ref());
        assert!(!raw.starts_with("xxxx"));
    }

    #[test]
    fn raw_str_parts_returns_empty() {
        let mc = MessageContent::Parts(vec![ContentPart::Text {
            text: "ignored".into(),
        }]);
        assert_eq!(mc.raw_str(), "");
    }

    // ── MessageContent::is_compressed() ──

    #[test]
    fn is_compressed_uncompressed_text() {
        let mc = MessageContent::Text("hello".to_string());
        assert!(!mc.is_compressed());
    }

    #[test]
    fn is_compressed_compressed_text() {
        let mc = MessageContent::from_text("x".repeat(2000));
        assert!(mc.is_compressed());
    }

    #[test]
    fn is_compressed_parts() {
        let mc = MessageContent::Parts(vec![]);
        assert!(!mc.is_compressed());
    }

    #[test]
    fn is_compressed_empty_not_compressed() {
        let mc = MessageContent::from_text(String::new());
        assert!(!mc.is_compressed());
    }

    #[test]
    fn is_compressed_threshold_boundary() {
        let mc_below = MessageContent::from_text("x".repeat(COMPRESS_THRESHOLD));
        assert!(!mc_below.is_compressed(), "at threshold should not compress");

        let mc_above = MessageContent::from_text("x".repeat(COMPRESS_THRESHOLD + 1));
        assert!(mc_above.is_compressed(), "above threshold should compress");
    }

    // ── serialization round-trip ──

    #[test]
    fn serde_message_text_uncompressed() {
        let msg = Message {
            role: Role::User,
            content: MessageContent::Text("hello".to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deser: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.role, Role::User);
        assert_eq!(deser.content.text(), "hello");
    }

    #[test]
    fn serde_message_text_compressed() {
        let input = "x".repeat(2000);
        let msg = Message {
            role: Role::User,
            content: MessageContent::from_text(input.clone()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deser: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.role, Role::User);
        assert!(deser.content.is_compressed());
        assert_eq!(deser.content.text(), input);
    }

    #[test]
    fn serde_message_parts() {
        let msg = Message {
            role: Role::User,
            content: MessageContent::Parts(vec![ContentPart::Text {
                text: "part1".into(),
            }]),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deser: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.role, Role::User);
        assert!(matches!(deser.content, MessageContent::Parts(_)));
    }

    #[test]
    fn test_compress_decompress_empty() {
        let input = "";
        let compressed = compress_text(input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn test_compress_decompress_unicode() {
        let input = "日本語テスト中文测试한국어테스트";
        let compressed = compress_text(input).unwrap();
        let decompressed = decompress_text(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn test_base64_roundtrip_random() {
        let data: Vec<u8> = (0..1000).map(|i| ((i * 7 + 13) ^ (i >> 3)) as u8).collect();
        let encoded = base64_encode(&data);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_message_content_from_text_small() {
        let input = "short text";
        let mc = MessageContent::from_text(input.to_string());
        assert!(!mc.is_compressed());
        assert_eq!(mc.text(), input);
    }

    #[test]
    fn test_message_content_from_text_large() {
        let input = "x".repeat(COMPRESS_THRESHOLD + 100);
        let mc = MessageContent::from_text(input.clone());
        assert!(mc.is_compressed());
        assert_eq!(mc.text(), input);
    }

    #[test]
    fn test_message_content_cow_borrowed() {
        let mc = MessageContent::Text("hello".to_string());
        let cow = mc.text();
        assert!(matches!(cow, Cow::Borrowed(_)));
    }

    #[test]
    fn test_message_content_cow_owned() {
        let input = "x".repeat(2000);
        let mc = MessageContent::from_text(input);
        assert!(mc.is_compressed());
        let cow = mc.text();
        assert!(matches!(cow, Cow::Owned(_)));
    }
}

// ── Tool types ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
    pub is_error: bool,
}

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

// ── Provider response ──

#[derive(Debug, Clone)]
pub enum ProviderEvent {
    Text(String),
    Reasoning(String),
    ToolCalls(Vec<ToolCall>),
    Done,
    #[allow(dead_code)]
    Error(String),
}

// ── Session config ──

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub provider: ProviderKind,
    pub model: String,
    pub api_key: String,
    pub api_base: Option<String>,
    pub max_iterations: usize,
    pub system_prompt: String,
    /// Timeout per LLM request (seconds)
    pub llm_timeout_secs: u64,
    /// Timeout per tool execution (seconds)
    pub tool_timeout_secs: u64,
    /// Heartbeat interval during long ops (seconds, 0 = disabled)
    pub heartbeat_interval_secs: u64,
    /// Max parallel tool executions
    #[allow(dead_code)]
    pub concurrency: usize,
    /// Render markdown in terminal output
    #[allow(dead_code)]
    pub use_markdown: bool,
    /// Agent operating mode
    pub mode: AgentMode,
    /// Max context tokens before compression kicks in (default 120k)
    pub max_context_tokens: usize,
    /// Compress when context exceeds this ratio of max_context_tokens (default 0.8)
    pub context_compress_ratio: f64,
    /// Whether the harness should auto-continue when the orchestrator has ready tasks.
    /// When false, the harness stops after each LLM turn and waits for the user.
    pub auto_continue: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentMode {
    /// All tools available (default)
    Auto,
    /// Read-only: only read_file, search_code, find_files
    Plan,
    /// Write-enabled: all tools including edit_file, write_file, run_command
    Exec,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderKind {
    OpenAI,
    Anthropic,
    Ollama,
}

impl ProviderKind {
    pub fn name(&self) -> &str {
        match self {
            ProviderKind::OpenAI => "openai",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Ollama => "ollama",
        }
    }

    pub fn default_base(&self) -> &str {
        match self {
            ProviderKind::OpenAI => "https://api.openai.com/v1",
            ProviderKind::Anthropic => "https://api.anthropic.com/v1",
            ProviderKind::Ollama => "http://localhost:11434/v1",
        }
    }

    pub fn default_model(&self) -> &str {
        match self {
            ProviderKind::OpenAI => "gpt-4o",
            ProviderKind::Anthropic => "claude-sonnet-4-20250514",
            ProviderKind::Ollama => "codellama",
        }
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            provider: ProviderKind::OpenAI,
            model: ProviderKind::OpenAI.default_model().into(),
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            api_base: None,
            max_iterations: 32,
            system_prompt: default_system_prompt(),
            llm_timeout_secs: 120,
            tool_timeout_secs: 300,
            heartbeat_interval_secs: 10,
            concurrency: 8,
            use_markdown: true,
            mode: AgentMode::Auto,
            max_context_tokens: 1_000_000,
            context_compress_ratio: 0.8,
            auto_continue: true,
        }
    }
}

pub fn default_system_prompt() -> String {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "unknown".into());
    let os = std::env::consts::OS;
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "?".into());
    let env_info = format!("\n\n## Environment\n- OS: {os}\n- Shell: {shell}\n- Workspace: {cwd}\n- Line endings: LF (use \\n, not \\r\\n)");

    // Inject core memory into prompt
    let mem = crate::memory::Memory::load().unwrap_or_default();
    let core_ctx = mem.core_context();

    if let Ok(content) = std::fs::read_to_string("system_prompt.md") {
        if !content.trim().is_empty() {
            return content + &env_info + &core_ctx;
        }
    }
    r#"You are Radiumical, a lean CLI coding agent. Your job is to help the user with software engineering tasks."#.to_string() + &env_info + &core_ctx + "\n\n" + r#"

## How you work
1. Read and understand the codebase using tools before making changes.
2. Use `search_code` (regex grep) and `find_files` (glob) to locate relevant code.
3. Use `read_file` to examine files. NEVER assume file contents.
4. Use `write_file` to create or overwrite files. Use `edit_file` for targeted changes.
5. Use `run_command` to execute build, test, or diagnostic commands.
6. Always validate your changes by running tests or builds when possible.

## Rules
- Be precise. Make minimal, focused edits. Do not change unrelated code.
- Match the existing code style and conventions.
- Explain your reasoning concisely before making changes.
- If you're uncertain, inspect the code first. Never guess.
- Report what you changed and why when done.
"#
}
#[derive(Debug)]
pub enum UiEvent {
    LlmChunk(String),
    LlmReasoning(String),
    ThinkingTick,
    LlmDone,
    ToolStart {
        name: String,
        index: usize,
        total: usize,
        args: String,
    },
    ToolDone,
    ToolResult {
        content: String,
    },
    Choice {
        id: String,
        mode: String,
        options: Vec<String>,
    },
    Error(String),
    ThinkingDone,
    ProvidersLoaded(Vec<ProviderSource>),
    ModelsLoaded(Vec<String>),
    Toast {
        message: String,
        level: String,
        duration_secs: u64,
    },
    TitleGenerated(String),
    SubAgentDone {
        id: String,
        success: bool,
    },
    McpStatus {
        name: String,
        alive: bool,
        tool_count: usize,
    },
    PlanUpdated {
        title: String,
        tasks: Vec<PlanTaskUpdate>,
    },
}

#[derive(Debug, Clone)]
pub struct PlanTaskUpdate {
    pub id: u32,
    pub title: String,
    pub status: crate::orchestrator::TaskStatus,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum BackendCmd {
    RunTask(String),
    Cancel,
    ChoiceResponse { id: String, value: String },
    SetModel(String),
    SetMode(AgentMode),
    SetThinkingEffort(String),
    RefreshModels,
    FetchProviders,
    FetchModels(ProviderSource),
    /// Reset the backend conversation history (e.g. /new).
    ResetConversation,
    /// Load a saved session into the backend conversation.
    LoadSession(Vec<SessionItem>),
    /// Toggle an MCP server on/off by name.
    ToggleMcpServer { name: String },
}

// ═══ Slash hints ═══

// copied
