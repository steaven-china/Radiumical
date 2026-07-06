//! Config persistence — reads/writes ~/.radi/config.toml.
//!
//! Config inheritance chain:
//! 1. `SessionConfig::default()` — hardcoded defaults
//! 2. `~/.radi/config.toml` — global user config
//! 3. `~/.radi/sessions/{hash}/workspace.toml` — workspace-level overrides
//!
//! Use `Config::load_for_workspace(hash)` to get the merged result.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::providers::{find_provider, parse_provider_kind};
use crate::session::WorkspaceSettings;
use crate::types::{AgentMode, SessionConfig};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub api_key: Option<String>,
    pub api_base: Option<String>,
    pub heartbeat_secs: Option<u64>,
    pub llm_timeout_secs: Option<u64>,
    pub max_iterations: Option<usize>,
    pub reasoning_effort: Option<String>,
    pub mode: Option<String>,
    pub max_context_tokens: Option<usize>,
    pub context_compress_ratio: Option<f64>,
    pub auto_resume_last_task: Option<bool>,
}

impl Config {
    pub fn dir() -> PathBuf {
        dirs::home_dir()
            .map(|h| h.join(".radi"))
            .unwrap_or_else(|| PathBuf::from(".radi"))
    }

    pub fn path() -> PathBuf {
        Self::dir().join("config.toml")
    }

    pub fn load() -> Result<Self> {
        let path = Self::path();
        if path.exists() {
            let data = fs::read_to_string(&path)?;
            Ok(toml::from_str(&data)?)
        } else {
            Ok(Config {
                model: None,
                provider: None,
                api_key: None,
                api_base: None,
                heartbeat_secs: None,
                llm_timeout_secs: None,
                max_iterations: None,
                reasoning_effort: None,
                mode: None,
                max_context_tokens: None,
                context_compress_ratio: None,
                auto_resume_last_task: None,
            })
        }
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::dir();
        fs::create_dir_all(&dir).with_context(|| format!("create config dir {}", dir.display()))?;
        let path = Self::path();
        let data = toml::to_string_pretty(self)?;
        fs::write(&path, data)?;
        Ok(())
    }

    /// Apply config over CLI args (CLI takes priority)
    pub fn apply(&self, model: &mut String, _provider: &mut String) {
        if let Some(ref m) = self.model {
            *model = m.clone();
        }
    }

    /// Load config with the full inheritance chain applied:
    /// SessionConfig::default() ← config.toml ← workspace.toml
    pub fn load_for_workspace(workspace_hash: &str) -> SessionConfig {
        let mut config = SessionConfig::default();

        // Layer 1: global config.toml
        match Self::load() {
            Ok(global) => global.apply_to_config(&mut config),
            Err(e) => eprintln!("[radiumical] failed to load config.toml: {e}"),
        }

        // Layer 2: workspace.toml overrides
        let ws_settings = crate::session::load_workspace_settings(workspace_hash);
        ws_settings.apply_to_config(&mut config);

        config
    }
}

impl Config {
    /// Apply this file-based config onto a SessionConfig.
    fn apply_to_config(&self, config: &mut SessionConfig) {
        if let Some(ref m) = self.model {
            config.model = m.clone();
        }
        if let Some(ref p) = self.provider {
            let source = find_provider(p);
            config.provider = parse_provider_kind(p);

            // Resolve api_base from registry if not explicitly configured (or explicitly empty).
            let explicit_empty = self
                .api_base
                .as_deref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(false);
            if self.api_base.is_none() || explicit_empty {
                if let Some(ref s) = source {
                    config.api_base = Some(s.api_base.clone());
                }
            }

            // Resolve API key from registry's key_env if not explicitly configured.
            if self.api_key.is_none() {
                if let Some(key) = source.as_ref().and_then(|s| s.api_key()) {
                    config.api_key = key;
                }
            }

            // Resolve default model from registry if not explicitly configured.
            if self.model.is_none() {
                if let Some(ref s) = source {
                    if let Some(ref m) = s.default_model {
                        config.model = m.clone();
                    }
                }
            }
        }
        if let Some(ref k) = self.api_key {
            config.api_key = k.clone();
        }
        if let Some(ref b) = self.api_base {
            if !b.trim().is_empty() {
                config.api_base = Some(b.clone());
            }
        }
        if let Some(h) = self.heartbeat_secs {
            config.heartbeat_interval_secs = h;
        }
        if let Some(t) = self.llm_timeout_secs {
            config.llm_timeout_secs = t;
        }
        if let Some(n) = self.max_iterations {
            config.max_iterations = n;
        }
        if let Some(ref e) = self.reasoning_effort {
            config.thinking_effort = Some(e.clone());
        }
        if let Some(ref m) = self.mode {
            config.mode = match m.to_lowercase().as_str() {
                "plan" => AgentMode::Plan,
                "exec" => AgentMode::Exec,
                _ => AgentMode::Auto,
            };
        }
        if let Some(n) = self.max_context_tokens {
            config.max_context_tokens = n;
        }
        if let Some(r) = self.context_compress_ratio {
            config.context_compress_ratio = r;
        }
        if let Some(b) = self.auto_resume_last_task {
            config.auto_resume_last_task = b;
        }
    }
}

impl WorkspaceSettings {
    /// Apply workspace-level overrides onto a SessionConfig.
    pub fn apply_to_config(&self, config: &mut SessionConfig) {
        if let Some(ref m) = self.model {
            config.model = m.clone();
        }
        if let Some(ref m) = self.mode {
            config.mode = match m.to_lowercase().as_str() {
                "plan" => AgentMode::Plan,
                "exec" => AgentMode::Exec,
                _ => AgentMode::Auto,
            };
        }
        if let Some(n) = self.max_context_tokens {
            config.max_context_tokens = n;
        }
        if let Some(t) = self.llm_timeout_secs {
            config.llm_timeout_secs = t;
        }
        if let Some(t) = self.tool_timeout_secs {
            config.tool_timeout_secs = t;
        }
        if let Some(r) = self.context_compress_ratio {
            config.context_compress_ratio = r;
        }
        if let Some(b) = self.auto_continue {
            config.auto_continue = b;
        }
        if let Some(ref e) = self.thinking_effort {
            config.thinking_effort = Some(e.clone());
        }
        if let Some(b) = self.auto_resume_last_task {
            config.auto_resume_last_task = b;
        }
    }
}
