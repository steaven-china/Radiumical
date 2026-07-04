# Context Compression

Radiumical implements automatic context compression to handle long conversations within the LLM's context window.

## Overview

Large conversations are expensive in tokens and may exceed the model's context limit. Radiumical compresses context at two levels:

1. **Message-level lz4 compression** — transparent compression of individual large messages
2. **Conversation-level LLM summarization** — replaces old messages with a summary

## Message-Level Compression (lz4)

Individual message content exceeding 1 KB is automatically compressed:

### How It Works

```
Original text (2 KB)
    │
    ▼ lz4 compress
Compressed bytes
    │
    ▼ base64 encode
Encoded string
    │
    ▼ prepend "\x00lz4:" prefix
Stored in MessageContent
```

On read, the process reverses: strip prefix → base64 decode → lz4 decompress → original text.

### Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `COMPRESS_THRESHOLD` | 1024 bytes | Messages larger than this are compressed |
| `LZ4_PREFIX` | `"\x00lz4:"` | Magic prefix identifying compressed content |

### Transparency

Compression is fully transparent — the `text()` accessor on `MessageContent` always returns the decompressed string. Serialization preserves the compressed form for storage efficiency.

## Conversation-Level Compression (LLM Summary)

When the conversation grows too large, the LLM summarizes old messages.

### Trigger Conditions

Compression is triggered when the estimated token count exceeds:

```
threshold = max_context_tokens × context_compress_ratio
```

| Setting | Default | Description |
|---------|---------|-------------|
| `max_context_tokens` | 1,000,000 | Maximum context window size |
| `context_compress_ratio` | 0.8 | Compress at 80% of max |

### Compression Process

1. **Estimate tokens** — rough heuristic: 1 token ≈ 4 characters (including reasoning and tool arguments)
2. **Identify range** — keep system prompt + recent messages, compress the middle
3. **LLM summary** — ask the LLM to summarize the middle portion
4. **Replace** — old messages replaced with a single system message containing the summary
5. **Rewrite** — full conversation JSONL rewrite with compressed content

### Token Estimation

```rust
fn estimate_tokens(messages: &[Message]) -> usize {
    messages.iter().map(|m| {
        let content_len = m.content.len();
        let reasoning_len = m.reasoning_content.as_ref().map_or(0, |r| r.len());
        let tool_args_len = m.tool_calls.iter().map(|tc| tc.function.arguments.len()).sum::<usize>();
        (content_len + reasoning_len + tool_args_len) / 4
    }).sum()
}
```

## Conversation Truncation

As a fallback when compression isn't possible (e.g., LLM unavailable), the conversation can be truncated:

```
Keep: system prompt (index 0)
Keep: most recent messages (fitting within token budget)
Drop: oldest messages
```

`truncate_to_tokens(max_tokens)` preserves the system prompt and fills the budget with the most recent messages.

## Context Preview

For debugging or display purposes, `build_context_with_preview()` shows:

```
[System prompt]
[First few messages]
... 15 messages omitted ...
[Last few messages]
[Current task]
```

The preview respects a character limit and shows the head + tail with an omission indicator.

## Configuration

```toml
# ~/.radi/config.toml

# Maximum context window (tokens)
max_context_tokens = 1000000

# Compress when context exceeds this ratio (0.0 - 1.0)
context_compress_ratio = 0.8
```

Or per-workspace:

```bash
/session ws-set max_context_tokens 500000
/session ws-set context_compress_ratio 0.7
```

## Conversation Persistence

Conversations are stored as zstd-compressed JSONL:

- **Format**: each line is a JSON-serialized `Message`
- **Compression**: zstd level 3
- **Async flush**: background task drains pending messages every 500ms
- **Full rewrite**: triggered by compression or reset operations

| File | Description |
|------|-------------|
| `conversation.jsonl.zst` | Primary (zstd compressed) |
| `conversation.jsonl` | Fallback (plain JSONL, backward compat) |

## File Change Tracking

The conversation tracks which files the agent has read:

- `mark_file_seen(workspace, path)` — record that a file was read
- `changed_seen_files(workspace)` — detect if read files have been modified since

This enables the agent to warn about stale context when files change after being read.
