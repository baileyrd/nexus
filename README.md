# Nexus

An AI-native, plugin-extensible knowledge environment built in Rust. Nexus combines file-based note management with full-text search, a knowledge graph, AI-powered RAG, and a WASM plugin system — accessible via CLI, terminal UI, or MCP server.

> **Status:** Alpha (v0.1.0) — Phase 1 foundation is solid, Phase 4-5 features (AI, MCP) are functional. Not yet production-ready.

## Architecture

Nexus follows a **microkernel** design. A small core (kernel + event bus) coordinates independent subsystems, each in its own crate:

```
nexus-kernel        Event bus, plugin lifecycle, capability enforcement
nexus-storage       File-as-truth, SQLite index, Tantivy FTS, file watcher, knowledge graph
nexus-security      OS keyring credential vault, audit logging, path validation
nexus-plugins       WASM sandbox (wasmtime), plugin manifests, hot-reload
nexus-ai            Provider traits (Claude, OpenAI, Ollama, llama.cpp), embeddings, RAG
nexus-mcp           MCP server over stdio — 13 tools for forge operations
nexus-cli           `nexus` binary — headless CLI with full subcommands
nexus-tui           `nexus-tui` binary — ratatui-based terminal interface
nexus-types         Shared type definitions
```

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

### MCP Server

Start the MCP server for use with Claude Code, Cursor, or any MCP client:

```bash
nexus mcp    # Serves 13 tools over stdio
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
  forge    init, status
  content  create, read, delete, search, tasks, task-toggle, links, backlinks, daily, export
  graph    status, unresolved, neighbors
  plugin   install, list, call, uninstall, scaffold
  watch    Monitor filesystem changes (glob patterns)
  logs     tail, show, path
  ai       ask, embed, status, config
  mcp      Start MCP server (stdio)
```

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

When running as an MCP server (`nexus mcp`), the following tools are exposed:

| Tool | Description |
|------|-------------|
| `forge_read` | Read file content |
| `forge_write` | Write/create file |
| `forge_search` | Full-text search |
| `note_create` | Create markdown note |
| `note_delete` | Delete note |
| `note_list` | List all notes |
| `graph_status` | Knowledge graph stats |
| `graph_unresolved` | Broken links |
| `task_list` | List tasks |
| `task_toggle` | Toggle task completion |
| `search` | Scoped FTS (tag:, path:, prop:) |
| `export_html` | Export note to HTML |
| `plugin_call` | Invoke a plugin command |

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

### Crate Dependency Graph

```
Phase 1: Kernel → Security → Storage → Plugins → CLI
Phase 2: File Formats → Theming/UI → Editor
Phase 3: Terminal, Database, Git (parallel)
Phase 4: AI Engine → Skills → MCP
Phase 5: Agents → Workflows
Phase 6: Cross-Platform (Tauri, WASM, Mobile)
```

Full design docs live in [`docs/PRDs/`](docs/PRDs/00-index.md) (17 implementation-ready PRDs) and [`docs/adr/`](docs/adr/) (10 architecture decision records).

## License

MIT OR Apache-2.0
