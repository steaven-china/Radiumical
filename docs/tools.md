# Tools Reference

Radiumical ships with 25+ built-in tools. The agent selects tools automatically based on the task.

## File Operations

| Tool | Description |
|------|-------------|
| `read_file` | Read file contents with optional line range and pagination |
| `write_file` | Create or overwrite a file |
| `edit_file` | Apply targeted edits (search/replace) to an existing file |

## Code Search

| Tool | Description |
|------|-------------|
| `search_code` | Regex search across files with context lines |
| `find_files` | Glob-based file discovery with pattern matching |

## Shell & System

| Tool | Description |
|------|-------------|
| `run_command` | Execute shell commands (uses Git Bash on Windows) |
| `sysinfo` | System information (OS, CPU, memory, disk) |
| `list_dir` | List directory contents with metadata |
| `tree_dir` | Directory tree visualization |
| `time_now` | Current date/time in multiple formats |
| `cron_tab` | Display cron schedule information |
| `lsp_diagnostics` | Auto-detect language and run linter for diagnostics |

## Orchestration

| Tool | Description |
|------|-------------|
| `todo_list` | Task tracking with status management |
| `orchestrate` | Multi-step plan creation with dependency tracking |
| `goal` | Set and track high-level goals |
| `choice` | Present interactive choices to the user |
| `annotate` | Add annotations and notes to the conversation |

## Sub-agents

| Tool | Description |
|------|-------------|
| `sub_agent` | Spawn an async sub-agent for parallel work |
| `sub_agent_list` | List running sub-agents |
| `sub_agent_wait` | Wait for a sub-agent to complete |

## Memory

| Tool | Description |
|------|-------------|
| `memory` | Read/write persistent memory across sessions (`~/.radi/mem/`) |

## Skills & Agents

| Tool | Description |
|------|-------------|
| `list_skills` | Discover available skills |
| `load_skill` | Activate a skill by name |
| `list_agents` | List available agent roles |
| `load_agent` | Switch to a different agent role |

## Layout

| Tool | Description |
|------|-------------|
| `layout_page` | Render structured layouts (grid, table, split, cols, rows, box) |

## External

| Tool | Description |
|------|-------------|
| `playwright` | Headless browser automation (screenshot, content extraction) |
| `cluster` | Agent cluster management with worker slots |
| `settings` | Runtime configuration changes via TUI |
| `source_code` | Source code analysis via plugin API |
| MCP tools | Dynamically loaded from MCP servers (see [MCP](mcp.md)) |

## Tool Execution

- **Concurrency**: Up to 8 tools run in parallel by default (configurable via `--concurrency`)
- **Timeout**: 300 seconds per tool by default (configurable via `--tool-timeout`)
- **Output compression**: Tool outputs >1KB are lz4-compressed transparently
- **Error handling**: Failed tools return structured error results; the agent decides how to recover
