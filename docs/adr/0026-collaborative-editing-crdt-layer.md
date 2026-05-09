# ADR 0026: Collaborative Editing CRDT Layer

**Date:** 2026-05-08
**Status:** Accepted. Phases 1–4 of the `nexus-crdt` library plus the
editor wiring (`OpObserver` callback, `CrdtPublisher` orchestrator,
on-open/on-close persistence) shipped 2026-05-08. Remaining work
(Tauri popout forwarding, BL-007 git merge driver registration,
op-log compaction, conflict UI for `StructuralDeleteEdit`) is
tracked under "Open follow-ups" below.
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

### Phase 2 (shipped — silent text merge)

`CrdtDoc` now eagerly maintains a per-block `text::RgaText` mirror
materialised at construction from baseline content using
deterministic synthetic `OpId`s (`merge::baseline_op_id`,
`merge::subop_id`). Both peers materialise identical RGAs from
equal `BlockTree`s, so concurrent ops gossiped between them
converge.

`CrdtOp` gained an `rga_ops: Vec<RgaTextOp>` field carrying the
position-free RGA translation authored at `apply_local` time. On
`apply_remote` for concurrent text ops, the doc replays `rga_ops`
on the local RGA and rebuilds `block.content` from `rga.render()`
— the editor op's byte-positional payload is stale and is skipped.
Causally-ordered text ops still apply through the editor and use
`rga_ops` to keep the mirror in sync.

The conflict surface narrows: `Conflict::ConcurrentBlockEdit` now
only surfaces for whole-block replacements (`UpdateBlockContent` /
`UpdateAnnotations`) that the RGA can't merge;
`Conflict::StructuralDeleteEdit` always surfaces.

### Phase 3 (shipped — sync infrastructure)

`nexus_crdt::wire`: per-file topic `com.nexus.editor.ops.<relpath>`
plus `OpEnvelope { op: CrdtOp }` JSON payload. `nexus_crdt::sync`:
`DocHandle` (cloneable `Arc<Mutex<CrdtDoc>>`) and `SyncLoop` that
owns a kernel `EventSubscription` and drains it into
`CrdtDoc::apply_remote`. Self-echo ops (`op.id.site ==
doc.site()`) are dropped so a single bus shared by author and
receiver in the same process doesn't loop.

The shipped infrastructure is what the `EditorCorePlugin` will
publish into and what cross-process / cross-window peers consume.
Editor adoption (per-session `CrdtDoc` shadow + per-op publish on
`apply_transaction`) is part of the deferred "Editor wiring" tail
below — it's blocked on adding an `OpObserver` callback hook in
`nexus-editor` because `nexus-crdt` already depends on
`nexus-editor` and a direct reverse-dep would cycle. The
orchestration belongs in `nexus-bootstrap`, which already pulls
both crates.

### Phase 4 (shipped — persistence + git merge primitive)

`nexus_crdt::state::PersistedCrdt`: schema-versioned envelope around
a `CrdtState` snapshot (site, lamport, op log, per-block meta,
per-block RGA — no tree, since the tree comes from the markdown
source on load). Helpers: `crdt_state_path(relpath)` lays out
files at `<forge>/.forge/.editor/crdt/<sha-of-relpath>.json`
(matching the BL-072 undo-state convention), and
`content_hash_hex(bytes)` is the SHA-256 integrity tag the editor
will check on load to detect external markdown edits.

`OpLog::merge(other)` is the idempotent-union primitive the BL-007
git merge driver registers as the conflict resolver for the
state file: replaying the merged log on a fresh `CrdtDoc` produces
the same state regardless of which side merged into which.

`CrdtDoc::state()` / `from_state(tree, state)` is the snapshot/restore
pair. `from_state` tolerates tree drift — blocks added externally
get a fresh RGA, removed blocks have their RGA dropped — so a
session can survive a compatible markdown edit without losing
CRDT continuity.

JSON-format detail: `RgaText.nodes` and `OpLog.ops` are
`HashMap<OpId, …>`, but JSON map keys must be strings, so the
on-disk form serialises both as `Vec<(OpId, …)>` via serde shims.

### Editor wiring (shipped 2026-05-08)

Three pieces sit on top of the library:

1. **`nexus-editor::OpObserver`** — public trait with
   `on_session_opened` / `on_session_closed` /
   `on_apply_transaction` hooks. `EditorCorePlugin` invokes them
   from `finish_open*` / `handle_sync_content` / `handle_close*` /
   `handle_apply_transaction` at the right lifecycle points. The
   trait lives in `nexus-editor` (not `nexus-crdt`) because
   `nexus-crdt` already deps on `nexus-editor` — putting the trait
   alongside the consumer keeps the dep graph acyclic.
2. **`nexus-bootstrap::crdt_publisher::CrdtPublisher`** — concrete
   `OpObserver` impl. Owns a per-process `SiteId`, a
   `HashMap<relpath, CrdtDoc>` shadow, and a clone of the kernel
   `EventBus`. On `on_session_opened` it tries to load
   `PersistedCrdt` from disk (degrades to a fresh doc on hash
   mismatch / version mismatch / decode failure, mirroring the
   BL-072 undo invalidation policy). On `on_apply_transaction` it
   calls `doc.apply_local` for each op and publishes the
   `OpEnvelope` via `publish_plugin`. On `on_session_closed` it
   atomic-writes the snapshot via tmp+rename.
3. **Wired into `build_*_runtime`** so all invokers (CLI, TUI,
   MCP, Tauri shell) get publishing + persistence by default.

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

- **Tauri popout window forwarding** (ADR 0020) — the shell's
  Tauri host doesn't relay `com.nexus.editor.ops.<relpath>`
  events between popout windows. Within a single backend process
  the editor's `CrdtDoc` is the same object across all windows so
  they already converge through it; the gap is for plugin code
  running *inside* a popout window that wants to subscribe to ops
  directly.
- **BL-007 git merge driver registration** —
  `OpLog::merge` and a CLI shim
  (`nexus crdt merge-driver --base ... --ours ... --theirs ...`)
  ship; users still need a one-time `.gitattributes` rule
  (`.forge/.editor/crdt/* merge=nexus-crdt`) and a `git config
  merge.nexus-crdt.driver "nexus crdt merge-driver %O %A %B"`
  invocation. The CLI command tells the user what to add.
- **Conflict UI for `StructuralDeleteEdit`.** When peer-sync is
  driving `apply_remote` (BL-007 git pulls or popout-window
  bridge), the resolver UI for delete-vs-edit conflicts needs a
  surface — currently the publisher only authors local ops via
  `apply_local`, so no `Conflict` reaches the caller in the
  shipped wiring.
- **Reparenting / move-loop detection.** Conflict detection is still
  silent on concurrent reparents — two sites moving the same block
  to different parents will *both* apply, and the second op wins by
  causal ordering. This is acceptable while reparent is rare;
  revisit if collaborative outlining becomes a primary use case.
