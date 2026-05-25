# nexus-storage

> Kind: lib · IPC plugin id: com.nexus.storage · CorePlugin: yes · Has settings: StorageConfig · As of: 2026-05-25

## Overview

`nexus-storage` is the largest service crate (~23.4 k lines across `src/`) and the concrete owner of the **forge** — a user's directory of markdown files. It is the single crate that embodies invariant #1, *file-as-truth*: the on-disk markdown is the authoritative source of record, while the SQLite index at `<forge>/.forge/index.db` and the Tantivy full-text index at `<forge>/.forge/search/` are *derived* state, rebuildable at any time from the files. Code in this crate never treats the databases as the source of truth — `rebuild_index()` wipes the SQLite tables and FTS index and reconstructs them by walking the filesystem. The default `.forge/.gitignore` (shipped by `Forge::write_default_gitignore`) lists `index.db`, `search/` and the per-machine SQLite stores as excluded, reinforcing that they are throwaway.

The public façade is `StorageEngine` (`src/lib.rs`), constructed via `StorageEngine::init(root)` for a fresh forge or `StorageEngine::open(root, config)` for an existing one. The engine composes every subsystem: a `Forge` layout manager + advisory `flock(2)` lock, an r2d2 read-connection pool plus a single dedicated write `Connection` (guarded by `Mutex`), the `SearchIndex` (Tantivy), and an `Arc<RwLock<KnowledgeGraph>>` (petgraph). The engine is `Send + Sync` (it owns no non-`Sync` receiver after issue #80), so the core plugin holds it as `Arc<StorageEngine>` and dispatches IPC concurrently with no per-call lock.

The write path is the spine of the crate. `write_file(path, content)` does: (1) confine `path` to the forge root via `resolve_within` (issue #72 path-traversal fix), (2) `atomic_write` (temp → fsync → rename → parent-dir fsync), (3) decode UTF-8, (4) open a SQLite transaction on the write connection, delete any prior index rows for the path (files / fts_blocks / canvas / code_symbols), (5) branch by file type — `.canvas` parses canvas JSON, `.mdx` runs the MDX pre-processor, everything else runs the comrak markdown parser — insert the file + blocks + links + tags + frontmatter, then `tx.commit()`. **Only after the commit succeeds** is the in-memory knowledge graph mutated (add node, refresh outgoing links), so a mid-sequence DB failure can never leave the graph believing in state that does not exist on disk. For code-language files the tree-sitter symbol index (BL-114) is refreshed in its own inner transaction, with errors logged and swallowed so a broken parser never blocks a save.

Around the manual write path runs the **watcher → reconcile pipeline**. The `Watcher` (notify + notify-debouncer-mini) watches `notes/` and `attachments/`, debounces bursts, and emits `StorageEvent`s on a bounded channel. The `StorageCorePlugin` bridge thread translates those into `com.nexus.storage.*` kernel bus events (and `com.nexus.activity.appended` fan-out for the timeline) — but the bridge does **not** itself update the index; index updates remain the caller's responsibility via explicit `write_file` / `reconcile_index` / `rebuild_index` calls. A second subscriber thread watches `com.nexus.git.commit` and runs an incremental `reconcile_index` after an external pull / rebase / checkout so the FTS, graph, and code-symbol indexes catch up.

Search is BM25 over block content via Tantivy, with scoped operators (`tag:`, `path:`, `prop:`, `type:`) extracted by `search_scope` and post-filtered through SQLite. The knowledge graph answers backlinks, outgoing links, unresolved (broken) links, neighbours within N hops, and a full snapshot for the global graph view. On top of the core file/index machinery the crate also owns: Bases (structured `.bases` databases — full CRUD over records / properties / views), Obsidian read-only single-file `.base` queries, canvas files, a SQLite-backed vector store for AI embeddings, multi-file find/replace (BL-078), forge-to-forge import (BL-083), Git-LFS pointer smudging (BL-091), the tree-sitter code-symbol index (BL-114), a file-backed personal entity index (BL-128/129), and config-file read/write/reset handlers.

Storage is a **critical** core plugin: bootstrap registers it with `.or_critical("com.nexus.storage")`, so a lifecycle hang aborts boot rather than presenting an editor that silently saves into the void.

## Position in the dependency graph

- **Direct nexus-* dependencies:**
  - `nexus-kernel` — `EventBus`, `EventFilter`, `EventSubscription` for the watcher bridge and git-commit subscriber threads.
  - `nexus-plugins` — `CorePlugin` / `PluginError` traits, and the `define_dispatch_helpers!` macro (`exec_err`, `parse_args`, `to_value`, `string_arg`) used by every handler module.
  - `nexus-types` — `paths::resolve_within` (path confinement), the `bases::*` types + on-disk load/save helpers, and `activity::*` for the timeline fan-out.
  - `nexus-database` — present in `Cargo.toml` (bases schema-migration support).
  - `nexus-formats` — `sha256_hex`, the `config::*` types (AppConfig, AiConfig, McpConfig, WorkspaceState, …), and the `canvas::*` format types this crate re-exports and persists.
- **Notable external dependencies (+ why):**
  - `rusqlite` + `r2d2` + `r2d2_sqlite` — the index DB; a pool of read connections plus one dedicated write connection.
  - `tantivy` — on-disk BM25 full-text index over block content.
  - `tree-sitter` + `tree-sitter-{rust,typescript,python,go,javascript}` — BL-114 code-symbol extraction across the default code-extension set.
  - `comrak` — CommonMark/GFM markdown AST parsing.
  - `notify` + `notify-debouncer-mini` — filesystem watcher.
  - `petgraph` (`StableGraph`) — the in-memory knowledge graph.
  - `regex-lite` — BL-078 find/replace matcher (same engine as the editor's Find/Replace).
  - `fs4` — cross-platform advisory forge lock (`flock` / `LockFileEx`).
  - `rayon` — parallel index workers; `sha2` — import content hashing; `chrono` — date property comparisons; `serde_yml` / `toml` — frontmatter and config parsing; `libc` — low-level fs helpers.
  - Optional `ts-rs` + `schemars` behind the `ts-export` feature emit the TypeScript / JSON-Schema bindings for the WI-36 pilot IPC types in `src/ipc.rs`.
- **Crates that depend on this one:** `nexus-bootstrap` (constructs and registers the core plugin), `nexus-cli` (drives the engine directly during forge commands), `nexus-database` and `nexus-lsp`. Frontends (TUI, MCP, shell) reach storage exclusively through `context.ipc_call("com.nexus.storage", …)` rather than linking the crate.

## Public API surface

Module-by-module. `lib.rs` declares `#![deny(missing_docs)]` and re-exports the public types listed under each module.

### `lib.rs` — `StorageEngine` façade + `StorageConfig`
- `StorageConfig { pool_size: u32 = 4, debounce_ms: u64 = 300, rayon_threads: usize = 0 }`.
- `StorageEngine` — owns `Forge`, `ForgeLock`, the r2d2 pool, a `Mutex<Connection>` write connection, the `SearchIndex`, and `Arc<RwLock<KnowledgeGraph>>`.
  - Constructors: `init`, `open`; internal `open_internal` (acquire lock → clean temp → build pool → pragmas + migrate → open write conn → open SearchIndex → reconcile if existing → build/rebuild graph).
  - File ops: `write_file`, `write_raw` (bypasses index — `.forge/` metadata only), `read_file` (with LFS smudge), `delete_file`, `list_files`, `file_exists`.
  - Forge-tree ops (disk, not index): `list_dir` → `Vec<TreeEntry>`, `create_file`, `create_dir`, `rename_entry`, `delete_entry`.
  - Canvas: `read_canvas`, `write_canvas`, `patch_canvas`, `canvas_nodes`/`_edges`(`_by_path`).
  - Bases: `index_base`, `list_bases`, `base_create`, `base_record_create/update/delete/soft_delete/restore`, `base_property_create/update/rename/delete`, `base_view_create/update/delete`. (Property retype runs best-effort value coercion via `coerce_property_value`.)
  - Index queries: `query_files`, `query_symbols`, `query_blocks`(`_by_path`), `query_links`, `query_backlinks`, `query_tags`.
  - Rebuild/reconcile: `rebuild_index` (clears fts_blocks/code_symbols/files → reconcile → refresh FTS — the FTS refresh is part of the contract), `reconcile_index`.
  - Graph: `backlinks`, `backlinks_to_block`, `outgoing_links`, `unresolved_links`, `graph_stats`, `list_all_links`, `graph_neighbors`.
  - Tasks: `query_tasks`, `toggle_task` (DB + file write-back).
  - Search: `search` (scope-parse + Tantivy + SQLite post-filter), `rebuild_search_index`.
  - Obsidian base: `obsidian_base_query`.
  - Vector store: `vector_insert`, `vector_query`, `vector_delete_by_file`, `vectorstore_count`.
  - Import: `plan_import`, `apply_import`.
  - Accessors: `forge()`, `pool_connection()`.
  - Free helpers: `infer_file_type` (extension/dir-based attachment vs markdown/canvas/mdx classification, issue #84), `resolve_within`, `resolve_target`.

### `forge.rs` — `Forge` + `ForgeLock`
Directory-layout manager. Path accessors (`root`, `notes_dir`, `attachments_dir`, `forge_dir`, `temp_dir`, `index_db_path`, `search_dir`, `lock_path`, `forge_gitignore_path`); `init()` (creates `notes/ attachments/ .forge/ .forge/temp/ .forge/search/` + writes default gitignore, idempotent); `write_default_gitignore()` (BL-007, never overwrites an existing file, returns `true` on fresh write); `clean_temp()` (removes temp files older than 1 h); `acquire_lock()` → `ForgeLock` (RAII `flock` guard, returns `StorageError::LockHeld` on contention). `DEFAULT_FORGE_GITIGNORE` const documents the share/no-share policy (indexes + per-machine SQLite excluded; `.editor/crdt/*.json` intentionally tracked for the BL-007 git transport).

### `atomic.rs` — `atomic_write`
Temp-fsync-rename with parent-dir fsync for durability (issue #84). Up to 3 retries (100/400/1600 ms back-off) on *transient* `io::ErrorKind`s only (`Interrupted`, `WouldBlock`, `TimedOut`, `ConnectionReset`, `UnexpectedEof`); permanent failures bail immediately. Windows skips the parent-dir fsync (NTFS journals rename metadata).

### `schema.rs` — SQLite schema + migrations
`CURRENT_VERSION = 8`. `configure_pragmas` (WAL, `synchronous=NORMAL`, 16 MB cache, foreign keys ON); `migrate` (`_schema_version` tracking table, each migration in its own transaction). Tables: `files`, `blocks`, `links`, `tags`, `properties` (+ typed `value_num`/`value_date`/`value_bool` columns, mig 3), `fts_blocks` (FTS5 virtual), `tasks` (mig 2), `embeddings` (mig 4), `jsx_components` (mig 5), `canvas_nodes`/`canvas_edges`/`bases`/`bases_records`/`bases_views` (mig 6), `bases_schema_versions` (mig 7), `code_symbols` (mig 8, BL-114, self-referential `parent_id` with `ON DELETE CASCADE`).

### `parser.rs` — markdown parser
`parse_markdown(text) -> ParsedFile`. Public types `ParsedFile { content_hash, frontmatter, blocks, links, tags, tasks }`, `Property`, `ParsedBlock` (block_type, level, content, line range, block_ref_id `^anchor`, callout_type), `ParsedLink` (link_text, target_path, link_type wikilink/markdown/embed, fragment), `ParsedTag`. comrak AST walk extracts blocks; YAML frontmatter parsed separately.

### `index.rs` — index CRUD
Row types `FileRecord`, `FileMetadata`, `BlockRecord`, `LinkRecord`, `TagResult`, `JsxRecord`, `FileFilter` (`prefix` / `file_type` / `include_deleted`), `RebuildStats`. Functions `insert_file`, `query_files`, `query_blocks`, `query_links`, `query_backlinks`, `query_tags`, `delete_file`, `soft_delete_file`, `file_by_path`, `insert_jsx_components`, `query_jsx_components`.

### `search.rs` — Tantivy FTS
`SearchIndex` (fields: path/STORED, block_id/u64-STORED, block_type/STORED, content/TEXT|STORED, mtime/STORED|INDEXED-reserved). `open` (50 MB writer buffer), `open_in_memory` (tests), `add_block`, `commit`, `search` (TopDocs BM25), `clear`. `SearchResult { file_path, block_id, block_type, excerpt, score }`.

### `search_scope.rs` — scoped queries
`parse_scoped_query(input) -> (String, Vec<ScopeFilter>)` strips `tag:`/`path:`/`prop:`/`type:` prefixes. `ScopeFilter`, `PropertyOp`, `CmpOp`. `prop:` supports legacy substring (`prop:KEY:VALUE`) and typed comparisons (`prop:priority>3`, `prop:due<2026-01-01`) against the typed columns. `filter_results` post-filters Tantivy hits via SQLite.

### `watcher.rs` — file watcher
`Watcher::start(forge_root, debounce_ms)`, `events()`. `StorageEvent` enum (`FileCreated`/`FileModified`/`FileDeleted`/`FileRenamed`/`ReconcileRequested { dropped_events }`). Helpers `should_ignore` (component-based exclusion of `.git`/`.forge`/`node_modules`/`target` + `~`/`.swp`/`.DS_Store`), `relative_path`. Bounded channel (`WATCHER_CHANNEL_BOUND = 1024`); on overflow it drops per-file events and latches a single `ReconcileRequested`. Git batch-mode detection: while `.git/index.lock` exists, events are counted and dropped, and a `ReconcileRequested` with the dropped tally is emitted when the lock clears. Symlink and directory events are skipped (BL-082).

### `reconcile.rs` — filesystem ↔ index sync
`reconcile(conn, forge_root) -> ReconcileDelta`. Walks `notes/` + `attachments/`, compares content hashes against the index, inserts new / updates changed / soft-deletes removed files, re-paths code symbols on rename.

### `graph.rs` — knowledge graph
`KnowledgeGraph` (petgraph `StableGraph<NodeData, EdgeData>` + `path_to_node` map + `phantom_nodes` set). `new`, `rebuild_from_db`, `add_note`, `remove_note`, `add_link`, `remove_links_from`, `backlinks`, `backlinks_to_block` (filters by `^block-id`, case-insensitive), `outgoing_links`, `unresolved_links`, `neighbors` (BFS depth), `snapshot`, `stats`. Result types `BacklinkResult`, `OutgoingLink`, `UnresolvedLink`, `GraphSnapshot` (`GraphNodeEntry`/`GraphEdgeEntry` with phantom flags), `GraphStats`, `EdgeData`.

### `code_index.rs` — BL-114 tree-sitter symbol index
`CodeLanguage` (Rust/TypeScript/Tsx/JavaScript/Jsx/Python/Go), `DEFAULT_CODE_EXTENSIONS`, `detect_language(path)`. `extract_symbols(lang, source) -> Vec<ExtractedSymbol>` (per-language walkers `walk_rust` / `walk_js_ts` / `walk_python` / `walk_go`; captures kind, name, line range, `parent_idx`, doc comment). Persistence `upsert_file_symbols` / `upsert_file_symbols_in_tx` (DELETE-then-INSERT, parent-idx → row-id resolution), `delete_file_symbols`, `query_symbols`. `SymbolRecord`, `SymbolFilter { name?, path?, limit?=200 }`.

### `mdx.rs` — MDX pre-processing
`parse_mdx(content) -> MdxParseResult { parsed_file, components }`. Extracts JSX tags, replaces with placeholders, feeds cleaned markdown to comrak. `ParsedJsxComponent`.

### `canvas.rs` — canvas persistence
Re-exports `nexus_formats::canvas::{CanvasFile, CanvasNode, CanvasNodeType, CanvasEdge, CanvasEdgeType}`. `parse_canvas`, `serialize_canvas`, `apply_patch`, `extract_file_links`, SQLite ops `insert_canvas`/`query_canvas_nodes`/`query_canvas_edges`/`delete_canvas`. `CanvasNodeRecord`, `CanvasEdgeRecord`, `CanvasPatchOp`, `CanvasPatchError`.

### `bases/` — structured databases
`mod.rs`: `insert_base` (INSERT OR REPLACE + record wipe/reinsert), `query_bases`, `delete_base`, `BaseSummary`. `schema.rs`, `query.rs` (`QueryResult` for `base_query`), `relation.rs`.

### `obsidian_base.rs` — read-only `.base`
`query(conn, root, path) -> ObsidianBaseQueryResult` — parses an Obsidian single-file `.base`, evaluates its filter against indexed notes, projects configured properties. Read-only (ADR 0019).

### `vectorstore.rs` — embeddings
`ChunkEmbedding`, `ChunkMatch`; `upsert`, `delete_by_file`, `search` (cosine similarity, all vectors loaded into memory), `count`. AI plugin reaches these only via IPC so storage stays sole DB owner.

### `find_replace.rs` — BL-078
`find_in_files(root, args)`, `replace_in_files(engine, root, args)`. `FindInFilesArgs`, `ReplaceInFilesArgs`, `FileMatches`, `LineMatch`, `ReplaceReport`, `ReplaceError`. `DEFAULT_MAX_FILES=200`, `DEFAULT_MAX_RESULTS=1000`. Replace writes back through the normal write path so index + events stay consistent.

### `entity_index.rs` — BL-128/129 personal entities
File-backed (`<forge>/entities/*.md`, YAML frontmatter). `EntityIndex`, `EntityRecord`, `EntityRelation`, `ResolvedRelation`, `RelationDirection`; `ENTITY_TYPES`, `RELATION_TYPES`, `normalize_relation_type`; `render_entity_markdown`, `parse_entity`, `merge_records` (`MergedEntity`), `decay_file_content` (`DecayParams`/`DecayedFile`), `DraftRelationCandidate`, `DuplicateCandidate` (Jaccard similarity). `EntitySearchHit`.

### `import.rs` — BL-083
`plan_import`, `apply_import`. `ConflictStrategy` (Skip/Overwrite/Rename), `ImportConflict`, `ImportPlan`, `ImportOptions`, `ImportReport`.

### `lfs.rs` — BL-091
`is_pointer`, `parse_pointer` (`LfsPointer { oid, size }`), `smudge(cwd, bytes)` — runs `git lfs smudge`, degrades gracefully to pointer text. `POINTER_VERSION_LINE` const.

### `export.rs`
`export_to_html(content, title)` — markdown → standalone HTML document with embedded CSS.

### `config.rs`
Thin wrapper over `nexus_formats::config`. `load_/save_` for `AppConfig` (`app.toml`), `WorkspaceState` (`workspace.json`), `McpConfig` (`mcp.toml`), `AiConfig` (`ai.toml`). Re-exports the format structs.

### `ipc.rs` — WI-36 pilot contract types
Stable arg/return types for the pilot handlers (`search`, `read_file`, `write_file`, `list_dir`, plus note_append, read_frontmatter, query_symbol, and all entity handlers). Each derives the `ts-export` bindings under that feature. `frontmatter_from_source(content) -> ReadFrontmatterResult`.

## IPC handlers

All handlers are dispatched by `StorageCorePlugin::dispatch` keyed on the numeric `HANDLER_*` ids; the `(name, id)` pairs are the single source of truth in `IPC_HANDLERS` (consumed by the bootstrap manifest, registered under both `<cmd>` and `<cmd>.v1` per ADR 0021). 72 commands. There are **no per-command capability strings** declared — storage is a trusted core plugin (`trust_level = "core"`); the kernel gates *community* callers' `ipc.call` capability before dispatch reaches this crate (see Capabilities). Args/returns are JSON over `ipc_call`.

### File CRUD
| command | id | args | returns | description |
|---|---|---|---|---|
| `query_files` | 1 | `FileFilter` | `Vec<FileRecord>` | Query the files table by prefix/type/include_deleted |
| `read_file` | 2 | `{ path }` | `{ bytes: Vec<u8>\|null }` | Read bytes; LFS-smudge; `null` bytes when missing |
| `write_file` | 8 | `{ path, bytes }` | `FileMetadata` | Atomic write + full index/graph/FTS update |
| `note_append` | 54 | `{ path, snippet }` | `FileMetadata` | Append `\n\n{snippet}\n`; treats missing file as empty (BL-043) |
| `write_vault_file` | 33 | `{ path, bytes }` | `{}` | Atomic write, **no** indexing; confined to `.forge/` only |
| `delete_file` | 9 | `{ path }` | `{}` | Delete file + index + graph rows |
| `file_exists` | 10 | `{ path }` | `{ exists: bool }` | Index membership check |
| `read_frontmatter` | 59 | `{ path }` | `ReadFrontmatterResult { status, fields }` | Flat string-valued frontmatter; `{status:null,fields:{}}` for non-md/missing |
| `write_frontmatter` | 72 | `{ path, key, value: string\|null }` | `{ ok: true }` | Splice/remove a top-level scalar key; routes through write_file |
| `write_default_gitignore` | 60 | _none_ | `{ wrote: bool }` | BL-007 idempotent `.forge/.gitignore` bootstrap |

### Forge tree (on-disk, no index)
| command | id | args | returns | description |
|---|---|---|---|---|
| `list_dir` | 27 | `{ relpath }` | `Vec<TreeEntry>` | List files+dirs from disk; hides `.forge/`; missing dir → empty |
| `create_file` | 28 | `{ relpath }` | `{}` | Create empty file; refuses overwrite |
| `create_dir` | 29 | `{ relpath }` | `{}` | Create empty directory; refuses if exists |
| `rename_entry` | 30 | `{ from, to }` | `{}` | Rename/move; re-keys index row + graph node for indexed files |
| `delete_entry` | 31 | `{ relpath }` | `{}` | Delete file or directory (recursive); soft-deletes index rows under prefix |

### Search / symbols
| command | id | args | returns | description |
|---|---|---|---|---|
| `search` | 7 | `{ query, limit }` | `Vec<SearchResult>` | Scoped BM25 search + SQLite post-filter |
| `query_tags` | 16 | `{ name }` | `Vec<TagResult>` | Tags by name joined to file path |
| `query_blocks` | 21 | `{ path }` | `Vec<BlockRecord>` | All blocks for a file (by path) |
| `rebuild_index` | 6 | _none_ | `RebuildStats` | Full SQL rebuild + FTS refresh (coupled) |
| `rebuild_search_index` | 11 | _none_ | `{}` | Refresh Tantivy from current SQL state |
| `query_symbol` | 63 | `SymbolFilter` | `{ symbols: Vec<SymbolRecord> }` | BL-114 code-symbol query (read-only) |
| `find_in_files` | 57 | `FindInFilesArgs` | `Vec<FileMatches>` | BL-078 multi-file search |
| `replace_in_files` | 58 | `ReplaceInFilesArgs` | `ReplaceReport` | BL-078 multi-file replace (writes via write path) |

### Graph
| command | id | args | returns | description |
|---|---|---|---|---|
| `backlinks` | 3 | `{ path }` | `Vec<BacklinkResult>` | Files linking to `path` |
| `backlinks_to_block` | 55 | `{ path, block_id }` | `Vec<BacklinkResult>` | Backlinks filtered to `^block_id` (BL-049) |
| `outgoing_links` | 13 | `{ path }` | `Vec<OutgoingLink>` | Links from `path` |
| `unresolved_links` | 14 | _none_ | `Vec<UnresolvedLink>` | All broken links |
| `graph_neighbors` | 15 | `{ path, depth }` | `Vec<String>` | BFS neighbours within depth |
| `graph_stats` | 5 | _none_ | `GraphStats` | Node/edge/phantom counts |
| `list_all_links` | 34 | _none_ | `GraphSnapshot` | Every node + edge (global graph view) |

### Tasks
| command | id | args | returns | description |
|---|---|---|---|---|
| `query_tasks` | 4 | `TaskFilter` | `Vec<TaskRecord>` | Query checkbox tasks |
| `toggle_task` | 12 | `{ task_id }` | `TaskRecord` | Toggle completion in DB + source file |

### Vector store
| command | id | args | returns | description |
|---|---|---|---|---|
| `vector_insert` | 17 | `{ file_path, chunks }` | `{}` | Upsert embeddings (replaces file's rows) |
| `vector_query` | 18 | `{ embedding, limit }` | `Vec<ChunkMatch>` | Cosine-similarity search |
| `vector_delete_by_file` | 19 | `{ path }` | `{}` | Delete a file's embeddings |
| `vectorstore_count` | 20 | _none_ | `{ count }` | Total embedding rows |

### Canvas
| command | id | args | returns | description |
|---|---|---|---|---|
| `canvas_read` | 35 | `{ path }` | `CanvasFile` | Parse a `.canvas` from disk |
| `canvas_write` | 36 | `{ path, canvas }` | `FileMetadata` | Serialize + write through write_file |
| `canvas_patch` | 37 | `{ path, ops }` | `FileMetadata` | Apply `CanvasPatchOp[]` and rewrite |
| `canvas_nodes` | 38 | `{ path }` | `Vec<CanvasNodeRecord>` | Indexed nodes (empty if unindexed) |
| `canvas_edges` | 39 | `{ path }` | `Vec<CanvasEdgeRecord>` | Indexed edges (empty if unindexed) |

### Bases
| command | id | args | returns | description |
|---|---|---|---|---|
| `base_index` | 24 | `{ path }` | `{ base_id: i64 }` | Load `.bases` from disk + index |
| `base_list` | 25 | _none_ | `Vec<BaseSummary>` | All indexed bases |
| `base_query` | 26 | `{ path, filters, sorts, limit?, offset? }` | `QueryResult` | Query a base |
| `base_load` | 32 | `{ path }` | `Base` | Read-only full base parse from disk |
| `base_create` | 49 | `{ path, schema, seed_records? }` | `Base` | Create `.bases` dir; rejects existing |
| `base_record_create` | 40 | `{ path, record }` | `BaseRecord` | Append record (UUID if id empty); rejects dup id |
| `base_record_update` | 41 | `{ path, record_id, fields }` | `BaseRecord` | Shallow-merge fields |
| `base_record_delete` | 42 | `{ path, record_id }` | `{}` | Hard delete (idempotent) |
| `base_record_soft_delete` | 51 | `{ path, record_id }` | `{}` | Set `deleted_at` (record stays on disk) |
| `base_record_restore` | 52 | `{ path, record_id }` | `{}` | Clear `deleted_at` |
| `base_property_create` | 43 | `{ path, name, definition }` | `{}` | Add schema field; rejects dup |
| `base_property_update` | 44 | `{ path, name, definition, migrate_values? }` | `{}` | Replace field def; optional value coercion |
| `base_property_rename` | 50 | `{ path, old_name, new_name }` | `{}` | Rename schema column + record keys |
| `base_property_delete` | 45 | `{ path, name }` | `{}` | Remove field + drop key from records (idempotent) |
| `base_view_create` | 46 | `{ path, view }` | `{}` | Append view (keyed by name); rejects dup |
| `base_view_update` | 47 | `{ path, view }` | `{}` | Replace view by name |
| `base_view_delete` | 48 | `{ path, name }` | `{}` | Remove view (idempotent) |
| `obsidian_base_query` | 53 | `{ path }` | `ObsidianBaseQueryResult` | Read-only Obsidian `.base` evaluation (ADR 0019) |

### Config / settings
| command | id | args | returns | description |
|---|---|---|---|---|
| `config_read` | 22 | `{ kind: app\|workspace\|mcp\|ai }` | `{ format, content }` | Read a config file (TOML/JSON) |
| `config_reset` | 23 | `{ kind }` | `{}` | Write defaults |
| `settings_read` | 61 | _none_ | `{ "pluginId.field": value }` | `[settings]` table from `app.toml`; `{}` = defaults |
| `settings_write` | 62 | `{ key, value }` | `{}` | RMW one `[settings]` entry; `null` removes |

### Entities (BL-128/129)
| command | id | args | returns | description |
|---|---|---|---|---|
| `entity_search` | 64 | `EntitySearchArgs` | `EntitySearchResult` | Search `entities/*.md` (no SQLite) |
| `entity_get` | 65 | `EntityGetArgs` | `EntityGetResult` | Resolve by id/alias; `entity:null` if absent |
| `entity_relations` | 66 | `EntityRelationsArgs` | `EntityRelationsResult` | Relations (direction default `both`) |
| `entity_upsert` | 67 | `EntityUpsertArgs` | `EntityUpsertResult` | Atomic write `entities/<id>.md`; normalises relation kinds |
| `entity_find_duplicates` | 68 | `EntityFindDuplicatesArgs` | `EntityFindDuplicatesResult` | Jaccard similarity (default 0.92) |
| `entity_decay_relations` | 69 | `EntityDecayRelationsArgs` | `EntityDecayRelationsResult` | Multiply confidences by factor, clamp to floor |
| `entity_merge` | 70 | `EntityMergeArgs` | `EntityMergeResult` | Merge `drop` into `keep`; deletes `drop` |
| `list_draft_relations` | 71 | `ListDraftRelationsArgs` | `ListDraftRelationsResult` | Outgoing relations ≤ confidence threshold (default 0.5) |

### Import
| command | id | args | returns | description |
|---|---|---|---|---|
| `import_forge` | 56 | `{ source, dry_run, on_conflict }` | `ImportPlan` (dry-run) / `ImportReport` | BL-083 forge-to-forge import into this engine's forge |

> Handler id 67 is `entity_upsert`; ids are append-only and never reused (enforced by the `handler_ids_are_unique` test). Ids 69/70/71 appear out of numeric order in the source but are distinct.

## Capabilities

This crate declares no capability strings and performs no in-crate capability checks. As a `trust_level = "core"` plugin, `StorageEngine` runs with full host access; the capability system (invariant #4, ADR 0002) gates the *callers*: a community/WASM plugin must hold `ipc.call` before the kernel will dispatch into `com.nexus.storage`, and the kernel mediates `fs.read` / `fs.write` for plugins that touch the filesystem directly rather than routing through storage IPC.

What storage enforces instead is **filesystem confinement to the forge root**, the load-bearing safety boundary (issue #72). Every path-taking engine method runs the caller-supplied relative path through `resolve_within(root, relpath)` — a thin wrapper over `nexus_types::paths::resolve_within` that rejects any path component that is not `Component::Normal` (so absolute paths, `..` traversal, and root/prefix components all error with `StorageError::PermissionDenied`) and `join`s only normal components onto the root. `resolve_target` adds a non-empty-filename requirement for create paths. `atomic_write`, `read_file`, `write_raw`, `delete_file`, `list_dir`, `create_*`, `rename_entry`, and `delete_entry` all go through these. The `write_vault_file` handler additionally restricts writes to the `.forge/` namespace via `is_forge_metadata_path` so raw (unindexed) writes cannot diverge vault content from the index. The path-traversal regression is locked by `tests/path_traversal.rs`.

## Settings / Config

`StorageConfig` (in `lib.rs`):

| field | type | default | notes |
|---|---|---|---|
| `pool_size` | `u32` | `4` | r2d2 read-connection pool size (`r2d2::Pool::max_size`) |
| `debounce_ms` | `u64` | `300` | Watcher debounce window passed to `notify-debouncer-mini` |
| `rayon_threads` | `usize` | `0` | Rayon worker count (0 = auto-detect) |

`StorageConfig` is a plain struct with a hand-written `Default` (no serde derive); bootstrap constructs it with `StorageConfig::default()`. Two related tunables are hardcoded consts (flagged for promotion to `StorageConfig`): `WATCHER_CHANNEL_BOUND = 1024` (`watcher.rs`) and `DEFAULT_GIT_COMMIT_POLL_INTERVAL = 500 ms` (`core_plugin.rs`). The Tantivy writer buffer (50 MB) and FTS5/migration parameters are not configurable. The four user-facing config *files* (`app.toml`, `workspace.json`, `mcp.toml`, `ai.toml`) are owned by `nexus-formats` and merely surfaced through this crate's `config.rs` and the `config_*` / `settings_*` handlers.

## Events

The bridge thread publishes to the kernel event bus (`publish_plugin(PLUGIN_ID, type_id, payload)`); failures log at warn (audit gap A2). State-change topics:

- `com.nexus.storage.file_created` — `{ path, content_hash }`
- `com.nexus.storage.file_modified` — `{ path, content_hash }`
- `com.nexus.storage.file_deleted` — `{ path }`
- `com.nexus.storage.file_renamed` — `{ from, to, content_hash }`
- `com.nexus.storage.indexing.started` — `{}` (emitted around `ReconcileRequested`)
- `com.nexus.storage.indexing.completed` — `{ triggered_by, dropped_events }`

Each file event also fans out to the universal `com.nexus.activity.appended` topic (BL-052) as an `ActivityEntry` (surface `File`, origin `Storage`) so the timeline pane sees file writes. **Subscribed:** `com.nexus.git.commit` (`EventFilter::CustomExact`) — coalesces all pending commits per tick and runs one incremental `reconcile_index` so the index recovers after external git operations.

> The watcher bridge only *publishes* events; it never writes the index itself. `FileCreated` / `FileRenamed` variants exist in the enum but are not currently emitted (the debouncer collapses them into modify/delete shapes).

## Internals & notable implementation details

- **Atomic writes:** temp file in `.forge/temp/` → `sync_all` → `rename` → parent-dir `fsync` (Unix). Transient-only retry with exponential back-off; permanent errors bail immediately (issue #84). `.forge/temp/` is swept of >1 h-old files on every `open`.
- **SQLite topology:** one r2d2 pool of read connections + one dedicated write `Connection` behind a `Mutex`, both opened on the same `index.db` with WAL + `synchronous=NORMAL` + 16 MB cache + foreign keys. Writes go through transactions on the write connection; the pool serves concurrent reads. The write path deletes-then-inserts per file (fts_blocks, canvas, code_symbols, files), and the in-memory graph is only mutated post-commit. Migrations are linear v1→v8, each in its own transaction, tracked in `_schema_version`.
- **Tantivy:** schema = path/block_id/block_type/content(+TEXT) and a reserved indexed `mtime` field with no reader yet. `add_block` then `commit`. `rebuild_search_index` clears all docs and re-adds every non-deleted block joined from SQLite — `rebuild_index` always calls it so search never points at stale block ids.
- **Watcher debouncing & recovery:** `notify-debouncer-mini` with `debounce_ms`; bounded `sync_channel` (1024); overflow drops per-file events and latches `ReconcileRequested`; git-batch detection via `.git/index.lock` presence; symlinks/directories skipped (BL-082).
- **Knowledge graph:** petgraph `StableGraph`; real nodes plus phantom nodes for unresolved link targets (promoted to real when the file appears). Rebuilt from SQLite on `open` of an existing forge. Backlink/outgoing/neighbour/snapshot/stats queries are all O(graph) in-memory.
- **Code-symbol index (BL-114):** tree-sitter parse per supported language; per-language AST walkers capture symbols + parent chain + doc comments. Persisted to `code_symbols` (path-keyed, not FK'd to `files`, so it needs its own DELETE in the write/rebuild/delete paths). Parser failures are logged-and-swallowed so a broken file never blocks a save. Fully rebuildable from disk.
- **Rebuild path:** `rebuild_index` wipes `fts_blocks` + `code_symbols` + `files` (cascades blocks/links/tags/canvas), runs `reconcile`, counts rows for `RebuildStats`, then refreshes FTS — proving file-as-truth.
- **Concurrency (#80):** the engine no longer owns the watcher's non-`Sync` receiver, so it is `Send + Sync` and the plugin holds it as `Arc<StorageEngine>`; IPC dispatch is fully concurrent with no per-call lock (only the write connection serialises writes).

## Tests

In-module `#[cfg(test)]` blocks are extensive: `lib.rs` (engine init/read/write/delete, graph↔index consistency, code-symbol index, rebuild+FTS coupling, full bases CRUD/property/view roundtrips), `core_plugin.rs` (`handler_ids_are_unique`, `note_append` behaviours, frontmatter splice), `forge.rs` (layout, gitignore, temp cleanup, lock contention), `atomic.rs` (write/retry/transient-classifier matrix), `schema.rs` (pragmas + migration table creation), and the graph/parser/canvas/etc. modules.

Integration tests in `tests/`:
- `prd-03-smoke.rs` — public-API end-to-end smoke (10 tests).
- `prd-06-smoke.rs` — block refs, callouts, tasks, alias resolution, MDX (18).
- `prd-06-formats-smoke.rs` — canvas / config / bases (14).
- `prd-06-phase1-smoke.rs` — knowledge graph + backlinks (8).
- `issue_80_concurrent_dispatch.rs` — concurrent IPC dispatch regression (6).
- `path_traversal.rs` — issue #72 path-confinement regression (6).
- `lfs_pointer.rs` — BL-091 LFS graceful-degradation (2).
- `read_file_missing.rs` — `read_file` returns typed-null bytes rather than a crash for missing paths (2).
