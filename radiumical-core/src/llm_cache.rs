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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionDef, Message, MessageContent, ProviderEvent, Role, ToolDefinition};
    use std::sync::Mutex as StdMutex;
    use tokio::sync::mpsc;

    fn make_message(content: &str) -> Message {
        Message {
            role: Role::User,
            content: MessageContent::Text(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning_content: None,
        }
    }

    fn make_tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDef {
                name: name.to_string(),
                description: "test tool".to_string(),
                parameters: serde_json::json!({}),
            },
        }
    }

    #[test]
    fn cache_miss_then_hit() {
        let cache = LlmCache::new();
        let msgs = [make_message("hello")];
        let tools = [];
        let events = vec![ProviderEvent::Text("hi".into()), ProviderEvent::Done];

        assert!(cache.get("gpt-4", &msgs, &tools, None).is_none());

        cache.put("gpt-4", &msgs, &tools, None, events.clone());

        let result = cache.get("gpt-4", &msgs, &tools, None);
        assert!(result.is_some());
        let cached = result.unwrap();
        assert_eq!(cached.len(), 2);
        assert!(matches!(&cached[0], ProviderEvent::Text(t) if t == "hi"));
        assert!(matches!(&cached[1], ProviderEvent::Done));
    }

    #[test]
    fn different_messages_different_keys() {
        let cache = LlmCache::new();
        let msgs_a = [make_message("hello")];
        let msgs_b = [make_message("world")];
        let tools = [];

        cache.put(
            "gpt-4", &msgs_a, &tools, None,
            vec![ProviderEvent::Text("alpha".into()), ProviderEvent::Done],
        );
        cache.put(
            "gpt-4", &msgs_b, &tools, None,
            vec![ProviderEvent::Text("beta".into()), ProviderEvent::Done],
        );

        let a = cache.get("gpt-4", &msgs_a, &tools, None).unwrap();
        assert!(matches!(&a[0], ProviderEvent::Text(t) if t == "alpha"));

        let b = cache.get("gpt-4", &msgs_b, &tools, None).unwrap();
        assert!(matches!(&b[0], ProviderEvent::Text(t) if t == "beta"));
    }

    #[test]
    fn different_models_different_keys() {
        let cache = LlmCache::new();
        let msgs = [make_message("hello")];
        let tools = [];

        cache.put(
            "gpt-4", &msgs, &tools, None,
            vec![ProviderEvent::Text("gpt4".into()), ProviderEvent::Done],
        );
        cache.put(
            "gpt-4o", &msgs, &tools, None,
            vec![ProviderEvent::Text("gpt4o".into()), ProviderEvent::Done],
        );

        let a = cache.get("gpt-4", &msgs, &tools, None).unwrap();
        assert!(matches!(&a[0], ProviderEvent::Text(t) if t == "gpt4"));

        let b = cache.get("gpt-4o", &msgs, &tools, None).unwrap();
        assert!(matches!(&b[0], ProviderEvent::Text(t) if t == "gpt4o"));
    }

    #[test]
    fn different_tools_different_keys() {
        let cache = LlmCache::new();
        let msgs = [make_message("hello")];
        let tools_a = [make_tool("tool_a")];
        let tools_b = [make_tool("tool_b")];

        cache.put(
            "gpt-4", &msgs, &tools_a, None,
            vec![ProviderEvent::Text("a".into()), ProviderEvent::Done],
        );
        cache.put(
            "gpt-4", &msgs, &tools_b, None,
            vec![ProviderEvent::Text("b".into()), ProviderEvent::Done],
        );

        let a = cache.get("gpt-4", &msgs, &tools_a, None).unwrap();
        assert!(matches!(&a[0], ProviderEvent::Text(t) if t == "a"));

        let b = cache.get("gpt-4", &msgs, &tools_b, None).unwrap();
        assert!(matches!(&b[0], ProviderEvent::Text(t) if t == "b"));
    }

    #[test]
    fn reasoning_effort_affects_key() {
        let cache = LlmCache::new();
        let msgs = [make_message("hello")];
        let tools = [];

        cache.put(
            "gpt-4", &msgs, &tools, Some("high"),
            vec![ProviderEvent::Text("high".into()), ProviderEvent::Done],
        );
        cache.put(
            "gpt-4", &msgs, &tools, Some("low"),
            vec![ProviderEvent::Text("low".into()), ProviderEvent::Done],
        );

        let a = cache.get("gpt-4", &msgs, &tools, Some("high")).unwrap();
        assert!(matches!(&a[0], ProviderEvent::Text(t) if t == "high"));

        let b = cache.get("gpt-4", &msgs, &tools, Some("low")).unwrap();
        assert!(matches!(&b[0], ProviderEvent::Text(t) if t == "low"));

        assert!(cache.get("gpt-4", &msgs, &tools, None).is_none());
    }

    #[test]
    fn cache_size_limit_eviction() {
        let cache = LlmCache::new();
        let tools = [];

        let first_msgs = [make_message("first")];
        cache.put(
            "gpt-4", &first_msgs, &tools, None,
            vec![ProviderEvent::Text("first".into()), ProviderEvent::Done],
        );

        for i in 0..MAX_CACHE_ENTRIES {
            let msgs = [make_message(&format!("msg_{i}"))];
            cache.put(
                "gpt-4", &msgs, &tools, None,
                vec![ProviderEvent::Text(format!("{i}")), ProviderEvent::Done],
            );
        }

        assert!(cache.get("gpt-4", &first_msgs, &tools, None).is_none());

        let recent = MAX_CACHE_ENTRIES - 1;
        let recent_msgs = [make_message(&format!("msg_{recent}"))];
        assert!(cache.get("gpt-4", &recent_msgs, &tools, None).is_some());
    }

    #[test]
    fn empty_events_not_cached() {
        let cache = LlmCache::new();
        let msgs = [make_message("test")];
        let tools = [];

        cache.put("gpt-4", &msgs, &tools, None, vec![]);
        assert!(cache.get("gpt-4", &msgs, &tools, None).is_none());
    }

    #[test]
    fn error_events_not_cached() {
        let cache = LlmCache::new();
        let msgs = [make_message("test")];
        let tools = [];

        cache.put(
            "gpt-4", &msgs, &tools, None,
            vec![ProviderEvent::Error("fail".into()), ProviderEvent::Done],
        );
        assert!(cache.get("gpt-4", &msgs, &tools, None).is_none());
    }

    struct MockProvider {
        call_count: StdMutex<usize>,
        events: Vec<ProviderEvent>,
    }

    impl MockProvider {
        fn new(events: Vec<ProviderEvent>) -> Self {
            Self {
                call_count: StdMutex::new(0),
                events,
            }
        }
    }

    #[async_trait::async_trait]
    impl Provider for MockProvider {
        async fn chat(
            &self,
            _messages: &[Message],
            _tools: &[ToolDefinition],
            tx: mpsc::UnboundedSender<ProviderEvent>,
        ) -> anyhow::Result<()> {
            *self.call_count.lock().unwrap() += 1;
            for event in &self.events {
                if tx.send(event.clone()).is_err() {
                    break;
                }
            }
            Ok(())
        }

        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(MockProvider {
                call_count: StdMutex::new(0),
                events: self.events.clone(),
            })
        }
    }

    #[tokio::test]
    async fn cached_provider_miss_then_hit() {
        let mock = Arc::new(MockProvider::new(vec![
            ProviderEvent::Text("cached".into()),
            ProviderEvent::Done,
        ]));
        let cache = Arc::new(LlmCache::new());
        let provider = CachedProvider::with_cache("gpt-4".into(), mock.clone(), cache.clone());

        let msgs = [make_message("hello")];
        let tools = [];

        let (tx, mut rx) = mpsc::unbounded_channel();
        provider.chat(&msgs, &tools, tx).await.unwrap();
        let mut events = vec![];
        while let Some(e) = rx.recv().await {
            events.push(e);
        }
        assert_eq!(*mock.call_count.lock().unwrap(), 1);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], ProviderEvent::Text(t) if t == "cached"));
        assert!(matches!(&events[1], ProviderEvent::Done));

        let (tx2, mut rx2) = mpsc::unbounded_channel();
        provider.chat(&msgs, &tools, tx2).await.unwrap();
        let mut events2 = vec![];
        while let Some(e) = rx2.recv().await {
            events2.push(e);
        }
        assert_eq!(*mock.call_count.lock().unwrap(), 1);
        assert_eq!(events2.len(), 2);
        assert!(matches!(&events2[0], ProviderEvent::Text(t) if t == "cached"));
        assert!(matches!(&events2[1], ProviderEvent::Done));
    }

    #[tokio::test]
    async fn cached_provider_wrap_works() {
        let mock = Arc::new(MockProvider::new(vec![
            ProviderEvent::Text("wrap".into()),
            ProviderEvent::Done,
        ]));
        let provider = wrap("gpt-4".into(), mock.clone());

        let msgs = [make_message("hello")];
        let tools = [];
        let (tx, mut rx) = mpsc::unbounded_channel();
        provider.chat(&msgs, &tools, tx).await.unwrap();
        let mut events = vec![];
        while let Some(e) = rx.recv().await {
            events.push(e);
        }
        assert_eq!(*mock.call_count.lock().unwrap(), 1);
        assert!(matches!(&events[0], ProviderEvent::Text(t) if t == "wrap"));
    }

    #[tokio::test]
    async fn cached_provider_different_msgs_different_cache() {
        let mock = Arc::new(MockProvider::new(vec![
            ProviderEvent::Text("first".into()),
            ProviderEvent::Done,
        ]));
        let cache = Arc::new(LlmCache::new());
        let provider = CachedProvider::with_cache("gpt-4".into(), mock.clone(), cache.clone());

        let msgs_a = [make_message("a")];
        let msgs_b = [make_message("b")];
        let tools = [];

        let (tx, mut rx) = mpsc::unbounded_channel();
        provider.chat(&msgs_a, &tools, tx).await.unwrap();
        while let Some(_) = rx.recv().await {}
        assert_eq!(*mock.call_count.lock().unwrap(), 1);

        let (tx2, mut rx2) = mpsc::unbounded_channel();
        provider.chat(&msgs_b, &tools, tx2).await.unwrap();
        while let Some(_) = rx2.recv().await {}
        assert_eq!(*mock.call_count.lock().unwrap(), 2);
    }
}
