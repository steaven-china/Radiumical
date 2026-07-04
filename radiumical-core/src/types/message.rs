//! OpenAI-compatible message types for LLM communication.
//!
//! Includes transparent lz4 compression for large message content.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;

use super::compress::{compress_text, decompress_text, COMPRESS_THRESHOLD, LZ4_PREFIX};

/// A chat message in the OpenAI-compatible format.
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

/// Role of a message participant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// Message body — either a plain text string or a list of content parts.
/// Text content larger than 1 KB is transparently lz4-compressed.
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

/// A single content part within a multipart message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
}

/// A tool call requested by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: FunctionCall,
}

/// The function name and serialized arguments of a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

/// Describes a tool available to the LLM (sent in the API request).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDef,
}

/// Function metadata inside a [`ToolDefinition`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// The result of executing a tool, sent back to the LLM as a tool-role message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
    pub is_error: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(
            !mc_below.is_compressed(),
            "at threshold should not compress"
        );

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
