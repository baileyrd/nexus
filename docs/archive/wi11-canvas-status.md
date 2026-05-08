> **Archived 2026-04-26** — Point-in-time validation audit (2026-04-23) of the `nexus.canvas` plugin. Useful as history; the live Canvas plugin has evolved past this snapshot.

# WI-11 Canvas — Validation Audit

**Source plan:** docs/planning/PHASE-2-IMPLEMENTATION-PLAN.md §4.3
**Sub-plan:** docs/canvas-shell-plan.md
**Audited against:** shell/src/plugins/nexus/canvas/, crates/nexus-storage/src/{core_plugin.rs,canvas.rs,lib.rs}
**Date:** 2026-04-23
**Auditor:** validation-only, static analysis (no runtime tests run)

## 1. Plugin overview

The `nexus.canvas` shell plugin (5,322 LOC across 13 files under `shell/src/plugins/nexus/canvas/`) registers a `'canvas'` view type and the `.canvas` extension. `CanvasView.tsx` (1,474 LOC) drives a `<canvas>` 2D renderer (`renderer.ts`, 954 LOC) for chrome and a parallel DOM overlay (`CanvasOverlay.tsx`, 1,089 LOC) for node bodies (markdown, OG cards, file previews, base mini-grids, terminal transcripts). All structural mutations route through the kernel's `canvas_patch` op stream; chrome (Inspector / Minimap / control strip) is overlay-tagged for export filtering.

## 2. Phase-by-phase status matrix

| Phase | Title | Status | Code paths | IPC handlers used | Tests | Closing work |
|-------|-------|--------|------------|-------------------|-------|--------------|
| 1 | View registration + blank surface | done | `index.ts:33-114`, `CanvasPaneView.tsx`, `CanvasView.tsx` mount path | `canvas_read` | none | — |
| 2 | Renderer + camera | done | `renderer.ts` (954 LOC), `CanvasView.tsx:265-298` (wheel/zoom/pan) | none (read-only) | none | — |
| 3 | Interactions (select / move / resize / create / delete / undo) | done | `CanvasView.tsx:241-774`, `canvasStore.ts:157-181` (history) | `canvas_patch` | none | — |
| 4 | Edges + inspector | done | `Inspector.tsx` (439 LOC), edge hit-test in `CanvasView.tsx`/`renderer.ts` | `canvas_patch` (`edge_*` ops) | none | Multi-select node editing intentionally out of scope (sub-plan §Phase 4). |
| 5 | Node body embeds | done | `CanvasOverlay.tsx` mounts text/file/link/database/terminal bodies | `read_file`, `base_load`, `com.nexus.linkpreview::fetch`, `com.nexus.terminal::{create_session,send_input,read_raw_since,close_session}` | none | — |
| 6 | Polish (minimap, tidy, export, grid, shortcuts, background) | done | `Minimap.tsx`, `autoLayout.ts`, `exportPng.ts`, `exportFormats.ts`, `index.ts` keybinding contributions | `canvas_patch` (`set_background`) | none | PNG-overlay caveat: `exportPng.ts` is 2D-only; `exportFormats.ts` uses `html-to-image`/`jspdf` for overlay-inclusive output. **Concurrency model — see §3.** |

All six phases ship code that does what the plan claims. The sole structural risk is the patch path; UI features are functionally complete.

## 3. canvas_patch debounce — concurrent edit safety

Plan §4.3 explicitly calls out "edge cases around `canvas_patch` debounce and concurrent-edit scenarios." This audit concludes the **debounce is not implemented in the shell, the kernel comment about it is stale, and the patch model is last-write-wins with no revision check.**

### Debounce — claimed vs actual

- **Sub-plan §Persistence (`canvas-shell-plan.md:294-296`)**: "Debounce `canvas_patch` calls — one flush per ~300 ms of idle, plus a flush on blur / close / save."
- **Kernel doc (`crates/nexus-storage/src/lib.rs:410-411`)**: "the shell debounces patch flushes so this is called once per idle burst rather than per frame."
- **Actual shell behaviour**: `commit()` in `CanvasView.tsx:247-259` is fire-and-forget — every call invokes `client.patch(relpath, forward)` immediately and discards the promise (`void clientRef.current.patch(...).catch(...)`). There is no setTimeout, no debounce wrapper, no idle queue.
- **Saving grace**: drag operations DO coalesce into a single `node_move` patch on `pointerup` (`CanvasView.tsx:715-728`) — start positions are captured at `pointerdown`, deltas accumulate in local doc state during the drag, and one patch fires when the gesture ends. So the "rewrite the file per frame" worst case the plan worried about is avoided structurally rather than via debounce — but every other edit (single click delete, inspector text change, double-click create) still hits the kernel synchronously.
- **Net:** the plan's debounce-with-flush-on-close design is unimplemented. The actual behaviour is "one network round trip per coalesced gesture" which is fine for low-rate edits but offers no batching for keyboard-driven rapid edits (e.g. Inspector typing — see Inspector.tsx for whether there's any local debounce).

### Concurrency / race window

The kernel's `patch_canvas` (`crates/nexus-storage/src/lib.rs:419-430`) is a textbook check-then-set:

```
let mut canvas_file = self.read_canvas(path)?;
canvas::apply_patch(&mut canvas_file, ops)?;
self.write_canvas(path, &canvas_file)
```

There is **no exclusive lock around read+write**, **no etag**, **no revision number**, and **no compare-and-swap**. `apply_patch` itself (`canvas.rs:355-404`) treats unknown-id removes/moves/updates as no-ops "so that optimistic client-side state that races a concurrent save doesn't crash the patch" — a tacit acknowledgement of the race — but two concurrent moves on the same node will execute as last-writer-wins with no warning.

The shell makes this worse by firing patches via fire-and-forget (`void clientRef.current.patch(...)`); a slow first patch can land **after** a faster second patch that was issued later, producing apparent state regression. There is no retry, no ordering guarantee, no in-flight queue.

### Two-tab / two-process race

Open the same `.canvas` in two leaves (or one in Nexus, one in Obsidian). Tab A drags node X; Tab B drags node Y. Both patch within the same idle window. Outcome: whichever write hits disk last wins; the loser's edit is silently lost because `read_canvas` in tab-B's patch happened before tab-A's `write_canvas` completed. There is no file-watcher reload of `canvasStore.doc` after a save either, so tab A still shows X moved even though disk now has Y moved at X's old position.

**Closing work for §3:** ~2 days for a defensible fix:
1. Add a 250 ms trailing-edge debounce around `commit` in `CanvasView.tsx`, plus a flush on the `unmount` and `pagehide` lifecycle hooks (~0.5d).
2. Add a `revision: u64` field to `CanvasFile` (Nexus extension), bump on every `write_canvas`, expose in the `read` response, and reject `canvas_patch` with `If-Match: revision` mismatch — shell on mismatch reloads + replays its in-flight patches against the new doc (~1.5d).

Both are necessary to claim §4.3 acceptance honestly. Option 1 alone is the minimum viable.

## 4. Live-data round-trip

A test fixture `fixtures/forge/Workspace.canvas` exists. No automated test runs against it.

- **Shell test surface:** zero. `find shell -name "*.test.*" -path "*canvas*"` returns nothing. The plugin has no vitest specs.
- **Kernel test surface:** `crates/nexus-storage/src/canvas.rs` has 19 `#[test]` functions covering parse / serialize / `apply_patch` semantics (round-trip, duplicate-id error, unknown-id no-op, edge incident-removal, set_background round-trip, node_update replace).

What's exercised by the kernel tests:
- `canvas::parse_canvas` / `serialize_canvas` round trip.
- Each `CanvasPatchOp` variant against `apply_patch` in isolation.
- Background field round-trip through serde.

What's **not** exercised anywhere:
- Mounting a real `.canvas` and asserting render → drag → save → reload converges (no headless DOM fixture).
- `CanvasOverlay`'s five node body renderers against real markdown, real OG fetches, real file bytes, real base records, real PTY output.
- Minimap viewport sync after camera changes.
- `exportPng.ts` and `exportFormats.ts` end-to-end (image assertions).
- Auto-layout (`autoLayout.ts`) determinism / convergence on a known graph.
- Concurrent-patch race (would need two-process harness).
- Undo/redo across a session-save boundary (sub-plan §canvas-shell-plan.md:297-298 says "ctrl+z after save prompts before rewinding" — no such prompt exists in `CanvasView.tsx`; **another small gap**).

**Manual smoke path** (what an operator would do): open `fixtures/forge/Workspace.canvas`, drag a node, watch for the patch in DevTools network/IPC log, reload the leaf, confirm the node stayed put. Then create a text node by double-click, retype its body in the Inspector, undo twice, redo once, save (close + reopen), assert state. Then test export PNG/SVG/PDF and visually compare.

**Closing work for §4:** ~2 days. Vitest harness with a `MockKernel` that records `canvas_patch` ops and replays them through `applyPatchOps` to assert round-trip. Plus a Playwright smoke against the dev server running the fixture (~1d). The undo-after-save prompt is ~0.25d.

## 5. IPC coverage matrix

| ID | Handler | Wired in plugin? | Where (file:line) |
|----|---------|------------------|-------------------|
| 35 | `canvas_read` | yes | `kernelClient.ts:151-153`, called from `CanvasView.tsx` mount path |
| 36 | `canvas_write` | **no** | typed in kernel (`core_plugin.rs:507-518`), **no shell wrapper, no callers** |
| 37 | `canvas_patch` | yes | `kernelClient.ts:154-160`, called from `CanvasView.tsx:257, 788` (commit + undo/redo) |
| 38 | `canvas_nodes` | **no** | kernel-only |
| 39 | `canvas_edges` | **no** | kernel-only |

Adoption rate: 2 of 5. The unused three are not necessarily bugs — `canvas_patch` is the canonical edit path and the kernel rewrites the file each call, so `canvas_write` is redundant for the shell, and the SQLite-backed `canvas_nodes`/`canvas_edges` are intended for graph-view aggregation rather than per-canvas rendering. But they should either be deleted from the IPC surface or documented as graph-view-only.

Cross-plugin handlers used: `read_file` (storage), `base_load` (storage), `com.nexus.linkpreview::fetch`, `com.nexus.terminal::{create_session, send_input, read_raw_since, close_session}` — all wired via `kernelClient.ts:149-209`.

## 6. Cross-cutting findings + closing work

The canvas plugin is **feature-complete against the sub-plan** — every Phase-1-through-6 item ships, plus the second-pass Phase-6 closers (overlay-inclusive export, per-canvas background, palette-routed shortcuts). The validation gaps are:

- **No debounce on `canvas_patch`** (§3) — sub-plan and kernel doc both claim it; reality is fire-and-forget per gesture. Coalesced drags save the worst-case but leave keyboard/Inspector edits unbatched. ~0.5d to add.
- **No concurrency safety** (§3) — last-write-wins with no revision check; two-tab and external-editor races silently lose edits. ~1.5d to add a revision field + reject-and-reload protocol.
- **No automated tests on the shell side** (§4) — kernel has 19 unit tests; shell has zero. ~2d for vitest + a single Playwright smoke.
- **Three of five `canvas_*` handlers unused by the shell** (§5) — `canvas_write` is redundant given `canvas_patch`'s read-modify-write; `canvas_nodes`/`canvas_edges` are presumably for the graph view. Either delete or label "graph-view only." ~0.25d.
- **Save-boundary undo prompt missing** (§4) — sub-plan §Persistence says "ctrl+z after save prompts before rewinding"; not implemented. ~0.25d.

**Total estimated closing effort:** ~4.5 person-days against §4.3's "M ~1 week" budget. Confidence **medium-high** — debounce is straightforward; concurrency safety is a small protocol change but needs a kernel field migration. The plan budget is realistic if the team is willing to accept "one revision counter" as the concurrency model.

## 7. Open questions

1. Should `canvas_write` survive as an explicit "save current in-memory doc" path (useful when the shell wants to persist a transient state), or be removed?
2. Is the file watcher meant to trigger `canvasStore.setDoc` on external file changes? `CanvasView.tsx` has no `files:changed` subscription that I could find — Obsidian-side edits won't reload an open Nexus tab. Worth confirming whether this is intentional (avoid overwriting unsaved local edits) or a gap.
3. Is the `revision`-number approach acceptable, or does the team prefer a CRDT-style merge (much higher cost)? The plan only requires "concurrent-edit scenarios are handled" — not "perfectly merged."
4. Does `canvas_nodes` / `canvas_edges` actually feed the graph view, or is it dead code? If dead, deleting it shrinks the IPC attack surface for free.
