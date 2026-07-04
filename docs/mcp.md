# MCP Integration

Radiumical supports the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) for extending the agent with external tool servers.

## Configuration

Create `~/.radi/mcp.json`:

```json
{
  "mcpServers": {
    "fs": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_TOKEN": "ghp_..."
      }
    }
  }
}
```

### Server Config Fields

| Field | Type | Description |
|-------|------|-------------|
| `command` | `string` | Executable path or name |
| `args` | `string[]` | Command arguments |
| `env` | `object` | Environment variables for the server process |

## How It Works

1. **Startup**: Radiumical spawns each configured MCP server as an async child process
2. **Discovery**: Servers are queried via JSON-RPC for available tools
3. **Integration**: MCP tools appear alongside built-in tools — the agent uses them transparently
4. **Timeout**: All MCP operations have configurable timeouts (default: 30s)

## TUI Controls

MCP servers can be controlled from the TUI:

- Server status shown in the status bar with `[●]` (running) / `[○]` (stopped) indicators
- Start/stop individual servers via the MCP control panel

## Limitations

- Only `stdio` transport is supported (no SSE/HTTP yet)
- Server processes are managed per-session (restarted on new session)
- Tool schemas must be valid JSON Schema for the agent to use them

## Example: Filesystem Server

```json
{
  "mcpServers": {
    "fs": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/projects"]
    }
  }
}
```

## Example: SQLite Server

```json
{
  "mcpServers": {
    "sqlite": {
      "command": "uvx",
      "args": ["mcp-server-sqlite", "--db-path", "/tmp/data.db"]
    }
  }
}
```
