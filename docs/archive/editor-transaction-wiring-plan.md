> **Archived 2026-04-26** — Phased build-out plan for routing editor transactions through the kernel. Phases shipped; the current architecture reference is [`editor-transaction-architecture.md`](../editor-transaction-architecture.md).

# Editor Transaction Wiring Plan

Replace the shell's `<textarea>` editor with CodeMirror 6 and route every edit
through `com.nexus.editor::apply_transaction` so the Rust `BlockTree` owns
authoritative document state. Ship undo/redo, outline/backlink freshness, and
future AI edits through the same transaction bus.

**Branch**: start from current trunk (`main`, post-leaf-migration).
**Reference**: `docs/leaf-migration-plan.md` for phase discipline,
`docs/PRDs/08-editor-engine.md` for the transaction model.

## Why

The Rust editor engine (`crates/nexus-editor/`) already implements the
authoritative block-tree model — `Transaction`, `Operation`, `BlockTree`,
`UndoTree` — and exposes it through the `com.nexus.editor` core plugin
(handlers `open`, `apply_transaction`, `undo`, `redo`, `save`,
`sync_content`, etc.; see `crates/nexus-editor/src/core_plugin.rs:46-68`
and the command-id wiring at `crates/nexus-bootstrap/src/lib.rs:431-459`).

None of this is reachable from the shell. `shell/src/plugins/nexus/editor/EditorView.tsx:537-547`
is a `<textarea>` that writes directly into `useEditorStore` via
`setContent`; persistence goes through `com.nexus.storage::write_file` in
`shell/src/plugins/nexus/editor/index.ts:261-285`. Consequences: no
transaction history (so no real undo/redo, no AI-edit audit trail), no
block-aware features (slash commands, outline staying in sync without a
re-parse), no path for sync/collab that isn't "diff the whole file".

This plan closes that gap without touching the existing leaf/view
plumbing. The `MarkdownView` wrapper stays thin; `EditorView` pivots
internally from textarea to CodeMirror 6 with a transaction bridge.

## Non-goals (v1)

- Multi-cursor presence visible across clients.
- Rich embeds (iframes, image editors, Mermaid live preview in-line).
- Block-level CRDT. Single-writer per doc in v1.
- Undo stack persistence across app reloads. Undo dies with the `Session`.
- Popout markdown windows (blocked on Tauri multi-window anyway).
- WYSIWYG / live preview rendering over CM. Source mode only in v1; the
  existing preview mode keeps its `marked`+DOMPurify path.
- Replacing `sync_content` — AI/MCP/outline keep their current resync path.

## Success criteria

- Typing in an editor tab dispatches `apply_transaction` calls. No
  `setContent(..., e.target.value)` textarea writes remain on the hot
  path.
- Undo (Ctrl-Z) / redo (Ctrl-Y / Ctrl-Shift-Z) route to the kernel's
  `undo` / `redo` handlers, not CM's local history.
- `com.nexus.storage::read_file` on a dirty doc still returns the latest
  saved content; `apply_transaction` persists through the shared
  `MarkdownSerializer → storage.write_file` path.
- Opening the same file in two tabs reflects edits from one in the other
  within one event loop tick (via the event subscription in Phase 4).
- Outline + backlinks update without a full re-parse: they subscribe to
  editor-change events instead of diffing `tab.content`.
- `tsc` clean, `cargo test -p nexus-editor -p nexus-bootstrap` green,
  manual smoke: edit, save, close, reopen — content survives; undo past
  save point works; close-without-save + reopen shows pre-edit content.

## Phases

Each phase is a standalone commit. Sized ~½–1 day each.

### Phase 0 — Dependency + scaffolding

Add CodeMirror 6 to `shell/package.json`. Chosen packages (pinned):
- `@codemirror/state` ^6.5.0
- `@codemirror/view` ^6.36.0
- `@codemirror/commands` ^6.8.0
- `@codemirror/language` ^6.10.0
- `@codemirror/lang-markdown` ^6.3.0
- `@codemirror/search` ^6.5.0

**Files:**
- `shell/package.json` — add deps.
- `shell/src/plugins/nexus/editor/cm/` — empty module dir with a
  placeholder `index.ts`.

**Exit:** `pnpm install` clean, `pnpm typecheck` clean, no runtime
import of CM yet.

### Phase 1 — Kernel client for `com.nexus.editor`

Typed client that hides the raw `kernel.invoke` shape from callers.

**Files:**
- `shell/src/plugins/nexus/editor/kernelClient.ts`:
  - `openSession(relpath): Promise<EditorSnapshot>`
  - `closeSession(relpath): Promise<void>`
  - `getTree(relpath): Promise<EditorSnapshot>`
  - `applyTransaction(relpath, transaction): Promise<EditorSnapshot>`
  - `undo(relpath): Promise<EditorSnapshot>`
  - `redo(relpath): Promise<EditorSnapshot>`
  - `saveSession(relpath): Promise<void>`
  - Uses `api.kernel.invoke('com.nexus.editor', '<cmd>', args)` per the
    surface in `shell/src/types/plugin.ts:241-268`.
- `shell/src/plugins/nexus/editor/types.ts`:
  - TS mirrors of Rust types (`BlockId = string`, `Annotation`,
    `Operation` tagged union, `Transaction`, `TransactionMetadata`,
    `UserAction`, `TransactionSource`, `EditorSnapshot`). Shapes follow
    `serde(rename_all = "snake_case", tag = "kind")` — see
    `crates/nexus-editor/src/transaction.rs:22`.

**Exit:** Unit test mocks `api.kernel.invoke`, round-trips an
`InsertText` transaction through the client. `tsc` clean.

### Phase 2 — CodeMirror mount + markdown rendering (no transactions yet)

Swap the `<textarea>` for a CM `EditorView`, still driven by
`useEditorStore.content`. No kernel calls.

**Files:**
- `shell/src/plugins/nexus/editor/cm/CodeMirrorHost.tsx` — imperative
  React wrapper. Constructs `EditorView` in `useEffect`, destroys in
  cleanup. Props: `value`, `onChange(newValue)`, `readOnly`.
- `shell/src/plugins/nexus/editor/cm/extensions.ts` — baseline:
  `markdown()`, `history()` (temporary; ripped out Phase 5),
  `keymap.of([...defaultKeymap, ...historyKeymap])`, `lineNumbers()`
  behind a setting, `EditorView.lineWrapping`.
- `shell/src/plugins/nexus/editor/EditorView.tsx` — replace lines
  535-547 (`<textarea>`) with `<CodeMirrorHost value={tab.content}
  onChange={(v) => store.setContent(tab.relpath, v)} />`. Keep the
  scroll-to-heading logic; rewire `sourceRef` to `cmViewRef` + a
  `viewToLine` helper using CM's line API.

**Exit:** Source mode renders in CM; typing still mutates
`useEditorStore`; preview toggle works; outline scroll-to-heading still
jumps correctly. Manual smoke only.

### Phase 3 — Session lifecycle (open / close)

Open a kernel session when a markdown tab mounts; close on tab close.
Still no transactions, but the server now has a tree for every open doc.

**Files:**
- `shell/src/plugins/nexus/editor/sessionManager.ts` — refcount per
  `relpath`. `acquire(relpath)` calls `openSession`, returns the
  snapshot. `release(relpath)` calls `closeSession` when the count hits
  zero. Keeps a `Map<relpath, EditorSnapshot>` as hydration cache.
- `shell/src/plugins/nexus/editor/MarkdownView.tsx` — on `onOpen`,
  extract `relpath` from `state`, call `acquire`. On `onClose`, call
  `release`. Replaces the current placeholder `state` handling
  (`shell/src/plugins/nexus/editor/MarkdownView.tsx:44-62`).
- `shell/src/plugins/nexus/editor/index.ts` — when a file is opened via
  `files:open`, after `openTab`, await `acquire(relpath)` and
  **replace** the `storage::read_file` content-load path with
  `snapshot.tree → MarkdownSerializer`-equivalent. Two options — plan
  picks (b):
  - (a) call `storage::read_file` in parallel and trust it matches.
  - (b) add a new handler `get_markdown(relpath) → String` on
    `com.nexus.editor` that runs `MarkdownSerializer::serialize` on the
    session and returns it, so the shell gets the exact canonical form
    the kernel will write back on save.

**Rust-side addition (b):** `crates/nexus-editor/src/core_plugin.rs` —
add `HANDLER_GET_MARKDOWN = 10`, handler that locks the session map and
returns `MarkdownSerializer::serialize(&s.tree)`. Register in
`crates/nexus-bootstrap/src/lib.rs:431-459`.

**Exit:** Opening `notes/a.md` produces a `Session` on the Rust side
(`cargo test` on `bootstrap::editor_ipc` covers this). Closing the tab
removes it. Unit test: `sessionManager` refcount works under open-twice
/ close-once.

### Phase 4 — Event bus: server → client push

Let other panes (second editor tab, outline, backlinks) observe edits
without polling.

**Rust side:**
- `crates/nexus-editor/src/core_plugin.rs` — after every successful
  `apply_transaction` / `undo` / `redo` / `sync_content`, emit a
  `com.nexus.editor.changed.<relpath>` custom event carrying `{
  relpath, revision, transaction_id }`. Use the same mechanism
  `com.nexus.theme` uses (see bootstrap around line 478-500). A
  monotonic per-session `revision: u64` (incremented on each mutation)
  must also be added to `Session` and returned in `EditorSnapshot` —
  this is what the shell uses for echo suppression.

**Shell side:**
- `shell/src/plugins/nexus/editor/sessionManager.ts` — on `acquire`,
  `api.kernel.on('com.nexus.editor.changed.', handler)`. Handler
  dispatches into a local Zustand slice keyed by relpath.
- `shell/src/plugins/nexus/editor/editorStore.ts` — add
  `sessionRevision: Map<relpath, number>` and
  `pendingLocalRevisions: Set<transactionId>` for echo suppression
  (see Phase 5).

**Exit:** Unit + integration: fire a `sync_content` on the Rust side,
assert the shell's subscriber saw the `changed` event. Two editor tabs
on the same file: type in one (via the Phase-5 path, so merge this test
into Phase 5 if ordering preferred).

### Phase 5 — Transaction bridge

The load-bearing phase. CM changes become `InsertText` / `DeleteText`
ops; dispatch goes to the kernel; the kernel's authoritative snapshot
is reconciled back into CM.

**Model:**
- **DocId = vault-relative path** (`relpath`). Matches every other
  kernel handler's argument. Renames become: close session at old
  path, open session at new path, carry CM state across via a
  helper. Not a UUID — the storage layer and all existing consumers
  key on relpaths, and `files:open` emits relpaths. See resolved
  decision #1.
- **Block identity in a flat-markdown world**: v1 treats the whole doc
  as a single implicit paragraph block from CM's perspective. The CM
  change-to-op mapping emits `UpdateBlockContent` on the root block of
  the tree when a change spans a block boundary; `InsertText` /
  `DeleteText` on the containing block otherwise. Precise block-level
  mapping (so CM position ↔ `BlockId + offset`) is a follow-up; v1
  punts by calling `sync_content` on every dispatch if the change
  can't be mapped cleanly. See resolved decision #4.
- **Concurrency**: single-writer (the tab that originated the edit) +
  single-reader (the kernel). Echo suppression: the shell tags every
  outgoing `Transaction` with a UUID, adds it to
  `pendingLocalRevisions`, and on the subscription-channel echo drops
  events whose `transaction_id` is in the set. Remote transactions
  (ones whose id isn't pending) are merged in via a CM
  `EditorView.dispatch({ changes })` computed from the kernel
  snapshot diff.
- **Authoritative state**: `applyTransaction` returns the full
  `EditorSnapshot`. The shell compares its `revision` to the last one
  applied; if the snapshot is newer than the last local edit
  (shouldn't happen for a local echo, but happens for a remote
  transaction arriving mid-compose), the shell re-derives the CM doc
  from `MarkdownSerializer`-equivalent output and dispatches a
  wholesale replace. For v1 this is acceptable; delta-merge is a
  follow-up.

**Files:**
- `shell/src/plugins/nexus/editor/cm/transactionBridge.ts`:
  - `changesToOps(update: ViewUpdate, tabState): Operation[]` — walks
    `update.changes.iterChanges((fromA, toA, fromB, toB, inserted) => …)`
    and produces a single-op or multi-op `Transaction`.
  - CM `ViewPlugin` that listens for `update.docChanged` and calls
    `kernelClient.applyTransaction(relpath, tx)` fire-and-forget with
    error toasts via `api.notifications`.
- `shell/src/plugins/nexus/editor/cm/extensions.ts` — remove
  `history()` and `historyKeymap` (the Rust undo tree owns history
  now). Add a custom keymap binding Ctrl/Cmd-Z → `kernelClient.undo`,
  Ctrl-Y / Cmd-Shift-Z → `kernelClient.redo`. Both update CM via the
  returned snapshot.
- `shell/src/plugins/nexus/editor/editorStore.ts` — drop
  `setContent` from the CM-edit path. Keep it only for preview-mode
  fallback + untitled tabs (which don't have a session yet).

**Exit:**
- Unit test: typing "hello" in one CM tab produces exactly five
  `InsertText` transactions in the mock kernel client (or one batched,
  depending on the batching strategy chosen — document either).
- Unit test: remote `sync_content` arrives → CM view reflects the new
  content within one microtask, cursor/selection preserved if
  possible (at worst, snapped to end).
- Manual: Ctrl-Z undoes through kernel; `can_undo: false` grays out
  the (future) menu entry.

### Phase 6 — Persistence rewiring

Save still goes through storage, but through the kernel's session (so
the serialized form matches the in-memory tree byte-for-byte).

**Files:**
- `shell/src/plugins/nexus/editor/index.ts` — replace the
  `COMMAND_SAVE` body (lines 261-285) with a call to
  `kernelClient.saveSession(relpath)`. That handler already does
  `MarkdownSerializer::serialize` → `com.nexus.storage::write_file`
  atomically (`crates/nexus-editor/src/core_plugin.rs:330-360`).
- Untitled tabs: no session exists yet. On first save, create the file
  via `storage::write_file`, then call `openSession` to get a tree.

**Exit:** Ctrl-S writes canonical markdown (headings roundtripped
through the block tree). Reopening the file shows identical content.
Dirty indicator (`isDirty` in `editorStore.ts`) updates against the
server revision, not local content diff.

### Phase 7 — Downstream consumers

Outline, backlinks, and the graph now subscribe to editor-change
events instead of reading `tab.content`.

**Files:**
- `shell/src/plugins/nexus/outline/*` — replace the
  `useEditorStore.content` watcher with a subscription to the editor
  event bus (Phase 4). On `changed` for the active relpath, re-request
  `get_tree` and derive headings. Optionally add a dedicated
  `get_headings` handler on `com.nexus.editor` later.
- `shell/src/plugins/nexus/backlinks/*` — same pattern.
- Graph: unchanged for v1 (operates on saved files, not in-progress
  edits).

**Exit:** Outline updates live as you type in CM. Tests cover the
event-driven path.

### Phase 8 — Cleanup + docs

- Remove the textarea code path in `EditorView.tsx` (source mode now
  exclusively CM).
- Delete `setContent` if no callers remain (grep confirms).
- Add `docs/editor-transaction-architecture.md` — quick reference for
  how an edit flows: keystroke → CM → `changesToOps` → kernel client
  → `com.nexus.editor::apply_transaction` → `UndoTree::execute` →
  event → subscribers.
- Update `docs/leaf-architecture.md` with a pointer: "MarkdownView
  owns a kernel session lifecycle (see editor-transaction-plan
  Phase 3)."

**Exit:** `rg "setContent\(" shell/src/plugins/nexus/editor/` returns
only untitled-tab handling. `tsc` clean. Full manual smoke passes.

## Risks

- **Change-to-op mapping is lossy in v1.** Any CM change that crosses
  block boundaries falls back to `sync_content` (no undo history).
  Heavy paste operations → no granular undo. Accept for v1; flag for
  follow-up.
- **IPC latency on every keystroke.** `ipc_call` is in-process
  (Tauri's `invoke` bridge), not network. Measured overhead for
  `invoke` in this codebase is sub-ms; still, debounce remote-echo
  merges on the CM side to avoid thrashing.
- **Echo races.** If a remote transaction arrives before the local
  echo, the shell may apply it, then re-apply on echo. The
  `transaction_id` dedupe set prevents double-apply but requires the
  Rust side to thread the id through events — verify in Phase 4
  integration test.
- **Undo across save.** The kernel's `UndoTree` is in-memory; closing
  a session wipes it. Ctrl-Z after Ctrl-S works; Ctrl-Z after tab
  close does not. Document this. Persistence of undo history is out
  of scope.
- **Untitled tabs.** They have no session until first save.
  `applyTransaction` on an untitled tab is a no-op — edits stay in
  `useEditorStore.content` until save creates the file. Code path
  divergence is a footgun; isolate it in `sessionManager.acquire`
  returning `null` for untitled.

## Resolved decisions

1. **`docId = vault-relative path`**, not a UUID. Every other Rust
   handler already keys on `relpath`; adding an indirection layer is
   pure cost. Renames: explicit close-at-old + open-at-new; no
   rename-transaction type. Trade-off: moving a file while edited
   loses the session. Acceptable — file-tree moves already close
   tabs in the current shell.

2. **Full-snapshot reconciliation for remote transactions in v1**,
   not delta-merge. `applyTransaction` returns `EditorSnapshot` with
   the whole tree; shell diff'ing is future work. Keeps Phase 5
   tractable; regression risk is negligible for single-user mode
   (where remote transactions only arrive from `sync_content` /
   AI, which are already full-doc rewrites today).

3. **Kernel owns undo history, CM does not.** Rip out
   `@codemirror/commands` `history()` entirely. One source of truth
   simplifies AI edits (which should also be undoable via the same
   stack). Cost: CM local history features (e.g. undo-merge of rapid
   keystrokes) must be re-implemented on the `UndoTree` side. The
   `UndoTree` already groups at the `Transaction` boundary, so batch
   CM changes within a single `requestAnimationFrame` tick into one
   transaction.

4. **Block identity is coarse-grained in v1.** The CM doc maps 1:1 to
   the serialized markdown; block-aware operations use the root
   block. Precise per-block operations come when the editor learns
   about block boundaries (outline-driven cursor placement, slash
   commands) — a separate project.

5. **MarkdownView stays thin; EditorView pivots.** The leaf-wrapper
   role (`MarkdownView.tsx`) only grows session lifecycle hooks.
   All CM knowledge stays inside `EditorView.tsx` + `cm/`. This
   matches the guidance in `docs/leaf-architecture.md` — views are
   mount shims, not feature containers.

## References

- `docs/leaf-architecture.md` — view/leaf primitives.
- `docs/leaf-migration-plan.md` — phase discipline template.
- `docs/PRDs/08-editor-engine.md` — transaction model spec.
- `crates/nexus-editor/src/core_plugin.rs` — handler ids 1-9,
  session map, `apply_transaction` flow at line 362.
- `crates/nexus-editor/src/transaction.rs` — `Operation`,
  `Transaction`, `TransactionMetadata` — the wire format.
- `crates/nexus-bootstrap/src/lib.rs:431-459` — command-id mapping
  for `com.nexus.editor`.
- `shell/src/plugins/nexus/editor/index.ts` — current storage-only
  wiring; the diff surface for Phases 3, 5, 6.
- `shell/src/plugins/nexus/editor/EditorView.tsx:537-547` — the
  `<textarea>` being replaced.
- `shell/src/types/plugin.ts:241-268` — `KernelAPI` surface
  (`invoke`, `on`).
- CodeMirror 6 docs: <https://codemirror.net/docs/ref/>
- CodeMirror 6 system guide: <https://codemirror.net/docs/guide/>
- `@codemirror/lang-markdown`: <https://github.com/codemirror/lang-markdown>

---

## Appendix — future efforts (sketches, not plans)

### Graph force-directed layout

Current: `shell/src/plugins/nexus/graph/layout.ts` computes a static
radial arrangement with neighbours on a single ring. Data pipeline
(1-hop neighbours from `graphStore`) is fine; only `layout.ts` + a
rAF animation loop in `GraphView.tsx` need to change.

Library candidates:
- **d3-force** — battle-tested, small, 2D, SVG-friendly. Recommended
  default. Adequate up to ~500 nodes in SVG.
- **force-graph / ngraph** — Canvas/WebGL, scales to 10k+ nodes; big
  for what we need today but worth noting if multi-hop expansion is
  on deck.
- **cytoscape** — heavier, brings its own renderer and a layout
  plugin ecosystem; reach for this only if we need advanced layouts
  (dagre, cose) beyond force.

Perf flag: the existing SVG renderer (`GraphView.tsx`) will choke past
~500 nodes. Budget a Canvas rewrite if we expand past 1-hop.

### Bases CRUD plugin

Types in `crates/nexus-types/src/bases.rs`; storage layer in
`crates/nexus-storage/src/bases/{mod.rs,query.rs,relation.rs,schema.rs}`.
Stub plan:
1. Audit `crates/nexus-storage/src/bases/mod.rs` public surface; list
   what CRUD ops exist and what's missing (likely need
   `create_base`, `list_bases`, `upsert_record`, `delete_record`,
   `run_query`).
2. Register handler ids + command mapping in
   `nexus-bootstrap/src/lib.rs` under a new `com.nexus.bases` core
   plugin (mirror the editor plugin's structure).
3. Shell plugin: `shell/src/plugins/nexus/bases/` with a `BasesView`
   subclass of `ViewBase`, registered via `viewRegistry.register('bases', …)`.
4. Table rendering: use `@tanstack/react-virtual` (new dep) above
   ~500 rows; plain DOM below.

### Sync

Green-field. Fork in the road — **pick one before any code**:
- **CRDT (Yjs / Automerge)** — convergent under concurrent edits,
  heavy client-side state, needs each `Operation` re-expressed as
  CRDT mutations. Best for real-time collab; overkill for single-user
  multi-device.
- **OT** — lighter than CRDT, requires a central server for
  transform; conflicts with our local-first posture.
- **Git-per-document** — trivial conceptually, works offline, but
  merges fall to three-way text merge (loses block structure on
  conflict). Best for infrequent multi-device single-user.

Decisions needed first: (a) multi-user collab Y/N; (b) offline-first
Y/N; (c) authoritative server Y/N; (d) conflict-resolution UX — auto-
merge, manual-resolve, or "last-writer-wins-with-backup".
