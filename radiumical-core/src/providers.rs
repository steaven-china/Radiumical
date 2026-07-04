//! Remote provider registry: fetch, cache, and discover LLM endpoints.
//!
//! The registry is a JSONL file where each line is a [`ProviderSource`]:
//! ```jsonl
//! {"provider":"openai","name":"OpenAI","api_type":"openai-chat","api_base":"https://api.openai.com/v1","key_env":"OPENAI_API_KEY","models_endpoint":"/models"}
//! ```
//!
//! The client fetches the list from a remote URL (default `https://radiumical.dev/providers.jsonl`),
//! falls back to a local cache, and can refresh on demand.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

pub const DEFAULT_REGISTRY_URL: &str = "https://radiumical.dev/providers.jsonl";
const CACHE_FILE_NAME: &str = "providers.jsonl";
const CACHE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60); // 1 day

/// Bundled fallback provider list (always available, never stale).
const EMBEDDED_PROVIDERS: &str = include_str!("../../providers-record/providers.jsonl");

/// A single provider endpoint entry from the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSource {
    pub provider: String,
    pub name: String,
    #[serde(rename = "api_type")]
    pub api_type: String,
    pub api_base: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_env: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl ProviderSource {
    /// Resolve the API key for this source from its configured env var.
    pub fn api_key(&self) -> Option<String> {
        self.key_env.as_ref().and_then(|k| std::env::var(k).ok())
    }

    /// Full URL for the models list endpoint, if any.
    pub fn models_url(&self) -> Option<String> {
        self.models_endpoint.as_ref().map(|ep| {
            let base = self.api_base.trim_end_matches('/');
            let ep = ep.trim_start_matches('/');
            format!("{base}/{ep}")
        })
    }

    /// Whether this source can be used with our current adapters.
    pub fn is_supported(&self) -> bool {
        matches!(
            self.api_type.as_str(),
            "openai-chat" | "anthropic" | "ollama"
        )
    }
}

/// A discovered model from a provider endpoint.
#[derive(Debug, Clone)]
pub struct DiscoveredModel {
    pub id: String,
    pub source: ProviderSource,
}

/// Registry manager: fetches and caches provider sources.
#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    cache_dir: PathBuf,
    client: reqwest::Client,
}

impl ProviderRegistry {
    pub fn new(cache_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&cache_dir)
            .with_context(|| format!("create cache dir {}", cache_dir.display()))?;
        Ok(Self {
            cache_dir,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?,
        })
    }

    fn cache_path(&self) -> PathBuf {
        self.cache_dir.join(CACHE_FILE_NAME)
    }

    /// Fetch sources from the remote registry and refresh the local cache.
    pub async fn fetch(&self, url: &str) -> Result<Vec<ProviderSource>> {
        let text = self
            .client
            .get(url)
            .send()
            .await
            .with_context(|| format!("fetch registry from {url}"))?
            .text()
            .await
            .context("read registry response")?;

        let sources = parse_jsonl(&text)?;
        self.save_cache(&text)?;
        Ok(sources)
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Load sources from the local cache, returning `None` if stale or missing.
    pub fn load_cache(&self) -> Result<Option<Vec<ProviderSource>>> {
        let path = self.cache_path();
        if !path.exists() {
            return Ok(None);
        }

        let modified = fs::metadata(&path)?
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::MAX)
            > CACHE_MAX_AGE
        {
            return Ok(None);
        }

        let text = fs::read_to_string(&path)?;
        parse_jsonl(&text).map(Some)
    }

    /// Fetch remote registry, falling back to local cache, then embedded list.
    pub async fn fetch_or_cache(&self, url: &str) -> Result<Vec<ProviderSource>> {
        match self.fetch(url).await {
            Ok(sources) => Ok(sources),
            Err(e) => {
                if let Ok(Some(cached)) = self.load_cache() {
                    return Ok(cached);
                }
                // Last resort: use the bundled fallback.
                match parse_jsonl(EMBEDDED_PROVIDERS) {
                    Ok(embedded) => Ok(embedded),
                    Err(_) => Err(e), // Return original fetch error
                }
            }
        }
    }

    /// Load the embedded (bundled) provider list. Always succeeds.
    pub fn embedded_fallback(&self) -> Vec<ProviderSource> {
        parse_jsonl(EMBEDDED_PROVIDERS).unwrap_or_default()
    }

    fn save_cache(&self, text: &str) -> Result<()> {
        let path = self.cache_path();
        let mut file = fs::File::create(&path)?;
        file.write_all(text.as_bytes())?;
        file.flush()?;
        Ok(())
    }
}

fn parse_jsonl(text: &str) -> Result<Vec<ProviderSource>> {
    let mut sources = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let source: ProviderSource = serde_json::from_str(line)
            .with_context(|| format!("parse provider registry line {}", i + 1))?;
        sources.push(source);
    }
    Ok(sources)
}

/// Discover available models from a single source.
pub async fn discover_models(
    client: &reqwest::Client,
    source: &ProviderSource,
    api_key: Option<String>,
) -> Vec<String> {
    let Some(url) = source.models_url() else {
        return source.models.clone().unwrap_or_default();
    };

    let Some(key) = api_key.or_else(|| source.api_key()) else {
        return source.models.clone().unwrap_or_default();
    };

    let mut request = client.get(&url);
    if let Some(header) = &source.auth_header {
        let value = if header.eq_ignore_ascii_case("x-api-key") {
            key
        } else if header.eq_ignore_ascii_case("Authorization") {
            format!("Bearer {key}")
        } else {
            key
        };
        request = request.header(header.clone(), value);
    } else {
        request = request.bearer_auth(key);
    }

    if let Some(version) = &source.version_header {
        request = request.header("anthropic-version", version.clone());
    }

    let is_ollama_native = source
        .models_endpoint
        .as_deref()
        .is_some_and(|ep| ep.ends_with("/api/tags"));

    match request.send().await {
        Ok(resp) if resp.status().is_success() => {
            if is_ollama_native {
                match resp.json::<OllamaModelList>().await {
                    Ok(list) => list.models.into_iter().map(|m| m.name).collect(),
                    Err(_) => source.models.clone().unwrap_or_default(),
                }
            } else {
                match resp.json::<OpenAiModelList>().await {
                    Ok(list) => list.data.into_iter().map(|m| m.id).collect(),
                    Err(_) => source.models.clone().unwrap_or_default(),
                }
            }
        }
        _ => source.models.clone().unwrap_or_default(),
    }
}

/// Convenience wrapper that discovers models for a [`SessionConfig`].
pub async fn discover_models_for_config(config: &crate::types::SessionConfig) -> Vec<String> {
    use crate::types::ProviderKind;
    let client = reqwest::Client::new();
    let api_base = config
        .api_base
        .clone()
        .unwrap_or_else(|| config.provider.default_base().to_string());
    let source = ProviderSource {
        provider: config.provider.name().to_string(),
        name: config.provider.name().to_string(),
        api_type: match config.provider {
            ProviderKind::OpenAI | ProviderKind::Ollama => "openai-chat".into(),
            ProviderKind::Anthropic => "anthropic".into(),
        },
        api_base,
        key_env: None,
        models_endpoint: Some("/models".into()),
        auth_header: if config.provider == ProviderKind::Anthropic {
            Some("x-api-key".into())
        } else {
            None
        },
        version_header: if config.provider == ProviderKind::Anthropic {
            Some("2023-06-01".into())
        } else {
            None
        },
        models: None,
        extra: std::collections::HashMap::new(),
    };
    discover_models(
        &client,
        &source,
        if config.api_key.is_empty() {
            None
        } else {
            Some(config.api_key.clone())
        },
    )
    .await
}

/// Fetch the provider registry from the remote URL, local cache, or embedded fallback.
pub async fn fetch_provider_sources() -> Vec<ProviderSource> {
    let cache_dir = dirs::cache_dir().unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    });
    match ProviderRegistry::new(cache_dir) {
        Ok(registry) => match registry.fetch_or_cache(DEFAULT_REGISTRY_URL).await {
            Ok(sources) if !sources.is_empty() => sources,
            _ => registry.embedded_fallback(),
        },
        Err(_) => parse_jsonl(EMBEDDED_PROVIDERS).unwrap_or_default(),
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiModelList {
    data: Vec<OpenAiModelEntry>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelEntry {
    id: String,
}

#[derive(Debug, Deserialize)]
struct OllamaModelList {
    models: Vec<OllamaModelEntry>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelEntry {
    name: String,
}

/// Convenience: discover models from every supported source concurrently.
pub async fn discover_all_models(
    client: &reqwest::Client,
    sources: &[ProviderSource],
) -> Vec<DiscoveredModel> {
    let mut handles = Vec::new();
    for source in sources.iter().filter(|s| s.is_supported()) {
        let source = source.clone();
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            let ids = discover_models(&client, &source, source.api_key()).await;
            ids.into_iter()
                .map(|id| DiscoveredModel {
                    id,
                    source: source.clone(),
                })
                .collect::<Vec<_>>()
        }));
    }

    let mut result = Vec::new();
    for h in handles {
        if let Ok(models) = h.await {
            result.extend(models);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jsonl() {
        let text = r#"{"provider":"openai","name":"OpenAI","api_type":"openai-chat","api_base":"https://api.openai.com/v1","key_env":"OPENAI_API_KEY","models_endpoint":"/models"}
{"provider":"anthropic","name":"Anthropic","api_type":"anthropic","api_base":"https://api.anthropic.com/v1","key_env":"ANTHROPIC_API_KEY","models_endpoint":"/models","auth_header":"x-api-key","version_header":"2023-06-01"}"#;
        let sources = parse_jsonl(text).unwrap();
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].provider, "openai");
        assert_eq!(sources[1].auth_header.as_deref(), Some("x-api-key"));
        assert!(sources[0].is_supported());
        assert!(sources[1].is_supported());
    }

    #[test]
    fn test_models_url() {
        let s = ProviderSource {
            provider: "x".into(),
            name: "X".into(),
            api_type: "openai-chat".into(),
            api_base: "https://api.example.com/v1/".into(),
            key_env: None,
            models_endpoint: Some("/models".into()),
            auth_header: None,
            version_header: None,
            models: None,
            extra: HashMap::new(),
        };
        assert_eq!(
            s.models_url(),
            Some("https://api.example.com/v1/models".into())
        );
    }

    #[test]
    fn test_parse_model_list_formats() {
        let openai: OpenAiModelList =
            serde_json::from_str(r#"{"data":[{"id":"gpt-4"},{"id":"gpt-3.5-turbo"}]}"#).unwrap();
        assert_eq!(
            openai.data.into_iter().map(|m| m.id).collect::<Vec<_>>(),
            vec!["gpt-4", "gpt-3.5-turbo"]
        );

        let ollama: OllamaModelList =
            serde_json::from_str(r#"{"models":[{"name":"qwen2.5:14b"},{"name":"llama3.2"}]}"#)
                .unwrap();
        assert_eq!(
            ollama
                .models
                .into_iter()
                .map(|m| m.name)
                .collect::<Vec<_>>(),
            vec!["qwen2.5:14b", "llama3.2"]
        );
    }
}
