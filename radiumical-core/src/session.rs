//! Session management — stored in ~/.radi/session/ as custom JSONL.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub name: String,
    pub created: String,
    pub model: String,
    pub description: String,
    pub message_count: usize,
}

pub struct Session;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum SessionRecord {
    #[serde(rename = "meta")]
    Meta {
        name: String,
        created: String,
        model: String,
        description: String,
        message_count: usize,
    },
    #[serde(rename = "output")]
    Output { line: String },
}

fn hash_name(name: &str) -> String {
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    format!("{:x}", h.finish())
}

impl Session {
    pub fn dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".radi")
            .join("session")
    }

    pub fn list() -> Result<Vec<SessionMeta>> {
        let dir = Self::dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut metas = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "jsonl") {
                if let Ok(data) = fs::read_to_string(&path) {
                    if let Some(first) = data.lines().next() {
                        if let Ok(SessionRecord::Meta {
                            name,
                            created,
                            model,
                            description,
                            message_count,
                        }) = serde_json::from_str(first)
                        {
                            metas.push(SessionMeta {
                                name,
                                created,
                                model,
                                description,
                                message_count,
                            });
                        }
                    }
                }
            }
        }
        metas.sort_by(|a, b| b.created.cmp(&a.created));
        Ok(metas)
    }

    pub fn save(
        name: &str,
        messages_jsonl: &str,
        model: &str,
        description: Option<&str>,
    ) -> Result<()> {
        let dir = Self::dir();
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.jsonl", hash_name(name)));
        let created = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        let description = description.unwrap_or("").to_string();
        let message_count = messages_jsonl.lines().count();

        let meta = SessionRecord::Meta {
            name: name.to_string(),
            created,
            model: model.to_string(),
            description,
            message_count,
        };

        let mut lines = vec![serde_json::to_string(&meta)?];
        for line in messages_jsonl.lines() {
            let record = SessionRecord::Output {
                line: line.to_string(),
            };
            lines.push(serde_json::to_string(&record)?);
        }
        fs::write(&path, lines.join("\n"))?;
        Ok(())
    }

    pub fn load(name: &str) -> Result<Option<(SessionMeta, Vec<String>)>> {
        let dir = Self::dir();
        let path = dir.join(format!("{}.jsonl", hash_name(name)));
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(&path)?;
        let mut lines = data.lines();
        let first = lines
            .next()
            .context("session file is empty")?
            .to_string();
        let meta = match serde_json::from_str::<SessionRecord>(&first)? {
            SessionRecord::Meta {
                name,
                created,
                model,
                description,
                message_count,
            } => SessionMeta {
                name,
                created,
                model,
                description,
                message_count,
            },
            _ => anyhow::bail!("first record is not meta"),
        };
        let mut output = Vec::new();
        for line in lines {
            match serde_json::from_str::<SessionRecord>(line)? {
                SessionRecord::Output { line } => output.push(line),
                SessionRecord::Meta { .. } => {}
            }
        }
        Ok(Some((meta, output)))
    }

    pub fn delete(name: &str) -> Result<bool> {
        let dir = Self::dir();
        let path = dir.join(format!("{}.jsonl", hash_name(name)));
        if path.exists() {
            fs::remove_file(&path)?;
            Ok(true)
        } else {
            Ok(false)
        }
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
        let result = Session::save("_test_session", "line1\nline2", "test-model", Some("test desc"));
        assert!(result.is_ok());
        let loaded = Session::load("_test_session").unwrap();
        assert!(loaded.is_some());
        let (meta, output) = loaded.unwrap();
        assert_eq!(meta.name, "_test_session");
        assert_eq!(meta.model, "test-model");
        assert_eq!(meta.description, "test desc");
        assert_eq!(output, vec!["line1", "line2"]);
        let deleted = Session::delete("_test_session").unwrap();
        assert!(deleted);
        let gone = Session::load("_test_session").unwrap();
        assert!(gone.is_none());
    }
}
