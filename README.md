# Radiumical

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-orange?style=flat-square" alt="Rust">
  <img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="License">
  <img src="https://img.shields.io/badge/platform-cross--platform-green?style=flat-square" alt="Platform">
</p>

**Radiumical** is a powerful agentic CLI tool for coding. It lives in your terminal, understands your codebase, and helps you ship — whether that's fixing bugs, refactoring, or exploring unfamiliar code.

```
██████╗  █████╗ ██████╗ ██╗██╗   ██╗███╗   ███╗██╗ ██████╗ █████╗ ██╗
██╔══██╗██╔══██╗██╔══██╗██║██║   ██║████╗ ████║██║██╔════╝██╔══██╗██║
██████╔╝███████║██║  ██║██║██║   ██║██╔████╔██║██║██║     ███████║██║
██╔══██╗██╔══██║██║  ██║██║██║   ██║██║╚██╔╝██║██║██║     ██╔══██║██║
██║  ██║██║  ██║██████╔╝██║╚██████╔╝██║ ╚═╝ ██║██║╚██████╗██║  ██║███████╗
╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝ ╚═╝ ╚═════╝ ╚═╝     ╚═╝╚═╝ ╚═════╝╚═╝  ╚═╝╚══════╝
```

---

## ✨ Features

- **🤖 LLM Agent Loop** — Reads your code, plans changes, executes tools, and iterates until done.
- **🖥️ Rich TUI** — Built with [ratatui](https://ratatui.rs), 60 FPS rendering, markdown output, scrollable history.
- **🔧 17 Built-in Tools** — `read_file`, `write_file`, `edit_file`, `search_code`, `find_files`, `run_command`, LSP diagnostics, system info, and more.
- **🌐 Multi-Provider** — Supports OpenAI, DeepSeek, Anthropic, Ollama, and any OpenAI-compatible API.
- **📋 Slash Commands** — `/plan`, `/exec`, `/auto`, `/review`, `/tools`, `/session`, `/models`, and more.
- **🎯 Three Agent Modes**:
  - **Auto** — Full capabilities, all tools available.
  - **Plan** — Read-only: explore code without making changes.
  - **Exec** — Write mode: make edits and run commands.
- **💾 Session Management** — Save, load, list, and delete conversation sessions.
- **📝 JSONL Conversation Logging** — Full message history persisted to `conversation.jsonl`.
- **🎨 Markdown Rendering** — Code blocks, tables, headings, inline styles via `pulldown-cmark`.
- **🔍 LSP Diagnostics** — Auto-detect language and run `cargo check`, `ruff`, `eslint`, `go vet`.
- **⚡ Async Pipeline** — Tokio-based, parallel tool execution, cancellable, with configurable timeouts.
- **🖱️ Mouse Support** — Click, scroll, drag-resize panels, select text.
- **📊 Performance Monitor** — Built-in FPS counter and frame-timing overlay (`/perf`).
- **🔤 CRLF Auto-Normalization** — Cross-platform line ending handling out of the box.

---

## 📦 Installation

### Prerequisites

- [Rust](https://rustup.rs) toolchain (latest stable)
- An API key for your LLM provider (e.g., DeepSeek, OpenAI)

### Build from Source

```bash
git clone https://github.com/steaven-china/Radiumical.git
cd radiumical
cargo build --release
```

The binary will be at `target/release/radiumical` (or `radiumical.exe` on Windows).

### Install via Cargo

```bash
cargo install --path .
```

---

## 🚀 Quick Start

```bash
# Set your API key (pick one)
export DEEPSEEK_API_KEY="sk-..."
export OPENAI_API_KEY="sk-..."

# Launch interactive TUI (defaults to DeepSeek)
radiumical

# Use a different provider
radiumical -p openai -m gpt-4o

# Run a one-shot task (non-interactive)
radiumical -t "Fix the bug in src/main.rs"

# Specify workspace
radiumical -w /path/to/project

# Use local Ollama
radiumical -p ollama -m codellama
```

---

## ⌨️ Usage

### CLI Options

| Flag | Description | Default |
|------|-------------|---------|
| `-t, --task` | Task to execute (skips TUI) | — |
| `-w, --workspace` | Workspace directory | `.` |
| `-p, --provider` | Provider: `openai`, `deepseek`, `anthropic`, `ollama` | `deepseek` |
| `-m, --model` | Model name | auto-detected |
| `-k, --api-key` | API key | `$DEEPSEEK_API_KEY` / `$OPENAI_API_KEY` |
| `--api-base` | Custom API base URL | provider default |
| `--max-iterations` | Max tool-calling loops | `32` |
| `--concurrency` | Max parallel tool executions | `8` |
| `--llm-timeout` | LLM request timeout (seconds) | `120` |
| `--tool-timeout` | Tool execution timeout (seconds) | `300` |
| `--heartbeat` | Heartbeat interval (seconds, 0=off) | `10` |
| `-v, --verbose` | Debug logging | `false` |

### Interactive Mode Keys

| Key | Action |
|-----|--------|
| `Enter` | Submit task |
| `Shift+Enter` | Newline in input |
| `Ctrl+C` | Quit |
| `Ctrl+O` | Toggle reasoning display |
| `Ctrl+Shift+C` | Copy all output to clipboard |
| `↑` / `↓` | Navigate history / hints |
| `PgUp` / `PgDn` | Scroll output (or hint pages) |
| `//` | Toggle Dashboard |

### Slash Commands

| Command | Description |
|---------|-------------|
| `/help`, `/?` | Show help overlay |
| `/plan` | Read-only mode (explore, no edits) |
| `/exec` | Write mode (make changes) |
| `/auto` | Full auto mode |
| `/review` | Self-review recent changes |
| `/tools`, `/t` | List available tools |
| `/models` | Model picker panel |
| `/model <n>` | Switch model |
| `/session save <name>` | Save conversation to named session |
| `/session load <name>` | Load a saved session |
| `/session list` | List all saved sessions |
| `/session delete <name>` | Delete a session |
| `/settings`, `/config` | Show configuration |
| `/clear`, `/cls` | Clear screen |
| `/perf` | Toggle performance monitor |
| `/exit`, `/q` | Quit |

---

## 🔧 Configuration

Place a `config.toml` in the workspace root to set persistent defaults:

```toml
provider = "deepseek"
model = "deepseek-v4-pro"
heartbeat_secs = 10
llm_timeout_secs = 180
max_iterations = 32
```

CLI arguments always take priority over config file values.

---

## 🧰 Built-in Tools

| Tool | Description |
|------|-------------|
| `read_file` | Read file contents with line numbers and pagination |
| `write_file` | Create or overwrite a file |
| `edit_file` | Targeted search-and-replace edits with auto CRLF/LF normalization |
| `search_code` | Regex grep across the workspace |
| `find_files` | Glob pattern file search |
| `run_command` | Execute shell commands with timeout |
| `todo_list` | Manage a task list |
| `plan` | Step-by-step planning |
| `goal` | Goal/sub-goal decomposition |
| `choice` | Prompt the user for a choice |
| `diagnostics` | Language-aware linting/checking |
| `sysinfo` | System information (OS, CPU, memory, disk) |
| `list_dir` | Directory listing with sizes |
| `tree` | Directory tree view (max depth 3) |
| `time_now` | Current date and time |
| `cron_info` | Crontab listing |
| `annotate` | Virtual annotations on file lines |

---

## 🏗️ Architecture

```
src/
├── main.rs          # Entry point, CLI parsing, backend loop
├── pipeline.rs       # Agent loop: LLM → tool → LLM
├── provider.rs       # LLM provider abstraction (OpenAI-compatible)
├── tools.rs          # Tool trait + 17 tool implementations
├── conversation.rs   # JSONL-backed persistent conversation
├── session.rs        # Named session save/load
├── types.rs          # Core types: Message, ToolCall, SessionConfig, AgentMode
├── commands.rs       # Slash command registry
├── config.rs         # config.toml persistence
├── markdown.rs       # Markdown → ratatui Line converter
├── layout.rs         # Two-pass output block layout engine
├── tui.rs            # TUI runner, channels, slash hints, logo
├── tui/
│   ├── app.rs        # TUI application state & key handling
│   └── draw.rs       # Output/input/status rendering
├── board.rs          # Reusable UI widgets (panels, toasts, confirm dialogs)
├── dashboard.rs      # Navigation hub with categories
├── lsp.rs            # Language detection & diagnostics
├── systools.rs       # System tools (sysinfo, list_dir, tree, cron)
├── perf.rs           # Performance monitor (FPS, frame times)
├── hooks/
│   ├── mod.rs
│   └── crlf.rs       # CRLF ↔ LF auto-normalization hook
└── layout.rs         # Output block measurement & layout
```

### Data Flow

```
User Input → Commands/Slash Dispatch
    ↓
PipelineRunner.run()
    ↓  (loop)
Provider.chat() → SSE stream → ProviderEvents
    ↓
ToolCall → execute with timeout → ToolResult
    ↓  (repeat until done)
Final response → TUI output buffer → ratatui render
```

---

## 📄 License

MIT
