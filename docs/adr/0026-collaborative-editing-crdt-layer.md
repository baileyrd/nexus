# ADR 0026: Collaborative Editing CRDT Layer

**Date:** 2026-05-08
**Status:** Accepted (Phase 1). Phases 2–4 deferred — see "Phase plan"
below.
**Related:** BL-074 (CRDT layer), BL-007 (CRDT-over-Git transport),
ADR 0017 (block-id stability), PRD-08 §8 (collaborative editing).

## Context

PRD-08 §8 specifies that two sessions on the same forge should converge
to identical state when editing the same document. The block tree
already had the foundations for this:

- ADR 0017 stamped each block with a stable cross-session id.
- `nexus-editor::Operation` is invertible — every variant carries
  enough state to reverse itself, which is exactly what an op-based
  CRDT needs for tombstoning and idempotency.
- Annotation ranges adjust on insert/delete, so concurrent edits
  cannot silently corrupt formatting.

What was missing was the **envelope**: a way to ship those ops between
sessions, decide whether two ops were causally ordered or concurrent,
and resolve concurrency without losing data.

## Decision

A new `nexus-crdt` crate wraps `nexus-editor::Operation` in an op-based
CRDT envelope and owns the convergence logic. The kernel and editor
remain CRDT-unaware; sync loops (Phase 3+) drive `nexus-crdt` directly.

### Phase 1 (this ADR — shipped)

- **Identity types** (`id.rs`): `SiteId(Uuid)`, `Lamport(u64)`,
  `OpId { lamport, site }` ordered by `(lamport, site)`,
  `VersionVector(HashMap<SiteId, Lamport>)`.
- **`CrdtOp`** (`op.rs`): `{ id, vv_at_creation, op: Operation }`.
  Causality is the standard vector-clock test: two ops `A` and `B`
  are concurrent iff neither's `vv_at_creation` contains the other's
  id.
- **`OpLog`** (`log.rs`): append-only, idempotent by `OpId`. Exposes a
  `missing_for(remote_vv)` helper for gossip / catch-up sync.
- **`CrdtDoc`** (`doc.rs`): wraps `BlockTree` + `OpLog` and implements
  `apply_local` / `apply_remote`. Tracks per-block "last writer" and
  "deleted by" metadata so it can detect cross-session conflicts in
  O(1) per op.
- **Conflict surface** (`conflict.rs`):
  - `ConcurrentBlockEdit` — two sites mutated the same block's
    content without seeing each other. Phase 1 surfaces this; Phase 2
    silences it for pure text overlap (via `text::RgaText`).
  - `StructuralDeleteEdit` — a delete raced an edit on the same
    block. Always surfaces — there is no automatic resolution.
- **`text::RgaText`** (`text.rs`): full RGA sequence CRDT for in-block
  character text, tested standalone. Phase 2 wires it into
  `CrdtDoc`'s text-conflict path.

The crate has 21 unit tests covering: lamport ordering, version-vector
domination, idempotency (log + RGA), gossip slicing, three convergence
scenarios (different blocks, same site sequential, three-site
interleaving), structural delete-edit, and concurrent-edit surfacing.

### Phase 2 (deferred — silent text merge)

Wire `text::RgaText` into `CrdtDoc::detect_conflict`: when a remote
text op (`InsertText` / `DeleteText`) is concurrent with a local op on
the same block, replay both as `RgaTextOp` sequences against the
block's `RgaText` snapshot, then rebuild `block.content` from
`RgaText::render()`. The conflict surface narrows to structural
delete-edit only.

This requires a per-block initialisation step: when a block first sees
a concurrent text edit, the doc must materialise an `RgaText` from the
current `block.content` (each character gets a synthetic OpId derived
from the block's last writer + position). The synthetic IDs need to be
deterministic across sites so two peers initialising independently
land on the same RGA state — keying on `(last_writer_op_id, position)`
works because both peers have the same `last_writer_op_id` by the time
they start diverging.

### Phase 3 (deferred — sync loop)

A new event-bus topic `com.nexus.editor.ops.<relpath>` carries
`CrdtOp` payloads between sessions. The editor `core_plugin` publishes
on every `apply_transaction`; a new `nexus-crdt::SyncLoop` subscribes
and drives `CrdtDoc::apply_remote`. The Tauri shell forwards the same
topic across windows so popouts (ADR 0020) stay in sync.

### Phase 4 (deferred — persistence, BL-007)

The op log persists alongside the markdown source so reopening a file
restores CRDT state. We serialise the `OpLog` + per-block `RgaText`
snapshots as JSON in `<forge>/.forge/.editor/crdt/<sha>.json`. On
git push the file is committed; on pull with conflict, the file's
own conflict markers are resolved by `OpLog::merge` (idempotent
union) before any markdown reconciliation runs.

## Consequences

**Positive:**

- The microkernel invariant holds: `nexus-crdt` depends only on
  `nexus-editor`, and no existing crate depends on `nexus-crdt`. Future
  sync loops, frontends, and persistence layers integrate via this
  one crate without growing the kernel surface.
- Phase 1 is testable in isolation with no IPC, no event bus, no
  filesystem — just the crate's pure-Rust API. The 21 unit tests
  verify convergence properties before any wiring lands.
- The conflict surface is honest: the layer never invents merges it
  cannot prove correct. `StructuralDeleteEdit` always reaches the
  user, even after Phase 2.
- The `OpLog` doubles as the BL-007 persistence format with no
  schema changes — Phase 4 adds I/O, not new types.

**Negative / costs:**

- Phase 1 surfaces concurrent text edits as conflicts even when they
  could be merged silently. Users who edit the same block from two
  sessions before Phase 2 lands will see conflict UI. (Today there
  *is* no live-sync UI at all, so the regression is theoretical.)
- Memory cost of storing `vv_at_creation` per op is O(sites × ops).
  For typical forges (≤4 sites, ≤10⁴ ops/file) this is < 1 MB; if
  this changes, switch to a delta encoding against the previous op's
  VV in Phase 4's persisted form.
- The op log grows unbounded. Compaction (collapsing ops superseded
  by later ones with no concurrent peers) is left to a later phase.

## Alternatives considered

- **Automerge (PRD-08's original suggestion).** Rejected for Phase 1:
  embedding automerge requires translating every `Operation` variant
  into automerge changes, and the round-trip back from automerge's
  document model to our block tree is non-trivial. The op-based
  approach reuses our existing `Operation` type with a thin wrapper.
  Phase 4 could swap the persistence format to automerge if needed
  for BL-007 git-merge ergonomics.
- **Last-writer-wins by Lamport.** Rejected: drops concurrent
  insertions silently. Not a real CRDT.
- **Adding the CRDT envelope to `nexus-editor::Operation` directly.**
  Rejected: it would force every editor consumer (CLI, TUI, MCP, the
  Tauri shell) to deal with site IDs and version vectors even when
  they're not collaborating. The wrapper crate keeps the local-only
  path zero-cost.

## Open follow-ups

- BL-074 Phase 2 — wire `RgaText` into `CrdtDoc::detect_conflict`.
- BL-074 Phase 3 — `com.nexus.editor.ops.<path>` event topic +
  `SyncLoop` subscriber + Tauri-window forwarding.
- BL-074 Phase 4 / BL-007 — persist `OpLog` to
  `<forge>/.forge/.editor/crdt/<sha>.json`, define git merge driver.
- Reparenting / move-loop detection. Phase 1 conflict detection is
  silent on concurrent reparents — two sites moving the same block
  to different parents will *both* apply, and the second op wins by
  causal ordering. This is acceptable while reparent is rare; revisit
  if collaborative outlining becomes a primary use case.
