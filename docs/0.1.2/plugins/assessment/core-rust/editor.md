# com.nexus.editor

- **Path:** `crates/nexus-editor/`
- **Tier:** Core Rust
- **Bootstrap order:** 4 (after database)

## Architecture

- Entry point: `crates/nexus-editor/src/core_plugin.rs` (`EditorCorePlugin`). Re-exports in `lib.rs`.
- Bootstrap wiring: `crates/nexus-bootstrap/src/plugins/editor.rs:19` — manifest from `IPC_HANDLERS`, `LifecycleFlags { on_init: true, .. NONE }`. The plugin is constructed `with_event_bus`, then a `CrdtPublisher` (`crates/nexus-bootstrap/src/crdt_publisher.rs`) is wired as `OpObserver` (BL-074) and the pull-landing subscriber is started.
- Key modules:
  - `tree.rs` — `BlockTree` (the in-memory block model that's the authoritative session state).
  - `transaction.rs` — `Transaction` / `Operation` (insert_text, delete_text, insert_block, delete_block, reparent_block, update_block_content, update_annotations).
  - `undo_tree.rs` — `UndoTree` (full transactional undo/redo, including invertible ops).
  - `block.rs`, `annotation.rs`, `excerpt_map.rs` (multibuffer / BL-141 excerpts).
  - `markdown/` — markdown serializer (canonical output that the kernel writes back on save).
  - `database_view.rs` — translates `DatabaseViewConfig` to a `BaseView` for the BL-012 inline `[[{db:query}]]` executor.
  - `handlers/` — per-handler dispatch impls.
  - `ipc.rs` — wire types.
- Sessions: per-relpath `Session` map. Each session owns a `BlockTree` + `UndoTree`. Mutations are serialized `Transaction`s so the kernel-side tree is the authoritative copy across UI, CLI, AI, MCP.
- Calls out to: `com.nexus.storage` for `read_file` / `write_file` (constants `STORAGE_PLUGIN_ID` and `STORAGE_IPC_TIMEOUT = 30s` at `core_plugin.rs:30`,`:39`); `com.nexus.database` for `apply_view` (constant `DATABASE_PLUGIN_ID` at `core_plugin.rs:35`).
- Events emitted: `com.nexus.editor.changed.<relpath>` after every successful apply/undo/redo (prefix at `core_plugin.rs:109`).
- Observer hook: `OpObserver` trait (`core_plugin.rs:54`) — defined here to avoid a circular dep with `nexus-crdt`. The CRDT publisher implements it to mirror sessions into a `CrdtDoc`, publish per-op envelopes on `com.nexus.editor.ops.<relpath>`, and persist CRDT state on close.
- Persistence: none of its own. State flushes through `com.nexus.storage::write_file`. The CRDT publisher (wired at bootstrap) writes `<forge>/.forge/.editor/crdt/<sha>.json` on session close; that file is **not** owned by this plugin.
- Settings owned: none. (Editor-related settings like font size live in shell-side `nexus.editor.*` settings; cross-reference `docs/0.1.2/settings/plugin-manifests.md`.)
- External dependencies of note: `comrak`, `serde_yml`, `uuid`, `sha2`. No SQL, no network.

## Surface

15 IPC commands (from `core_plugin.rs:279` `IPC_HANDLERS`):

| Id | Command                 | Purpose                                                        |
|---:|-------------------------|----------------------------------------------------------------|
|  1 | `open`                  | Load a file into a session, return `EditorSnapshot`            |
|  2 | `close`                 | Drop the session (observer's last chance to flush)             |
|  3 | `get_tree`              | Return the current `EditorSnapshot`                            |
|  4 | `save`                  | Serialize the tree and write through storage                   |
|  5 | `apply_transaction`     | Apply a `Transaction`, returns `Slim` (text-only) or `Full`    |
|  6 | `undo`                  | Reverse the last transaction                                   |
|  7 | `redo`                  | Re-apply the last undone transaction                           |
|  8 | `list_open`             | List open session relpaths                                     |
|  9 | `sync_content`          | Reset a session's tree from a content string (no-undo resync)  |
| 10 | `get_markdown`          | Serialize the session to canonical markdown                    |
| 11 | `stamp_block`           | Promote a block to a stable id (ADR-0017)                      |
| 12 | `execute_database_view` | Resolve a `[[{db:query}]]` inline block (BL-012)               |
| 13 | `resolve_block_link`    | Resolve `[[file#^block-id]]` (BL-049)                          |
| 14 | `open_excerpts`         | Build a synthetic read-only multibuffer session (BL-141)       |
| 15 | `refresh_excerpts`      | Re-read all excerpt sources for an open multibuffer session    |

## Necessity

- **Verdict:** Essential.
- **Required for basic capabilities?** Yes. Editing markdown in the desktop shell goes through `open` → `apply_transaction` → `save`. The shell's CM6 transaction bridge is the primary client. Without this plugin, edits cannot be committed to the in-memory tree, undo/redo collapses, and the canonical-markdown round-trip on save disappears.
- **Depended on by:** the shell editor pane (CM6 bridge), `nexus-crdt` (Cargo dep — and implements `OpObserver` via the bootstrap-side `CrdtPublisher`), agent / AI features that read/sync content, MCP tools that mutate editor sessions.
- **Depends on:** `nexus-formats` (markdown parse/serialize), `nexus-kernel` (event bus, IPC), `nexus-plugins`, `nexus-types`. IPC-calls `com.nexus.storage` and `com.nexus.database`.
- **What breaks if removed:** in-shell markdown editing, undo/redo, the inline `[[{db:query}]]` widget, the multibuffer / excerpt UX, the CRDT mirroring chain (no `apply_transaction` ⇒ no `OpObserver` callback ⇒ no `com.nexus.editor.ops.*` events). The basic forge-open / browse / search / git path still works because those reach `com.nexus.storage` directly — but the user cannot edit a file in the shell.

## Notes

- Only `on_init` is enabled in `LifecycleFlags`; `on_start` / `on_stop` are off. The session map is built lazily on first `open`.
- `apply_transaction`'s response is a tagged union (`Slim { revision }` for text-only ops, `Full(EditorSnapshot)` for structural ops) per BL-123 — the optimisation matters for typing-hot loops; the snapshot serialize cost dominates the baseline (39 → 24190 µs across 10/100/5000-block docs).
- The crate manages the `OpObserver` indirection so `nexus-crdt` (which depends on `nexus-editor`) doesn't induce a cycle.
- `STORAGE_IPC_TIMEOUT = 30s` is generous for local file I/O but worth flagging as a candidate for promotion to `StorageConfig` (see `docs/0.1.2/settings/hardcoded-rust.md`).
