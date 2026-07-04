# Configuration

Radiumical stores all configuration in `~/.radi/`. The directory structure:

```
~/.radi/
├── config.toml          # Global settings
├── mcp.json             # MCP server definitions
├── providers.jsonl      # Cached provider registry
├── agents/              # Custom agent roles (*.md)
├── skills/              # Installed skills
├── sessions/            # Workspace-scoped session storage
│   └── {workspace-hash}/
│       ├── *.jsonl.zst  # Conversation files
│       └── workspace.toml  # Workspace-level overrides
└── mem/                 # Memory storage
```

## config.toml

Global configuration file. All fields are optional — defaults are used when omitted.

```toml
# LLM provider (default: "deepseek")
provider = "deepseek"

# Model name (auto-detected from provider if omitted)
model = "deepseek-v4-pro"

# API key (prefer environment variables over config file)
api_key = "sk-..."

# Custom API base URL (for OpenAI-compatible endpoints)
api_base = "https://api.example.com/v1"

# Maximum context window in tokens (default: 1000000)
max_context_tokens = 1000000

# Trigger context compression at this ratio of max_context_tokens (default: 0.8)
context_compress_ratio = 0.8

# LLM request timeout in seconds (default: 120)
llm_timeout_secs = 180

# Tool execution timeout in seconds (default: 300)
tool_timeout_secs = 300

# Maximum tool-call loops per task (default: 32)
max_iterations = 50

# Heartbeat interval for thinking indicator in seconds (default: 3)
heartbeat_secs = 3

# Reasoning effort level: "low", "medium", "high", "max", "xhigh"
reasoning_effort = "medium"

# Agent mode: "auto", "plan", "exec"
mode = "auto"
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `DEEPSEEK_API_KEY` | Default API key for DeepSeek provider |
| `OPENAI_API_KEY` | API key for OpenAI provider |
| `ANTHROPIC_API_KEY` | API key for Anthropic provider |
| `GOOGLE_API_KEY` | API key for Google provider |
| `{PROVIDER}_API_KEY` | Pattern: uppercase provider name + `_API_KEY` |
| `RADI_DISABLE_LLM_CACHE` | Set to `1` to disable LLM response caching |
| `RADI_LOG` | Tracing filter (e.g. `debug`, `radiumical=trace`) |

## Workspace Overrides

Each workspace can override global settings via `~/.radi/sessions/{hash}/workspace.toml`:

```toml
# Same fields as config.toml — takes precedence over global
provider = "openai"
model = "gpt-4o"
```

## CLI Flags

All config fields can be overridden via CLI flags:

```
radiumical [OPTIONS]

Options:
  -t, --task <TEXT>           Non-interactive task mode
  -w, --workspace <DIR>       Workspace directory [default: .]
  -p, --provider <NAME>       LLM provider [default: deepseek]
  -m, --model <NAME>          Model name (auto-detected)
  -k, --api-key <KEY>         API key
      --api-base <URL>        Custom API base URL
      --max-iterations <N>    Max tool-call loops [default: 32]
      --concurrency <N>       Parallel tool executions [default: 8]
      --llm-timeout <SECS>    LLM timeout [default: 120]
      --tool-timeout <SECS>   Tool timeout [default: 300]
```

Priority order: CLI flags > workspace.toml > config.toml > defaults.
