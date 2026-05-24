//! Three-tier memory system: core → mino → short.
//! Stored in ~/.radi/mem/ as JSON.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct MemoryEntry {
    pub content: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Memory {
    pub core: Vec<MemoryEntry>,
    pub mino: Vec<MemoryEntry>,
    pub short: Vec<MemoryEntry>,
}

impl Memory {
    pub fn dir() -> PathBuf {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".radi").join("mem")
    }

    pub fn load() -> Result<Self> {
        let path = Self::dir().join("memory.json");
        if path.exists() {
            Ok(serde_json::from_str(&fs::read_to_string(&path)?)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::dir();
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("memory.json"), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn add(&mut self, tier: &str, content: &str) -> Result<()> {
        let entry = MemoryEntry {
            content: content.to_string(),
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
        };
        match tier {
            "core" => self.core.push(entry),
            "mino" => self.mino.push(entry),
            "short" => self.short.push(entry),
            _ => anyhow::bail!("Unknown tier: {tier}. Use core/mino/short."),
        }
        self.save()?;
        Ok(())
    }

    /// Core memory injected directly into system prompt.
    pub fn core_context(&self) -> String {
        if self.core.is_empty() { return String::new(); }
        let mut ctx = String::from("\n## Memory (Core)\n");
        for m in &self.core {
            ctx.push_str(&format!("- {}\n", m.content));
        }
        ctx
    }

    /// Mino + short as retrieved context.
    pub fn context(&self) -> String {
        let mut ctx = String::new();
        if !self.mino.is_empty() {
            ctx.push_str("\n## Memory (Recent)\n");
            for m in self.mino.iter().rev().take(5) {
                ctx.push_str(&format!("- {}\n", m.content));
            }
        }
        if !self.short.is_empty() {
            ctx.push_str("\n## Recent Sessions\n");
            for m in self.short.iter().rev().take(3) {
                ctx.push_str(&format!("- [{}] {}\n", m.timestamp, m.content));
            }
        }
        ctx
    }
}
