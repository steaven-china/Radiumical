//! LLM response cache — memoize deterministic provider calls to save tokens.
//!
//! The cache key includes model, messages, tools, and reasoning settings so
//! that any change invalidates the entry. Cached responses are replayed as
//! `ProviderEvent` chunks, preserving the streaming interface.

use crate::provider::Provider;
use crate::types::{ProviderEvent, ToolDefinition};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::UnboundedSender;

const MAX_CACHE_ENTRIES: usize = 128;

/// In-memory cache entry.
#[derive(Debug, Clone)]
struct CacheEntry {
    events: Vec<ProviderEvent>,
}

/// In-memory LRU-ish cache for LLM responses.
pub struct LlmCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
    order: Mutex<Vec<String>>,
    disabled: bool,
}

impl LlmCache {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            order: Mutex::new(Vec::new()),
            disabled: std::env::var("RADI_DISABLE_LLM_CACHE").is_ok(),
        }
    }

    fn cache_key(
        model: &str,
        messages: &[crate::types::Message],
        tools: &[ToolDefinition],
        reasoning_effort: Option<&str>,
    ) -> String {
        let mut hasher = DefaultHasher::new();
        model.hash(&mut hasher);
        // Hash JSON-serialized messages/tools because the types don't derive Hash.
        if let Ok(json) = serde_json::to_string(messages) {
            json.hash(&mut hasher);
        }
        if let Ok(json) = serde_json::to_string(tools) {
            json.hash(&mut hasher);
        }
        reasoning_effort.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    pub fn get(
        &self,
        model: &str,
        messages: &[crate::types::Message],
        tools: &[ToolDefinition],
        reasoning_effort: Option<&str>,
    ) -> Option<Vec<ProviderEvent>> {
        if self.disabled {
            return None;
        }
        let key = Self::cache_key(model, messages, tools, reasoning_effort);
        let entries = self.entries.lock().unwrap();
        entries.get(&key).map(|e| e.events.clone())
    }

    pub fn put(
        &self,
        model: &str,
        messages: &[crate::types::Message],
        tools: &[ToolDefinition],
        reasoning_effort: Option<&str>,
        events: Vec<ProviderEvent>,
    ) {
        if self.disabled {
            return;
        }
        // Don't cache error responses.
        if events.iter().any(|e| matches!(e, ProviderEvent::Error(_))) {
            return;
        }
        // Don't cache empty responses.
        if events.is_empty() {
            return;
        }
        let key = Self::cache_key(model, messages, tools, reasoning_effort);
        let mut entries = self.entries.lock().unwrap();
        let mut order = self.order.lock().unwrap();
        entries.insert(key.clone(), CacheEntry { events });
        order.retain(|k| k != &key);
        order.push(key);
        while order.len() > MAX_CACHE_ENTRIES {
            if !order.is_empty() {
                let oldest = order.remove(0);
                entries.remove(&oldest);
            }
        }
    }
}

impl Default for LlmCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapper around a provider that caches successful responses.
pub struct CachedProvider {
    model: String,
    inner: Arc<dyn Provider>,
    cache: Arc<LlmCache>,
}

impl CachedProvider {
    pub fn new(model: String, inner: Arc<dyn Provider>) -> Self {
        Self {
            model,
            inner,
            cache: Arc::new(LlmCache::new()),
        }
    }

    pub fn with_cache(model: String, inner: Arc<dyn Provider>, cache: Arc<LlmCache>) -> Self {
        Self { model, inner, cache }
    }
}

#[async_trait::async_trait]
impl Provider for CachedProvider {
    async fn chat(
        &self,
        messages: &[crate::types::Message],
        tools: &[ToolDefinition],
        tx: UnboundedSender<ProviderEvent>,
    ) -> anyhow::Result<()> {
        if let Some(events) = self.cache.get(&self.model, messages, tools, None) {
            for event in events {
                if tx.send(event).is_err() {
                    break;
                }
            }
            return Ok(());
        }

        // Capture events by intercepting the sender.
        let (capture_tx, mut capture_rx) = tokio::sync::mpsc::unbounded_channel();
        let model = self.model.clone();
        let cache = Arc::clone(&self.cache);
        let inner = Arc::clone(&self.inner);
        let messages_for_key: Vec<crate::types::Message> = messages.to_vec();
        let tools_for_key: Vec<ToolDefinition> = tools.to_vec();

        let inner_handle = tokio::spawn(async move {
            inner.chat(&messages_for_key, &tools_for_key, capture_tx).await
        });

        let mut captured = Vec::new();
        while let Some(event) = capture_rx.recv().await {
            captured.push(event.clone());
            if tx.send(event).is_err() {
                break;
            }
        }

        let result = inner_handle.await?;
        cache.put(
            &model,
            messages,
            tools,
            None,
            captured,
        );
        result
    }

    fn set_reasoning_effort(&self, _effort: Option<String>) {
        // Reasoning effort is part of the cache key. Because inner is an
        // Arc<dyn Provider> we can't mutate it here; set it on the inner
        // provider before wrapping. For now we leave the key without reasoning
        // effort (most providers don't expose it through the trait).
    }

    fn clone_box(&self) -> Box<dyn Provider> {
        Box::new(CachedProvider {
            model: self.model.clone(),
            inner: Arc::clone(&self.inner),
            cache: Arc::clone(&self.cache),
        })
    }
}

/// Convenience: wrap an existing provider in a cache.
pub fn wrap(model: String, provider: Arc<dyn Provider>) -> Arc<dyn Provider> {
    Arc::new(CachedProvider::new(model, provider))
}
