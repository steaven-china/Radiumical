# Session Management

Radiumical provides workspace-scoped session management with full state persistence.

## Concepts

### Workspace

A workspace is a project directory. Sessions are isolated per workspace — each workspace gets its own session storage identified by a hash of the canonical path.

### Session

A session is a conversation with the agent. Each session contains:
- **Meta** — name, timestamps, model, provider, mode, thinking effort, description, message count
- **Items** — typed records: `User`, `Assistant`, `Reasoning`, `Tool` (call + result), `Raw`

### Session Item Types

| Type | Fields | Description |
|------|--------|-------------|
| `Meta` | name, created, updated, model, provider, mode, thinking_effort, description, message_count | Session metadata (always line 0) |
| `User` | content | User message |
| `Assistant` | content | LLM response text |
| `Reasoning` | content | LLM reasoning/thinking content |
| `Tool` | call_id, name, arguments, result, is_error, duration_ms | Tool call and its result |
| `Raw` | content | Raw system messages |

## Storage Layout

```
~/.radi/
├── sessions/
│   ├── {workspace-hash-1}/
│   │   ├── {session-name}.jsonl.zst    # Session file (zstd compressed)
│   │   ├── {session-name}.jsonl        # Session file (plain, fallback)
│   │   ├── workspace.toml              # Workspace-level config overrides
│   │   └── {session-id}/
│   │       ├── checkpoints.jsonl       # Checkpoint metadata
│   │       └── snapshots/              # File snapshots (non-git workspaces)
│   └── {workspace-hash-2}/
│       └── ...
└── workspaces.json                     # Global workspace registry
```

## TUI Commands

### Quick Commands

| Command | Description |
|---------|-------------|
| `/new` | New session (auto-saves current) |
| `/sessions` | Open full-screen session manager |
| `/session save <name> [description]` | Save current session |
| `/session load <name>` | Load a saved session |
| `/session list` | List all sessions in current workspace |
| `/session delete <name>` | Delete a session |

### Session Manager TUI

`/sessions` opens a full-screen overlay with:

- **Session list** — all sessions in current workspace, sorted by last updated
- **Actions** — Load, Save, Delete, New
- **Inline editing** — edit name and description directly
- **Keyboard navigation** — Up/Down to browse, Left/Right to switch focus, Tab to cycle

### Workspace Commands

| Command | Description |
|---------|-------------|
| `/ws` | Open workspace manager TUI |
| `/session ws` | Show active workspace |
| `/session list-ws` | List all registered workspaces |
| `/session switch-ws <name>` | Switch to another workspace |
| `/session add-ws <path> [name]` | Register a new workspace |
| `/session remove-ws <name>` | Unregister a workspace |
| `/session tag <ws> <tag>` | Add tag to workspace |
| `/session untag <ws> <tag>` | Remove tag from workspace |
| `/session pin <ws>` | Pin workspace (always shown first) |
| `/session unpin <ws>` | Unpin workspace |

### Workspace Settings

Per-workspace configuration overrides (stored in `workspace.toml`):

| Command | Description |
|---------|-------------|
| `/session ws-set <key> <value>` | Set workspace config |
| `/session ws-unset <key>` | Unset workspace config (revert to global) |
| `/session ws-settings` | Show current workspace overrides |

Available keys: `model`, `mode`, `thinking_effort`, `max_context_tokens`, `llm_timeout_secs`, `tool_timeout_secs`, `context_compress_ratio`, `auto_continue`

## Workspace Registry

Global registry at `~/.radi/workspaces.json`:

```json
[
  {
    "name": "my-project",
    "path": "/home/user/projects/my-project",
    "hash": "a1b2c3d4",
    "tags": ["rust", "active"],
    "pinned": true,
    "last_active": "2026-07-04T10:00:00Z"
  }
]
```

Features:
- **Auto-discovery** — scans `~/.radi/sessions/` for unregistered workspaces
- **Tags** — organize workspaces by category
- **Pinning** — pinned workspaces appear first in the list
- **Last active** — tracks when each workspace was last used

## Config Inheritance

```
1. SessionConfig::default()     ← hardcoded defaults
2. ~/.radi/config.toml          ← global user config
3. ~/.radi/sessions/{hash}/workspace.toml  ← workspace overrides
4. CLI flags                    ← highest priority
```

## Auto-save

Sessions are automatically saved when:
- Starting a new session (`/new`)
- Switching sessions (`/session load`)
- Quitting the application

The session file is named after the session (auto-generated or user-provided).
