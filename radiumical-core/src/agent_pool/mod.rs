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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_temp_file(name: &str, content: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_agent.md");
        fs::write(&path, content).unwrap();
        (dir, path)
    }

    fn cleanup(dir: std::path::PathBuf) {
        let _ = fs::remove_dir_all(&dir);
    }

    // ---------------------------------------------------------------------------
    // parse_agent_file
    // ---------------------------------------------------------------------------

    #[test]
    fn test_parse_agent_all_fields() {
        let (dir, path) = make_temp_file("rad_test_all_fields", "---\nname: coder\ndescription: General coder\nmode: auto\ntools: read_file, write_file, edit_file\n---\n\nYou are a coder.");
        let agent = parse_agent_file(&path).unwrap();
        assert_eq!(agent.name, "coder");
        assert_eq!(agent.description, "General coder");
        assert_eq!(agent.mode, AgentRoleMode::Auto);
        assert_eq!(agent.tools, vec!["read_file", "write_file", "edit_file"]);
        assert_eq!(agent.prompt, "You are a coder.");
        cleanup(dir);
    }

    #[test]
    fn test_parse_agent_mode_plan() {
        let (dir, path) = make_temp_file("rad_test_mode_plan", "---\nname: planner\ndescription: Planner agent\nmode: plan\n---\n\nPlan mode prompt.");
        let agent = parse_agent_file(&path).unwrap();
        assert_eq!(agent.mode, AgentRoleMode::Plan);
        cleanup(dir);
    }

    #[test]
    fn test_parse_agent_mode_exec() {
        let (dir, path) = make_temp_file("rad_test_mode_exec", "---\nname: executor\ndescription: Executor agent\nmode: exec\n---\n\nExec mode prompt.");
        let agent = parse_agent_file(&path).unwrap();
        assert_eq!(agent.mode, AgentRoleMode::Exec);
        cleanup(dir);
    }

    #[test]
    fn test_parse_agent_mode_unknown_defaults_to_auto() {
        let (dir, path) = make_temp_file("rad_test_mode_unknown", "---\nname: unknown\ndescription: Unknown mode\nmode: invalid_mode\n---\n\nPrompt.");
        let agent = parse_agent_file(&path).unwrap();
        assert_eq!(agent.mode, AgentRoleMode::Auto);
        cleanup(dir);
    }

    #[test]
    fn test_parse_agent_no_frontmatter() {
        let (dir, path) = make_temp_file("rad_test_no_fm", "Just a plain prompt with no frontmatter.");
        let agent = parse_agent_file(&path).unwrap();
        assert_eq!(agent.name, "test_agent"); // derived from filename
        assert!(agent.description.is_empty());
        assert_eq!(agent.mode, AgentRoleMode::Auto);
        assert!(agent.tools.is_empty());
        assert_eq!(agent.prompt, "Just a plain prompt with no frontmatter.");
        cleanup(dir);
    }

    #[test]
    fn test_parse_agent_name_from_filename() {
        let (dir, path) = make_temp_file("rad_test_name_fallback", "---\ndescription: No name field in frontmatter\n---\n\nBody here.");
        let agent = parse_agent_file(&path).unwrap();
        assert_eq!(agent.name, "test_agent"); // derived from test_agent.md
        assert_eq!(agent.description, "No name field in frontmatter");
        cleanup(dir);
    }

    #[test]
    fn test_parse_agent_defaults_missing_mode_and_tools() {
        let (dir, path) = make_temp_file("rad_test_defaults", "---\nname: minimal\ndescription: Minimal agent\n---\n\nMinimal prompt.");
        let agent = parse_agent_file(&path).unwrap();
        assert_eq!(agent.name, "minimal");
        assert_eq!(agent.description, "Minimal agent");
        assert_eq!(agent.mode, AgentRoleMode::Auto);
        assert!(agent.tools.is_empty());
        assert_eq!(agent.prompt, "Minimal prompt.");
        cleanup(dir);
    }

    #[test]
    fn test_parse_agent_multiple_tools_with_spaces() {
        let (dir, path) = make_temp_file("rad_test_tools_spaces", "---\nname: toolsy\ndescription: Tools test\nmode: auto\ntools: a,  b , c ,d\n---\n\nTools prompt.");
        let agent = parse_agent_file(&path).unwrap();
        assert_eq!(agent.tools, vec!["a", "b", "c", "d"]);
        cleanup(dir);
    }

    #[test]
    fn test_parse_agent_single_tool() {
        let (dir, path) = make_temp_file("rad_test_single_tool", "---\nname: solo\ndescription: Single tool\nmode: auto\ntools: read_file\n---\n\nSingle tool prompt.");
        let agent = parse_agent_file(&path).unwrap();
        assert_eq!(agent.tools, vec!["read_file"]);
        cleanup(dir);
    }

    #[test]
    fn test_parse_agent_empty_tools() {
        let (dir, path) = make_temp_file("rad_test_empty_tools", "---\nname: no_tools\ndescription: No tools\nmode: auto\ntools:\n---\n\nNo tools prompt.");
        let agent = parse_agent_file(&path).unwrap();
        assert!(agent.tools.is_empty());
        cleanup(dir);
    }

    #[test]
    fn test_parse_agent_frontmatter_with_comments() {
        let (dir, path) = make_temp_file("rad_test_fm_comments", "---\n# a comment\nname: commented\n# another\ndescription: Has comments\n# more\n---\n\nPrompt.");
        let agent = parse_agent_file(&path).unwrap();
        assert_eq!(agent.name, "commented");
        assert_eq!(agent.description, "Has comments");
        cleanup(dir);
    }

    #[test]
    fn test_parse_agent_frontmatter_empty_lines() {
        let (dir, path) = make_temp_file("rad_test_fm_empty", "---\n\n\nname: spaced\n\n\ndescription: With spaces\n\n---\n\nPrompt.");
        let agent = parse_agent_file(&path).unwrap();
        assert_eq!(agent.name, "spaced");
        assert_eq!(agent.description, "With spaces");
        cleanup(dir);
    }

    #[test]
    fn test_parse_agent_multiline_prompt() {
        let (dir, path) = make_temp_file("rad_test_multiline", "---\nname: multi\ndescription: Multi line\n---\n\nLine 1.\nLine 2.\n\nLine 3.");
        let agent = parse_agent_file(&path).unwrap();
        assert_eq!(agent.prompt, "Line 1.\nLine 2.\n\nLine 3.");
        cleanup(dir);
    }

    // ---------------------------------------------------------------------------
    // AgentRoleMode
    // ---------------------------------------------------------------------------

    #[test]
    fn test_agent_role_mode_to_agent_mode_auto() {
        assert_eq!(AgentRoleMode::Auto.to_agent_mode(), crate::types::AgentMode::Auto);
    }

    #[test]
    fn test_agent_role_mode_to_agent_mode_plan() {
        assert_eq!(AgentRoleMode::Plan.to_agent_mode(), crate::types::AgentMode::Plan);
    }

    #[test]
    fn test_agent_role_mode_to_agent_mode_exec() {
        assert_eq!(AgentRoleMode::Exec.to_agent_mode(), crate::types::AgentMode::Exec);
    }

    #[test]
    fn test_agent_role_mode_default_is_auto() {
        assert_eq!(AgentRoleMode::default(), AgentRoleMode::Auto);
    }

    #[test]
    fn test_agent_role_mode_partial_eq() {
        assert_eq!(AgentRoleMode::Auto, AgentRoleMode::Auto);
        assert_ne!(AgentRoleMode::Auto, AgentRoleMode::Plan);
        assert_ne!(AgentRoleMode::Plan, AgentRoleMode::Exec);
    }

    #[test]
    fn test_agent_role_mode_debug() {
        assert_eq!(format!("{:?}", AgentRoleMode::Auto), "Auto");
        assert_eq!(format!("{:?}", AgentRoleMode::Plan), "Plan");
        assert_eq!(format!("{:?}", AgentRoleMode::Exec), "Exec");
    }

    #[test]
    fn test_agent_role_mode_clone() {
        let mode = AgentRoleMode::Plan;
        let cloned = mode.clone();
        assert_eq!(mode, cloned);
    }

    // ---------------------------------------------------------------------------
    // AgentDef Default
    // ---------------------------------------------------------------------------

    #[test]
    fn test_agent_def_default() {
        let def = AgentDef::default();
        assert!(def.name.is_empty());
        assert!(def.description.is_empty());
        assert_eq!(def.mode, AgentRoleMode::Auto);
        assert!(def.tools.is_empty());
        assert!(def.prompt.is_empty());
    }

    // ---------------------------------------------------------------------------
    // load_agents / get_agent — ensure defaults exist then query
    // ---------------------------------------------------------------------------

    #[test]
    fn test_ensure_defaults_then_load() {
        ensure_defaults();
        let agents = load_agents();
        assert!(!agents.is_empty());
        let names: Vec<&str> = agents.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"coder"));
        assert!(names.contains(&"architect"));
        assert!(names.contains(&"debugger"));
        assert!(names.contains(&"reviewer"));
        assert!(names.contains(&"tester"));
    }

    #[test]
    fn test_get_agent_existing() {
        ensure_defaults();
        let agent = get_agent("coder");
        assert!(agent.is_some());
        assert_eq!(agent.unwrap().name, "coder");
    }

    #[test]
    fn test_get_agent_nonexistent() {
        ensure_defaults();
        let agent = get_agent("no-such-agent");
        assert!(agent.is_none());
    }

    #[test]
    fn test_load_agents_sorted_by_name() {
        ensure_defaults();
        let agents = load_agents();
        for window in agents.windows(2) {
            assert!(window[0].name <= window[1].name);
        }
    }
}
