# Checkpoints & Time Machine

Radiumical automatically creates checkpoints before the agent modifies files, enabling rollback to any previous state.

## How It Works

Before each batch of write/edit tool calls, the harness creates a checkpoint:

1. **Git workspaces** — lightweight commit on a private session branch `radi/{session_id}`
2. **Non-git workspaces** — full file snapshot stored under `~/.radi/sessions/{hash}/{session_id}/snapshots/`
3. **Metadata** — checkpoint ID, message, timestamp, optional commit hash stored in `checkpoints.jsonl`

### Git Checkpoints

For git repositories, checkpoints are branch-isolated:

```
main:          A ── B ── C ── D
                        \
radi/session-1:          cp-1 ── cp-2 ── cp-3
```

The checkpoint commit is created, then HEAD is soft-reset back one step. This means:
- The working tree is not affected
- Checkpoints exist only on the session branch
- No interference with the main branch or other sessions

### File Snapshots

For non-git workspaces, a complete copy of modified files is stored:

```
~/.radi/sessions/{hash}/{session_id}/
└── snapshots/
    └── cp-1719950400/
        ├── src/main.rs
        └── src/lib.rs
```

## TUI: Timeline

`/timeline` opens the checkpoint timeline overlay:

```
┌─ Checkpoint Timeline ──────────────────────────┐
│ ▸ cp-1719950400  "Edit auth.rs"    2 min ago   │
│   cp-1719950300  "Write config"    5 min ago   │
│   cp-1719950200  "Read files"      8 min ago   │
│                                                  │
│ [Enter] Diff  [r] Rollback  [Esc] Close        │
└──────────────────────────────────────────────────┘
```

### Keyboard Controls

| Key | Action |
|-----|--------|
| `Up/Down` | Navigate checkpoints |
| `Enter` | Show diff for selected checkpoint |
| `r` | Rollback to selected checkpoint |
| `Esc` | Close timeline |

## Checkpoint Operations

### Create

Checkpoints are created automatically by the harness. The agent can also create manual checkpoints via the checkpoint tool.

### Diff

View the difference between a checkpoint and the current working tree:

- Git workspaces: `git diff {commit} HEAD`
- Non-git workspaces: file-by-file diff comparison

### Rollback

Restore the workspace to a checkpoint state:

- Git workspaces: `git reset --hard {commit}`
- Non-git workspaces: copy snapshot files back to workspace

## Configuration

Checkpoints are always enabled. No configuration required.

## Limitations

- Git checkpoints require the workspace to be a git repository
- File snapshots only capture files that were modified by the agent (not manual edits)
- Checkpoint branches (`radi/*`) are session-specific and can be safely deleted
- Rollback is destructive — current uncommitted changes are lost
