# The editor

The desktop shell uses a CodeMirror 6 editor with a custom Markdown
mode that does syntax highlighting, live preview, wikilink resolution,
block decoration, slash commands, and inline AI completion. The TUI
opens files in your `$EDITOR` instead.

## Modes

- **Source** — raw markdown, no rendering. `Ctrl+Shift+E` toggles.
- **Live preview** — markdown rendered inline as you type. Headings get
  larger fonts, lists indent, code blocks colour, tables render.
  Wikilinks become clickable. This is the default.
- **Read-only preview** — fully rendered, no editor affordances. For
  presentation or sharing screenshots.

## Tabs

Open as many notes as you want. Each tab can be:

- **Reordered** by drag-and-drop within a tab strip.
- **Moved** between split panes.
- **Popped out** into its own window — right-click → **Pop out**, or use
  the command palette (see [ADR 0020](../../adr/0020-popout-window-architecture.md)).
- **Pinned** so close-all leaves them open.

The view-header arrows (`◀ ▶`) move the active tab one slot in the strip
without using a mouse.

## Slash commands

Type `/` at the start of a line to open the slash menu. Built-in entries
include: heading levels, lists, task list, code block, callout, table,
horizontal rule, embed, divider. Plugins can contribute more.

## Block handles

Hover the left margin of any block (paragraph, heading, list item) and
a `⋮` handle appears. Drag to reorder, click for a menu (delete,
duplicate, copy block link, convert to…).

## Decorations

Live preview applies decorations as the Lezer parser catches up: a
fast first pass highlights syntax, a second pass renders embeds and
math. Editing in the middle of a partially-rendered region won't lose
your cursor — decorations recompute incrementally.

## Search and replace

`Ctrl+F` — find in current file. `Ctrl+H` — find and replace. Regex is
supported with the `.*` toggle. Workspace-wide search is in the
**Search** panel (`Ctrl+Shift+F`).

## Saving

Auto-save is on by default — changes flush to disk after a short idle.
`Ctrl+S` forces an immediate save. Files are written atomically via
`.forge/temp/` so a crash mid-write can't truncate a note.

## Undo/redo

Per-file undo history is preserved across saves and (optionally) across
sessions. `Ctrl+Z` / `Ctrl+Shift+Z`.

## Open externally

Right-click a tab → **Reveal in file manager** or **Open with…**. Useful
for editing in another tool while keeping Nexus open as the index.

## Annotations and comments

Highlight any range and `Ctrl+K Ctrl+C` to start a comment thread on it.
Threads attach to the underlying block ID, so they survive edits to the
surrounding text. See [Comments](comments.md).

## Inline AI

`Ctrl+Shift+Space` (or `Cmd+Shift+Space`) at the cursor streams an
inline completion. See [Inline completion](../ai/inline-completion.md).
