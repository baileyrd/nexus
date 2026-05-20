# com.nexus.storage

- **Path:** `crates/nexus-storage/`
- **Tier:** Core Rust
- **Bootstrap order:** 2 (registered after `security` so AI/editor/etc. can call into it during their own lifecycle hooks)

## Architecture

- Entry point: `crates/nexus-storage/src/core_plugin.rs` (`StorageCorePlugin`). Re-exported from `lib.rs`.
- Bootstrap wiring: `crates/nexus-bootstrap/src/plugins/storage.rs:19` — manifest is constructed from `IPC_HANDLERS` (no `plugin.toml` on disk); plugin owns a `StorageEngine` opened against the forge root.
- Lifecycle:
  - `on_init` — verifies `<forge>/.forge/` exists, opens the `StorageEngine` (which mounts the SQLite index, Tantivy index, knowledge graph, and code-symbol index).
  - `on_start` — spawns the `nexus-storage-bridge` thread (translates `Watcher` events to `com.nexus.storage.*` kernel events) and the `nexus-storage-git-commit` subscriber (BL-114: reconciles index on external HEAD changes).
  - `on_stop` — signals both threads and joins.
- Key modules: `forge.rs`, `atomic.rs` (atomic write fsync-rename), `parser.rs` (markdown), `index.rs`/`schema.rs` (SQLite via `rusqlite`+`r2d2`), `search.rs` (Tantivy), `find_replace.rs`, `watcher.rs` (`notify` + `notify-debouncer-mini`), `reconcile.rs`, `graph.rs`, `code_index.rs` (tree-sitter for Rust/TS/JS/Python/Go), `entity_index.rs`, `canvas.rs`, `bases/` (forge-native bases), `obsidian_base.rs`, `vectorstore.rs`, `import.rs` (BL-083), `mdx.rs`, `lfs.rs`, `config.rs` (re-exports from `nexus-formats`).
- Persistence (all under `<forge>/.forge/`):
  - `index.db` — SQLite entity index (files, blocks, tasks, links, tags, symbols, JSX, bases, canvas nodes/edges, vectorstore). Created by `StorageEngine`.
  - `search/` — Tantivy FTS segments.
  - `app.toml`, `workspace.json`, `ai.toml`, `mcp.toml` — read/written via `config_read` / `config_reset` handlers (storage is the I/O owner; types live in `nexus-formats`).
  - `.lock` — exclusive forge lock (`fs4`).
  - `<forge>/entities/*.md` — BL-128 entity index (atomic markdown writes).
- Settings owned: `StorageConfig` (`crates/nexus-storage/src/config.rs`). Mediates reads/writes of `AppConfig` / `WorkspaceState` / `AiConfig` / `McpConfig` (defined in `nexus-formats`). Documented at `docs/0.1.2/settings/forge-config.md` and `docs/0.1.2/settings/README.md`.
- Subscribes to: `com.nexus.git.commit` (BL-114) for incremental reconcile after external commits.
- External dependencies of note: `rusqlite` (bundled), `tantivy`, `notify` + `notify-debouncer-mini`, `comrak` (markdown), `petgraph`, `rayon`, `tree-sitter` + 5 grammars, `fs4` (locking), `sha2`.

## Surface

71 IPC commands declared in `core_plugin.rs:388` `IPC_HANDLERS`. Categories:

- File CRUD: `read_file`, `write_file`, `write_vault_file`, `delete_file`, `file_exists`, `list_dir`, `create_file`, `create_dir`, `rename_entry`, `delete_entry`, `note_append`, `read_frontmatter`, `write_frontmatter`.
- Query / index: `query_files`, `query_blocks`, `query_tags`, `query_tasks`, `toggle_task`, `query_symbol`, `graph_stats`, `rebuild_index`, `rebuild_search_index`.
- Search: `search`, `find_in_files`, `replace_in_files`.
- Graph: `backlinks`, `backlinks_to_block`, `outgoing_links`, `unresolved_links`, `graph_neighbors`, `list_all_links`.
- Vectorstore: `vector_insert`, `vector_query`, `vector_delete_by_file`, `vectorstore_count`.
- Config: `config_read`, `config_reset`, `settings_read`, `settings_write` (mutates `[settings]` in `app.toml`).
- Bases (forge-native): `base_index`, `base_list`, `base_query`, `base_load`, `base_create`, `base_record_*` (5), `base_property_*` (4), `base_view_*` (3); `obsidian_base_query` for Obsidian `.base` files.
- Canvas: `canvas_read`, `canvas_write`, `canvas_patch`, `canvas_nodes`, `canvas_edges`.
- Entities (BL-128/129): `entity_search`, `entity_get`, `entity_relations`, `entity_upsert`, `entity_find_duplicates`, `entity_merge`, `entity_decay_relations`, `list_draft_relations`.
- Import/export & misc: `import_forge`, `write_default_gitignore`.

Events emitted: `com.nexus.storage.*` (file create/modify/delete from the watcher bridge).

## Necessity

- **Verdict:** Essential.
- **Required for basic capabilities?** Yes. Storage owns file-as-truth, atomic writes, the SQLite index, FTS search, and the file watcher. Every one of the basic-capabilities flows — open forge, browse markdown tree, edit + save, run global search — terminates here. The editor, AI, MCP, agent, comments, etc. all call its handlers.
- **Depended on by:** `nexus-editor` (read/write through IPC), `nexus-bootstrap` orchestration, indirectly every frontend (CLI/TUI/MCP/shell). `nexus-editor`'s `core_plugin.rs:30` hard-codes `STORAGE_PLUGIN_ID = "com.nexus.storage"`.
- **Depends on:** `nexus-kernel`, `nexus-plugins`, `nexus-types`, `nexus-database` (for forge-native `bases/` query/view logic), `nexus-formats` (for markdown / canvas / config parsing).
- **What breaks if removed:** everything. No forge access, no browse, no edit, no search.

## Notes

- The ADR-0021 v1 alias pass (`with_v1_aliases`) doubles the visible command count — `IPC_HANDLERS` lists 71 commands, the manifest registers each under both bare and `.v1` names.
- Storage is the sole `rusqlite` and Tantivy owner per ARCHITECTURE §7 invariant. `nexus-database`'s SQL-related work lives here, not there.
- Watcher publishes events; index updates are explicit (`process_watcher_events`, `rebuild_index`, or the BL-114 git-commit reconcile thread).
- `write_vault_file` is the documented escape for shell-owned `.forge/` metadata that must skip FTS / KG / listeners (e.g. `workspace.json`).
