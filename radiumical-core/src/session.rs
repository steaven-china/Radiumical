//! Session management — stored in ~/.radi/session/ as semantic JSONL.
//! Each line is a typed record: meta / user / assistant / reasoning / tool / raw.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SessionItem {
    #[serde(rename = "meta")]
    Meta {
        name: String,
        created: String,
        model: String,
        description: String,
        message_count: usize,
    },
    #[serde(rename = "user")]
    User { content: String },
    #[serde(rename = "assistant")]
    Assistant { content: String },
    #[serde(rename = "reasoning")]
    Reasoning { content: String },
    #[serde(rename = "tool")]
    Tool {
        id: String,
        name: String,
        args: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<String>,
    },
    #[serde(rename = "raw")]
    Raw { lines: Vec<String> },
}

pub struct Session;

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
                        if let Ok(SessionItem::Meta {
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
        items: &[SessionItem],
        model: &str,
        description: Option<&str>,
    ) -> Result<()> {
        let dir = Self::dir();
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.jsonl", hash_name(name)));
        let created = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
        let description = description.unwrap_or("").to_string();
        let message_count = items.len();

        let mut records: Vec<SessionItem> = vec![SessionItem::Meta {
            name: name.to_string(),
            created,
            model: model.to_string(),
            description,
            message_count,
        }];
        records.extend_from_slice(items);

        let lines: Vec<String> = records
            .iter()
            .map(|r| serde_json::to_string(r))
            .collect::<Result<Vec<_>, _>>()?;
        fs::write(&path, lines.join("\n"))?;
        Ok(())
    }

    pub fn load(name: &str) -> Result<Option<(SessionMeta, Vec<SessionItem>)>> {
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
        let meta = match serde_json::from_str::<SessionItem>(&first)? {
            SessionItem::Meta {
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
        let mut items = Vec::new();
        for line in lines {
            match serde_json::from_str::<SessionItem>(line)? {
                SessionItem::Meta { .. } => {}
                item => items.push(item),
            }
        }
        Ok(Some((meta, items)))
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
        let result = Session::list();
        assert!(result.is_ok());
    }

    #[test]
    fn test_save_delete_cycle() {
        let items = vec![
            SessionItem::User {
                content: "hello".into(),
            },
            SessionItem::Assistant {
                content: "hi".into(),
            },
            SessionItem::Tool {
                id: "call_1".into(),
                name: "read_file".into(),
                args: "{\"path\":\"x\"}".into(),
                result: Some("content".into()),
            },
        ];
        let result = Session::save("_test_session", &items, "test-model", Some("test desc"));
        assert!(result.is_ok());
        let loaded = Session::load("_test_session").unwrap();
        assert!(loaded.is_some());
        let (meta, loaded_items) = loaded.unwrap();
        assert_eq!(meta.name, "_test_session");
        assert_eq!(meta.model, "test-model");
        assert_eq!(meta.description, "test desc");
        assert_eq!(loaded_items.len(), 3);
        match &loaded_items[2] {
            SessionItem::Tool { result, .. } => assert_eq!(result.as_deref(), Some("content")),
            _ => panic!("expected tool item"),
        }
        let deleted = Session::delete("_test_session").unwrap();
        assert!(deleted);
        let gone = Session::load("_test_session").unwrap();
        assert!(gone.is_none());
    }
}
