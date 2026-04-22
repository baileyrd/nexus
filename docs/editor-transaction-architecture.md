# Editor Transaction Architecture

Quick reference for how a single keystroke travels from CodeMirror
through the kernel and back. Companion to
[`editor-transaction-wiring-plan.md`](./editor-transaction-wiring-plan.md)
(the phased build-out) and
[`PRDs/08-editor-engine.md`](./PRDs/08-editor-engine.md) (the product
contract). For the surrounding `Leaf` / `View` plumbing see
[`leaf-architecture.md`](./leaf-architecture.md).

## The edit flow

```
keystroke in CM
   │
   ▼
EditorView.updateListener  ── shell/src/plugins/nexus/editor/cm/transactionBridge.ts
   │   (rAF-batched; one kernel transaction per animation frame)
   ▼
changesToOps(update, rootBlockId)
   │   - single-segment insert  → InsertText
   │   - single-segment delete  → DeleteText
   │   - anything else          → UpdateBlockContent (whole-doc fallback)
   ▼
makeTransaction(ops, { source: 'user' })  → assigns UUID `tx.id`
   │
   ▼
editorStore.addPendingLocalRevision(tx.id)   ── echo-suppression marker
   │
   ▼
kernelClient.applyTransaction(relpath, tx)  ── shell/src/plugins/nexus/editor/kernelClient.ts
   │   invoke 'com.nexus.editor::apply_transaction'
   ▼
(Rust) Session::apply_transaction
   │   UndoTree::execute(tx)
   │       - pushes inverse on the undo stack
   │       - mutates BlockTree
   │       - bumps `revision`
   │   emits  com.nexus.editor.changed.<relpath>
   ▼
Shell: SessionManager.handleChanged
   │   1. consumePendingLocalRevision(tx.id)  → if present, DROP (echo)
   │   2. editorStore.setSessionRevision(relpath, revision)
   │   3. fan out to registered EditorChangedListener(s)
   ▼
Subscribers: outline, backlinks, secondary editor leaves
```

The `applyTransaction` call also resolves with the post-state
`EditorSnapshot`; the bridge follows up with `getMarkdown` and, if the
canonical serialization diverges from CM's current doc (e.g. the
serializer normalized whitespace), dispatches a `changes: { from: 0, to:
N, insert: canonical }` reconciliation. The `flushing` guard inside the
bridge prevents that reconciliation from re-entering the listener.

## Session lifecycle

One `Session` per open relpath, held by Rust
(`crates/nexus-editor/src/core_plugin.rs`). Shell-side refcounting:
`shell/src/plugins/nexus/editor/sessionManager.ts`.

- `acquire(relpath)` — 0→1 calls `openSession` and caches the initial
  `EditorSnapshot`; 1→N returns the cached snapshot with no kernel
  round-trip. On open it seeds `editorStore.savedRevision` from the
  snapshot so `isDirty` starts false.
- `release(relpath)` — N→1 decrements; 1→0 awaits any in-flight event
  subscription, unsubscribes, clears revision bookkeeping, and calls
  `closeSession`.
- Leaves own the refcount: `MarkdownView.onOpen` acquires,
  `MarkdownView.onClose` releases
  (`shell/src/plugins/nexus/editor/MarkdownView.tsx`). Splits and
  layout-restore therefore never force a close+reopen of a live
  session.
- Untitled placeholders (`untitled-N`) short-circuit everything:
  `acquire` returns `null`, no kernel session opens, and the tab
  drives the store locally (see "Untitled-tab divergence" below).

## Echo suppression

The kernel broadcasts `com.nexus.editor.changed.<relpath>` on every
apply — including the shell's own transactions. Without filtering, the
shell would double-apply its own edit when the echo arrived.

Mechanism:

1. Bridge adds `tx.id` to
   `editorStore.pendingLocalRevisions` just before dispatch
   (`shell/src/plugins/nexus/editor/editorStore.ts`).
2. When the echo lands, `SessionManager.handleChanged` calls
   `consumePendingLocalRevision(tx.id)`. If the id was present, the
   handler returns early — the snapshot was already reconciled inline
   by the bridge, and listeners are deliberately not invoked.
3. Genuinely external changes (future collab, AI rewrites, disk
   watcher) carry a `tx.id` the shell never staged, so they flow
   through to listeners normally.

An `applyTransaction` failure also removes the id from the pending
set, so a dropped round-trip doesn't leak a stale marker that would
later swallow an unrelated echo by coincidence.

## Undo ownership

CodeMirror ships its own history extension. We do not use it for
markdown tabs with a live session — kernel `UndoTree::execute` is the
source of truth. Keeping undo kernel-side is what lets
non-CM surfaces (outline, AI edits, future collab) participate in the
same history stack, and is why the bridge always sends edits through
`apply_transaction` rather than mutating CM locally.

CM still owns cursor / selection, and the `kernelUndo` wiring in
`EditorView.tsx` binds `Mod-z` / `Mod-Shift-z` to
`kernelClient.undo` / `kernelClient.redo`, then applies the returned
canonical doc back into CM via `applyCanonical`.

## Dirty detection

Revision-based, not content-based. The store keeps two per-relpath
maps:

- `sessionRevision[relpath]` — latest revision seen from the kernel
  (bumped by `setSessionRevision` on every echo).
- `savedRevision[relpath]` — the revision at the moment of the last
  successful save (snapshotted by `markSavedRevision`).

`isDirty(tab)` returns `sessionRevision !== savedRevision` when both
are present. For untitled buffers (no session), it falls back to
`content !== savedContent`. Renaming (`remapRelpath`) re-keys both
maps so a moved tab doesn't flip dirty spuriously.

## Untitled-tab divergence

Untitled tabs (`untitled-N`) have no file, no kernel session, no
`BlockTree`, and no `UndoTree`. They exist only in `editorStore.tabs`.

In source mode, the editor view renders a `<CodeMirrorHost>` WITHOUT
the `transactionBridge` extension; its `onChange` writes straight to
`editorStore.setContent(tab.relpath, v)`. This is the one surviving
production caller of `setContent` in the editor plugin.
`shell/src/plugins/nexus/editor/EditorView.tsx` picks the branch via
`bridgeEligible`:

```
bridgeEligible =
  runtime !== null
  && !isUntitled(tab.relpath)
  && isMarkdown(tab.name)
  && sessionManager.refcount(tab.relpath) > 0
```

First save (the `files:saveAs`-style flow) transitions the buffer into
a real relpath, at which point `MarkdownView.onOpen` acquires a
session and subsequent edits route through the bridge like any other
tab. Undo history from the untitled phase is not carried forward — the
kernel `UndoTree` starts fresh.

## File map

- `shell/src/plugins/nexus/editor/EditorView.tsx` — tab chrome +
  bridge-vs-local branch selection.
- `shell/src/plugins/nexus/editor/MarkdownView.tsx` — `ViewBase`
  implementation; owns `acquire` / `release` for one leaf.
- `shell/src/plugins/nexus/editor/sessionManager.ts` — refcount,
  snapshot cache, event subscription, echo filter.
- `shell/src/plugins/nexus/editor/kernelClient.ts` — typed wrapper
  over `com.nexus.editor` IPC commands.
- `shell/src/plugins/nexus/editor/editorStore.ts` — tab state,
  revision bookkeeping, `pendingLocalRevisions`, `isDirty`.
- `shell/src/plugins/nexus/editor/cm/transactionBridge.ts` —
  `changesToOps`, rAF batching, dispatch + reconciliation.
- `shell/src/plugins/nexus/editor/cm/CodeMirrorHost.tsx` — thin React
  wrapper around `EditorView`; hosts bridge extension + kernel-undo
  keymap.
- `crates/nexus-editor/src/core_plugin.rs` — Rust `Session`,
  `apply_transaction`, `com.nexus.editor.changed.<relpath>` event.

## See also

- [`editor-transaction-wiring-plan.md`](./editor-transaction-wiring-plan.md)
  — phased rollout notes, change-to-op heuristics, risks.
- [`PRDs/08-editor-engine.md`](./PRDs/08-editor-engine.md) — product
  contract for the editor engine.
- [`leaf-architecture.md`](./leaf-architecture.md) — `Leaf` /
  `ViewRegistry` primitives that host `MarkdownView`.
