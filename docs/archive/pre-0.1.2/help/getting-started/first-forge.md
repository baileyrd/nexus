# Create your first forge

A **forge** is just a directory of markdown files plus a hidden
`.forge/` subdirectory that holds the index and configuration. You can
have many forges. You can move or rename a forge at any time. You can
also point Nexus at a folder of notes you already have — it will index
them in place without modifying the markdown.

## Initialize

```bash
nexus forge init ~/notes
```

This creates `~/notes/.forge/` with a fresh SQLite index, an empty
Tantivy search index, and default config files. Your existing `.md`
files (if any) are scanned and indexed; nothing is moved or rewritten.

## Tell Nexus which forge to use

Three ways, in order of priority:

1. **Per-command flag**: `nexus --forge-path ~/notes content list`
2. **Environment variable**: `export NEXUS_FORGE_PATH=~/notes`
3. **Default**: `~/.nexus/default`

For the desktop shell, the forge is chosen on first launch and remembered
in `~/.config/nexus-shell/last-forge.json` (or the platform equivalent).
You can switch forges from the **File** menu.

## Open it

Pick the frontend you want:

```bash
nexus desktop          # Tauri GUI (the main experience)
nexus tui              # terminal UI
nexus content list     # one-shot CLI
nexus mcp              # MCP server, for Claude Code / Cursor
```

All four read the same files and the same `.forge/` index. You can edit
in the shell, query from the CLI, and search from an MCP client all at
once.

## What lives in `.forge/`

```
~/notes/
├── .forge/
│   ├── index.db          # SQLite — file tree, blocks, links, tags
│   ├── search/           # Tantivy full-text index
│   ├── app.toml          # core settings
│   ├── ai.toml           # AI provider config + API keys
│   ├── mcp.toml          # registered MCP servers
│   ├── workspace.json    # open tabs, layout
│   ├── kv.sqlite3        # per-plugin storage
│   ├── chat/sessions/    # AI chat history
│   ├── skills/           # your prompt templates
│   ├── logs/             # operation + AI activity logs
│   └── temp/             # atomic write staging
└── ... your markdown ...
```

Everything in `.forge/` is **rebuildable** from your markdown. If it gets
corrupted, delete it and run `nexus forge reindex` — your notes are
untouched.

## Multiple forges

Forges are independent. A common layout:

- `~/notes/` — personal
- `~/work/` — work
- `~/projects/foo/docs/` — project-scoped, lives in the project repo

Switch with `--forge-path` or `NEXUS_FORGE_PATH`. The desktop shell can
also keep multiple windows open against different forges.
