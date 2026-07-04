# Skills

Radiumical supports the [agentskills.io](https://agentskills.io) specification for pluggable skill extensions.

## Directory Structure

Skills are stored in `~/.radi/skills/{name}/`:

```
~/.radi/skills/
├── my-skill/
│   ├── SKILL.md           # Skill definition (required)
│   ├── references/        # Reference files (optional)
│   └── scripts/           # Helper scripts (optional)
└── another-skill/
    └── SKILL.md
```

## SKILL.md Format

Each skill is defined by a `SKILL.md` file with YAML frontmatter:

```markdown
---
name: my-skill
description: Does something useful for the agent
allowed-tools:
  - read_file
  - search_code
---

# My Skill

When this skill is activated, follow these instructions:

1. Read the target file
2. Search for patterns
3. Apply the transformation
```

### Frontmatter Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | `string` | Yes | Skill identifier (must match directory name) |
| `description` | `string` | Yes | What the skill does (used for discovery matching) |
| `allowed-tools` | `string[]` | No | Restrict which tools the skill can use |

## Progressive Disclosure

Skills use a three-stage loading model to minimize memory footprint:

1. **Discovery** — Only `name` and `description` are loaded at startup
2. **Activation** — Full `SKILL.md` body loaded when a task matches the skill's description
3. **Execution** — Agent follows the skill instructions, optionally loading `references/` or `scripts/`

## Usage

```bash
# List available skills
/skills

# Activate a skill
/skill my-skill
```

The agent also discovers and loads skills automatically when a task matches a skill's description via the `list_skills` and `load_skill` tools.

## Creating a Skill

1. Create the directory: `mkdir -p ~/.radi/skills/my-skill`
2. Write `SKILL.md` with frontmatter and instructions
3. Optionally add `references/` and `scripts/` directories
4. Test with `/skills` to verify discovery
