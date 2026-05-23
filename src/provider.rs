use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::types::{Message, ProviderEvent, ToolCall, ToolDefinition};

// ── Request / Response types (OpenAI-compatible) ──

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
    tools: &'a [ToolDefinition],
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<String>,
}

#[derive(Debug, Serialize)]
struct ThinkingConfig {
    #[serde(rename = "type")]
    thinking_type: String,
}

#[derive(Debug, Deserialize)]
struct ChatChunk {
    choices: Vec<ChoiceChunk>,
}

#[derive(Debug, Deserialize)]
struct ChoiceChunk {
    delta: Option<DeltaChunk>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeltaChunk {
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    index: usize,
    id: Option<String>,
    #[serde(rename = "type")]
    call_type: Option<String>,
    function: Option<FunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct FunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

// ── Provider trait ──

/// Abstracts over different LLM providers (OpenAI, DeepSeek, Anthropic, Ollama).
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    /// Stream a chat completion. Sends `ProviderEvent`s through the channel.
    async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        tx: mpsc::UnboundedSender<ProviderEvent>,
    ) -> Result<()>;
}

// ── OpenAI-compatible provider (works with OpenAI, DeepSeek, Ollama) ──

pub struct OpenAICompatibleProvider {
    client: reqwest::Client,
    api_base: String,
    api_key: String,
    model: String,
    reasoning_effort: Option<String>,
}

impl OpenAICompatibleProvider {
    pub fn new(api_base: &str, api_key: &str, model: &str) -> Self {
        let reasoning = if model.contains("deepseek") || model.contains("v4") {
            Some("max".to_string())
        } else {
            None
        };
        Self {
            client: reqwest::Client::new(),
            api_base: api_base.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            reasoning_effort: reasoning,
        }
    }
}

#[async_trait::async_trait]
impl Provider for OpenAICompatibleProvider {
    async fn chat(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
        tx: mpsc::UnboundedSender<ProviderEvent>,
    ) -> Result<()> {
        let url = format!("{}/chat/completions", self.api_base);

        let request = ChatRequest {
            model: &self.model,
            messages,
            tools,
            stream: true,
            thinking: self.reasoning_effort.as_ref().map(|_| ThinkingConfig {
                thinking_type: "enabled".into(),
            }),
            reasoning_effort: self.reasoning_effort.clone(),
        };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {status}: {body}"));
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        // Accumulate tool calls across chunks
        let mut tool_call_acc: Vec<ToolCallAccumulator> = Vec::new();
        let mut reasoning_buf = String::new();

        use futures::StreamExt;
        while let Some(chunk_result) = stream.next().await {
            let chunk_bytes = chunk_result?;
            let chunk_str = String::from_utf8_lossy(&chunk_bytes);
            buffer.push_str(&chunk_str);

            // Process complete SSE lines
            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                let data = line.strip_prefix("data: ").unwrap_or("");
                if data.is_empty() || data == "[DONE]" {
                    if data == "[DONE]" {
                        // Stream finished
                        break;
                    }
                    continue;
                }

                let chunk: ChatChunk = match serde_json::from_str(data) {
                    Ok(c) => c,
                    Err(_) => continue,
                };

                for choice in chunk.choices {
                    // Check finish reason
                    if let Some(ref reason) = choice.finish_reason {
                        if reason == "tool_calls" {
                            // We have complete tool calls
                            let tool_calls: Vec<ToolCall> = tool_call_acc
                                .iter()
                                .map(|acc| ToolCall {
                                    id: acc.id.clone().unwrap_or_default(),
                                    call_type: acc.call_type.clone().unwrap_or_else(|| "function".into()),
                                    function: crate::types::FunctionCall {
                                        name: acc.name.clone().unwrap_or_default(),
                                        arguments: acc.arguments.clone().unwrap_or_default(),
                                    },
                                })
                                .collect();

                            let _ = tx.send(ProviderEvent::ToolCalls(tool_calls));
                            let _ = tx.send(ProviderEvent::Done);
                            return Ok(());
                        }
                    }

                    if let Some(delta) = choice.delta {
                        // Reasoning / thinking content (DeepSeek)
                        if let Some(rc) = delta.reasoning_content {
                            if !rc.is_empty() {
                                reasoning_buf.push_str(&rc);
                                let _ = tx.send(ProviderEvent::Reasoning(rc));
                            }
                        }
                        // Text content
                        if let Some(content) = delta.content {
                            if !content.is_empty() {
                                let _ = tx.send(ProviderEvent::Text(content));
                            }
                        }

                        // Tool calls
                        if let Some(tc_deltas) = delta.tool_calls {
                            for tc_delta in tc_deltas {
                                while tool_call_acc.len() <= tc_delta.index {
                                    tool_call_acc.push(ToolCallAccumulator::default());
                                }
                                let acc = &mut tool_call_acc[tc_delta.index];

                                if let Some(id) = tc_delta.id {
                                    acc.id = Some(id);
                                }
                                if let Some(ct) = tc_delta.call_type {
                                    acc.call_type = Some(ct);
                                }
                                if let Some(func) = tc_delta.function {
                                    if let Some(name) = func.name {
                                        acc.name = Some(name);
                                    }
                                    if let Some(args) = func.arguments {
                                        acc.arguments.get_or_insert_default().push_str(&args);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // If we got here, stream ended without explicit [DONE] or tool_calls finish
        let _ = tx.send(ProviderEvent::Done);
        Ok(())
    }
}

#[derive(Default)]
struct ToolCallAccumulator {
    id: Option<String>,
    call_type: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
}

// ── Factory ──

use std::sync::Arc;

use crate::types::ProviderKind;

pub fn create_provider(kind: &ProviderKind, api_base: Option<&str>, api_key: &str, model: &str) -> Arc<dyn Provider> {
    let base = api_base.unwrap_or_else(|| kind.default_base());
    match kind {
        ProviderKind::OpenAI | ProviderKind::Ollama => {
            Arc::new(OpenAICompatibleProvider::new(base, api_key, model))
        }
        ProviderKind::Anthropic => {
            // TODO: implement Anthropic provider
            Arc::new(OpenAICompatibleProvider::new(base, api_key, model))
        }
    }
}
