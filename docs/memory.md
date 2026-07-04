# Memory System

Radiumical provides a three-tier memory system for persistent knowledge across sessions.

## Tiers

| Tier | Capacity | Purpose | Persistence |
|------|----------|---------|-------------|
| **Core** | Unlimited | Critical facts, preferences, project conventions | Always in system prompt |
| **Mino** (Medium) | 50 entries | Important context, recent decisions | Retrieved on demand |
| **Short** | 20 entries | Temporary notes, recent observations | Retrieved on demand, FIFO eviction |

## Storage

Memory is stored per-workspace at `~/.radi/mem/{workspace_hash}/memory.json`:

```json
{
  "core": [
    {"content": "This project uses Rust 2021 edition", "timestamp": "2026-07-01T00:00:00Z", "tags": ["rust", "project"]}
  ],
  "mino": [...],
  "short": [...]
}
```

## How Memory Is Used

### System Prompt Injection

Core memories are automatically injected into the LLM system prompt:

```
## Memory (Core)
- This project uses Rust 2021 edition
- Always run `cargo fmt` before committing
- The auth module uses JWT with RS256
```

### Retrieved Context

Mino and short-term memories are available as retrieved context when the agent needs them:

```
## Memory (Recent)
- Discussed migration plan for database v2
- Decided to use connection pooling

## Recent Sessions
- Session "refactor-auth" from 2026-07-03
```

## TUI Commands

### View Memory

| Command | Description |
|---------|-------------|
| `/memory` | Show all memories across all tiers |
| `/memory search <query>` | Search memories by content or tags |

### Add Memory

```bash
/remember core "This project uses Rust 2021 edition" --tag rust --tag project
/remember mino "Decided to use connection pooling for DB v2" --tag decision
/remember short "Found bug in auth module line 42" --tag bug
```

### Manage Memory

| Command | Description |
|---------|-------------|
| `/memory clear core` | Clear all core memories |
| `/memory clear mino` | Clear all medium-term memories |
| `/memory clear short` | Clear all short-term memories |

## Agent Tool

The agent can also manage memory via the `memory` tool:

```json
{
  "action": "add",
  "tier": "core",
  "content": "Always run cargo fmt before committing",
  "tags": ["convention", "rust"]
}
```

Available actions: `add`, `delete`, `edit`, `search`, `clear`, `list`

## Eviction Policy

When a tier exceeds its capacity:
- **Core**: no eviction (unbounded)
- **Mino**: FIFO — oldest entry removed first
- **Short**: FIFO — oldest entry removed first

## Workspace Isolation

Memory is isolated per workspace. Different projects have independent memory stores, so conventions and context from one project don't leak into another.
