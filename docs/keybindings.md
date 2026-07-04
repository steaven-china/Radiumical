# Keybindings

Complete keyboard reference for the Radiumical TUI.

## Global (No Overlay Active)

### Navigation

| Key | Action |
|-----|--------|
| `PageUp` | Scroll up (hints or output) |
| `PageDown` | Scroll down (hints or output) |
| `Up` | Previous hint / input history (filtered by prefix) |
| `Down` | Next hint / input history |
| `Home` | Move cursor to start of input |
| `End` | Move cursor to end of input (scroll to bottom if input empty) |
| `Left` | Move cursor left |
| `Right` | Move cursor right |

### Editing

| Key | Action |
|-----|--------|
| `Enter` | Confirm / dispatch command / select hint |
| `Shift+Enter` | Insert newline in input |
| `Tab` | Autocomplete slash command / select first hint |
| `BackTab` (Shift+Tab) | Previous hint |
| `Ctrl+A` | Move cursor to start of input |
| `Ctrl+E` | Move cursor to end of input |
| `Ctrl+W` | Delete word before cursor |
| `Ctrl+U` | Delete from cursor to start |
| `Ctrl+V` | Paste from clipboard (text or image) |
| `Esc` | Close overlay / cancel thinking / clear hints |

### Actions

| Key | Action |
|-----|--------|
| `Ctrl+C` | Cancel if thinking → confirm exit if session has items → quit if empty |
| `Ctrl+L` | Clear output |
| `Ctrl+O` | Toggle full reasoning display |

### Mouse

| Action | Effect |
|--------|--------|
| Scroll wheel | Scroll output |
| Double-click tool call | Expand/collapse tool box |
| Click scrollbar | Drag to scroll |

## Choice Panel

Displayed when the agent presents options.

| Key | Action |
|-----|--------|
| `Up/Down` | Navigate options |
| `Enter` | Confirm selection |
| `Space` | Toggle (multi-select mode) |
| `1-9` | Quick select by number |
| `Esc` | Close |
| Any char | Input mode: append to filter buffer |

## Provider Picker (`/provider`)

| Key | Action |
|-----|--------|
| `Up/Down` | Navigate providers/models |
| `Tab` | Toggle focus between providers and models lists |
| `Enter` | Select provider (fetches models) or select model |
| `Esc` / `Ctrl+C` | Close |

## Settings Overlay (`/settings`)

| Key | Action |
|-----|--------|
| `Up/Down` | Navigate settings |
| `Left` / `-` | Decrease value |
| `Right` / `+` / `=` | Increase value |
| `Enter` | Begin editing value |
| `Esc` / `q` | Close and commit |

### Settings Edit Mode

| Key | Action |
|-----|--------|
| `Enter` | Commit edit |
| `Esc` | Cancel edit |
| `Left/Right` | Move cursor |
| `Backspace/Delete` | Delete character |
| `Home/End` | Jump to start/end |

## MCP Server List (`/mcp`)

| Key | Action |
|-----|--------|
| `Up/Down` | Navigate servers |
| `Enter` | Toggle server enabled/disabled |
| `Esc` | Close |

## Timeline (`/timeline`)

| Key | Action |
|-----|--------|
| `Up/Down` | Navigate checkpoints |
| `Enter` | Show diff for selected checkpoint |
| `r` | Rollback to selected checkpoint |
| `Esc` | Close |

## Session Manager (`/sessions`)

| Key | Action |
|-----|--------|
| `Up/Down` | Navigate sessions/workspaces |
| `Left/Right` | Switch focus (list ↔ actions) |
| `Tab` | Cycle focus: list → actions → name → desc → list |
| `BackTab` | Reverse cycle |
| `Enter` | Execute selected action |
| `Esc` | Close |
| Any char | Edit name/desc buffer (when in edit mode) |

## Dashboard (`//`)

| Key | Action |
|-----|--------|
| `Up/Down` | Navigate items within category |
| `Left/Right` | Switch category |
| `Enter` | Select item |
| `Esc` | Close dashboard |

## Overlay Priority

When multiple overlays are active, input is consumed by the topmost overlay. Overlays stack in this order:

1. Choice Panel (highest priority)
2. Provider Picker
3. Settings Overlay
4. MCP Server List
5. Timeline
6. Session Manager
7. Dashboard
8. Help (`/help`)
9. Base TUI (lowest priority)
