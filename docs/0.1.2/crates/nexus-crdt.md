# nexus-crdt

> Kind: lib · IPC plugin id: — · CorePlugin: no · Has settings: no (the wiring layer in bootstrap/collab carries config, not this crate) · As of: 2026-05-25

## Overview

`nexus-crdt` is the operation-based CRDT layer for collaborative editing (BL-074 / PRD-08 §8, ADR 0026). It wraps `nexus_editor::Operation` in a CRDT envelope (`CrdtOp`) so two sessions on the same forge — popout windows (ADR 0020), separate CLI/TUI processes, or future cross-process peers — can exchange edits and converge without user intervention. The headline guarantee: concurrent **character-level** text edits on the same block merge silently; only edits that genuinely contend (a delete racing an edit on the same block, or two whole-block replacements) reach the caller as a `Conflict`.

The crate is a layered design. At the bottom sits `text::RgaText`, a standalone RGA (Replicated Growable Array) sequence CRDT that merges per-character inserts/deletes within one block's content. Above it, `CrdtDoc` pairs an editor `BlockTree` with an append-only `OpLog`, per-block `BlockMeta`, and a per-block `RgaText` mirror; it translates byte-positional editor ops into position-free RGA ops at authoring time so receivers can replay them regardless of their local cursor state. Identity is carried by `SiteId` (per-session UUID), `Lamport` (per-site monotonic counter), `OpId = (lamport, site)`, and `VersionVector` for causality tests. `wire`, `sync`, and `state` provide the bus payload schema, an inbound apply loop, and on-disk persistence respectively.

It sits **between** `nexus-editor` (the block model and the `Operation` primitive it wraps) and `nexus-collab` (the WebSocket transport that relays op envelopes between machines). The crate itself does no I/O routing and registers nothing on the kernel — it is a pure library of types and algorithms. The actual plumbing lives in `nexus-bootstrap`: `crdt_publisher::CrdtPublisher` implements `nexus_editor::OpObserver`, mirrors every editor session into a `CrdtDoc`, publishes op envelopes on the bus, and persists state. `nexus-bootstrap`'s `collab` bridge then ships those bus events to a remote relay.

It is **not a `CorePlugin`** because it owns no kernel-mediated capability surface: it exposes no IPC handlers, holds no plugin id, and is never registered in the bootstrap plugin order. Microkernel fit is preserved precisely by keeping it a leaf-ish library — its one cross-service dependency (`nexus-editor`) is deliberate and explicitly allowed by `crates/nexus-bootstrap/tests/dep_invariants.rs`, because the kernel depends on neither crate, so no microkernel isolation rule is violated. The "new capability ⇒ new IPC handler" guardrail is satisfied by the publisher and collab bridge living in bootstrap, not by adding handlers here.

## Position in the dependency graph

- **Direct `nexus-*` deps:** `nexus-editor` (wraps its `Operation`/`BlockId`/`BlockTree`; the one sanctioned cross-service edge), `nexus-kernel` (for `EventBus`/`EventFilter`/`EventSubscription`/`NexusEvent`/`RecvError` used by `sync::SyncLoop`), `nexus-types` (for `plugin_ids::EDITOR`, used in the compile-time topic-prefix assertions in `wire`).
- **Notable external deps:** `serde`/`serde_json` (all wire and persisted types), `uuid` (site ids and the v5-namespaced synthetic ids in `merge`), `tokio` (sync/rt/macros — async `SyncLoop::run`), `thiserror` (`CrdtError`), `tracing`. Dev-only: `nexus-formats`, plus tokio `time`/`test-util`.
- **Notably absent:** no `sha2` dependency — `state` ships a small self-contained FIPS-180-4 SHA-256 just for the content-hash integrity tag.
- **Crates depending on it:** `nexus-bootstrap` (the `CrdtPublisher` and `collab` bridge) and `nexus-cli` (collab command). It is consumed indirectly by the editor flow rather than depended on by `nexus-editor` itself (which would be a cycle — hence the publisher lives in bootstrap).

## Public API surface

**`id` module** — identity primitives
- `SiteId(Uuid)` — per-session site id; unique-per-session is a hard CRDT invariant. `new()`/`Default` mint a v4 UUID.
- `Lamport(u64)` — per-site logical clock; `next()` ticks it.
- `OpId { lamport, site }` — globally-unique op identity; total order is `(lamport, site)` (lamport primary, site UUID tiebreak).
- `VersionVector(HashMap<SiteId, Lamport>)` — causality witness: `observe`, `get`, `contains`, `dominates`.

**`op` module** — the CRDT envelope
- `CrdtOp { id, vv_at_creation, op, rga_ops }` — an editor `Operation` plus its op id, the authoring site's VV *before* the op (the concurrency test), and the position-free `RgaTextOp` translation for text ops.
- `primary_block_id(&Operation) -> BlockId` — the block an op primarily targets (for conflict bucketing).
- `affected_blocks(&Operation) -> Vec<BlockId>` — every block an op reads/writes (catches delete-vs-child-edit).

**`text` module** — the sequence CRDT
- `RgaText` — RGA state for one block's text: `new`, `len`/`is_empty` (visible chars, tombstones excluded), `render` (RGA traversal order), `apply` (idempotent, returns changed-bool), `build_insert`/`build_delete` (author wire ops from a visible position), `from_chars` (materialise a baseline from a string + id factory), `op_id_at`.
- `RgaTextOp` — wire primitive, `Insert { id, parent, ch }` / `Delete { id, target }`; `id()` accessor.

**`log` module**
- `OpLog` — append-only, idempotent-by-`OpId`: `append`, `contains`, `len`/`is_empty`, `iter` (causal order), `get`, `version_vector`, `missing_for` (gossip catch-up), `merge` (idempotent union — git-merge driver), `prune_dominated`/`pruned_floor`/`compact_to` (BL-074 follow-up compaction).

**`doc` module** — the façade the editor/sync layers drive
- `CrdtDoc` — `new(site, tree)`, `apply_local(&Operation) -> Result<CrdtOp>`, `apply_remote(CrdtOp) -> Result<RemoteOutcome>`, `tree`, `log`, `site`, `block_rga`, `state`, `from_state`, `compact_to`.
- `RemoteOutcome` — `Duplicate` / `Applied` / `Conflict(Conflict)`.
- `BlockMeta { last_writer, deleted_by }` — minimal per-block conflict state (public for persistence).

**`conflict` module**
- `Conflict` — `ConcurrentBlockEdit { block_id, local, remote }` / `StructuralDeleteEdit { block_id, delete, edit }`; `block_id()` accessor.
- `ConflictDetail` — `Conflict` flattened plus resolver content snapshots (`local_content`, `remote_content`, `delete_origin`); `bare`/`From<Conflict>`/`block_id`.
- `ConflictOrigin` — `Local` / `Remote`.

**`wire` module** — bus payload schema
- `OpEnvelope { op }` / `ConflictEnvelope { conflicts }` — JSON envelopes (`to_json`/`from_json`); structs so future fields (cursor/presence) stay additive.
- `ops_topic`/`conflict_topic` and `relpath_of_topic`/`relpath_of_conflict_topic`; constants `OPS_TOPIC_PREFIX` (`com.nexus.editor.ops.`), `CONFLICT_TOPIC_PREFIX` (`com.nexus.editor.crdt.conflict.`).

**`sync` module** — inbound apply loop
- `DocHandle` — `Arc<Mutex<CrdtDoc>>` wrapper with `with_doc`/`with_doc_mut`.
- `SyncLoop` — `new`/`from_parts`, `topic`, `doc`, `apply_remote_payload`, `apply_remote_op` (drops self-echoes), async `run`.
- `InboundOutcome` — `Applied` / `SelfEcho` / `Conflict`.

**`state` module** — persistence
- `CrdtState { site, lamport, log, block_meta, rga }` — snapshot **without** the block tree (rebuilt from markdown on load).
- `PersistedCrdt { version, content_hash, persisted_at_unix, state }` — on-disk envelope; `new`.
- `PERSISTED_VERSION` (= 1), `crdt_state_path(relpath)`, `content_hash_hex(bytes)`.

**`merge` module** — Phase 2 deterministic id helpers
- `baseline_op_id(block_id, char_pos)` — deterministic synthetic `OpId` (lamport 0) for a baseline character so two peers materialise identical RGAs.
- `subop_id(envelope, char_offset)` — per-character ids for multi-char inserts (`offset 0` == envelope).
- `byte_to_char_pos(content, byte_pos)` — byte→char index translation (multibyte-safe).

## IPC handlers

None. `nexus-crdt` is a library with no plugin id and no `register`/dispatch surface; it never appears in the bootstrap plugin registration order. Everything reachable across the IPC boundary is the *editor's* surface — the CRDT layer is driven entirely through `nexus_editor::OpObserver` callbacks (mirrored by `nexus-bootstrap`'s `CrdtPublisher`) and the kernel event bus, not through `context.ipc_call`. This is by design: a new CRDT-related capability would be added as an editor or collab handler, not here.

## Capabilities

None declared or checked in this crate. Capability gating happens upstream — at the editor IPC handlers that produce transactions and at the kernel `EventBus` publish/subscribe path the publisher and collab bridge use. The CRDT types themselves are capability-agnostic plain data + algorithms.

## Settings / Config

No config types live in this crate. Knobs that govern its *use* live with the wiring:
- `CrdtPublisher::with_checkpoint_every` (default `DEFAULT_CHECKPOINT_EVERY_OPS = 32`) and the pull-landing poll cadence (`PULL_LANDING_TICK = 250ms`) are hardcoded constants in `nexus-bootstrap`'s `crdt_publisher.rs`.
- The collab relay (`relay_url`, token, peer id) is configured via `[collab]` in config and handled by `nexus-bootstrap`'s `collab.rs` / `nexus-collab`.

`PERSISTED_VERSION` in `state` is a schema version, not a user setting.

## Events

Op exchange is a publish/subscribe model over the kernel `EventBus`, but this crate only *defines the contract* and provides one side of the pump:

- **Topics** (`wire`): per-file ops channel `com.nexus.editor.ops.<relpath>` carrying an `OpEnvelope { op: CrdtOp }`; per-file conflict channel `com.nexus.editor.crdt.conflict.<relpath>` carrying a `ConflictEnvelope { conflicts: Vec<ConflictDetail> }`. Both prefixes are checked at compile time to begin with `plugin_ids::EDITOR + "."`.
- **Producer side** lives in `nexus-bootstrap`'s `CrdtPublisher`: on every `on_apply_transaction` / undo / redo it authors via `CrdtDoc::apply_local` and publishes an `OpEnvelope` (as `EDITOR_PLUGIN_ID`) on the ops topic; on a pull-landing reload it republishes absorbed ops and fires a `ConflictEnvelope` on the conflict topic.
- **Consumer side** is `sync::SyncLoop`: it subscribes (`EventFilter::CustomExact(ops_topic(relpath))`), decodes each `OpEnvelope`, drops self-echoes (`op.id.site == doc.site()`), and dispatches to `CrdtDoc::apply_remote`. `run` survives `Lagged` (logs the gap) and single-payload decode failures, exiting only on `Closed`.
- **Cross-machine relay**: `nexus-bootstrap`'s `collab` bridge subscribes to `EventFilter::CustomPrefix(OPS_TOPIC_PREFIX)` and ships every `com.nexus.editor.ops.*` event to the remote relay (`nexus-collab`), republishing inbound envelopes back onto the local bus.
- **Pull-landing trigger**: the publisher's background thread subscribes to `com.nexus.git.commit` and re-absorbs the on-disk state file after a `git pull` / merge.

## Internals & notable implementation details

**RGA algorithm (`text`).** Each visible character is a node with a unique `OpId`, a child list, and a tombstone flag; the document head is `parent == None`. Siblings sharing a parent are ordered **descending by `OpId`** — the standard RGA tiebreak: a newer author's character sorts left of an older one within the same insertion point. Insertion finds the first sibling with a smaller `OpId` and splices in before it; deletion sets the tombstone flag (the node stays so later inserts that referenced it as parent still resolve). `render` is a depth-first traversal skipping tombstones; `len`/positional lookups walk visible nodes only. `apply` is idempotent (re-inserting an existing id or re-deleting a tombstone is a no-op) and returns whether state changed — the property that makes the layer safe to drive from a gossip pipeline. The standalone `text` tests assume causal delivery (an insert whose parent isn't present yet is dropped); causal readiness is the doc layer's job.

**Operation model & causal ordering.** `CrdtOp.vv_at_creation` is the authoring site's `VersionVector` *immediately before* the op. Two ops are concurrent iff neither's id is contained in the other's `vv_at_creation`. Lamport clocks advance on every local op (`lamport.next()`) and catch up on every remote op (`lamport = max(local, remote.lamport)`), so future local ops dominate everything seen. `OpLog.append` is idempotent by id and maintains a cached `VersionVector` summary plus a `pruned_floor` so compacted ids still report as "seen".

**Commutativity / idempotency / convergence.** Convergence rests on three facts: `OpLog.append` is idempotent by id; `RgaText.apply` is idempotent and order-independent (sibling order is a pure function of `OpId`); and `OpLog.merge` is an idempotent union. The `doc` and `text` test suites assert convergence across multiple replay permutations and 2-/3-site interleavings.

**Editor-op integration & silent merge (`doc`).** `apply_local` ticks the lamport, mints an `OpId`, translates text ops to position-free `RgaTextOp`s *against pre-op content* (so byte→char positions resolve correctly), applies the editor op to the tree, mirrors the RGA, updates `BlockMeta`, and appends to the log — returning the wire `CrdtOp`. `apply_remote` first checks idempotency, then conflict. For a **concurrent text op** on a block (remote's VV doesn't contain the local last-writer), it ignores the editor op's stale byte position entirely, replays the carried `rga_ops` on the local RGA, and rebuilds `block.content` from `rga.render()` — the Phase 2 silent merge. For causally-ordered or non-text ops it applies the editor op to the tree directly and mirrors RGA-affecting ops. Conflict detection surfaces `StructuralDeleteEdit` (delete racing an edit, symmetric in both directions, takes precedence) and `ConcurrentBlockEdit` (concurrent `UpdateBlockContent`/`UpdateAnnotations` whole-block replacements the RGA can't merge); pure concurrent text never surfaces.

**Deterministic synthetic ids (`merge`).** A fresh `CrdtDoc` eagerly materialises a per-block RGA from baseline content using `baseline_op_id` (UUID v5 over a fixed namespace + block id + char index, lamport fixed at 0 so any real op sorts after baseline chars). Two peers building a `CrdtDoc` from equal `BlockTree` content thus produce *identical* RGAs, which is what lets gossiped concurrent ops converge. Multi-char inserts derive per-character ids via `subop_id` (offset 0 reuses the envelope id, keeping the wire op self-identifying).

**Persistence (`state`, `.editor/crdt/`).** The persisted snapshot deliberately omits the block tree (file-as-truth: it's reparsed from markdown on load) and stores only the delta: site, lamport, op log, block meta, and per-block RGA mirror. On disk at `<forge>/.forge/.editor/crdt/<sha-of-relpath>.json` (relpath hashed to 8 bytes / 16 hex chars, matching the BL-072 undo-state convention). `PersistedCrdt` wraps `CrdtState` with a schema version and a SHA-256 `content_hash` of the canonical markdown; on open, a hash mismatch means the markdown changed externally so the cached log is discarded rather than mis-replayed. The pull-landing path (`reload_after_external_change` in bootstrap) deliberately *skips* the hash check, since a `git pull` changes both the markdown and the state file — the op-log union stays correct because it keys on `OpId`, not byte offsets. The bundled SHA-256 (`sha256_bytes`) avoids pulling in `sha2`. Both `HashMap<OpId, …>` maps serialize as `Vec<(OpId, …)>` because JSON object keys must be strings.

**Phase status (ADR 0026).** Phase 1 = core types + op log + conflict detection; Phase 2 = the RGA silent text merge (now load-bearing in `CrdtDoc`); Phase 3 = the bus sync loop + collab transport; Phase 4 = on-disk persistence + the BL-007 git merge driver. Deferred: reparent / move-loop detection.

## Tests

All tests are inline `#[cfg(test)]` modules — there is **no `tests/` directory** in this crate (the integration-style e2e test `crdt_publisher_e2e.rs` lives in `nexus-bootstrap`). Coverage by module:

- `id` — Lamport-before-site ordering; VV observe/dominates/contains semantics.
- `op` — `primary_block_id` / `affected_blocks` for text, reparent (both parents), and root insert (no parent).
- `text` — linear and head insertion order; tombstone delete; idempotent apply; **convergence** under concurrent head inserts (2-site), and a 3-site interleaving replayed across four permutations.
- `log` — idempotent append; VV tracking; `prune_dominated` keeps `contains` truthful; prune-then-merge doesn't resurrect pruned ops; `merge` is an idempotent union; `missing_for` returns only unseen ops.
- `doc` — local apply mutates tree + log + carries one RGA op per char; remote duplicate no-op; concurrent edits to different blocks converge; **concurrent same-block text silently merges** (2- and 3-site, plus insert-at-different-positions and delete+insert cases all asserting byte-exact merged results); sequential edits after gossip don't conflict; `StructuralDeleteEdit` and `ConcurrentBlockEdit` surface as expected; JSON round-trip of `CrdtState`; `from_state` drops orphan blocks and seeds new ones.
- `state` — full `PersistedCrdt` round-trip restores an RGA that re-renders the post-edit text; merged op logs replay to convergent state; storage path is under `.forge/.editor/crdt/` with a `.json` extension and is collision-resistant; SHA-256 matches known FIPS test vectors and changes with input.
- `sync` — applies a remote op through an envelope; drops self-echo; an end-to-end `run` loop drains a shared bus and converges.
- `wire` — ops/conflict topic round-trips; `ConflictEnvelope` JSON round-trip with and without content snapshots; legacy (pre-BL-074) toast-shape payloads still decode with defaulted detail fields.
