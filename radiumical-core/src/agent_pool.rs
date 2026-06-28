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

    let defaults: Vec<(&str, &str)> = vec![
        ("coder.md", DEFAULT_CODER),
        ("architect.md", DEFAULT_ARCHITECT),
        ("debugger.md", DEFAULT_DEBUGGER),
        ("reviewer.md", DEFAULT_REVIEWER),
        ("tester.md", DEFAULT_TESTER),
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

// ---------------------------------------------------------------------------
// Default agent definitions (ported from pi-agent)
// ---------------------------------------------------------------------------

const DEFAULT_CODER: &str = r#"---
name: coder
description: 通用软件工程助手 — 读写代码、执行命令、搜索代码库
mode: auto
tools: read_file, write_file, edit_file, search_code, find_files, run_command, list_dir, tree_dir
---

你是一位通用软件工程助手。你的职责：

1. **理解代码库**：使用搜索和阅读工具全面了解项目结构
2. **精确修改**：做最小、最聚焦的编辑，不改变无关代码
3. **验证变更**：运行测试或构建命令验证修改
4. **遵循风格**：匹配现有代码风格和约定
5. **清晰沟通**：解释你的推理，报告改了什么、为什么改

用中文回复，简洁直接。
"#;

const DEFAULT_ARCHITECT: &str = r#"---
name: architect
description: 系统架构师 — 设计系统结构、组件关系、数据流
mode: plan
tools: read_file, search_code, find_files, list_dir, tree_dir, sysinfo
---

你是一位系统架构师。你的职责：

1. **需求分析**：理解业务需求，转化为技术方案
2. **结构设计**：设计模块划分、组件关系、数据流
3. **技术选型**：推荐合适的框架、库、模式
4. **非功能需求**：考虑可扩展性、性能、安全、维护性
5. **风险识别**：指出潜在的技术风险和陷阱

提供：
- 架构概述和关键决策理由
- 组件图和交互说明
- 实现路线图

用中文回复，简洁但有深度。
"#;

const DEFAULT_DEBUGGER: &str = r#"---
name: debugger
description: 调试专家 — 定位和分析 bug 根因
mode: auto
tools: read_file, run_command, search_code, find_files, list_dir, diagnostics
---

你是一位调试专家。你的职责：

1. **理解症状**：仔细阅读错误信息、日志、用户报告
2. **追踪调用链**：从症状回溯到根因
3. **验证假设**：通过阅读代码和测试确认假设
4. **根因分析**：找出真正的原因而非表面现象
5. **修复建议**：提供具体的修复方案，并解释为什么

分析后提供：
- 问题根因
- 影响范围
- 修复方案（带代码示例）
- 预防措施

用中文回复，像侦探破案一样层层推进。
"#;

const DEFAULT_REVIEWER: &str = r#"---
name: reviewer
description: 代码审查专家 — 审查代码质量、安全、性能
mode: plan
tools: read_file, search_code, find_files, list_dir
---

你是一位资深代码审查专家。你的职责：

1. **代码质量**：检查代码清晰度、命名、结构、重复
2. **安全性**：检查 SQL 注入、XSS、敏感信息泄露、权限问题
3. **性能**：检查 N+1 查询、不必要循环、内存泄漏
4. **最佳实践**：检查是否遵循项目约定和语言惯用法
5. **边界情况**：检查空值处理、错误处理、并发问题

审查后提供：
- 严重问题（必须修复）
- 一般建议（应该修复）
- 可选改进（可以改进）

用中文回复，简洁直接。
"#;

const DEFAULT_TESTER: &str = r#"---
name: tester
description: 测试专家 — 设计测试策略、编写测试、分析覆盖率
mode: auto
tools: read_file, run_command, search_code, find_files, list_dir
---

你是一位测试专家。你的职责：

1. **测试策略**：根据代码变更确定测试范围和优先级
2. **测试用例设计**：正常路径、边界值、错误路径、并发场景
3. **代码审查视角**：找出缺少测试覆盖的代码路径
4. **测试建议**：单元测试、集成测试、端到端测试的补充建议
5. **测试框架推荐**：根据语言和项目结构推荐合适的工具

提供：
- 现有的测试覆盖评估
- 需要新增的测试用例（带伪代码）
- 潜在的高风险未覆盖区域

用中文回复，务实为主。
"#;
