# Changelog

All notable changes to Radiumical will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [pre-v0.1.0b] - 2026-07-06

### Added

#### Tauri Desktop App
- Full TypeScript UI layer with Vite bundling, live HMR, and session-based chat interface
- Real-time streaming markdown rendering via `marked` + `highlight.js`
- Slash-command palette with categorized command discovery
- Session management UI: list, save, load, delete sessions
- Settings panel with model/provider/mode configuration
- Choice modal for tool-driven user interactions (e.g. confirm, pick options)
- Provider picker with automatic API key resolution from embedded registry
- Custom provider support (`ProviderKind::Custom`) with manual API base + key entry
- Welcome screen with hint chips and ASCII logo

#### Build & Release
- Multi-binary release packaging: TUI, RPC, and Tauri output organized into `tui/`, `rpc/`, `tauri/` subdirectories
- Platform-suffixed binary names in CI/CD (e.g. `radiumical-linux-x64`)
- `custom-protocol` feature for Tauri release builds
- `build.sh` and `build.ps1` convenience scripts for local development
- CLI command mode (`-c help|update|status`) with self-update scaffolding
- Registry-driven provider resolution replacing all hardcoded provider-to-model mappings

#### Registry & Providers
- Embedded `providers.jsonl` at compile time for offline provider key resolution
- `find_provider()` and `ProviderSource::default_model` for registry lookups
- `ProviderKind::Custom(name, api_type)` for arbitrary OpenAI-compatible endpoints
- Short-form RPC messages: `{proc, params}` alongside standard JSON-RPC

#### Session Management
- `SessionTools`, `SessionFilter`, `SortBy`, `SortOrder`, `SessionStats` for programmatic session queries
- Content-level filtering (name, model, mode, provider) with sorting and stats aggregation
- `auto_resume_last_task` flag to resume the previous session on TUI startup

#### Version Display
- Compile-time version info via `build.rs` (`git describe --tags --always --dirty`)
- `radiumical_core::version` module — format: `[hash/profile+arch]` (e.g. `[7a742e9c/release-small+x86_64]`)
- Web UI: version badge in toolbar top-right corner at 4px monospace
- TUI: version appended to status bar

### Fixed

#### Cancellation & Reliability
- Cancellation propagated throughout harness, tool loop, and sub-agents via watch channels
- Cancelled tool calls now fill in proper error results instead of burning the conversation
- `run_command` tool switched from sync `spawn_blocking` to `tokio::process::Command` with `kill_on_drop` — child processes terminate immediately on cancel/timeout
- Search tool wrapped in `spawn_blocking` with 512 KB file-size guard

#### Windows Subprocess UX
- Console windows no longer flash when Tauri spawns subprocesses (`cmd.exe`, `bash.exe`, `git.exe`, `cargo`, `node`, etc.)
- Added `process_util` module: `std_command()` / `tokio_command()` with `CREATE_NO_WINDOW` on Windows
- Sync `std::process::Command` replaced with async `tokio::process::Command` in checkpoint, systools, secure_env, and playwright tools

#### TUI Rendering
- Wrapped-line height measurement in `measure_blocks` fixing viewport math
- Multi-line errors split into separate output blocks for correct rendering
- ASCII art alignment and toolbar layout fixes
- Deprecated broken auto-load feature on startup

#### Lint Hygiene
- 8× `map_or(true, ...)` → `is_none_or(...)` in session filter code
- 2× redundant `&` on `config.provider.name()` in Tauri autosave

### Changed

#### Documentation
- README overhaul with comparison table (Electron vs Python vs Radiumical), value proposition, and version badge
- Tightened tagline: "No Electron, No Python, Pure Rust"
- Updated architecture, provider-flow, and building docs

#### Project Structure
- Tauri frontend moved to `radiumical-tauri/src-ui/` with TypeScript modules (`api.ts`, `commands.ts`, `state.ts`, `ui.ts`, `markdown.ts`)
- TUI `App` struct decomposed into grouped state types (`InputState`, `ThinkingState`, `ViewportState`, `OverlayState`)

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
