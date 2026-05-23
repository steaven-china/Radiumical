//! Session management — save/load conversation sessions to disk.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
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

impl Session {
    pub fn dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("radiumical")
            .join("sessions")
    }

    /// List all saved sessions.
    pub fn list() -> Result<Vec<SessionMeta>> {
        let dir = Self::dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
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

    /// Save current conversation to a named session.
    pub fn save(name: &str, messages_jsonl: &str, model: &str) -> Result<()> {
        let dir = Self::dir();
        fs::create_dir_all(&dir)?;

        let path = dir.join(format!("{}.json", sanitize_name(name)));
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

    /// Load a session by name.
    pub fn load(name: &str) -> Result<Option<Session>> {
        let dir = Self::dir();
        let path = dir.join(format!("{}.json", sanitize_name(name)));
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(&path)?;
        Ok(Some(serde_json::from_str(&data)?))
    }

    /// Delete a session.
    pub fn delete(name: &str) -> Result<bool> {
        let dir = Self::dir();
        let path = dir.join(format!("{}.json", sanitize_name(name)));
        if path.exists() {
            fs::remove_file(&path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_name_normal() {
        assert_eq!(sanitize_name("hello-world_123"), "hello-world_123");
    }

    #[test]
    fn test_sanitize_name_spaces() {
        assert_eq!(sanitize_name("my session name"), "my_session_name");
    }

    #[test]
    fn test_sanitize_name_slashes() {
        assert_eq!(sanitize_name("path/to/session"), "path_to_session");
    }

    #[test]
    fn test_sanitize_name_special_chars() {
        assert_eq!(sanitize_name("hello@world!"), "hello_world_");
    }

    #[test]
    fn test_sanitize_name_unicode() {
        // Non-alphanumeric symbols get replaced; CJK characters are alphanumeric in Unicode
        assert_eq!(sanitize_name("hello@world!"), "hello_world_");
        assert_eq!(sanitize_name("你好"), "你好"); // CJK is alphanumeric
    }

    #[test]
    fn test_sanitize_name_empty() {
        assert_eq!(sanitize_name(""), "");
    }
}
