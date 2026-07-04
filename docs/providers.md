# Providers

Radiumical supports 21 LLM providers out of the box, with auto-detection of available models.

## Supported Providers

| Provider | API Type | Key Env Var | Notes |
|----------|----------|-------------|-------|
| `openai` | OpenAI Chat | `OPENAI_API_KEY` | GPT-4o, GPT-4, o1, o3, etc. |
| `anthropic` | Anthropic | `ANTHROPIC_API_KEY` | Claude 3.5/4 family |
| `google` | Google AI | `GOOGLE_API_KEY` | Gemini family |
| `deepseek` | OpenAI-compatible | `DEEPSEEK_API_KEY` | DeepSeek V3, R1, etc. (default) |
| `mistral` | OpenAI-compatible | `MISTRAL_API_KEY` | Mistral family |
| `groq` | OpenAI-compatible | `GROQ_API_KEY` | Fast inference |
| `cohere` | Cohere | `COHERE_API_KEY` | Command family |
| `ollama` | OpenAI-compatible | — | Local models, no API key needed |
| `openrouter` | OpenAI-compatible | `OPENROUTER_API_KEY` | Multi-model routing |
| `azure` | Azure OpenAI | `AZURE_API_KEY` | Enterprise deployments |
| + 11 more | — | — | Any OpenAI-compatible endpoint |

## Usage

```bash
# Default (DeepSeek)
radiumical

# Specify provider and model
radiumical -p openai -m gpt-4o
radiumical -p anthropic -m claude-sonnet-4-20250514
radiumical -p ollama -m codellama

# Custom OpenAI-compatible endpoint
radiumical -p custom --api-base https://api.example.com/v1 -m my-model -k sk-...
```

## Provider Registry

Radiumical maintains a remote provider registry at `radiumical.dev/providers.jsonl`:

1. **Remote fetch**: Downloads latest registry on first use (cached for 24h)
2. **Local cache**: Stored at `~/.radi/providers.jsonl`
3. **Embedded fallback**: Bundled providers always available, even offline

## Model Discovery

For providers with a `/models` endpoint, Radiumical auto-discovers available models:

```bash
# In TUI, use /provider to see available models
/provider
```

## Custom Providers

Any OpenAI-compatible API can be used as a provider:

```bash
# Via CLI
radiumical --api-base https://your-api.com/v1 -m model-name -k your-key

# Via config.toml
api_base = "https://your-api.com/v1"
model = "model-name"
api_key = "your-key"
```
