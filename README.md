# Radiumical

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-dea584?style=flat-square&logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="License">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-green?style=flat-square" alt="Platform">
  <img src="https://img.shields.io/badge/binary-6%20MB-orange?style=flat-square" alt="Binary Size">
  <img src="https://img.shields.io/badge/memory-10%20MB%20idle-brightgreen?style=flat-square" alt="Memory">
  <img src="https://img.shields.io/badge/version-0.1.0--pre.1-purple?style=flat-square" alt="Version">
</p>

<p align="center">
  <b>A lightning-fast, Rust-native AI coding assistant that lives in your terminal.</b><br>
  6 MB binary. 10 MB idle memory. 21 LLM providers. Full TUI. <i>No Electron. No Python. Pure Rust.</i>
</p>

```
██████╗  █████╗ ██████╗ ██╗██╗   ██╗███╗   ███╗██╗ ██████╗ █████╗ ██╗
██╔══██╗██╔══██╗██╔══██╗██║██║   ██║████╗ ████║██║██╔════╝██╔══██╗██║
██████╔╝███████║██║  ██║██║██║   ██║██╔████╔██║██║██║     ███████║██║
██╔══██╗██╔══██║██║  ██║██║██║   ██║██║╚██╔╝██║██║██║     ██╔══██║██║
██║  ██║██║  ██║██████╔╝██║╚██████╔╝██║ ╚═╝ ██║██║╚██████╗██║  ██║███████╗
╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝ ╚═╝ ╚═════╝ ╚═╝     ╚═╝╚═╝ ╚═════╝╚═╝  ╚═╝╚══════╝
```

---

## Why Radiumical?

Most AI coding tools are either **heavy** (VS Code forks, Electron shells) or **slow** (Python overhead, chained HTTP calls). Radiumical is different:

|  | Radiumical | Other Tools |
|---|---|---|
| **Runtime** | Rust — single native binary | Node.js / Python — interpreter + deps |
| **Binary size** | ~6 MB | 50–300+ MB |
| **Idle memory** | ~10 MB | 100–500+ MB |
| **Startup** | Instant (< 100 ms) | 1–5 seconds |
| **Interface** | Full TUI (60 FPS) + Tauri GUI | CLI only / heavy IDE |
| **Providers** | 21 built-in | Usually 3–5 |
| **Offline** | ✅ Ollama, local models | Often cloud-only |
| **Extensible** | MCP + Skills + Agent Pool | Limited or plugin-heavy |

> **Radiumical is for developers who want an AI pair programmer that's as fast as their terminal — not another browser tab.**

---

## Features

### 🧠 Agent Intelligence

| Feature | Description |
|---|---|
| **21 LLM Providers** | OpenAI, Anthropic, Google Gemini, DeepSeek, Mistral, Groq, Cohere, Ollama, OpenRouter, and any OpenAI-compatible endpoint — switch on the fly with `/provider` |
| **25+ Built-in Tools** | Read/write/edit files, regex code search, shell execution, LSP diagnostics, directory tree, system info, persistent memory, sub-agents, browser automation, layout DSL, and more |
| **3 Agent Modes** | `auto` (full access) · `plan` (read-only exploration) · `exec` (write & execute) — toggle mid-session |
| **Smart Orchestration** | Multi-step plans with dependency graphs, conditional guards, event bus — the agent can break complex tasks into ordered sub-goals and execute them autonomously |
| **Sub-Agent Spawning** | Delegate independent work to background agents (coder, architect, reviewer, tester) — parallel execution with result collection |
| **Context Compression** | Auto LLM summarization when conversation hits 80% of max tokens (default 1M) — no more "context window full" errors |

### 🖥️ Terminal UI (ratatui)

| Feature | Description |
|---|---|
| **60 FPS Rendering** | Smooth, flicker-free TUI with real-time markdown and syntax highlighting (via syntect) |
| **Streaming Output** | Tokens render as they arrive — see the agent *think* in real-time, with reasoning content in collapsible blocks |
| **Collapsible Tool Calls** | Double-click to expand/collapse tool call details — embedded scrollbar for long output |
| **Session Manager** | `/sessions` — full-screen browser with load, save, delete, and new session actions |
| **Settings Overlay** | `/settings` — inline config editor, change provider/model/limits without restarting |
| **Dashboard** | `/` — 2D keyboard-navigable panel for quick access to all features |
| **Mouse Support** | Scroll wheel, double-click tool boxes, scrollbar drag — works naturally in modern terminals |
| **Perf Monitor** | `/perf` — live FPS, frame time, render stats overlay |

### 🧩 Extensibility

| Feature | Description |
|---|---|
| **MCP Client** | [Model Context Protocol](https://modelcontextprotocol.io) — async stdio JSON-RPC with timeout, configure servers in `~/.radi/mcp.json` |
| **Skills System** | Follows [agentskills.io](https://agentskills.io) spec — drop markdown files in `~/.radi/skills/`, agent loads them on-demand |
| **Agent Pool** | Custom agent roles in `~/.radi/agents/*.md` — define specialized personas with restricted toolsets |
| **Layout DSL** | `layout_page` tool — render structured output as grids, tables, splits, columns, rows, or boxed sections |

### ⚡ Performance

| Feature | Description |
|---|---|
| **lz4 Compression** | Messages > 1 KB auto-compressed in-transit — transparent to both agent and LLM |
| **zstd Persistence** | Conversations saved as `.jsonl.zst` — **5–100× disk savings** vs plain JSON |
| **LLM Response Cache** | 128-entry LRU cache — avoid duplicate API calls, disable with `RADI_DISABLE_LLM_CACHE=1` |
| **Fully Async** | Tokio multi-threaded runtime — `mpsc` channels, zero blocking in async context, parallel tool execution |
| **Workspace Isolation** | Sessions scoped per project under `~/.radi/sessions/{hash}/` — no cross-contamination |

---

## Installation

### Prerequisites

- **Rust** toolchain (1.75+) — [rustup.rs](https://rustup.rs)
- **Git** (with submodule support)

### Build from Source

```bash
# Clone with submodules
git clone --recurse-submodules https://github.com/steaven-china/Radiumical.git
cd Radiumical

# Release build (optimized, ~6 MB)
cargo build --release
# → target/release/radiumical(.exe)

# Ultra-small build (~5 MB, slightly slower)
cargo build --profile release-small
```

> If you cloned without `--recurse-submodules`, run `git submodule update --init` before building.

### Pre-built Binaries

Pre-built binaries for Windows, macOS, and Linux are available on the [Releases](https://github.com/steaven-china/Radiumical/releases) page.

---

## Quick Start

```bash
# 1. Set your API key
export DEEPSEEK_API_KEY="sk-..."
# or: export OPENAI_API_KEY="sk-..."
# or: export ANTHROPIC_API_KEY="sk-ant-..."

# 2. Launch the TUI
radiumical

# 3. Start coding! Just describe what you want:
#    "Fix the race condition in src/harness.rs"
#    "Write unit tests for the session module"
#    "Explain how the orchestrator works"
```

### Common Usage Patterns

```bash
# Use a specific provider + model
radiumical -p anthropic -m claude-sonnet-4-20250514

# Non-interactive: run a task and exit
radiumical -t "Refactor auth.rs to use async/await"

# Local model via Ollama (free, offline)
radiumical -p ollama -m codellama

# Custom workspace
radiumical -w ~/my-project

# High-concurrency mode (faster tool execution)
radiumical --concurrency 16
```

---

## CLI Reference

| Flag | Description | Default |
|---|---|---|
| `-t, --task <TASK>` | Non-interactive task (run & exit) | — |
| `-w, --workspace <DIR>` | Workspace directory | `.` |
| `-p, --provider <NAME>` | LLM provider (`deepseek`, `openai`, `anthropic`, `ollama`, ...) | `deepseek` |
| `-m, --model <NAME>` | Model name | auto-detected |
| `-k, --api-key <KEY>` | API key | `$DEEPSEEK_API_KEY` |
| `--api-base <URL>` | Custom API base URL | provider default |
| `--max-iterations <N>` | Max tool-call loops per turn | `32` |
| `--concurrency <N>` | Parallel tool executions | `8` |
| `--llm-timeout <SECS>` | LLM request timeout | `120` |
| `--tool-timeout <SECS>` | Tool execution timeout | `300` |
| `-v, --verbose` | Debug logging | off |

---

## In-App Commands

| Command | Description |
|---|---|
| `/help` | Show help |
| `/auto` · `/plan` · `/exec` | Switch agent mode |
| `/review` | Self-review recent changes |
| `/tools` | List available tools |
| `/sessions` | Session manager (full-screen) |
| `/session save <name>` | Save current session |
| `/session load <name>` | Load a saved session |
| `/session list` | List all saved sessions |
| `/session delete <name>` | Delete a session |
| `/skills` | List available skills |
| `/skill <name>` | Activate a skill |
| `/agents` | List available agent roles |
| `/agent <name>` | Switch agent role |
| `/provider` | Provider & model picker |
| `/model <name>` | Quick model switch |
| `/settings` | Configuration overlay |
| `/new` | New session (auto-saves current) |
| `/clear` · `/cls` | Clear & start fresh |
| `/retry` · `/r` | Retry last prompt |
| `/perf` | Performance monitor overlay |

---

## Configuration

All config files live under `~/.radi/`:

### `~/.radi/config.toml`

```toml
# LLM settings
provider = "deepseek"
model = "deepseek-v4-pro"
api_key = "sk-..."        # optional — falls back to env var

# Performance tuning
max_context_tokens = 1000000     # 1M token context window
context_compress_ratio = 0.8     # compress at 80% capacity
llm_timeout_secs = 180
tool_timeout_secs = 300
max_iterations = 32

# Behavior
mode = "auto"             # auto | plan | exec
heartbeat_secs = 10       # 0 to disable
```

### `~/.radi/mcp.json`

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"]
    }
  }
}
```

### Agent Definitions (`~/.radi/agents/*.md`)

```markdown
---
name: security-auditor
description: Security-focused code reviewer
mode: plan
tools:
  - read_file
  - search_code
  - find_files
  - run_command
---

You are a security auditor. For every file you review, check for:
1. Input validation gaps
2. Injection vulnerabilities
3. Insecure crypto usage
4. Hardcoded secrets
```

### Skills (`~/.radi/skills/*/SKILL.md`)

Follows the [agentskills.io](https://agentskills.io) specification. Drop a `SKILL.md` in a named directory under `~/.radi/skills/`, and the agent can load it on-demand.

---

## Project Structure

```
Radiumical/                   Rust workspace (6 crates)
│
├── radiumical-core/           Core engine library
│   └── src/
│       ├── harness.rs         Agent runtime: LLM ↔ Tool loop
│       ├── pipeline.rs        PipelineRunner wrapper
│       ├── provider.rs        LLM Provider trait + implementations
│       ├── providers.rs       21 providers + remote registry
│       ├── orchestrator/      Multi-step plans + dependency DAG
│       ├── dynamic/           Reactive orchestrator with guards & events
│       ├── cluster.rs         Agent cluster with worker slots
│       ├── tools/             25+ built-in tool implementations
│       ├── conversation.rs    zstd JSONL persistent storage
│       ├── session.rs         Session pool & workspace isolation
│       ├── memory.rs          Persistent agent memory
│       ├── mcp.rs             MCP async stdio client
│       ├── skill/             agentskills.io skill system
│       ├── agent_pool/        Agent role pool
│       ├── types.rs           Core types + lz4 compression
│       └── ...
│
├── radiumical-tui/            Terminal UI (ratatui + crossterm)
├── radiumical-tauri/          Desktop GUI (Tauri v2, experimental)
├── radiumical-rpc/            RPC server for external integrations
├── tools/csv-to-jsonl/        Provider registry converter
├── tools/memory-bench/        Memory & compression benchmarks
│
├── docs/                      15 docs covering architecture, agents,
│                              skills, MCP, memory, sessions, and more
└── providers-record/          Embedded provider registry (JSONL)
```

---

## Data Flow

```
User Input (plain text or slash command)
    │
    ▼
┌──────────┐    slash cmd?    ┌──────────────┐
│  TUI App  │ ───────────────▶ │ Command Exec  │
└────┬─────┘                  └──────────────┘
     │ plain task
     ▼
┌──────────────┐     ┌────────────────┐
│ BackendCmd   │────▶│ PipelineRunner  │
└──────────────┘     └───────┬────────┘
                             │
                             ▼
                    ┌──────────────────┐
                    │   Harness         │◀── Orchestrator injects next task
                    │  (LLM ↔ Tool loop)│
                    └────────┬─────────┘
                             │
              ┌──────────────┼──────────────┐
              ▼              ▼              ▼
        ┌──────────┐  ┌──────────┐  ┌──────────────┐
        │ Provider │  │  Tools   │  │ Conversation  │
        │  (SSE)   │  │ (native  │  │ (zstd JSONL)  │
        └────┬─────┘  │ / MCP)   │  └──────────────┘
             │        └──────────┘
             ▼
       ProviderEvent → UiEvent → 60 FPS TUI render
             │
             ▼
       Session saved (zstd JSONL) ← lz4 compression
```

> For a deeper dive, see [`docs/architecture.md`](docs/architecture.md).

---

## Supported Providers

| Provider | Kind | Key Env Var | Notes |
|---|---|---|---|
| **DeepSeek** | OpenAI-compatible | `DEEPSEEK_API_KEY` | Default provider |
| **OpenAI** | OpenAI | `OPENAI_API_KEY` | GPT-4o, GPT-4.1, o3, etc. |
| **Anthropic** | Anthropic | `ANTHROPIC_API_KEY` | Claude Sonnet 4, Opus 4 |
| **Google Gemini** | OpenAI-compatible | `GEMINI_API_KEY` | |
| **Mistral** | OpenAI-compatible | `MISTRAL_API_KEY` | |
| **Groq** | OpenAI-compatible | `GROQ_API_KEY` | Ultra-fast inference |
| **Cohere** | OpenAI-compatible | `COHERE_API_KEY` | |
| **Ollama** | OpenAI-compatible | — | Local, no API key needed |
| **OpenRouter** | OpenAI-compatible | `OPENROUTER_API_KEY` | Multi-provider gateway |
| **Any OpenAI-compatible** | OpenAI-compatible | custom | Bring your own endpoint |

Run `/provider` in-app to switch on the fly, or use `-p <name>` at launch.

---

## Roadmap

- [ ] **Tauri desktop app** — mature GUI with native performance
- [ ] **Plugin marketplace** — community-contributed tools & skills
- [ ] **VS Code extension** — in-editor agent panel
- [ ] **Multi-modal support** — image input for vision models
- [ ] **Session sharing** — export/import sessions as `.radi` files
- [ ] **Telemetry-free analytics** — opt-in, local-only usage stats

---

## Contributing

Contributions are welcome! Before diving in:

1. Read [`docs/architecture.md`](docs/architecture.md) for the big picture
2. Read [`docs/building.md`](docs/building.md) for dev setup
3. Check open issues — pick one tagged `good first issue`
4. Run `cargo fmt --all && cargo clippy --all -- -D warnings` before committing

PRs should target the `main` branch and include a brief description of what changed and why.

---

## Documentation

| Document | Topic |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | System design & data flow |
| [`docs/agents.md`](docs/agents.md) | Agent pool & custom roles |
| [`docs/skills.md`](docs/skills.md) | Skill system (agentskills.io) |
| [`docs/mcp.md`](docs/mcp.md) | MCP client configuration |
| [`docs/memory.md`](docs/memory.md) | Persistent memory system |
| [`docs/sessions.md`](docs/sessions.md) | Session management |
| [`docs/checkpoints.md`](docs/checkpoints.md) | Checkpoint time-machine |
| [`docs/orchestrator.md`](docs/orchestrator.md) | Task orchestration |
| [`docs/configuration.md`](docs/configuration.md) | Full config reference |
| [`docs/tools.md`](docs/tools.md) | Built-in tool catalog |
| [`docs/providers.md`](docs/providers.md) | Provider setup guide |
| [`docs/keybindings.md`](docs/keybindings.md) | Keyboard shortcuts |
| [`docs/context-compression.md`](docs/context-compression.md) | Context window management |
| [`docs/layout.md`](docs/layout.md) | Layout DSL reference |
| [`docs/building.md`](docs/building.md) | Build & dev guide |

---

## License

MIT © [Steven Jiang(steaven-china)](https://github.com/steaven-china)

---

<p align="center">
  <sub>Built with 🦀 Rust, 🐀 ratatui, and lots of ☕ and (o-o)tauri</sub>
</p>
