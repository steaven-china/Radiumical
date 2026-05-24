//! Config persistence — reads/writes config.toml.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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
}

impl Config {
    pub fn path() -> PathBuf {
        PathBuf::from("config.toml")
    }

    pub fn load() -> Result<Self> {
        let path = Self::path();
        if path.exists() {
            let data = fs::read_to_string(&path)?;
            Ok(toml::from_str(&data)?)
        } else {
            Ok(Config {
                model: None, provider: None, api_key: None, api_base: None,
                heartbeat_secs: None, llm_timeout_secs: None, max_iterations: None, reasoning_effort: None,
            })
        }
    }

    #[allow(dead_code)]
    pub fn save(&self) -> Result<()> {
        let data = toml::to_string_pretty(self)?;
        fs::write(Self::path(), data)?;
        Ok(())
    }

    /// Apply config over CLI args (CLI takes priority)
    #[allow(dead_code)]
    pub fn apply(&self, model: &mut String, _provider: &mut String) {
        if let Some(ref m) = self.model { *model = m.clone(); }
    }
}
