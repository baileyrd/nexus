# `nexus-tui` — Terminal UI

A ratatui-based interactive interface for browsing, searching, and editing a
forge from a terminal. Useful when you don't want the desktop shell — over
SSH, on a server, or just because you live in tmux.

## Launching

```sh
nexus-tui                       # uses $NEXUS_FORGE_PATH or ~/.nexus/default
nexus-tui ~/notes               # explicit forge root
NEXUS_FORGE_PATH=~/notes nexus-tui
```

Or via the unified CLI (functionally identical):

```sh
nexus tui
```

The `nexus tui` form calls `nexus_tui::run_tui()` as a library function —
no subprocess is spawned, so Ctrl+C behaves cleanly. See
[`cli.md`](cli.md#nexus-tui) for details.

## Key bindings

| Key | Action |
|---|---|
| `j` / `k` or arrows | Navigate |
| `Tab` | Toggle focus: tree / viewer |
| `Enter` / `l` | Open file or expand directory |
| `h` | Collapse directory |
| `b` | Toggle backlinks panel |
| `t` | Toggle task list view |
| `e` | Open in `$EDITOR` |
| `Ctrl+f` | Full-text search overlay |
| `/` | In-file find |
| `g` / `G` | Top / bottom |
| `Ctrl+d` / `Ctrl+u` | Page down / up |
| `q` / `Ctrl+c` | Quit |

## What it shows

The TUI reads the same SQLite + Tantivy index that the CLI and desktop
shell use. Index updates (made by another `nexus` process or by an
external editor) propagate via the file watcher in `nexus-storage` — no
restart needed.

## Limitations

- Read-mostly. The TUI surfaces backlinks, tasks, and search; it doesn't
  expose the editor engine, AI/RAG, or plugin commands. For those, use
  the CLI or the desktop shell.
- One forge per session. Switch forges by quitting and relaunching with
  a different `NEXUS_FORGE_PATH`.

## See also

- [`cli.md`](cli.md) — full CLI command reference.
- [`mcp.md`](mcp.md) — MCP server for AI clients.
- [`../README.md`](../README.md) — end-user docs hub.
