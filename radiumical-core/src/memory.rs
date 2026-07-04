//! Three-tier memory system: core → mino → short.
//! Stored in `~/.radi/mem/{workspace_hash}/memory.json` per workspace.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::session::workspace_hash;

const MAX_SHORT: usize = 20;
const MAX_MINO: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryEntry {
    pub content: String,
    pub timestamp: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Memory {
    pub core: Vec<MemoryEntry>,
    pub mino: Vec<MemoryEntry>,
    pub short: Vec<MemoryEntry>,
    #[serde(skip)]
    dir: Option<PathBuf>,
}

impl Memory {
    pub fn dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".radi")
            .join("mem")
    }

    pub fn for_workspace(workspace: &str) -> Self {
        let dir = Self::dir().join(workspace_hash(workspace));
        let mut mem = Self::load_from_dir(&dir).unwrap_or_default();
        mem.dir = Some(dir);
        mem
    }

    pub fn load() -> Result<Self> {
        let dir = Self::dir();
        let mut mem = Self::load_from_dir(&dir).unwrap_or_default();
        mem.dir = Some(dir);
        Ok(mem)
    }

    fn load_from_dir(dir: &Path) -> Result<Self> {
        let path = dir.join("memory.json");
        if path.exists() {
            Ok(serde_json::from_str(&fs::read_to_string(&path)?)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let dir = self.dir.clone().unwrap_or_else(Self::dir);
        fs::create_dir_all(&dir)?;
        fs::write(dir.join("memory.json"), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn add(&mut self, tier: &str, content: &str, tags: &[&str]) -> Result<()> {
        let entry = MemoryEntry {
            content: content.to_string(),
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
        };
        match tier {
            "core" => self.core.push(entry),
            "mino" => self.mino.push(entry),
            "short" => self.short.push(entry),
            _ => anyhow::bail!("Unknown tier: {tier}. Use core/mino/short."),
        }
        self.trim();
        self.save()?;
        Ok(())
    }

    pub fn delete(&mut self, tier: &str, index: usize) -> Result<()> {
        let entries = self.tier_mut(tier)?;
        if index >= entries.len() {
            anyhow::bail!(
                "Index {index} out of bounds for tier '{tier}' (len={}).",
                entries.len()
            );
        }
        entries.remove(index);
        self.save()
    }

    pub fn edit(&mut self, tier: &str, index: usize, content: &str) -> Result<()> {
        let entries = self.tier_mut(tier)?;
        if index >= entries.len() {
            anyhow::bail!(
                "Index {index} out of bounds for tier '{tier}' (len={}).",
                entries.len()
            );
        }
        entries[index].content = content.to_string();
        entries[index].timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        self.save()
    }

    pub fn clear(&mut self, tier: &str) -> Result<()> {
        let entries = self.tier_mut(tier)?;
        entries.clear();
        self.save()
    }

    pub fn remove_by_content(&mut self, tier: &str, content: &str) -> Result<bool> {
        let entries = self.tier_mut(tier)?;
        if let Some(pos) = entries.iter().position(|e| e.content == content) {
            entries.remove(pos);
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn search(&self, query: &str) -> Vec<(String, &MemoryEntry)> {
        let q = query.to_lowercase();
        let mut results = Vec::new();
        for (label, entries) in [
            ("core", &self.core),
            ("mino", &self.mino),
            ("short", &self.short),
        ] {
            for entry in entries.iter() {
                if entry.content.to_lowercase().contains(&q)
                    || entry.tags.iter().any(|t| t.to_lowercase().contains(&q))
                {
                    results.push((label.to_string(), entry));
                }
            }
        }
        results
    }

    fn tier_mut(&mut self, tier: &str) -> Result<&mut Vec<MemoryEntry>> {
        match tier {
            "core" => Ok(&mut self.core),
            "mino" => Ok(&mut self.mino),
            "short" => Ok(&mut self.short),
            _ => anyhow::bail!("Unknown tier: {tier}. Use core/mino/short."),
        }
    }

    fn trim(&mut self) {
        if self.short.len() > MAX_SHORT {
            let drain = self.short.len() - MAX_SHORT;
            self.short.drain(..drain);
        }
        if self.mino.len() > MAX_MINO {
            let drain = self.mino.len() - MAX_MINO;
            self.mino.drain(..drain);
        }
    }

    /// Core memory injected directly into system prompt.
    pub fn core_context(&self) -> String {
        if self.core.is_empty() {
            return String::new();
        }
        let mut ctx = String::from("\n## Memory (Core)\n");
        for m in &self.core {
            let tags = if m.tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", m.tags.join(", "))
            };
            ctx.push_str(&format!("- {}{}\n", m.content, tags));
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_memory() -> Memory {
        let dir = tempfile::tempdir().unwrap().keep();
        Memory {
            core: Vec::new(),
            mino: Vec::new(),
            short: Vec::new(),
            dir: Some(dir),
        }
    }

    #[test]
    fn test_add_with_tags() {
        let mut m = temp_memory();
        m.add("core", "user prefers Rust", &["preference", "lang"])
            .unwrap();
        assert_eq!(m.core.len(), 1);
        assert_eq!(m.core[0].tags, vec!["preference", "lang"]);
    }

    #[test]
    fn test_add_invalid_tier() {
        let mut m = temp_memory();
        assert!(m.add("invalid", "x", &[]).is_err());
    }

    #[test]
    fn test_delete() {
        let mut m = temp_memory();
        m.add("short", "a", &[]).unwrap();
        m.add("short", "b", &[]).unwrap();
        m.delete("short", 0).unwrap();
        assert_eq!(m.short.len(), 1);
        assert_eq!(m.short[0].content, "b");
    }

    #[test]
    fn test_delete_out_of_bounds() {
        let mut m = temp_memory();
        assert!(m.delete("core", 5).is_err());
    }

    #[test]
    fn test_edit() {
        let mut m = temp_memory();
        m.add("mino", "old content", &[]).unwrap();
        m.edit("mino", 0, "new content").unwrap();
        assert_eq!(m.mino[0].content, "new content");
    }

    #[test]
    fn test_edit_out_of_bounds() {
        let mut m = temp_memory();
        assert!(m.edit("mino", 0, "x").is_err());
    }

    #[test]
    fn test_clear() {
        let mut m = temp_memory();
        m.add("core", "a", &[]).unwrap();
        m.add("core", "b", &[]).unwrap();
        m.clear("core").unwrap();
        assert!(m.core.is_empty());
    }

    #[test]
    fn test_remove_by_content_found() {
        let mut m = temp_memory();
        m.add("short", "hello", &[]).unwrap();
        m.add("short", "world", &[]).unwrap();
        let removed = m.remove_by_content("short", "hello").unwrap();
        assert!(removed);
        assert_eq!(m.short.len(), 1);
        assert_eq!(m.short[0].content, "world");
    }

    #[test]
    fn test_remove_by_content_not_found() {
        let mut m = temp_memory();
        m.add("short", "hello", &[]).unwrap();
        let removed = m.remove_by_content("short", "missing").unwrap();
        assert!(!removed);
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut m = temp_memory();
        m.add("core", "User prefers Rust", &["lang"]).unwrap();
        m.add("mino", "Used python today", &[]).unwrap();
        m.add("short", "Nothing relevant", &[]).unwrap();
        let results = m.search("rust");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.content, "User prefers Rust");
    }

    #[test]
    fn test_search_by_tag() {
        let mut m = temp_memory();
        m.add("core", "Some content", &["important"]).unwrap();
        m.add("mino", "Other", &[]).unwrap();
        let results = m.search("important");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_no_match() {
        let mut m = temp_memory();
        m.add("core", "hello", &[]).unwrap();
        let results = m.search("xyz");
        assert!(results.is_empty());
    }

    #[test]
    fn test_trim_short() {
        let mut m = temp_memory();
        for i in 0..25 {
            m.short.push(MemoryEntry {
                content: format!("item {i}"),
                timestamp: String::new(),
                tags: vec![],
            });
        }
        m.trim();
        assert_eq!(m.short.len(), MAX_SHORT);
        assert_eq!(m.short[0].content, "item 5");
    }

    #[test]
    fn test_trim_mino() {
        let mut m = temp_memory();
        for i in 0..55 {
            m.mino.push(MemoryEntry {
                content: format!("item {i}"),
                timestamp: String::new(),
                tags: vec![],
            });
        }
        m.trim();
        assert_eq!(m.mino.len(), MAX_MINO);
        assert_eq!(m.mino[0].content, "item 5");
    }

    #[test]
    fn test_core_context_with_tags() {
        let mut m = temp_memory();
        m.core.push(MemoryEntry {
            content: "prefers Rust".into(),
            timestamp: String::new(),
            tags: vec!["lang".into()],
        });
        let ctx = m.core_context();
        assert!(ctx.contains("prefers Rust"));
        assert!(ctx.contains("[lang]"));
    }

    #[test]
    fn test_core_context_empty() {
        let m = temp_memory();
        assert!(m.core_context().is_empty());
    }

    #[test]
    fn test_context_empty() {
        let m = temp_memory();
        assert!(m.context().is_empty());
    }

    #[test]
    fn test_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap().keep();
        let mut m = Memory {
            core: Vec::new(),
            mino: Vec::new(),
            short: Vec::new(),
            dir: Some(dir.clone()),
        };
        m.add("core", "test entry", &["tag1"]).unwrap();

        let loaded = Memory::load_from_dir(&dir).unwrap();
        assert_eq!(loaded.core.len(), 1);
        assert_eq!(loaded.core[0].content, "test entry");
        assert_eq!(loaded.core[0].tags, vec!["tag1"]);
    }

    #[test]
    fn test_for_workspace_isolation() {
        let a = Memory::for_workspace("/tmp/workspace-a");
        let b = Memory::for_workspace("/tmp/workspace-b");
        let dir_a = a.dir.as_ref().unwrap();
        let dir_b = b.dir.as_ref().unwrap();
        assert_ne!(dir_a, dir_b);
        assert!(dir_a.to_string_lossy().contains("mem"));
        assert!(dir_b.to_string_lossy().contains("mem"));
    }
}
