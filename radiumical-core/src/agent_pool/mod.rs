//! Agent pool — load custom agent roles from ~/.radi/agents/*.md
//!
//! Each agent is defined by a Markdown file with YAML frontmatter:
//!
//! ---
//! name: architect
//! description: System architect — designs structure and data flow
//! mode: plan
//! tools: read_file, search_code, find_files
//! ---
//!
//! You are a system architect. Your job is to...

use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct AgentDef {
    pub name: String,
    pub description: String,
    pub mode: AgentRoleMode,
    pub tools: Vec<String>,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentRoleMode {
    Auto,
    Plan,
    Exec,
}

impl Default for AgentRoleMode {
    fn default() -> Self {
        AgentRoleMode::Auto
    }
}

impl AgentRoleMode {
    pub fn to_agent_mode(&self) -> crate::types::AgentMode {
        match self {
            AgentRoleMode::Auto => crate::types::AgentMode::Auto,
            AgentRoleMode::Plan => crate::types::AgentMode::Plan,
            AgentRoleMode::Exec => crate::types::AgentMode::Exec,
        }
    }
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

fn agents_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".radi")
        .join("agents")
}

/// Scan ~/.radi/agents/*.md and parse each as an AgentDef.
pub fn load_agents() -> Vec<AgentDef> {
    let dir = agents_dir();
    let mut agents = vec![];

    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            if let Some(agent) = parse_agent_file(&path) {
                agents.push(agent);
            }
        }
    }

    agents.sort_by(|a, b| a.name.cmp(&b.name));
    agents
}

/// Get a single agent by name.
pub fn get_agent(name: &str) -> Option<AgentDef> {
    load_agents().into_iter().find(|a| a.name == name)
}

/// Ensure default agents exist. Call once on startup.
pub fn ensure_defaults() {
    let dir = agents_dir();
    let _ = fs::create_dir_all(&dir);

    let defaults: &[(&str, &str)] = &[
        ("coder.md", include_str!("defaults/coder.md")),
        ("architect.md", include_str!("defaults/architect.md")),
        ("debugger.md", include_str!("defaults/debugger.md")),
        ("reviewer.md", include_str!("defaults/reviewer.md")),
        ("tester.md", include_str!("defaults/tester.md")),
    ];

    for (filename, content) in defaults {
        let path = dir.join(filename);
        if !path.exists() {
            let _ = fs::write(&path, content);
        }
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

fn parse_agent_file(path: &PathBuf) -> Option<AgentDef> {
    let content = fs::read_to_string(path).ok()?;

    // Split frontmatter and body
    let (frontmatter, body) = if content.starts_with("---") {
        if let Some(end) = content[3..].find("---") {
            let fm = content[3..3 + end].trim();
            let rest = content[3 + end + 3..].trim();
            (fm, rest)
        } else {
            ("", content.trim())
        }
    } else {
        ("", content.trim())
    };

    let mut def = AgentDef::default();
    def.prompt = body.to_string();

    // Parse frontmatter line by line
    for line in frontmatter.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim().to_string();
            match key {
                "name" => def.name = value,
                "description" => def.description = value,
                "mode" => {
                    def.mode = match value.as_str() {
                        "plan" => AgentRoleMode::Plan,
                        "exec" => AgentRoleMode::Exec,
                        _ => AgentRoleMode::Auto,
                    };
                }
                "tools" => {
                    def.tools = value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                _ => {}
            }
        }
    }

    // Fallback: derive name from filename if missing
    if def.name.is_empty() {
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            def.name = stem.to_string();
        }
    }

    Some(def)
}
