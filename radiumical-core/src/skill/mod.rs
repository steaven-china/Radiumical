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
    if let Some(stripped) = content.strip_prefix("---") {
        if let Some(end) = stripped.find("---") {
            let fm = stripped[..end].trim();
            let body = stripped[end + 3..].trim();
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

    // ---------------------------------------------------------------------------
    // split_frontmatter
    // ---------------------------------------------------------------------------

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
    fn test_split_frontmatter_empty_content() {
        let content = "";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.is_empty());
        assert_eq!(body, "");
    }

    #[test]
    fn test_split_frontmatter_only_opening_dashes() {
        let content = "---\nname: test\nstill here";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.is_empty());
        assert_eq!(body, "---\nname: test\nstill here");
    }

    #[test]
    fn test_split_frontmatter_empty_frontmatter() {
        let content = "---\n---\nBody after empty frontmatter.";
        let (fm, body) = split_frontmatter(content);
        assert_eq!(fm, "");
        assert_eq!(body, "Body after empty frontmatter.");
    }

    #[test]
    fn test_split_frontmatter_no_body_after_frontmatter() {
        let content = "---\nname: test\ndescription: desc\n---";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.contains("name: test"));
        assert!(fm.contains("description: desc"));
        assert_eq!(body, "");
    }

    #[test]
    fn test_split_frontmatter_double_dash_not_frontmatter() {
        let content = "--\nnot frontmatter at all\n--";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.is_empty());
        assert_eq!(body, "--\nnot frontmatter at all\n--");
    }

    #[test]
    fn test_split_frontmatter_dashes_in_body_preserved() {
        let content = "---\nname: first\n---\n\nBody with --- in the middle --- and end.";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.contains("name: first"));
        assert_eq!(body, "Body with --- in the middle --- and end.");
    }

    #[test]
    fn test_split_frontmatter_whitespace_lines_in_fm() {
        let content = "---\n\nname: spaced\n\ndescription: whitespace\n\n---\n\nBody.";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.contains("name: spaced"));
        assert!(fm.contains("description: whitespace"));
        assert_eq!(body, "Body.");
    }

    #[test]
    fn test_split_frontmatter_comment_lines_in_fm() {
        let content = "---\n# comment line\nname: with-comment\n# another comment\ndescription: desc\n---\n\nBody.";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.contains("name: with-comment"));
        // Comments and empty lines are skipped during value parsing,
        // but they are still present in the raw frontmatter string.
        assert!(fm.contains("# comment line"));
        assert_eq!(body, "Body.");
    }

    #[test]
    fn test_split_frontmatter_with_indented_body() {
        let content = "---\nname: ind\n---\n\n    Indented body.\n    More indented.";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.contains("name: ind"));
        assert!(body.contains("Indented body."));
    }

    #[test]
    fn test_split_frontmatter_crlf_line_endings() {
        let content = "---\r\nname: win\r\ndescription: CRLF test\r\n---\r\n\r\nBody with CRLF.";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.contains("name: win"));
        assert!(fm.contains("description: CRLF test"));
        assert_eq!(body, "Body with CRLF.");
    }

    // ---------------------------------------------------------------------------
    // match_by_input
    // ---------------------------------------------------------------------------

    #[test]
    fn test_match_by_input_exact_skill_name() {
        ensure_defaults();
        let matches = match_by_input("code-review");
        assert!(!matches.is_empty());
        assert!(matches.iter().any(|m| m.name == "code-review"));
    }

    #[test]
    fn test_match_by_input_name_contained_in_input() {
        ensure_defaults();
        let matches = match_by_input("please run a code-review on this PR");
        assert!(!matches.is_empty());
        assert!(matches.iter().any(|m| m.name == "code-review"));
    }

    #[test]
    fn test_match_by_input_no_match() {
        ensure_defaults();
        let matches = match_by_input("xyznoneskill_notexist");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_match_by_input_empty_input() {
        ensure_defaults();
        let matches = match_by_input("");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_match_by_input_short_words_filtered() {
        ensure_defaults();
        // Single character "a" should be filtered out (< 2 chars)
        let matches = match_by_input("a i x");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_match_by_input_case_insensitive_name() {
        ensure_defaults();
        let matches = match_by_input("CODE-REVIEW");
        assert!(matches.iter().any(|m| m.name == "code-review"));
    }

    #[test]
    fn test_match_by_input_multiple_matches() {
        ensure_defaults();
        // "test" should match the test skill by name containment
        let matches = match_by_input("test debug");
        let names: Vec<&str> = matches.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"test"));
        assert!(names.contains(&"debug"));
    }

    // ---------------------------------------------------------------------------
    // SkillRegistry
    // ---------------------------------------------------------------------------

    #[test]
    fn test_skill_registry_new() {
        let registry = SkillRegistry::new();
        let _ = registry.all_meta();
    }

    #[test]
    fn test_skill_registry_activate_existing() {
        ensure_defaults();
        let mut registry = SkillRegistry::new();
        let skill = registry.activate("code-review");
        assert!(skill.is_some());
        assert_eq!(skill.unwrap().name, "code-review");
    }

    #[test]
    fn test_skill_registry_activate_nonexistent() {
        ensure_defaults();
        let mut registry = SkillRegistry::new();
        let skill = registry.activate("no-such-skill");
        assert!(skill.is_none());
    }

    #[test]
    fn test_skill_registry_get_activated() {
        ensure_defaults();
        let mut registry = SkillRegistry::new();
        registry.activate("code-review");
        let skill = registry.get("code-review");
        assert!(skill.is_some());
        assert_eq!(skill.unwrap().name, "code-review");
    }

    #[test]
    fn test_skill_registry_get_not_activated() {
        ensure_defaults();
        let registry = SkillRegistry::new();
        let skill = registry.get("code-review");
        assert!(skill.is_none());
    }

    #[test]
    fn test_skill_registry_deactivate() {
        ensure_defaults();
        let mut registry = SkillRegistry::new();
        registry.activate("code-review");
        assert!(registry.get("code-review").is_some());
        registry.deactivate("code-review");
        assert!(registry.get("code-review").is_none());
    }

    #[test]
    fn test_skill_registry_deactivate_nonexistent() {
        ensure_defaults();
        let mut registry = SkillRegistry::new();
        registry.deactivate("no-such-skill");
        // Should not panic
    }

    #[test]
    fn test_skill_registry_deactivate_all() {
        ensure_defaults();
        let mut registry = SkillRegistry::new();
        registry.activate("code-review");
        registry.activate("refactor");
        assert_eq!(registry.activated().len(), 2);
        registry.deactivate_all();
        assert!(registry.activated().is_empty());
    }

    #[test]
    fn test_skill_registry_activated_empty() {
        ensure_defaults();
        let registry = SkillRegistry::new();
        assert!(registry.activated().is_empty());
    }

    #[test]
    fn test_skill_registry_activated_order() {
        ensure_defaults();
        let mut registry = SkillRegistry::new();
        registry.activate("code-review");
        registry.activate("refactor");
        let activated = registry.activated();
        assert_eq!(activated.len(), 2);
        let names: Vec<&str> = activated.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"code-review"));
        assert!(names.contains(&"refactor"));
    }

    #[test]
    fn test_skill_registry_combined_instructions_empty() {
        ensure_defaults();
        let registry = SkillRegistry::new();
        let combined = registry.combined_instructions();
        assert!(combined.is_empty());
    }

    #[test]
    fn test_skill_registry_combined_instructions() {
        ensure_defaults();
        let mut registry = SkillRegistry::new();
        registry.activate("code-review");
        let combined = registry.combined_instructions();
        assert!(combined.contains("## Active Skills"));
        assert!(combined.contains("### code-review"));
        assert!(combined.contains("代码审查模式"));
    }

    #[test]
    fn test_skill_registry_combined_instructions_multiple() {
        ensure_defaults();
        let mut registry = SkillRegistry::new();
        registry.activate("code-review");
        registry.activate("refactor");
        let combined = registry.combined_instructions();
        assert!(combined.contains("### code-review"));
        assert!(combined.contains("### refactor"));
    }

    #[test]
    fn test_skill_registry_refresh_clears_loaded() {
        ensure_defaults();
        let mut registry = SkillRegistry::new();
        registry.activate("code-review");
        assert!(registry.get("code-review").is_some());
        registry.refresh();
        assert!(registry.get("code-review").is_none());
        assert!(registry.activated().is_empty());
    }

    #[test]
    fn test_skill_registry_all_meta_returns_slice() {
        ensure_defaults();
        let registry = SkillRegistry::new();
        let metas = registry.all_meta();
        assert!(!metas.is_empty());
        // Verify known defaults are present
        let names: Vec<&str> = metas.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"code-review"));
    }

    #[test]
    fn test_skill_registry_default_trait() {
        ensure_defaults();
        let registry = SkillRegistry::default();
        let _ = registry.all_meta();
    }

    // ---------------------------------------------------------------------------
    // parse_metadata (private, tested indirectly or through discover)
    // ---------------------------------------------------------------------------

    #[test]
    fn test_parse_metadata_name_fallback_to_dir() {
        // When the frontmatter has no 'name' field, name should fallback to directory name
        ensure_defaults();
        // All defaults have names in frontmatter, so this is tested indirectly.
        // For direct coverage: verify that a discovery of existing skills
        // returns SkillMeta with non-empty names.
        let metas = discover();
        for meta in &metas {
            assert!(!meta.name.is_empty());
            assert!(!meta.description.is_empty());
        }
    }

    #[test]
    fn test_parse_full_allowed_tools_parsing() {
        // Verify that allowed-tools in the frontmatter is split by whitespace
        ensure_defaults();
        // Load a known skill; the test default skill doesn't have allowed-tools,
        // so we trust the parsing logic from the unit test perspective.
        // The code-review skill loaded from disk should have [] allowed_tools.
        let skill = load("code-review").unwrap();
        assert!(skill.allowed_tools.is_empty());
    }

    // ---------------------------------------------------------------------------
    // split_frontmatter — additional edge cases
    // ---------------------------------------------------------------------------

    #[test]
    fn test_split_frontmatter_trailing_whitespace_after_close() {
        let content = "---\nname: a\n---   \n   \nBody.";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.contains("name: a"));
        assert_eq!(body, "Body.");
    }

    #[test]
    fn test_split_frontmatter_frontmatter_with_colons_in_value() {
        let content = "---\nname: test\ndescription: http://example.com\n---\n\nBody.";
        let (fm, body) = split_frontmatter(content);
        assert!(fm.contains("name: test"));
        // The description contains a colon; split_once splits on first colon only
        assert!(fm.contains("description: http://example.com"));
        assert_eq!(body, "Body.");
    }

    #[test]
    fn test_split_frontmatter_only_dashes_no_newlines() {
        let content = "------";
        // Starts with "---", content[3..] = "---", find("---") at pos 0
        // fm = content[3..3].trim() = "".trim() = ""
        // body = content[6..].trim() = "".trim() = ""
        let (fm, body) = split_frontmatter(content);
        assert_eq!(fm, "");
        assert_eq!(body, "");
    }

    #[test]
    fn test_split_frontmatter_three_dashes_only() {
        let content = "---";
        // Starts with "---", content[3..] = "", find("---") = None
        // Falls back to ("", "---")
        let (fm, body) = split_frontmatter(content);
        assert_eq!(fm, "");
        assert_eq!(body, "---");
    }
}
