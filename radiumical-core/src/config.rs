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

use crate::session::WorkspaceSettings;
use crate::types::{AgentMode, SessionConfig, ProviderKind};

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
        if let Ok(global) = Self::load() {
            global.apply_to_config(&mut config);
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
            config.provider = match p.to_lowercase().as_str() {
                "anthropic" => ProviderKind::Anthropic,
                "ollama" => ProviderKind::Ollama,
                _ => ProviderKind::OpenAI,
            };
        }
        if let Some(t) = self.llm_timeout_secs {
            config.llm_timeout_secs = t;
        }
        if let Some(n) = self.max_iterations {
            config.max_iterations = n;
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
    }
}
