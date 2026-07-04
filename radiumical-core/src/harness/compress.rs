//! Context-window compression — summarises old messages when token count exceeds threshold.

use crate::conversation::Conversation;
use crate::provider::Provider;
use crate::types::{Message, MessageContent, ProviderEvent, Role, SessionConfig, UiEvent};
use std::sync::Arc;

/// Compress the conversation context by summarising older messages via the LLM.
///
/// Returns the number of messages that were compressed, or 0 if the context
/// was within budget.
pub async fn compress_context(
    config: &SessionConfig,
    provider: &Arc<dyn Provider>,
    conversation: &mut Conversation,
    ui_tx: &tokio::sync::mpsc::Sender<UiEvent>,
) -> usize {
    const KEEP_RECENT: usize = 6;

    let max_tokens = config.max_context_tokens;
    let threshold = (max_tokens as f64 * config.context_compress_ratio) as usize;
    let est = conversation.estimate_tokens();

    if est <= threshold || conversation.messages().len() <= KEEP_RECENT + 2 {
        return 0;
    }

    let total = conversation.messages().len();
    let split_at = total.saturating_sub(KEEP_RECENT).max(2);

    let range_text = conversation.messages()[1..split_at]
        .iter()
        .filter_map(|m| {
            let role = match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            };
            let text = match &m.content {
                MessageContent::Text(s) => s.as_str(),
                _ => "",
            };
            if text.is_empty() {
                return None;
            }
            Some(format!("[{role}] {text}"))
        })
        .collect::<Vec<_>>()
        .join("\n");

    if range_text.trim().is_empty() {
        conversation.compress_range(
            split_at,
            "[Context compressed: empty messages dropped]".into(),
        );
        return split_at - 1;
    }

    if let Err(e) = ui_tx
        .send(UiEvent::LlmChunk("\n[Compressing context…]\n".into()))
        .await
    {
        tracing::warn!(error = %e, "failed to send context compression notice to UI");
    }

    let compress_messages = vec![
        Message {
            role: Role::System,
            content: MessageContent::Text(
                "You are a conversation compressor. Given a conversation history, produce a \
                 concise summary (≤400 words) that preserves:\n\
                 1. What files were read/edited and key content found.\n\
                 2. What changes were made (edits, writes, commands run).\n\
                 3. Current task state and next steps.\n\
                 4. Any errors encountered and how they were resolved.\n\
                 5. Important decisions or constraints.\n\
                 Output ONLY the summary, no preamble."
                    .into(),
            ),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        },
        Message {
            role: Role::User,
            content: MessageContent::Text(range_text),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        },
    ];

    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let prov = Arc::clone(provider);
    let handle = tokio::spawn(async move { prov.chat(&compress_messages, &[], tx).await });

    let mut summary = String::new();
    while let Some(event) = rx.recv().await {
        match event {
            ProviderEvent::Text(chunk) => summary.push_str(&chunk),
            ProviderEvent::Done => break,
            ProviderEvent::Error(e) => {
                summary = format!("[Context compression failed: {e}]");
                break;
            }
            _ => {}
        }
    }
    let _ = handle.await;

    let summary = summary.trim().to_string();
    let compressed_count = split_at - 1;
    conversation.compress_range(
        split_at,
        format!("[Context compressed: {compressed_count} older messages summarised]\n\n{summary}"),
    );

    if let Err(e) = ui_tx
        .send(UiEvent::LlmChunk(format!(
            "[Context compressed: {compressed_count} messages → summary]\n"
        )))
        .await
    {
        tracing::warn!(error = %e, "failed to send compression result to UI");
    }

    compressed_count
}
