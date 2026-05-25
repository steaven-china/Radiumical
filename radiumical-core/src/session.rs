//! Session management — stored in ~/.radi/session/ with hash filenames.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionMeta {
    pub name: String,
    pub created: String,
    pub model: String,
    pub message_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Session {
    pub meta: SessionMeta,
    pub messages_jsonl: String,
}

fn hash_name(name: &str) -> String {
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    format!("{:x}", h.finish())
}

impl Session {
    pub fn dir() -> PathBuf {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".radi").join("session")
    }

    pub fn list() -> Result<Vec<SessionMeta>> {
        let dir = Self::dir();
        if !dir.exists() { return Ok(Vec::new()); }
        let mut metas = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.path().extension().map_or(false, |e| e == "json") {
                if let Ok(data) = fs::read_to_string(entry.path()) {
                    if let Ok(session) = serde_json::from_str::<Session>(&data) {
                        metas.push(session.meta);
                    }
                }
            }
        }
        metas.sort_by(|a, b| b.created.cmp(&a.created));
        Ok(metas)
    }

    pub fn save(name: &str, messages_jsonl: &str, model: &str) -> Result<()> {
        let dir = Self::dir();
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", hash_name(name)));
        let session = Session {
            meta: SessionMeta {
                name: name.to_string(),
                created: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
                model: model.to_string(),
                message_count: messages_jsonl.lines().count(),
            },
            messages_jsonl: messages_jsonl.to_string(),
        };
        fs::write(&path, serde_json::to_string_pretty(&session)?)?;
        Ok(())
    }

    pub fn load(name: &str) -> Result<Option<Session>> {
        let dir = Self::dir();
        let path = dir.join(format!("{}.json", hash_name(name)));
        if !path.exists() { return Ok(None); }
        let data = fs::read_to_string(&path)?;
        Ok(Some(serde_json::from_str(&data)?))
    }

    pub fn delete(name: &str) -> Result<bool> {
        let dir = Self::dir();
        let path = dir.join(format!("{}.json", hash_name(name)));
        if path.exists() { fs::remove_file(&path)?; Ok(true) } else { Ok(false) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_name_deterministic() {
        let a = hash_name("test");
        let b = hash_name("test");
        assert_eq!(a, b);
    }

    #[test]
    fn test_hash_name_different() {
        let a = hash_name("hello");
        let b = hash_name("world");
        assert_ne!(a, b);
    }

    #[test]
    fn test_list_empty() {
        // Session dir might not exist — should return empty
        let result = Session::list();
        assert!(result.is_ok());
    }

    #[test]
    fn test_save_delete_cycle() {
        let result = Session::save("_test_session", "{\"test\":1}", "test-model");
        assert!(result.is_ok());
        let loaded = Session::load("_test_session").unwrap();
        assert!(loaded.is_some());
        let deleted = Session::delete("_test_session").unwrap();
        assert!(deleted);
        let gone = Session::load("_test_session").unwrap();
        assert!(gone.is_none());
    }
}
