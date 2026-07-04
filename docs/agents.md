# Agent Pool

Radiumical supports custom agent roles that can be loaded on-demand during a session.

## Directory Structure

Agent definitions are stored in `~/.radi/agents/*.md`:

```
~/.radi/agents/
├── architect.md
├── reviewer.md
└── tester.md
```

## Agent Definition Format

Each agent is a Markdown file with YAML frontmatter:

```markdown
---
name: architect
description: System architect — designs structure and data flow
mode: plan
tools:
  - read_file
  - search_code
  - find_files
---

You are a system architect. Your job is to:

1. Analyze the codebase structure
2. Identify architectural patterns
3. Propose improvements with trade-off analysis
4. Never make changes directly — only recommend
```

### Frontmatter Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | `string` | Yes | Agent identifier |
| `description` | `string` | Yes | What the agent does |
| `mode` | `string` | No | `auto`, `plan`, or `exec` (default: `auto`) |
| `tools` | `string[]` | No | Restrict available tools |

### Agent Modes

| Mode | Description |
|------|-------------|
| `auto` | Full access — can read, write, and execute |
| `plan` | Read-only exploration — cannot modify files or run commands |
| `exec` | Write & execute — focused on implementation |

## Usage

```bash
# List available agents
/agents

# Switch to an agent
/agent architect
```

The agent can also switch roles dynamically via the `list_agents` and `load_agent` tools.

## Creating an Agent

```bash
# Create the directory
mkdir -p ~/.radi/agents

# Create an agent definition
cat > ~/.radi/agents/reviewer.md << 'EOF'
---
name: reviewer
description: Code reviewer — finds bugs and suggests improvements
mode: plan
tools:
  - read_file
  - search_code
  - find_files
  - lsp_diagnostics
---

You are a code reviewer. Focus on:
- Correctness and edge cases
- Performance implications
- Security concerns
- Code clarity and maintainability
EOF
```
