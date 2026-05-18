# Pick your frontend

Nexus is one engine with four faces. They all read and write the same
forge through the same kernel and IPC layer, so you can mix and match
freely — edit a note in the desktop shell, query it from the CLI, and
expose it to Claude Code over MCP at the same time.

## `nexus-shell` — Tauri desktop GUI

The full graphical experience. Editor, panels, graph, AI chat, terminal,
canvas, bases, plugin marketplace. The shell starts as an empty canvas
and every visible element is contributed by a plugin. This is the
recommended frontend if you're not sure.

```bash
nexus desktop          # launches the shell
```

See [Settings and customization](../customize/settings.md) for theming
and keybindings.

## `nexus` — CLI

Single binary, ~25 subcommands. Useful for scripting, CI, and
quick-fire queries from any shell. Output formats: text (default),
`json`, `jsonl`, `table`.

```bash
nexus content list --format json | jq '.[].path'
nexus content search "wikilinks" --limit 10
nexus ai ask "what changed this week?"
nexus graph neighbors README.md --depth 2
```

Full reference: [`docs/users/cli.md`](../../users/cli.md).

## `nexus-tui` — terminal UI

Keyboard-driven Ratatui app. File tree on the left, viewer on the right,
modal search, in-PTY terminal panel, task list, backlinks panel. Great
when you're already in a terminal and don't want to spawn a desktop
window.

```bash
nexus tui
```

Key bindings: [`docs/users/tui.md`](../../users/tui.md).

## `nexus-mcp` — MCP server

A [Model Context Protocol](https://modelcontextprotocol.io) server over
stdio. Lets Claude Code, Cursor, and any other MCP-aware client read,
search, and edit your forge as a set of tools.

```bash
nexus mcp              # starts the server on stdio
```

Tools exposed include `nexus_read_note`, `nexus_create_note`,
`nexus_search`, `nexus_backlinks`, `nexus_ask`, and ten more. Full list:
[MCP server](../advanced/mcp-server.md).

## Picking one

| You want to… | Use |
|---|---|
| Write notes day-to-day with a real editor | `nexus-shell` |
| Pipe note data into a script | `nexus` (CLI) |
| Triage notes from an SSH session | `nexus-tui` |
| Have Claude Code read and write your notes | `nexus-mcp` |
