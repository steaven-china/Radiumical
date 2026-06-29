//! Agent Skills — spec-compliant skill system following agentskills.io.
//!
//! Skills live in `~/.radi/skills/{name}/SKILL.md`. Each `SKILL.md` has YAML
//! frontmatter (`name`, `description`, optional `allowed-tools`) followed by
//! markdown instructions.
//!
//! Progressive disclosure:
//! 1. **Discovery** — only `name` + `description` loaded at startup (small footprint)
//! 2. **Activation** — full `SKILL.md` body loaded when a task matches
//! 3. **Execution** — agent follows instructions, optionally loading `references/` or `scripts/`

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Lightweight metadata loaded during discovery (stage 1).
#[derive(Debug, Clone)]
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    /// Path to the skill directory (for later full load).
    pub path: PathBuf,
}

/// Full skill with instructions loaded during activation (stage 2).
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub allowed_tools: Vec<String>,
    pub instructions: String,
    pub path: PathBuf,
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

pub fn skills_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".radi")
        .join("skills")
}

// ---------------------------------------------------------------------------
// Discovery (stage 1) — scan all skills, load only metadata
// ---------------------------------------------------------------------------

/// Scan `~/.radi/skills/*/SKILL.md` and return lightweight metadata for each.
pub fn discover() -> Vec<SkillMeta> {
    let dir = skills_dir();
    let mut metas = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let skill_md = path.join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }
            if let Some(meta) = parse_metadata(&skill_md) {
                metas.push(meta);
            }
        }
    }
    metas.sort_by(|a, b| a.name.cmp(&b.name));
    metas
}

/// Get a single skill's metadata by name.
pub fn get_meta(name: &str) -> Option<SkillMeta> {
    let skill_md = skills_dir().join(name).join("SKILL.md");
    if skill_md.exists() {
        parse_metadata(&skill_md)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Activation (stage 2) — load full SKILL.md body
// ---------------------------------------------------------------------------

/// Load the full skill including instructions. Returns None if not found.
pub fn load(name: &str) -> Option<Skill> {
    let skill_md = skills_dir().join(name).join("SKILL.md");
    if skill_md.exists() {
        parse_full(&skill_md)
    } else {
        None
    }
}

/// Load multiple skills by name (for batch activation).
pub fn load_many(names: &[String]) -> Vec<Skill> {
    names.iter().filter_map(|n| load(n)).collect()
}

/// Auto-match skills by scanning the user's input against descriptions and names.
/// Returns skills whose description or name contains significant keywords from input.
pub fn match_by_input(input: &str) -> Vec<SkillMeta> {
    let input_lower = input.to_lowercase();
    let words: Vec<&str> = input_lower
        .split_whitespace()
        .filter(|w| w.len() >= 2)
        .collect();

    discover()
        .into_iter()
        .filter(|m| {
            let desc_lower = m.description.to_lowercase();
            let name_lower = m.name.to_lowercase();
            // Match if skill name appears in input, or input contains keywords from description
            input_lower.contains(&name_lower)
                || words.iter().any(|w| desc_lower.contains(w))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Ensure defaults
// ---------------------------------------------------------------------------

/// Write bundled default skills to `~/.radi/skills/` if they don't exist yet.
pub fn ensure_defaults() {
    let dir = skills_dir();
    let _ = fs::create_dir_all(&dir);

    let defaults: &[(&str, &str)] = &[
        ("code-review", include_str!("defaults/code-review/SKILL.md")),
        ("refactor", include_str!("defaults/refactor/SKILL.md")),
        ("explain", include_str!("defaults/explain/SKILL.md")),
        ("test", include_str!("defaults/test/SKILL.md")),
        ("debug", include_str!("defaults/debug/SKILL.md")),
        ("git", include_str!("defaults/git/SKILL.md")),
        ("docs", include_str!("defaults/docs/SKILL.md")),
    ];

    for (name, content) in defaults {
        let skill_dir = dir.join(name);
        let skill_md = skill_dir.join("SKILL.md");
        if !skill_md.exists() {
            let _ = fs::create_dir_all(&skill_dir);
            let _ = fs::write(&skill_md, content);
        }
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

fn parse_metadata(path: &Path) -> Option<SkillMeta> {
    let content = fs::read_to_string(path).ok()?;
    let (fm, _) = split_frontmatter(&content);

    let mut name = String::new();
    let mut description = String::new();

    for line in fm.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            match key.trim() {
                "name" => name = value.trim().to_string(),
                "description" => description = value.trim().to_string(),
                _ => {}
            }
        }
    }

    if name.is_empty() {
        // Derive from directory name
        name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
    }

    Some(SkillMeta {
        name,
        description,
        path: path.parent().unwrap_or(path).to_path_buf(),
    })
}

fn parse_full(path: &Path) -> Option<Skill> {
    let content = fs::read_to_string(path).ok()?;
    let (fm, body) = split_frontmatter(&content);

    let mut name = String::new();
    let mut description = String::new();
    let mut allowed_tools = Vec::new();

    for line in fm.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            match key.trim() {
                "name" => name = value.trim().to_string(),
                "description" => description = value.trim().to_string(),
                "allowed-tools" => {
                    allowed_tools = value
                        .split_whitespace()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                _ => {}
            }
        }
    }

    if name.is_empty() {
        name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
    }

    Some(Skill {
        name,
        description,
        allowed_tools,
        instructions: body.to_string(),
        path: path.parent().unwrap_or(path).to_path_buf(),
    })
}

fn split_frontmatter(content: &str) -> (&str, &str) {
    if content.starts_with("---") {
        if let Some(end) = content[3..].find("---") {
            let fm = content[3..3 + end].trim();
            let body = content[3 + end + 3..].trim();
            (fm, body)
        } else {
            ("", content.trim())
        }
    } else {
        ("", content.trim())
    }
}

// ---------------------------------------------------------------------------
// Skill registry — in-memory cache for loaded skills
// ---------------------------------------------------------------------------

pub struct SkillRegistry {
    /// Discovered metadata (always loaded).
    metas: Vec<SkillMeta>,
    /// Fully loaded skills (populated on activation).
    loaded: HashMap<String, Skill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            metas: discover(),
            loaded: HashMap::new(),
        }
    }

    /// Reload metadata from disk.
    pub fn refresh(&mut self) {
        self.metas = discover();
        self.loaded.clear();
    }

    /// All discovered skill metadata.
    pub fn all_meta(&self) -> &[SkillMeta] {
        &self.metas
    }

    /// Activate a skill by name — loads full instructions.
    pub fn activate(&mut self, name: &str) -> Option<&Skill> {
        if !self.loaded.contains_key(name) {
            let skill = load(name)?;
            self.loaded.insert(name.to_string(), skill);
        }
        self.loaded.get(name)
    }

    /// Get an already-activated skill.
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.loaded.get(name)
    }

    /// All currently activated skills.
    pub fn activated(&self) -> Vec<&Skill> {
        self.loaded.values().collect()
    }

    /// Deactivate a skill (remove from memory).
    pub fn deactivate(&mut self, name: &str) {
        self.loaded.remove(name);
    }

    /// Deactivate all skills.
    pub fn deactivate_all(&mut self) {
        self.loaded.clear();
    }

    /// Collect all instructions from activated skills into a single string
    /// suitable for injection into the system prompt.
    pub fn combined_instructions(&self) -> String {
        if self.loaded.is_empty() {
            return String::new();
        }
        let mut out = String::from("\n\n## Active Skills\n\n");
        for skill in self.loaded.values() {
            out.push_str(&format!("### {}\n\n{}\n\n", skill.name, skill.instructions));
        }
        out
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_frontmatter() {
        let content = "---\nname: test\ndescription: A test skill\n---\n\nBody here.";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.contains("name: test"));
        assert!(fm.contains("description: A test skill"));
        assert_eq!(body, "Body here.");
    }

    #[test]
    fn test_split_frontmatter_no_frontmatter() {
        let content = "Just plain text.";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.is_empty());
        assert_eq!(body, "Just plain text.");
    }

    #[test]
    fn test_skill_registry_new() {
        // This test only works if ~/.radi/skills exists with defaults.
        // In CI it may be empty, which is fine.
        let registry = SkillRegistry::new();
        // Just verify it doesn't panic
        let _ = registry.all_meta();
    }
}
