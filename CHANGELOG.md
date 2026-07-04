# Changelog

All notable changes to Radiumical will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-pre.1] - 2026-07-04

### Added

#### Core Agent
- Dynamic LLM providers List: OpenAI, Anthropic, Google, DeepSeek, Mistral, Groq, Cohere, Ollama, OpenRouter, and any OpenAI-compatible API
- 20+ built-in tools: file read/write/edit, code search, shell execution, LSP diagnostics, directory tree, system info, memory, sub-agent, layout page, and more
- 3 agent modes: Auto (full access), Plan (read-only exploration), Exec (write & execute)
- Session-level orchestrator with multi-step plans, dependency tracking, and auto-continue
- Dynamic orchestrator with reactive DAG, guard conditions, and event bus
- Agent cluster abstraction with worker slots and event model
- Sub-agent system with async background workers

#### TUI
- Full-screen ratatui TUI with 60 FPS rendering
- Real-time markdown rendering with syntect code syntax highlighting
- Collapsible tool-call boxes with embedded scrollbar thumb
- Session manager (`/sessions`) with full-screen load/save/delete/new
- Provider picker (`/provider`) and settings overlay (`/settings`)
- Dashboard (`/`) with 2D keyboard navigation
- Mouse support: scroll wheel, double-click, scrollbar drag
- Status bar tips, contextual error hints, and keybinding display
- Perf monitor overlay (`/perf`) with live FPS, frame time, and render stats
- Toast notifications, progress board, confirm dialogs, and choice panels
- ANSI SGR, HTML color spans, and hex color support
- Interactive choice panel for tool-driven user selections

#### Performance
- lz4 transparent compression for messages >1KB
- zstd JSONL conversation persistence (5-100x disk savings)
- LLM response cache to avoid duplicate API calls
- Async conversation flush with dirty flag and background zstd write
- LRU render cache (max 512 entries)
- Outline memory cache with file limit
- Bounded channels (256) replacing unbounded channels
- Block-level render cache and visible-slice-only rendering

#### Extensibility
- MCP client with async stdio JSON-RPC transport and timeout
- Skills system following [agentskills.io](https://agentskills.io) spec
- Agent pool with custom agent roles loaded from `~/.radi/agents/`
- Layout DSL tool with grid/table/split/cols/rows/box layouts
- Remote provider registry with embedded fallback
- Source plugin API for tool implementations

#### Context Management
- Automatic context compression with LLM summarization when conversation exceeds 80% of max context tokens
- Workspace-scoped session isolation under `~/.radi/sessions/{hash}/`
- Workspace outline generation and context injection
- File change tracking with subsequent modification warnings
- Checkpoint time-machine and workspace registry

#### Configuration
- Config stored at `~/.radi/config.toml`
- MCP servers configured via `~/.radi/mcp.json`
- Runtime settings tool for inline configuration changes
- Device-bound secret store with TUI command surface

#### CI/CD
- GitHub Actions CI: lint (fmt + clippy), test, cross-platform build, bench, smoke test
- Release workflow with tag-triggered builds for Linux/macOS/Windows
- Pre-release support via `pre-v*` tag pattern
- Memory bench tool for compression and process memory analysis

### Fixed
- Carriage returns and ANSI escapes stripped from tool output
- Layout tool-box detection using `\x02` marker with cell clearing before redraw
- Windows command execution preferring Git Bash over cmd
- Silent error suppression replaced with structured tracing logs
- Diff view cleanup and tool result marker normalization
- Empty autosave guard and data loss prevention on new session
- Scroll clamping, ghost text prevention, and resize overflow handling
- Cancel mechanism using `tokio::select!` for immediate interruption
- Various TUI rendering edge cases: scrollbar, borders, padding, and overflow

### Changed
- Workspace reorganized into `radiumical-core`, `radiumical-tui`, `radiumical-tauri`, `radiumical-rpc` crates
- Monolithic modules decomposed into multi-file directory structures
- `&PathBuf` replaced with `&Path` for idiomatic Rust
- Idiomatic Rust patterns applied from clippy lints throughout workspace
- Rustdoc comments added across entire workspace
