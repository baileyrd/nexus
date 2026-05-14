# Typing Latency Plan

> Phased plan to drive perceived and measured typing latency in the Nexus
> shell editor down to a budget that scales **O(1) in document size** for the
> common case (single-character insert / delete in an open markdown tab).
>
> Companion to the deep-dive analysis on branch
> `claude/analyze-shell-typing-latency-pEzrj`. Pre-reads:
>
> - `docs/architecture/C4.md` (ADR 0011 — single active desktop target)
> - `crates/nexus-editor/src/core_plugin.rs` (`apply_transaction` hot path)
> - `shell/src/plugins/nexus/editor/cm/transactionBridge.ts` (rAF batching + mirror)
> - `experiments/perf/run.ts` (BL-112 harness)
> - `shell/src/stores/frameSnapshot.ts` (BL-110 hook)

---

## 1. Goal

A single keystroke in a markdown tab should:

1. Render the glyph in the **next browser paint** (≤ 16 ms from the keydown).
2. Complete every post-paint side effect (decoration rebuild, store notify,
   React re-render, kernel round-trip) **within the same animation frame**
   on a 500-block document and within **two frames** on a 5000-block
   document on the reference dev machine.
3. Take time that is **constant in document size** for text-only ops on
   docs up to 10,000 blocks. Today the slow path is O(N) at multiple
   layers — see §2.

This is a perceived-latency goal, not a code-aesthetic one. Every change
in this plan must be backed by a measurement from the BL-112 perf harness
(or extension thereof) before it merges.

---

## 2. Current cost map

Numbered to match the analysis on this branch. File:line are valid at
HEAD `4620a8b4` / `origin/main`.

| # | Stage | Where | Cost shape | Effect |
|---|---|---|---|---|
| 1 | Full `BlockTree` clone + `serde_json::to_value` of every block on every successful `apply_transaction` | `crates/nexus-editor/src/core_plugin.rs:1203` (via `snapshot_of`/`snapshot_to_value` at `:1538`, `:1573-1576`) | **O(N blocks)** | Largest single cost. Both clone and serialize walk every block. The webview ignores the payload for text-only ops (`transactionBridge.ts:362-389` `skipReconcile`) but the kernel still produces and ships it. |
| 2 | `buildLivePreviewDecorations` walks full Lezer syntax tree on every `docChanged` *and* every selection change | `shell/src/plugins/nexus/editor/cm/livePreviewDecorations.ts:141-153`, `cm/livePreview.ts:30-43` | **O(N nodes)** pre-paint | Runs before CM paints. For large docs this directly delays the glyph. |
| 3 | `useEditorStore((s) => s.tabs)` re-renders the whole `EditorView` subtree per keystroke | `shell/src/plugins/nexus/editor/EditorView.tsx:172`; mutation site `editorStore.ts:375-380` | O(K subscribers) per keystroke | Re-renders `EditorView`, `TabBody`, `ViewHeader`, `BreadcrumbSegments`, `ModeToggle`. Dev StrictMode doubles it. BL-110's `useFrameSnapshot` exists; not yet wired here. |
| 4 | Sessions mutex held across mutation + clone + serialize | `crates/nexus-editor/src/core_plugin.rs:1183-1205` | Lock span ∝ #1 | Concurrent edits to different open files block on the same lock. Mooted by fixing #1 for the common case but worth tightening. |
| 5 | Redundant `serde_json::to_vec(&tx_value)` purely to measure the 16 MiB cap | `crates/nexus-editor/src/core_plugin.rs:1169-1175` | Constant per call (small) | Pure waste; the value is already deserialized once by Tauri. |

### 2.1 Out-of-path but worth noting

- BL-053 (commits `eefde183`, `165e0b1f`, `ec82d94e`) expanded
  `markdownRender.ts` significantly. This is called per-keystroke **only**
  in preview mode and per-rebuild for live-mode `TableWidget`
  (`livePreviewDecorations.ts:31`). Source/live mode typing does not hit
  it. No action proposed here, but Phase 3's viewport scoping will
  also cut redundant `TableWidget` rebuilds.

---

## 3. Success criteria

The perf harness (`experiments/perf/run.ts`) gates the plan. Add the
following scenarios — they map 1:1 to the deferred entries BL-112
already scaffolds in its JSON schema:

| Scenario | Doc shape | Metric | Target |
|---|---|---|---|
| `typing.small` | 50 markdown blocks, no tables, no fenced code | p95 keystroke → next-frame paint (webview `performance.measure`) | < 16 ms |
| `typing.medium` | 500 blocks, ~5% tables, ~10% fenced code | p95 keystroke → paint | < 16 ms |
| `typing.large` | 5000 blocks | p95 keystroke → paint | < 33 ms (two frames) |
| `typing.rust.apply_tx` | as above, per-doc-size | p95 `apply_transaction` server-side wall time (tracing span) | constant in N for text-only ops |

Acceptance for each phase below references these scenarios. Until the
WDIO-Tauri runner exists (deferred per BL-112), Phase 0 establishes a
proxy via direct microbenchmarks; the runtime scenarios merge in when
the runner does.

---

## 4. Phases

Ordered by leverage × cost. Phase 1 is the single biggest fix and should
ship first; the rest are independent and can land in any order after.

### Phase 0 — Measurement scaffold (BL-122)

**Why first.** Every later change needs a baseline number to gate on, and
the harness already has a slot for it.

**Scope.**

1. Add three direct microbenchmarks to `experiments/perf/run.ts`:
   - `editor.apply_transaction.small/medium/large` — drives the editor
     core directly through a `nexus-editor` integration test that boots
     a minimal kernel (no Tauri), opens a synthetic doc, and times 100
     single-char `apply_transaction` calls. Reports p50/p95/p99 + per-op
     mean clone bytes and serialize bytes.
   - `editor.livePreview.decorate.small/medium/large` — calls
     `buildLivePreviewDecorations` against a CM `EditorState` built from
     a synthetic doc (no DOM needed; this is the pre-paint cost).
   - `editor.markdownRender.large` — calls `renderMarkdown(content)` on
     a 5k-block doc to track BL-053 regression risk in preview/table
     widget paths.
2. Add a `tracing::info_span!("apply_transaction", relpath, op_count,
   bytes_in, bytes_out)` around `handle_apply_transaction`
   (`core_plugin.rs:1150-1211`). Span uses `tracing_subscriber` filter
   so it's a no-op when the harness isn't running.
3. Commit a baseline JSON to `experiments/perf/baselines/` at the date
   of merge.

**Files.**

- `experiments/perf/run.ts` — new scenarios
- `crates/nexus-editor/src/core_plugin.rs:1150` — instrument span
- `crates/nexus-editor/tests/perf_apply_transaction.rs` (new) —
  integration harness consumed by `run.ts`
- `experiments/perf/baselines/<date>.json` — new

**Acceptance.**

- `pnpm -F nexus-shell perf:run` (or wherever the harness lives) emits
  the four new scenario sections in stable-sorted JSON.
- Microbench numbers show the asymptotic shape we expect (N-linear today)
  so the Phase-1 win is visible as a flat curve.

**Risk.** Low. Pure additive instrumentation; no production behavior
change.

---

### Phase 1 — Slim `apply_transaction` response for text-only ops (BL-123)

**Why this is the single biggest win.** The JS bridge already
short-circuits the snapshot for text-only ops (`transactionBridge.ts:362-367`,
`skipReconcile`), but the kernel still produces a full snapshot, clones
the tree, and serializes the whole thing through Tauri IPC. Returning
just the revision flips this from O(N) to O(1).

**Scope.**

1. Introduce a response envelope on the kernel side:
   ```rust
   #[derive(Serialize)]
   #[serde(tag = "kind", rename_all = "snake_case")]
   enum ApplyTransactionResponse {
       Slim { revision: u64 },
       Full(EditorSnapshot),
   }
   ```
2. In `handle_apply_transaction`, inspect the operations after a
   successful apply:
   - All ops in `{ insert_text, delete_text, update_annotations }`
     → emit `Slim`.
   - Otherwise → emit `Full` (preserves current behavior for structural
     edits, paste, AI rewrites, etc.).
3. Update `EditorKernelClient.applyTransaction`
   (`shell/src/plugins/nexus/editor/kernelClient.ts`) to type the
   response as a discriminated union.
4. In `transactionBridge.dispatchTransaction`
   (`shell/src/plugins/nexus/editor/cm/transactionBridge.ts:355-418`):
   - `Slim`: bump `setSessionRevision`, drop the existing
     `setSnapshot?.(...)` path. The optimistic mirror has already
     advanced; no kernel state is needed.
   - `Full`: existing behavior (push snapshot to session manager,
     maybe reconcile).
5. Update IPC drift checks: `scripts/check_ipc_drift.sh` regenerates
   bindings in `packages/nexus-extension-api/src/generated/ipc/` and
   `crates/nexus-bootstrap/schemas/ipc/`.

**Files.**

- `crates/nexus-editor/src/core_plugin.rs:1150-1211` — handler change
- `crates/nexus-editor/src/lib.rs` (or wherever `EditorSnapshot` lives)
  — response enum
- `shell/src/plugins/nexus/editor/kernelClient.ts` — JS type update
- `shell/src/plugins/nexus/editor/cm/transactionBridge.ts:355-418` —
  branch on the response
- `packages/nexus-extension-api/src/generated/ipc/*.ts` — regenerated
- `crates/nexus-bootstrap/schemas/ipc/*.json` — regenerated
- New tests:
  - `crates/nexus-editor/tests/apply_transaction_response.rs` — text-only
    op returns `Slim`, structural op returns `Full`
  - `shell/tests/transactionBridge-slim-response.test.ts` — bridge
    handles both shapes correctly, optimistic mirror remains coherent

**Acceptance.**

- `editor.apply_transaction.large` p95 server-side time is flat in N
  for text-only ops (target: < 1 ms for any doc size).
- `typing.large` p95 keystroke → paint < 33 ms (paint should land in
  one frame on a warmed-up runtime; budget for two as a safety margin).
- No regression in `editor.apply_transaction.*` for structural-op
  scenarios (those still take the `Full` path).
- IPC drift check passes.

**Risk.** Medium.

- The optimistic mirror in the bridge already advances on dispatch
  (`transactionBridge.ts:464-465`), so the JS side doesn't depend on
  the snapshot for next-translation correctness in the text-only case.
  But: external consumers of the snapshot (the session manager's
  `setSnapshot` is called by `dispatchTransaction`, fed downstream to
  drag-bridge, comments, etc.) need to keep getting refreshes. The
  proposed change drops `setSnapshot` on the slim path. **Mitigation:**
  either keep a periodic refresh (e.g., `get_tree` every Nth slim op)
  or have downstream consumers refetch via `get_tree` on demand.
- The pre-existing `update_annotations` op is included in the slim
  whitelist because text-only ops can carry pre-annotations
  (`transactionBridge.ts:362-367`); verify no observer relies on the
  full snapshot to react to annotation shifts.

---

### Phase 2 — `useFrameSnapshot` adoption in `EditorView` (BL-124)

**Why.** Bottleneck #3. The infrastructure shipped in BL-110; only the
reference site (`FileStats.tsx`) uses it today. Wiring it into the
editor flattens 4 separate Zustand notifies per keystroke into one
rAF-coalesced tuple and lets selectors actually shed irrelevant tabs.

**Scope.**

1. Replace the broad `useEditorStore((s) => s.tabs)` at
   `EditorView.tsx:172` with a `useFrameSnapshot` over a narrowed entry
   set:
   - active tab content / mode / loading / error
   - active relpath
   - dirty flag for the active tab
2. Apply the same pattern to `TabBody`, `ViewHeader`, `BreadcrumbSegments`,
   and `ModeToggle` so each component subscribes only to what it reads.
3. Refactor `editorStore.setContent` (line 375-380) to keep the per-tab
   identity stable when only content changes — i.e. mutate the matched
   tab object's content via a fresh object but leave non-matched tab
   refs intact. The current `s.tabs.map(...)` already does this; verify
   by snapshot test that non-target subscribers don't see a change.
4. Confirm StrictMode no longer doubles editor renders by inspecting
   React DevTools profile.

**Files.**

- `shell/src/plugins/nexus/editor/EditorView.tsx` — primary
- `shell/src/plugins/nexus/editor/editorStore.ts` — identity-stability
  tests
- `shell/tests/editor-view-rerender.test.ts` (new) — assert single
  EditorView render per keystroke

**Acceptance.**

- Render-count test: typing 10 characters into the active tab produces
  ≤ 10 `EditorView` re-renders (one per frame; coalesced) and zero
  re-renders for non-active-tab components.
- `typing.small` p95 sees a measurable improvement on the
  pre-paint-after side (where React re-render time lives).

**Risk.** Low/medium. The `useFrameSnapshot` API is small and tested
(BL-110 shipped 7 unit tests). Risk is in missing a subscriber and
breaking a derived UI piece — covered by snapshot tests on each
extracted slice.

---

### Phase 3 — Viewport-scoped live-preview decorations (BL-125)

**Why.** Bottleneck #2. `buildLivePreviewDecorations` walks the full
syntax tree pre-paint on every doc change and on every selection
change. For a 10k-line doc the syntax tree easily has 50k nodes; this
is the highest-variance cost in the typing path.

**Scope.**

1. Replace the unconditional `tree.iterate({...})` walk
   (`livePreviewDecorations.ts:147-151`) with a viewport-scoped walk
   using CM's `view.visibleRanges` — but the decoration set is a
   `StateField`, which doesn't have direct view access. Solutions:
   - Promote to a `ViewPlugin` that watches viewport changes and
     dispatches a `StateEffect` carrying the new visible ranges, which
     the `StateField` consumes. (CM's block-decoration restriction is
     the original reason we use a `StateField` — confirm via the
     `Decoration.block: true` table-widget path whether ViewPlugin
     would still work for inline decorations + a separate `StateField`
     for table block decorations only.)
   - Alternatively keep the `StateField` and use the `EditorState`'s
     viewport-iteration helpers if available, falling back to the full
     walk only for the part of the tree that intersects the viewport.
2. On `tr.docChanged`, iterate `tr.changes.iterChangedRanges` and walk
   only nodes that intersect those ranges, *unioned with* the viewport.
3. On selection-only changes, compute the delta in active-line set
   versus the prior selection, and only rebuild marks/replaces for lines
   that entered or left the active set.
4. Keep `EditorView.atomicRanges.of(...)` (`livePreview.ts:47`) wired so
   cursor motion still respects atomic ranges.

**Files.**

- `shell/src/plugins/nexus/editor/cm/livePreview.ts` — orchestration
- `shell/src/plugins/nexus/editor/cm/livePreviewDecorations.ts` —
  scoped walk
- `shell/tests/livePreview-viewport-scope.test.ts` (new) — assert
  decoration count and walk shape for synthetic 10k-line doc with a
  100-line viewport

**Acceptance.**

- `editor.livePreview.decorate.large` p95 ≤ p95 of `.small` (cost
  becomes viewport-bounded, not doc-bounded).
- Visual snapshot tests pass: emphasis/strong/inline-code/headings all
  still render correctly across viewport edges and on selection
  changes.
- No regression in `TableWidget` rebuild semantics — table widgets are
  block decorations and may need a separate code path; confirm before
  and after.

**Risk.** Medium. The `StateField` ↔ `ViewPlugin` split for inline vs
block decorations is the trickiest invariant. The existing module
header explicitly calls out that block decorations *must* come from a
`StateField`. If we can't cleanly split, fall back to scoping the walk
*inside* the `StateField` by reading the previous decoration set and
the `tr.changes` change spans — which still wins for `docChanged` but
not for selection-only updates.

---

### Phase 4 — Drop the redundant size-cap serialize + tighten the session lock (BL-126)

**Why.** Bottlenecks #4 and #5. Both small per-call costs that compound
under concurrent edits. Cheap to fix.

**Scope.**

1. Remove `serde_json::to_vec(&tx_value)` at
   `core_plugin.rs:1169-1175`. Replace with a bound check on the
   already-deserialized `Transaction`:
   - Sum of `op.text.len()` for `InsertText`
   - Sum of `op.deleted_text.len()` for `DeleteText`
   - Sum of inserted block content lengths for `InsertBlock`
   - Bound at 16 MiB (existing cap) so the test against malformed
     pathologically-large payloads still trips.
2. Restructure `handle_apply_transaction` so the session mutex is
   released **before** `snapshot_to_value` runs. Today the lock spans
   line 1183-1205; the snapshot serialization happens inside the
   scope. Move serialization out by collecting `(snapshot, rev, ops)`
   under the lock and serializing afterward.
   - Note: with Phase 1's slim path, `snapshot_to_value` is only
     called on the structural-op slow path, so this matters mainly for
     concurrency on big edits (paste, AI rewrite).

**Files.**

- `crates/nexus-editor/src/core_plugin.rs:1146-1211` only

**Acceptance.**

- No throughput regression on the perf harness.
- Add a multi-relpath concurrency test that exercises two sessions
  being mutated simultaneously; assert overlapping spans in tracing
  output.

**Risk.** Low. Both changes are local to one handler.

---

### Phase 5 — Runtime perf scenarios (BL-127, depends on WDIO-Tauri runner)

**Why.** Microbenchmarks gate the kernel path; they miss IPC, Tauri
serialization, webview paint, and React re-render. The original BL-112
DoD lists `typing.5char` as a deferred runtime scenario; this phase
fills it in.

**Scope.**

1. Stand up a WDIO-Tauri runner (this is the prerequisite BL-112 named).
   Tracked separately under whichever existing BL covers the runner;
   reference it here.
2. Add `typing.small/medium/large` scenarios that:
   - Boot the shell against a synthetic forge
   - Open a markdown tab with the target doc shape
   - Programmatically dispatch 100 keydown events
   - Measure via `performance.mark`/`performance.measure` from a hook in
     `EditorView.tsx` (`startMark` on input event, `endMark` on
     subsequent paint via `requestAnimationFrame` inside
     `requestAnimationFrame`).
3. Wire these into CI as a non-blocking PR comment first; promote to
   gate after a stabilization window.

**Files.**

- `experiments/perf/run.ts` — new scenario block
- `experiments/perf/scenarios/typing/*.ts` — fixture docs + driver
- A small instrumentation hook in `shell/src/plugins/nexus/editor/EditorView.tsx`
  gated by an env flag (`NEXUS_PERF_TYPING=1`)

**Acceptance.**

- Scenarios produce stable numbers across 3 consecutive runs (variance
  < 10% on the reference machine).
- A regression in any of Phases 1–4 surfaces as a CI delta.

**Risk.** Depends on the WDIO-Tauri runner which is itself non-trivial.
Phase 5 lands after that prerequisite, not before.

---

## 5. Sequencing rationale

```
Phase 0  (measurement)        — prerequisite for everything
   │
   ├─► Phase 1  (slim response)         ← biggest single win, ship first
   │
   ├─► Phase 2  (useFrameSnapshot)      ← independent of 1, parallelizable
   │
   ├─► Phase 3  (viewport decorations)  ← independent of 1/2
   │
   └─► Phase 4  (lock + size cap)       ← independent; small but cheap
        │
        └─► Phase 5  (runtime scenarios) ← gated on WDIO-Tauri runner
```

Phase 1 alone is expected to deliver the bulk of the visible improvement
for large documents. Phases 2 and 3 chip away at the pre-paint window
(which Phase 1 doesn't touch). Phase 4 is a small cleanup that becomes
trivial once Phase 1 lands.

---

## 6. Open questions

1. **`update_annotations` slim eligibility.** The optimistic mirror
   doesn't currently track annotation changes
   (`transactionBridge.ts:556-562` comment says "silent skip"). If we
   put `update_annotations` on the slim path, downstream consumers
   (margin glyphs, comments) won't get a snapshot refresh. Either
   exclude it from the slim path or refresh annotations via a separate
   lightweight IPC. **Default in this plan:** exclude from slim;
   structural-feeling op kinds stay on the Full path.
2. **Periodic `get_tree` refresh for slim-path consumers.** With Phase
   1, the session manager's snapshot for an actively-edited file goes
   stale until a `Full` op happens. If anything outside the bridge
   (drag-bridge, block-link nav, BL-049 backlinks) reads the snapshot
   expecting freshness, we need either an on-demand refresh or a
   debounced background refetch. Audit consumers before landing
   Phase 1.
3. **`StateField` vs `ViewPlugin` for live-preview decorations.** CM6
   disallows block decorations from `ViewPlugin`. Phase 3 needs a
   concrete answer: split inline (ViewPlugin) from block (StateField)?
   Pass viewport into the StateField via an effect? Pick the answer
   when Phase 3 starts; flag as a design decision in its PR.

---

## 7. Out of scope

- The BL-053 `renderMarkdown` expansion. It's bigger than it was, but
  not in the source/live-mode keystroke path. If preview-mode typing
  becomes a goal, file separately.
- Tauri's Linux JSON-string IPC encoding. Slimming the response (Phase
  1) removes the symptom; replacing the encoding is a much larger
  cross-cutting change tracked elsewhere.
- The kernel's own `BlockTree` representation. Switching to a
  copy-on-write tree would mean `snapshot_of` is O(1) even on the slow
  path; that's worth doing eventually but is out of scope for the
  typing-latency goal once Phase 1 lands.
- Synchronous CRDT observer (`core_plugin.rs:1206-1208`). Already runs
  outside the lock; no current evidence it's on the critical path.

---

## 8. Tracking

| BL | Phase | Owner | Status |
|---|---|---|---|
| BL-122 | 0 — measurement scaffold | unassigned | proposed |
| BL-123 | 1 — slim apply_transaction | unassigned | proposed |
| BL-124 | 2 — useFrameSnapshot in EditorView | unassigned | proposed |
| BL-125 | 3 — viewport-scoped decorations | unassigned | proposed |
| BL-126 | 4 — size-cap + lock tightening | unassigned | proposed |
| BL-127 | 5 — runtime perf scenarios | unassigned | proposed (gated) |

When each phase ships, file under `docs/PRDs/BACKLOG_COMPLETED.md` with
a one-paragraph close note linking back here and to the perf delta.
