# nexus-editor

> Kind: lib · IPC plugin id: com.nexus.editor · CorePlugin: yes · Has settings: no · As of: 2026-05-25

## Overview

`nexus-editor` is the editor engine — the authoritative in-memory domain model behind every document a user edits. It implements PRD-08 §§1–3 and §5: the **block tree** (an ordered, indexed tree of typed blocks with parent/child links), **annotations** (non-destructive `(start, end)` inline-formatting overlays on block plain text), the **markdown ↔ block-tree roundtrip** (via `comrak` plus Nexus-specific syntax), and **transactions** (atomic, self-reversing groups of edit operations) with a **branching undo tree**. CodeMirror 6 integration (§4) and slash-command dispatch (§6) live in the shell, not here.

The crate's runtime surface is the `EditorCorePlugin` (`com.nexus.editor`), which owns one `Session` per open document keyed by forge-relative path. Each session holds the canonical `BlockTree` and `UndoTree`; every consumer — the Tauri shell, CLI, TUI, AI, MCP — mutates documents by sending serialized `Transaction`s through `context.ipc_call`, so there is a single source of edit truth no matter who is driving. Text-only edits (insert/delete text) take a fast O(1) "slim" response carrying just a revision counter; structural edits return a full tree snapshot. Disk I/O is never done directly: `open` and `save` route through `com.nexus.storage` on the async dispatch path so the editor stays inside the kernel's capability and atomic-write envelope (file-as-truth — the block tree is rebuildable from the markdown file, never the reverse).

Markdown is the persistence format. The parser builds a tree from comrak's AST and assigns each block a **deterministic id** (`SHA-256` over `file_path | visit_order | block-type`, masked into a valid v4 UUID) so re-parsing the same file produces stable ids. ADR-0017 cross-session stability layers on top: a block can be *stamped* with a fresh v4 id persisted as a trailing `<!-- ^<uuid> -->` HTML comment, surviving upstream insertions across reloads. `SHA-256` also tags persisted undo state (BL-072) so an external edit between close and re-open invalidates the cached history.

`nexus-crdt` builds collaborative editing directly on these types — it depends on `nexus-editor` and wraps `Operation` / `BlockId` / `BlockTree` in CRDT envelopes. To avoid a dependency cycle, this crate *defines* the `OpObserver` trait (session-open / -close / apply / undo / redo hooks) but does not implement it; `nexus-bootstrap`'s `CrdtPublisher` is the implementor, wired in at registration to mirror each session into a `CrdtDoc`, publish per-op envelopes, and persist CRDT snapshots on close.

## Position in the dependency graph

- **Direct nexus-\* deps:** `nexus-formats` (frontmatter/format helpers consumed by markdown parse), `nexus-kernel` (`EventBus`, `KernelPluginContext`, `Ipc` trait), `nexus-plugins` (`CorePlugin`, `PluginError`, dispatch-helper macro), `nexus-types` (`paths::resolve_within` path confinement; `bases::{BaseSchema, BaseView, FilterRule, SortRule, ViewType}` for the database-view executor).
- **Notable external deps:** `comrak` (markdown AST), `sha2` (deterministic block ids, undo-state content hashing), `chrono` (millisecond timestamps), `uuid` (`BlockId`), `serde`/`serde_json`/`serde_yml`, `thiserror`, `tracing`. Optional `ts-rs` + `schemars` behind the `ts-export` feature emit TS/JSON-Schema bindings for the `ipc.rs` wire types.
- **Crates depending on it:** `nexus-crdt` (`path = "../nexus-editor"`) wraps `Operation`/`BlockId`/`BlockTree`/`EditorError`. `nexus-bootstrap` registers the plugin and supplies the `OpObserver` impl.

## Public API surface

### `block` (re-exported)
- `Block` — one node: `id`, optional `stable_id` (ADR-0017), `ty: BlockType`, plain-text `content`, `annotations`, `properties`, `parent_id`, `children`, `index_in_parent`, `created_at`/`updated_at` (ms), `is_deleted` (reserved). Builders `with_content` / `with_annotations`; `id()` returns the effective (stamped-or-positional) id.
- `BlockId` — type alias for `uuid::Uuid`.
- `BlockType` — tagged enum of every block variant: `Paragraph`, `Heading{level}`, `BulletList`, `NumberedList`, `ToggleList`, `CodeBlock{language,line_numbers,repl}` (BL-142 REPL flag), `MathBlock`, `Callout`, `Quote`, `Divider`, `Table`/`TableRow`, `Embed`, `DatabaseView`, `Image`/`Video`/`Audio`/`File`, `Bookmark`, `SyncedBlock`, `ColumnLayout`/`Column`, `TableOfContents`, and `Excerpt{source_relpath,line_start,line_end,label}` (BL-141 multibuffer, synthetic only).
- `EmbedType`, `FileType` (Markdown/Mdx), `DatabaseViewConfig`, `DatabaseViewType` (Table/Kanban/Calendar/Gallery/Custom) — embed and database-view shapes.
- `BlockProperties` (`attributes` + read-only `computed`), `PropertyValue` (untagged String/Number/Boolean/List/Object, with bit-exact `f64` equality).
- `DocumentMetadata` — per-document `file_path`, `file_type`, timestamps, `word_count`, `read_time_seconds`, frontmatter `properties`.
- `now_ms()` — shared millisecond clock.

### `annotation` (re-exported)
- `Annotation` — `{start, end, ty}` byte range; `overlaps()` (touching ranges don't overlap), `is_empty()`.
- `AnnotationType` — `Bold`, `Italic`, `Strikethrough`, `Underline`, `Code`, `TextColor`, `HighlightColor`, `Link`, `Wikilink`, `Mention`, `MathInline`, `BlockRef`, `Custom`.
- `merge(Vec<Annotation>)` — coalesce adjacent/overlapping same-payload ranges (full equality required for payload-carrying kinds).
- `adjust_annotations(&mut [..], edit_start, edit_length)` — shift ranges through an insert (`+len`) or delete (`-len`) with documented boundary conventions.

### `tree`
- `BlockTree` — `blocks: HashMap<BlockId,Block>`, ordered `root_blocks`, `metadata`. Accessors `get`/`get_mut`/`is_empty`; navigation `parent`/`children`/`next_sibling`/`prev_sibling`/`descendants` (pre-order). Mutation `insert`/`remove` (leaf-only) / `reparent` / `rekey` (ADR-0017 id promotion). `validate()` checks every documented invariant.

### `transaction` (re-exported)
- `Operation` — reversible primitives: `InsertText`/`DeleteText` (carry `pre_annotations` + exact text for round-trip safety), `InsertBlock`/`DeleteBlock` (carry full block + original parent/index), `ReparentBlock`, `UpdateBlockContent`, `UpdateAnnotations`. Methods `apply`, `reverse`, and `inverse(&tree)` (authors a fresh inverse op against post-apply state, for CRDT undo).
- `Transaction` — `{id, operations, created_at, metadata}`; `apply`/`reverse` (no auto-rollback on partial failure), and `move_block(...)` constructor producing a single-op atomic reparent.
- `TransactionMetadata`, `UserAction` (`Keystroke`/`Paste`/`Delete`/`SlashCommand`/`BlockOperation`/`DragDrop`), `BlockOp`, `TransactionSource` (`User`/`Ai`/`Sync`/`System`).

### `undo_tree`
- `UndoTree` — branching history. `execute`/`undo`/`redo`/`goto` (cross-branch via lowest-common-ancestor), `children_of`. Serialized via `PersistedUndoTree`; `to_persisted(cap)` linearizes to the current branch tail when the BL-072 ring-buffer cap fires.

### `markdown` (re-exported)
- `MarkdownParser` (+ `ParseOptions{gfm_enabled,nexus_syntax_enabled,file_path}`), `MarkdownSerializer`.
- `deterministic_block_id`, `parse_stable_id_marker`, `format_stable_id_marker`, `strip_trailing_stable_id_marker` (ADR-0017 markers).

### `error`
- `EditorError` — `BlockNotFound`, `InvalidRange`, `InvalidTree`, `TransactionInvalid`, `UndoRedo`; `Result<T>` alias.

### `core_plugin` (re-exported)
- `EditorCorePlugin` (`new` / `with_event_bus` / `set_op_observer`), `EditorSnapshot`, `ApplyTransactionResponse` (`Slim{revision}` | `Full(EditorSnapshot)`), `OpObserver` trait, `EDITOR_PLUGIN_ID` / `PLUGIN_ID = "com.nexus.editor"`.

### `database_view` (pub module)
- Translates `DatabaseViewConfig` → `nexus_types::bases::BaseView` (`config_to_view`); `TranslateError` for malformed filter/sort strings.

### `ipc` (pub module)
- Wire-mirror arg/reply structs (`deny_unknown_fields`): `EditorPathArgs`, `EditorSyncContentArgs`, `EditorStampBlockArgs`/`EditorStampBlockReply`, `EditorApplyTransactionArgs`, `EditorResolveBlockLinkArgs`, `EditorOpenExcerptsArgs`/`EditorExcerptRequest`, `EditorOk`. Structural returns (snapshots) are treated as opaque on the wire to avoid pulling the whole domain model through the `deny_unknown_fields` schema gate.
- `excerpt_map` — `pub(crate)` BL-141 byte-offset translation between excerpt content and source text.

## IPC handlers

Handler ids are stable (`IPC_HANDLERS` is the single source of truth consumed by `nexus-bootstrap`). Returns are snapshots unless noted; `EditorSnapshot` = `{relpath, tree, undoPosition, undoLen, canUndo, canRedo, revision}` (camelCase).

| Command | Args | Returns | Capability | Description |
|---|---|---|---|---|
| `open` (1) | `{relpath}` | `EditorSnapshot` | — | Read file (async: via `com.nexus.storage`), parse to tree, store session. Restores persisted undo (BL-072) if content hash matches. `multibuffer://` relpaths return the existing synthetic session. |
| `close` (2) | `{relpath}` | `{}` | — | Drop session; async path persists undo state + fires observer `on_session_closed`. |
| `get_tree` (3) | `{relpath}` | `EditorSnapshot` | — | Fresh snapshot of the open session. |
| `save` (4) | `{relpath}` | `{}` | — | Serialize tree to canonical markdown and write (async: via storage; downstream `fs.write`). Synthetic sessions splice each Excerpt back into its source file (BL-141). |
| `apply_transaction` (5) | `{relpath, transaction}` | `ApplyTransactionResponse` (`slim`\|`full`) | — | Decode + size-cap (16 MiB structural payload) the transaction, execute through the undo tree, auto-stamp inbound link targets (BL-073), bump revision, notify observer, publish changed event. Text-only ops → `slim{revision}`. |
| `undo` (6) | `{relpath}` | `EditorSnapshot` | — | Reverse current transaction; notify observer `on_undo_transaction`; publish changed. |
| `redo` (7) | `{relpath}` | `EditorSnapshot` | — | Re-apply most-recent child branch; `on_redo_transaction`; publish changed. |
| `list_open` (8) | `{}` | `Vec<String>` (sorted) | — | Open session relpaths. |
| `sync_content` (9) | `{relpath, content}` | `EditorSnapshot` | — | Reparse `content` and replace the tree (undo untouched); creates a session if absent. Background resync for read-only consumers. Publishes changed. |
| `get_markdown` (10) | `{relpath}` | `String` | — | Canonical markdown serialization of the in-memory tree (what `save` would write). |
| `stamp_block` (11) | `{relpath, block_id}` | `{block_id, stable_id, newly_stamped}` | — | ADR-0017: rekey the block to a fresh v4 stable id so the next save emits a `<!-- ^uuid -->` marker. Idempotent. |
| `execute_database_view` (12) | `{database_path, view_config}` | `{applied, schema}` | — | **Async only.** Load `.bases` via storage, translate config → `BaseView`, hand to `com.nexus.database::apply_view`. Read-only; touches no session. |
| `resolve_block_link` (13) | `{file_relpath, block_id}` | `{found, block, root_index}` | — | BL-049 `[[file#^id]]` resolution; reads the open session if present, else parses from disk transiently. |
| `open_excerpts` (14) | `{items:[{relpath,line_start,line_end,label?}]}` | `EditorSnapshot` (synthetic `multibuffer://<uuid>`) | — | **Async only.** Build read-only synthetic session of `Excerpt` blocks; reads sources via storage; merges overlapping ranges; rejects empty `items`. |
| `refresh_excerpts` (15) | `{relpath}` | `EditorSnapshot` | — | **Async only.** Re-read every Excerpt's source slice; bump revision; publish changed. Synthetic-session only. |

Sync-dispatch `execute_database_view` / `open_excerpts` / `refresh_excerpts` return an error directing callers to the async path (they require storage/database IPC).

## Capabilities

The crate declares and checks **no capabilities of its own**. Its bootstrap manifest (`core_manifest_with_ipc_and_deps`) registers only `on_init`, the IPC handler list, and `MANIFEST_DEPS = ["com.nexus.storage", "com.nexus.database"]`. All capability enforcement is downstream: `open`/`save` route file I/O through `com.nexus.storage` (which checks `fs.read`/`fs.write`), and `execute_database_view` routes through `com.nexus.database`. The sync-only test fallback in `resolve_within` enforces forge-root path confinement (rejecting traversal/absolute paths) via `nexus_types::paths::resolve_within` + a canonicalize pass.

## Settings / Config

None. The crate has no `Config` struct, reads no `.forge/*.toml`, and exposes no settings. `ParseOptions` and `DatabaseViewConfig` are per-call/per-block runtime parameters, not persisted configuration. (The shell-side `nexus.editor.replKernels` knob referenced by the `CodeBlock.repl` flag is shell config, not owned here.)

## Events

- **Published:** `com.nexus.editor.changed.<relpath>` (`NexusEvent::Custom`) after every successful `apply_transaction` / `undo` / `redo` / `sync_content` / `refresh_excerpts`. Payload `{relpath, revision, transaction_id}` — `transaction_id` is the applied tx UUID for `apply_transaction`, `null` otherwise. Prefix constant: `EVENT_CHANGED_PREFIX`. Shell subscribers filter by `EventFilter::CustomPrefix` and use `revision` to dedupe echoes of their own dispatches. Only published when the plugin was built `with_event_bus` (unit-test drivers drop events).
- **Subscribed:** none by this crate. (The `com.nexus.editor.ops.<relpath>` per-op CRDT envelopes and the `com.nexus.git.commit` pull-landing subscription are published/consumed by `nexus-bootstrap`'s `CrdtPublisher`, the `OpObserver` implementor, not by `nexus-editor` itself.)

## Internals & notable implementation details

- **Block-tree structure.** `BlockTree` is a HashMap keyed by `BlockId` plus an ordered `root_blocks` vec; each block also records `parent_id`, `children`, and `index_in_parent`. Insert/remove/reparent keep `index_in_parent` consistent by re-indexing trailing siblings. `remove` is leaf-only (subtrees must be removed post-order); `reparent` rejects cycles (under-self / under-descendant) and adjusts the target index for same-parent forward moves (auto `-1` after detach — an asymmetry the transaction-reverse path explicitly compensates for in backward same-parent moves). `validate()` checks root parent/index consistency, child back-pointers, multi-parent and orphan detection.
- **Transactions & invariants.** Every `Operation` carries enough state to reverse itself without reading the tree (a deliberate deviation from the PRD sample): `DeleteText` stores the exact deleted text and prior annotations; `DeleteBlock` stores the full block plus original parent/index. `apply_delete_text` validates the stored `deleted_text` actually matches the slice at `pos` (else `TransactionInvalid`). `Transaction::apply` does not auto-rollback on partial failure — the caller owns recovery; `UndoTree::execute` leaves history untouched if apply fails.
- **Annotation model.** Annotations are byte-range overlays, not text. `adjust_annotations` is the single shift routine used by text ops: insert-at-start pushes the annotation right; insert-at-end is outside; a delete fully covering a range collapses it to `start == end` (a reversible op restores it verbatim from `pre_annotations`).
- **Markdown ↔ block conversion.** `parse` (comrak AST → tree) and `serialize` are split across private `parse`/`serialize`/`inline`/`id`/`database_view_spec` submodules. Nexus syntax includes wikilinks, inline tags, math, callouts (`> [!warning]`), block-ref anchors, and the native `[[{db:...?view=&group=&filter=&sort=}]]` database-view block (BL-012; malformed forms fall back to a paragraph rather than erroring). The roundtrip is parse→serialize→parse idempotent on structure, content, and annotations.
- **Content hashing for change detection.** Two `SHA-256` uses: (1) `deterministic_block_id` mixes `file_path | visit_order | type` into a v4-masked UUID so re-parse is stable; (2) BL-072 persisted undo stamps the canonical-markdown content hash into `.forge/.editor/undo/<sha-of-relpath>.json` (`PersistedUndoState{version, persisted_at_unix, content_hash, undo}`) and re-checks it on open — a mismatch (external edit) discards the cached history.
- **Stable ids (ADR-0017).** `BlockTree::rekey` promotes a positional id to a fresh v4, retargeting parent/child references and root slots and setting `stable_id` so the serializer emits a `<!-- ^uuid -->` marker. BL-073 auto-stamps any block that newly becomes the target of an inbound `Wikilink#^uuid` or `BlockRef` annotation (best-effort; failures are silent since the transaction already committed).
- **Undo/redo.** `UndoTree` is a branching forest (not a stack): executing after an undo creates a new branch rather than truncating; `goto` walks across branches through the LCA; `redo` picks the most-recently-added child. Transactions are `Arc`-shared so capturing the reversed/replayed tx for the observer is cheap.
- **Concurrency / locking.** `SessionMap = Mutex<HashMap<String, Arc<Mutex<Session>>>>` (BL-126): the outer lock is held only long enough to clone the per-relpath `Arc`, so dispatches against different files run concurrently while a single session's mutations stay serialized. `acquire_session_entry` / `get_session_entry` / `insert_session_entry` / `remove_session_entry` encapsulate the lock discipline (with an `Arc::try_unwrap` fast path on close).
- **Slim vs full responses (BL-123).** Text-only transactions return `ApplyTransactionResponse::Slim{revision}` (O(1)) since the webview already short-circuits snapshot reconcile for them; structural ops (including `UpdateAnnotations`, whose changes the optimistic mirror doesn't track) return a `Full` snapshot. Payload size is a structural sum over typed op fields (`transaction_payload_size`), not a throwaway JSON serialize — the BL-126 fix for typing-hot-path latency.
- **Synthetic multibuffers (BL-141).** `open_excerpts` assembles a read-only `multibuffer://<uuid>` session of `Excerpt` blocks. Phase 2 accepts `InsertText`/`DeleteText`/`UpdateBlockContent` on Excerpt blocks (structural ops rejected); `save` splices each excerpt's current content back into its source file's line range in reverse-line order, then reflows the stored ranges (`apply_reflow_after_save`). `excerpt_map` provides the pure byte-offset translation between excerpt content (`\n` separators) and source text (which may use `\r\n`).

## Tests

Unit tests are colocated per module; one integration/perf test in `tests/`.

- `src/block.rs` — block construction, builder composition, every representative `BlockType` variant, `PropertyValue` bit-exact equality, empty `DocumentMetadata`.
- `src/annotation.rs` — `overlaps` (disjoint/touching/partial/contained), `merge` (adjacent/overlapping/same-vs-different payload), `adjust_annotations` across every insert/delete boundary case including full-collapse.
- `src/tree.rs` — insert/remove re-indexing, navigation, descendants pre-order, duplicate/out-of-bounds/non-leaf/cycle errors, `validate` corruption detection, ADR-0017 `rekey` (root position, child retarget, collision, unknown, no-op).
- `src/transaction.rs` — apply/reverse round-trips for every op kind, compound transactions, same-parent forward/backward reparent regressions, `move_block` constructor (single op, autofill, missing-id, reorder metadata), mismatch/out-of-range/not-found errors.
- `src/undo_tree.rs` — linear undo/redo, branch-on-execute-after-undo, redo picks most-recent child, `goto` to root / cross-branch via LCA / out-of-bounds, persisted full round-trip and cap-truncation, execute-failure leaves history untouched.
- `src/markdown/mod.rs` — parse/serialize/parse idempotency across paragraphs, headings, lists (nested, task lists), code fences, dividers, quote-vs-callout, tables, embeds, inline formatting/links/wikilinks/math, frontmatter, deterministic-id stability, ADR-0017 stamp round-trips (paragraph/code/database-view), native `[[{db:}]]` parse/serialize and malformed fallback.
- `src/markdown/id.rs` — deterministic-id stability/uniqueness across slot/file/type; stamp-marker parse/format/strip including whitespace tolerance and bad-input rejection.
- `src/error.rs` — error `Display` formatting.
- `src/core_plugin.rs` — `EditorCorePlugin::dispatch` end-to-end: open (parse, path-escape reject, missing file, re-open replace), get_tree, save round-trip, apply_transaction undo-history + slim-response contract, undo/redo cycle, close drops session, list_open, get_markdown reflects in-memory state, BL-126 payload-cap rejection, BL-123 per-op-kind response shape.
- `tests/perf_apply_transaction.rs` — BL-122 typing-latency microbench over small/medium/large docs, gated behind `NEXUS_PERF=1`; emits `PERF_RESULT::` JSON lines for the perf harness (no-op under normal `cargo test`).
