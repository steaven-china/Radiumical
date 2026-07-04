# Layout DSL

Radiumical includes a built-in layout DSL for rendering structured content in the terminal. The agent uses the `layout_page` tool to create rich formatted output.

## Directives

### Grid

Grid layout with rows and columns.

```
grid 2x3
Cell A1|Cell A2|Cell A3
Cell B1|Cell B2|Cell B3
```

Produces a 2-row, 3-column grid. Cells are pipe-separated.

### Split

Horizontal split by percentage (must sum to 100).

```
split 60 40
Left panel content here
|||
Right panel content here
```

The `|||` separator divides the two panels.

### Rows

Vertical stack of content blocks.

```
rows
First row content
---
Second row content
---
Third row content
```

Blocks are separated by `---`.

### Columns

Side-by-side columns.

```
cols 2
Left column
|||
Right column
```

Optional column count after `cols` (default: 2). Columns are separated by `|||`.

### Box

Bordered box with optional title.

```
box My Title
Content goes here.
Multiple lines supported.
```

Produces:

```
┌─ My Title ──────────┐
│ Content goes here.   │
│ Multiple lines supported. │
└──────────────────────┘
```

### Table

Auto-width table with header row.

```
table
Name|Age|Role
Alice|30|Engineer
Bob|25|Designer
Charlie|35|Manager
```

Column widths are automatically calculated based on content.

## Unicode/CJK Support

The layout engine handles multi-byte characters correctly:

- CJK characters count as 2 columns width
- Fullwidth characters count as 2 columns width
- Regular ASCII characters count as 1 column width
- Text wrapping respects display width, not byte length

## Agent Tool

The agent creates layouts via the `layout_page` tool:

```json
{
  "input": "grid 2x2\nReads: 150\nWrites: 42\nErrors: 3\nUptime: 99.9%"
}
```

## Implementation Details

- `parse(input: &str) -> Result<Layout, String>` — parses DSL text into a Layout enum
- `render(layout: &Layout, width: usize) -> String` — renders layout to terminal-width string
- `char_width(ch: char) -> usize` — returns display width of a character
- `str_display_width(s: &str) -> usize` — total display width of a string
- `wrap_lines(text: &str, max_w: usize) -> Vec<String>` — wraps text respecting display width
- `truncate_to_width(s: &str, max_w: usize) -> String` — truncates with `…` indicator
