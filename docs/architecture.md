# Architecture

Radiumical is a workspace of 6 Rust crates, designed as a lean CLI coding agent with a full-screen TUI.

## Crate Structure

```
radiumical-core/         Core library — agent runtime, providers, tools, persistence
radiumical-tui/          TUI frontend — ratatui full-screen interface
radiumical-tauri/        Tauri desktop app (experimental)
radiumical-rpc/          RPC server for external integrations
tools/csv-to-jsonl/      Provider registry conversion tool
tools/memory-bench/      Memory and compression benchmark tool
```

## Data Flow

```
User Input
    │
    ▼
┌─────────────┐  slash command?  ┌──────────────┐
│   TUI App   │ ──────────────── │ Command Exec │
│  (input.rs) │                  └──────────────┘
└──────┬──────┘
       │ plain task
       ▼
┌──────────────┐     ┌──────────────┐
│ BackendCmd   │────▶│ PipelineRunner│
│  (RunTask)   │     └──────┬───────┘
└──────────────┘            │
                            ▼
                   ┌─────────────────┐
                   │   Harness       │◀─── Orchestrator injects next task
                   │  (LLM↔Tool loop)│
                   └────────┬────────┘
                            │
              ┌─────────────┼─────────────┐
              ▼             ▼             ▼
        ┌──────────┐ ┌──────────┐ ┌──────────┐
        │ Provider │ │  Tools   │ │Conversation│
        │ (SSE)    │ │ (native/ │ │ (zstd JL) │
        └────┬─────┘ │  MCP)    │ └───────────┘
             │       └──────────┘
             ▼
        ProviderEvent
        (Text / Reasoning / ToolCalls / Done)
             │
             ▼
        UiEvent ──▶ TUI render (ratatui 60fps)
```

## Core Loop (Harness)

The harness implements the standard LLM agent loop:

1. **Build context** — system prompt + conversation history + task
2. **Call LLM** — streaming via SSE, tokens forwarded to TUI in real-time
3. **Process response** — text displayed, reasoning shown in collapsible block
4. **Execute tools** — tool calls parsed, executed in parallel (up to `concurrency`)
5. **Push results** — tool results appended to conversation
6. **Loop or done** — if more tool calls needed, go to step 2; otherwise save session

### Context Compression

When conversation exceeds `max_context_tokens * context_compress_ratio` (default 80% of 1M):

1. LLM summarizes the middle portion of conversation
2. Old messages replaced with a single summary system message
3. Recent messages preserved for continuity

### Auto-continue

When `auto_continue` is enabled (default), after the LLM finishes without tool calls, the harness automatically sends a "continue" prompt to keep the agent working.

## Agent Modes

| Mode | Tools Available | Use Case |
|------|----------------|----------|
| `Auto` | All 25+ tools | Default — full agent capability |
| `Plan` | `read_file`, `search_code`, `find_files` | Read-only exploration and planning |
| `Exec` | All tools | Implementation-focused |

## Provider Architecture

```
Provider (trait)
    │
    ├── OpenAICompatibleProvider   # OpenAI, DeepSeek, Ollama, Mistral, etc.
    │       └── SSE streaming, tool call accumulation, reasoning content
    │
    └── CachedProvider (wrapper)
            └── LRU cache (128 entries), keyed by (model, messages, tools, effort)
```

All providers implement the `Provider` trait:

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat(&self, messages: &[Message], tools: &[ToolDefinition], tx: Sender<ProviderEvent>) -> Result<()>;
    fn set_reasoning_effort(&self, effort: Option<String>) {}
    fn clone_box(&self) -> Box<dyn Provider>;
}
```

## Tool System

Tools implement the `Tool` trait:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, workspace: &Path, arguments: &str) -> ToolResult;
    async fn execute_with_context(&self, workspace: &Path, arguments: &str, ctx: &ToolContext) -> ToolResult;
}
```

Tools are registered in `all_tools()` and exposed to the LLM as JSON Schema definitions. MCP tools are dynamically added at runtime.

## Persistence Layer

| Component | Format | Location |
|-----------|--------|----------|
| Conversations | zstd-compressed JSONL | `~/.radi/sessions/{hash}/*.jsonl.zst` |
| Sessions | JSONL (SessionItem per line) | `~/.radi/sessions/{hash}/*.jsonl` |
| Config | TOML | `~/.radi/config.toml` |
| Workspace settings | TOML | `~/.radi/sessions/{hash}/workspace.toml` |
| Workspace registry | JSON | `~/.radi/workspaces.json` |
| Memory | JSON | `~/.radi/mem/{hash}/memory.json` |
| Checkpoints | JSONL + git commits | `~/.radi/sessions/{hash}/{session}/checkpoints.jsonl` |
| Provider cache | JSONL | `~/.radi/providers.jsonl` |
| Outline cache | JSON | `~/.radi/outline.json` |
| Secure env | Binary (XOR obfuscated) | `~/.radi/.env.bin` |
| MCP config | JSON | `~/.radi/mcp.json` |

## Compression Stack

```
Message Content > 1KB
    │
    ▼
lz4 compress → base64 encode → store with "\x00lz4:" prefix
    │
    ▼ (on read)
strip prefix → base64 decode → lz4 decompress → original text

Conversation File
    │
    ▼
Each message → JSON line → zstd compress (level 3) → .jsonl.zst file
```

## Async Architecture

- **Runtime**: Tokio multi-threaded
- **UI events**: `mpsc::unbounded` channel (Backend→UI)
- **Backend commands**: `mpsc::unbounded` channel (UI→Backend)
- **Conversation flush**: Background task drains pending messages every 500ms
- **Tool execution**: Parallel via `tokio::spawn` with concurrency limit
- **Cancel**: `tokio::select!` with `cancel_rx.changed()` for immediate interruption
- **Render loop**: 60 FPS fixed-interval with non-blocking event batch drain
