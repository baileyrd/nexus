# Using Nexus

End-user documentation. You've installed Nexus and pointed it at a forge
of markdown — now what?

> **Looking for task-oriented guides?** The [`../help/`](../help/README.md)
> tree is a full Obsidian-style help system: getting started, editing,
> linking, search, plugins, AI, advanced subsystems, customization. The
> page below is the dry reference (every command, every flag).

## Getting started

The repo-root [`README.md`](../../README.md) covers install, build, and
the high-level CLI / TUI / shell / MCP-server quickstart. Start there.

## Reference

| Read | What it covers |
|---|---|
| [`cli.md`](cli.md) | Full CLI command reference (every subcommand, every flag) |
| [`tui.md`](tui.md) | TUI key bindings and behaviour |
| [`mcp.md`](mcp.md) | MCP server: 15 `nexus_*` tools, Claude Code / Cursor setup |

## Configuration

Nexus reads settings from `<forge>/.forge/config.toml` and from a small
set of environment variables:

| Variable | Purpose | Default |
|---|---|---|
| `NEXUS_FORGE_PATH` | Forge root directory | `~/.nexus/default` |
| `RUST_LOG` | Tracing filter | `warn` |
| `NEXUS_NO_KEYRING` | Skip OS keyring (use plaintext config instead — see [ADR 0009](../adr/0009-keyring-hard-fail-policy.md)) | unset |
| `NEXUS_SHELL_BIN` | Override the `nexus desktop` shell-binary lookup path | unset |

## Forge layout

```
~/notes/                  # Your files (source of truth)
├── .forge/
│   ├── index.db          # SQLite index (WAL mode, rebuildable)
│   ├── search/           # Tantivy FTS index
│   ├── config.toml       # Forge-level config
│   ├── kv.sqlite3        # Plugin KV state
│   ├── logs/             # Operation + audit logs
│   └── temp/             # Atomic write staging
├── projects/
│   └── nexus.md
├── daily/
│   └── 2026-04-13.md
└── ...
```

Files on disk are always the source of truth. The `.forge/` index is
rebuildable — if you delete it, Nexus reconstructs from the markdown.
See [the file-as-truth invariant](../architecture/invariants.md#1-file-as-truth).
