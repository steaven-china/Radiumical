# Radiumical

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-dea584?style=flat-square&logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="License">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-green?style=flat-square" alt="Platform">
  <img src="https://img.shields.io/badge/binary-11%20MB-orange?style=flat-square" alt="Binary">
  <img src="https://img.shields.io/badge/memory-<30%20MB%20idle-brightgreen?style=flat-square" alt="Memory">
</p>

A lightweight, Rust-native agentic coding assistant for your terminal. 11 MB binary, 24 MB idle memory, 21 providers, full TUI.

```
██████╗  █████╗ ██████╗ ██╗██╗   ██╗███╗   ███╗██╗ ██████╗ █████╗ ██╗
██╔══██╗██╔══██╗██╔══██╗██║██║   ██║████╗ ████║██║██╔════╝██╔══██╗██║
██████╔╝███████║██║  ██║██║██║   ██║██╔████╔██║██║██║     ███████║██║
██╔══██╗██╔══██║██║  ██║██║██║   ██║██║╚██╔╝██║██║██║     ██╔══██║██║
██║  ██║██║  ██║██████╔╝██║╚██████╔╝██║ ╚═╝ ██║██║╚██████╗██║  ██║███████╗
╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝ ╚═╝ ╚═════╝ ╚═╝     ╚═╝╚═╝ ╚═════╝╚═╝  ╚═╝╚══════╝
```

## Features

### Core

- **21 Providers** — OpenAI, Anthropic, Google, DeepSeek, Mistral, Groq, Cohere, Ollama, OpenRouter, and any OpenAI-compatible API
- **20+ Built-in Tools** — File read/write/edit, code search, shell execution, LSP diagnostics, directory tree, system info, memory, sub-agent, and more
- **3 Agent Modes** — Auto (full access), Plan (read-only exploration), Exec (write & execute)
- **Context Compression** — Auto LLM summarization when conversation exceeds 80% of max context tokens (default 1M)
- **Session-level Orchestration** — Agent creates multi-step plans with dependencies, auto-executes in order, shared conversation context

### TUI

- **ratatui Full-screen TUI** — 60 FPS rendering, real-time markdown, code syntax highlighting
- **Collapsible Tool Call Boxes** — Double-click to expand/collapse, embedded scrollbar thumb
- **Session Manager** — `/sessions` opens full-screen session manager with load/save/delete/new
- **Provider Picker** — `/provider` to switch provider and model
- **Settings Overlay** — `/settings` to modify configuration inline
- **Dashboard** — `//` opens navigation panel
- **Mouse Support** — Scroll wheel, double-click tool calls, scrollbar drag

### Extensibility

- **MCP Client** — Configure in `~/.radi/mcp.json`, async stdio JSON-RPC with timeout
- **Skills System** — Follows [agentskills.io](https://agentskills.io) spec, `~/.radi/skills/{name}/SKILL.md`, agent loads on-demand via `list_skills`/`load_skill` tools
- **Agent Pool** — Custom agent roles in `~/.radi/agents/{name}.md`, agent switches via `list_agents`/`load_agent` tools
- **Layout DSL** — `layout_page` tool with grid/table/split/cols/rows/box layouts

### Performance

- **lz4 Transparent Compression** — Messages >1KB auto-compressed, transparent decompression
- **zstd JSONL Persistence** — Conversation stored as `.jsonl.zst`, 5-100x disk savings
- **Provider Response Cache** — LLM responses cached, disable with `RADI_DISABLE_LLM_CACHE=1`
- **Fully Async** — UiEvent channel uses `tokio::sync::mpsc::unbounded`, zero blocking in async context
- **Workspace-scoped Sessions** — Sessions isolated per project under `~/.radi/sessions/{hash}/`

## Installation

```bash
git clone https://github.com/steaven-china/Radiumical.git
cd Radiumical
cargo build --release
# binary: target/release/radiumical(.exe)
```

## Quick Start

```bash
# Set API key
export DEEPSEEK_API_KEY="sk-..."

# Launch TUI
radiumical

# Specify provider/model
radiumical -p openai -m gpt-4o

# Non-interactive mode
radiumical -t "Fix the bug in src/main.rs"

# Local Ollama
radiumical -p ollama -m codellama
```

## CLI Options

| Flag | Description | Default |
|------|-------------|---------|
| `-t, --task` | Non-interactive task | — |
| `-w, --workspace` | Workspace directory | `.` |
| `-p, --provider` | Provider | `deepseek` |
| `-m, --model` | Model name | auto-detected |
| `-k, --api-key` | API key | `$DEEPSEEK_API_KEY` |
| `--api-base` | Custom API base URL | provider default |
| `--max-iterations` | Max tool-call loops | `32` |
| `--concurrency` | Parallel tool executions | `8` |
| `--llm-timeout` | LLM timeout (seconds) | `120` |
| `--tool-timeout` | Tool timeout (seconds) | `300` |

## Commands

| Command | Description |
|---------|-------------|
| `/help` | Show help |
| `/plan` / `/exec` / `/auto` | Switch mode |
| `/review` | Self-review changes |
| `/tools` | List available tools |
| `/sessions` | Session manager TUI |
| `/session save/load/list/delete` | Session operations |
| `/skills` | List available skills |
| `/skill <name>` | Activate a skill |
| `/provider` | Provider/model picker |
| `/settings` | Configuration overlay |
| `/new` | New session (auto-saves current) |
| `/perf` | Performance monitor |

## Configuration

`~/.radi/config.toml`:

```toml
provider = "deepseek"
model = "deepseek-v4-pro"
max_context_tokens = 1000000
context_compress_ratio = 0.8
llm_timeout_secs = 180
tool_timeout_secs = 300
```

`~/.radi/mcp.json`:

```json
{
  "mcpServers": {
    "fs": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    }
  }
}
```

## Architecture

```
radiumical-core/              Core library
├── src/
│   ├── harness.rs            Agent runtime: LLM ↔ Tool loop, context compression, orchestrator
│   ├── pipeline.rs           PipelineRunner wrapper
│   ├── conversation.rs       zstd JSONL persistent conversation
│   ├── provider.rs           LLM Provider trait
│   ├── providers.rs          21 providers + registry (embedded fallback)
│   ├── session.rs            SessionPool, workspace-scoped isolation, items_to_messages
│   ├── skill/                agentskills.io standard skill system
│   ├── agent_pool/           Agent role pool
│   ├── mcp.rs                MCP async stdio client with timeout
│   ├── orchestrator.rs       Multi-step plan + dependency tracking
│   ├── tools/                20+ Tool implementations
│   ├── types.rs              Core types + lz4 transparent compression
│   └── ...

radiumical-tui/               TUI frontend
├── src/
│   ├── main.rs               Entry point, backend loop, MCP init
│   ├── tui.rs                TUI runner, event loop
│   ├── tui/
│   │   ├── app/              App state, event handling, mouse, commands
│   │   └── draw.rs           Two-pass rendering: measure → render
│   ├── layout/               Block layout engine
│   ├── session_tui.rs        Full-screen session manager
│   ├── board.rs              UI widgets (toast, confirm, picker)
│   └── markdown.rs           Markdown → ratatui Line converter
```

## Data Flow

```
User input → slash command / plain task
    ↓
BackendCmd → PipelineRunner.run()
    ↓
Harness.run() ←── orchestrator plan injects next task
    ↓  (loop)
Provider.chat() → SSE stream → ProviderEvent
    ↓
ToolCall → execute (MCP/native) → ToolResult
    ↓  (repeat until done / context compression triggered)
Final response → UiEvent → TUI output buffer → ratatui render
    ↓
Session save (zstd JSONL) ← lz4 transparent compression
```

## License

MIT
