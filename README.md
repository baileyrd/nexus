# Nexus

A personal, plugin-extensible knowledge environment built in Rust. Nexus combines file-based note management with full-text search, a knowledge graph, AI-powered RAG, and a plugin system — accessible via CLI, terminal UI, desktop shell, or MCP server.

The plugin-first desktop shell at [`shell/`](shell/) + [`shell/src-tauri/`](shell/src-tauri/) (crate `nexus-shell`) is the single desktop target per [ADR 0011](docs/adr/0011-adopt-plugin-first-shell.md). The legacy tri-pane shell was removed in 2026-04 — see [`docs/architecture/legacy-shell-retirement.md`](docs/architecture/legacy-shell-retirement.md) for the migration story, or recover the code via the `v0.1.0-legacy-shell` git tag.

## Architecture

Nexus follows a **microkernel** design. A small core (kernel + event bus) coordinates independent subsystems, each in its own crate. The Cargo workspace has 24 members; the most load-bearing are:

```
nexus-kernel        Event bus, plugin lifecycle, capability enforcement, IPC dispatcher
nexus-storage       File-as-truth, SQLite index, Tantivy FTS, file watcher, knowledge graph
nexus-security      OS keyring credential vault, audit logging, path validation
nexus-plugins       WASM sandbox (wasmtime), plugin manifests, hot-reload
nexus-ai            Provider traits (Claude, OpenAI, Ollama, llama.cpp), embeddings (fastembed), RAG
nexus-mcp           MCP server library — 15 nexus_* tools for forge operations
nexus-cli           `nexus` binary — headless CLI with full subcommands (also hosts `nexus mcp`)
nexus-tui           `nexus-tui` binary — ratatui-based terminal interface
nexus-bootstrap     Runtime assembler (build_cli_runtime, build_tui_runtime, init_forge)
nexus-theme         Theming engine: CSS variables, theme packages, layout, snippet cascade
nexus-shell         Tauri 2 desktop shell at `shell/` — plugin-first, hosts `@nexus/extension-api`
nexus-types         Shared type definitions (leaf)
```

Service plugins (each a `CorePlugin` registered by `nexus-bootstrap`):
`nexus-agent`, `nexus-comments`, `nexus-database`, `nexus-editor`, `nexus-formats`,
`nexus-git`, `nexus-kv`, `nexus-linkpreview`, `nexus-panic-log`, `nexus-skills`,
`nexus-terminal`, `nexus-workflow`, plus `nexus-plugin-api` (SDK surface).
See [`Cargo.toml`](Cargo.toml) for the authoritative list.

The central concept is the **Forge** — a directory of markdown files that Nexus indexes, links, searches, and extends with AI. Files on disk are always the source of truth; the SQLite index is rebuildable.

## Quick Start

### Prerequisites

- Rust stable toolchain ([rustup.rs](https://rustup.rs))
- A directory of markdown files (or start fresh)

### Build

```bash
git clone <repo-url> && cd nexus
cargo build --release
```

Binaries land in `target/release/`:
- `nexus` — headless CLI
- `nexus-tui` — terminal UI

### Initialize a Forge

```bash
# Create a new forge (creates .forge/ metadata directory)
./target/release/nexus forge init ~/notes

# Or set the env var to avoid passing --forge-path every time
export NEXUS_FORGE_PATH=~/notes
```

### Create and Search Notes

```bash
nexus content create projects/nexus.md --content "# Nexus\nMy AI-native knowledge base."
nexus content search "knowledge"
nexus content daily                    # Create/open today's daily note
nexus content tasks                    # List all tasks ([ ] items) across files
nexus content backlinks projects/nexus.md
```

### Browse in the TUI

```bash
nexus-tui ~/notes
# or with env var set:
nexus-tui
```

### Knowledge Graph

```bash
nexus graph status                     # Node/edge counts, density
nexus graph unresolved                 # Broken [[wikilinks]]
nexus graph neighbors projects/nexus.md --depth 2
```

### Desktop (Tauri) shell

The plugin-first shell at [`shell/`](shell/) is the single active
desktop target. Needs Node.js + pnpm and the Linux webview libs
(`webkit2gtk-4.1`, `libsoup-3.0`) on top of the Rust toolchain.

```bash
cd shell
pnpm install
pnpm tauri:dev    # launches the Rust shell + Vite + webview
```

Every visible UI element is a plugin contribution loaded by
`ExtensionHost` from `shell/src/plugins/{core,nexus,community}/`. See
[`shell/README.md`](shell/README.md) and the stable contract at
[`packages/nexus-extension-api/`](packages/nexus-extension-api/).

### MCP Server

Start the MCP server for use with Claude Code, Cursor, or any MCP client:

```bash
nexus mcp    # Serves 15 nexus_* tools over stdio
```

## CLI Reference

```
nexus [OPTIONS] <COMMAND>

Global options:
  --forge-path <PATH>    Forge directory (or set NEXUS_FORGE_PATH)
  --format <FMT>         Output: text | json | jsonl | table  [default: text]
  -v...                  Verbosity: -v (info), -vv (debug), -vvv (trace)
  --no-color             Disable color output

Commands:
  forge      init, status
  content    create, read, delete, search, tasks, task-toggle, links,
             backlinks, daily, export
  graph      status, unresolved, neighbors
  tags       list, locate
  plugin     install, list, call, uninstall, scaffold, enable, disable,
             reset, settings
  skill      list, render
  bases      query, validate
  canvas     render
  agent      run, list, history
  workflow   run, list
  db         query, schema (forge index introspection)
  config     get, set, list
  git        status, log, blame, diff
  proc       list, kill (process manager via nexus-terminal)
  term       saved, run (saved-command snippets)
  watch      monitor filesystem changes (glob patterns)
  logs       tail, show, path
  ai         ask, embed, status, config
  mcp        Start MCP server (stdio)
  tui        Launch the terminal UI in the current terminal
  desktop    Launch the Tauri desktop shell (forwards args to nexus-shell)
```

For details on individual subcommands see [`docs/users/cli.md`](docs/users/cli.md)
or run `nexus <subcommand> --help`.

## TUI Key Bindings

| Key | Action |
|-----|--------|
| `j`/`k` or arrows | Navigate |
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

## MCP Tools

When running as an MCP server (`nexus mcp`), the following 15 tools are exposed
(authoritative source: `crates/nexus-mcp/src/server.rs`):

| Tool | Description |
|------|-------------|
| `nexus_read_note` | Read a note's content by vault-relative path |
| `nexus_create_note` | Create a new note with the given path and markdown content |
| `nexus_update_note` | Update an existing note's content (creates if it does not exist) |
| `nexus_delete_note` | Delete a note by vault-relative path |
| `nexus_list_notes` | List notes in the forge, optionally filtered by a path prefix |
| `nexus_search` | Full-text search across notes (rebuilds the search index before querying) |
| `nexus_backlinks` | Find all notes that link to the specified note |
| `nexus_outgoing_links` | Find all outgoing links from the specified note |
| `nexus_graph_status` | Knowledge graph statistics: node count, edge count, unresolved links |
| `nexus_list_tags` | List all occurrences of a tag by name across the forge |
| `nexus_list_tasks` | List tasks (checkboxes) across notes with optional completed/file filters |
| `nexus_toggle_task` | Toggle a task's completed/incomplete state by its database ID |
| `nexus_ask` | Ask a question via RAG over your notes |
| `nexus_list_skills` | List skills declared in the forge's `.forge/skills` directory |
| `nexus_render_skill` | Render a skill template to its expanded prompt body |

## Plugin System

Nexus supports two plugin tiers:

- **Core plugins** — native Rust, full access
- **Community plugins** — WASM-sandboxed via wasmtime, capability-gated

```bash
# Scaffold a new plugin
nexus plugin scaffold --type wasm --id my-plugin --name "My Plugin" --author "Me"

# Install and use
nexus plugin install ./my-plugin
nexus plugin call my-plugin some-command --args '{"key": "value"}'
```

## Configuration

### Forge Structure

```
~/notes/                  # Your files (source of truth)
├── .forge/
│   ├── index.db          # SQLite index (WAL mode, rebuildable)
│   ├── search/           # Tantivy FTS index
│   ├── config.toml       # Forge-level config
│   ├── logs/             # Operation logs
│   └── temp/             # Atomic write staging
├── projects/
│   └── nexus.md
├── daily/
│   └── 2026-04-13.md
└── ...
```

### Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `NEXUS_FORGE_PATH` | Forge root directory | `~/.nexus/default` |
| `RUST_LOG` | Tracing filter | `warn` |

## Development

```bash
cargo test --workspace          # Run all tests
cargo clippy --workspace        # Lint
cargo build -p nexus-cli        # Build just the CLI
cargo build -p nexus-tui        # Build just the TUI
```

### Key Docs

- [`docs/architecture/C4.md`](docs/architecture/C4.md) — current architecture overview
- [`docs/adr/`](docs/adr/) — architecture decision records
- [`docs/PRDs/`](docs/PRDs/00-index.md) — product requirements (see `IMPLEMENTATION_STATUS.md` for current state)
- [`docs/developer/getting-started.md`](docs/developer/getting-started.md) — plugin quickstart
- [`docs/shell/writing-a-plugin.md`](docs/shell/writing-a-plugin.md) — plugin author reference
- [`docs/archive/planning/`](docs/archive/planning/) — historical phase plans and audits

## License

MIT OR Apache-2.0
