> **Archived 2026-04-26** — Point-in-time audit (2026-04-23) of the editor-transaction wiring phases. Phases are substantially complete; current architecture lives in [`editor-transaction-architecture.md`](../architecture/editor-transaction-architecture.md).

# Editor Wiring Plan — Phase Status Audit

**Source plan:** docs/editor-transaction-wiring-plan.md
**Audited against:** shell/src/plugins/nexus/editor/, shell/src-tauri/src/, crates/nexus-editor/src/
**Date:** 2026-04-23
**Audience:** WI-03 implementation owner

## 1. Plan summary (one paragraph)

The plan replaces the legacy `<textarea>` editor with a CodeMirror 6 surface whose every keystroke is dispatched through `com.nexus.editor::apply_transaction`, making the Rust `BlockTree` + `UndoTree` the single authoritative source of document state. Eight numbered phases stage the migration: deps + scaffolding (0), a typed kernel client (1), CM mount with markdown rendering (2), refcounted session lifecycle (3), a kernel→shell change-event bus (4), the load-bearing CM↔kernel transaction bridge with rAF batching, kernel-routed undo, and echo suppression via per-transaction UUIDs (5), persistence rewired through the kernel `save` (6), downstream subscribers (outline, backlinks) on the change bus rather than tab.content (7), and final cleanup + docs (8). Block identity is coarse-grained in v1 (root block) and snapshot reconciliation is full-doc.

## 2. Phase-by-phase status matrix

| # | Title | Status | Code paths | Tests | Closing work |
|---|-------|--------|-----------|-------|--------------|
| 0 | Dep + scaffolding | done | shell/package.json:20-25; shell/src/plugins/nexus/editor/cm/index.ts | n/a | none |
| 1 | Kernel client | done | shell/src/plugins/nexus/editor/kernelClient.ts:33-111; shell/src/plugins/nexus/editor/types.ts | shell/src/plugins/nexus/editor/kernelClient.test.ts (145 lines) | none |
| 2 | CM mount + markdown render | done | shell/src/plugins/nexus/editor/cm/CodeMirrorHost.tsx:66-176; shell/src/plugins/nexus/editor/cm/extensions.ts:52-104; shell/src/plugins/nexus/editor/EditorView.tsx:727-792 | none for the host (covered indirectly via bridge tests) | add a CodeMirrorHost unit test for value/onChange round-trip + readOnly compartment toggle |
| 3 | Session lifecycle | done | shell/src/plugins/nexus/editor/sessionManager.ts:101-217; shell/src/plugins/nexus/editor/MarkdownView.tsx:74-103; shell/src/plugins/nexus/editor/index.ts:166-174,883-897; crates/nexus-editor/src/core_plugin.rs:86,543-554 (HANDLER_GET_MARKDOWN added) | shell/src/plugins/nexus/editor/sessionManager.test.ts (304 lines); crates/nexus-bootstrap/tests/editor_ipc.rs:336-403 | none |
| 4 | Event bus push | done | crates/nexus-editor/src/core_plugin.rs:47,415-607 (publish_changed); shell/src/plugins/nexus/editor/sessionManager.ts:149-179,266-287; shell/src/plugins/nexus/editor/editorStore.ts:54-78,266-281 | crates/nexus-editor/src/core_plugin.rs:1063-1183 (in-crate); crates/nexus-bootstrap/tests/editor_ipc.rs:405-481 (e2e on kernel bus) | none |
| 5 | Transaction bridge | done | shell/src/plugins/nexus/editor/cm/transactionBridge.ts:264-398; shell/src/plugins/nexus/editor/cm/extensions.ts:32-94 (kernel undo/redo keymap); shell/src/plugins/nexus/editor/EditorView.tsx:741-779 | shell/src/plugins/nexus/editor/cm/transactionBridge.test.ts:267-385 (batching, reconciliation, undo round-trip) | optional: add cursor-preservation test for the full-doc reconcile path; verify no `history()` import remains across transitive deps |
| 6 | Persistence rewiring | done | shell/src/plugins/nexus/editor/index.ts:651-726 (COMMAND_SAVE branches: md+session → editorClient.saveSession; md+no-session → write_file then acquire); shell/src/plugins/nexus/editor/editorStore.ts:175-185 (revision-based isDirty); shell/src/plugins/nexus/editor/editorStore.ts:327-343 (markSaved promotes savedRevision) | revision-based isDirty has no dedicated test (editorStore.test.ts predates Phase 6 fields); manual smoke only for the untitled→named path | add editorStore tests for `isDirty` revision logic; add an integration test for the untitled save→acquire transition |
| 7 | Downstream consumers | done | shell/src/plugins/nexus/outline/index.ts:76-204 (sessionManager.onChanged → recompute via getTree+getMarkdown); shell/src/plugins/nexus/backlinks/index.ts:211-239 | shell/src/plugins/nexus/outline/outline.eventDriven.test.ts (254 lines); shell/src/plugins/nexus/backlinks/backlinks.eventDriven.test.ts (156 lines) | none — backlinks subscription is intentionally a no-op for same-file edits today (see backlinks/index.ts:211-220) and that's documented |
| 8 | Cleanup + docs | partial | docs/editor-transaction-architecture.md exists; EditorView.tsx:782-791 still mounts a textarea-equivalent fallback CM host with `setContent` for untitled / no-session tabs (this is intended per Phase 5 plan but the §8 exit "rg setContent returns only untitled-tab handling" was never run as a check); editorStore.setContent still exported | none | run the grep audit, prune any incidental `setContent` callers, update docs/leaf-architecture.md pointer |

## 3. Phase deep-dives — partial and TBD only

### Phase 2 — CM mount + markdown rendering

  - **What's done:** CodeMirrorHost is a fully-featured imperative wrapper with compartment-driven readOnly + lineNumbers reconfigure (CodeMirrorHost.tsx:90-172), prop-stable callback refs, and one-time mount semantics; EditorView.tsx swap from `<textarea>` to CM completed with two body modes — bridge-eligible and local — both mounting CodeMirrorHost (EditorView.tsx:741,784); scroll-to-heading and scroll-spy rewired to CM's posAtCoords/scrollDOM (EditorView.tsx:135-259).
  - **What's missing:** no isolated unit test for CodeMirrorHost itself — bridge tests in transactionBridge.test.ts use stub view objects (transactionBridge.test.ts:32-167) so the React lifecycle / compartment reconfigure is exercised only manually.
  - **Suggested closing work:** ~0.25 day. Add a jsdom-driven CodeMirrorHost.test.tsx covering: (a) value prop change replaces doc but doesn't fire onChange echo; (b) readOnly compartment toggle preserves doc; (c) imperative `view` handle resolves after mount.
  - **Risk:** very low — the implementation is stable and battle-tested transitively through Phases 5–7 tests.
  - **Tests required:** above three cases.

### Phase 6 — Persistence rewiring

  - **What's done:** COMMAND_SAVE has three explicit branches (index.ts:651-726): md+session → `editorClient.saveSession` (kernel-canonical write); md+no-session (untitled) → write_file then acquire + markSaved + warn-on-acquire-failure; non-md → write_file. `markSavedRevision` and revision-based `isDirty` (editorStore.ts:175-185) are wired and the rename-aware revision-map remap exists (editorStore.ts:232-264). End-to-end save→storage write path is covered by crates/nexus-bootstrap/tests/editor_ipc.rs:207-296.
  - **What's missing:** no shell-side tests for `isDirty`'s new revision branch — editorStore.test.ts (165 lines) appears to predate Phase 6 fields. The untitled→named transition (index.ts:673-715) is single-stepped logic with three failure modes (write OK / acquire OK, write OK / acquire fails, write fails) and has no integration test.
  - **Suggested closing work:** ~0.5 day. Two test files: (1) editorStore.test.ts additions for `setSessionRevision` + `markSavedRevision` + `isDirty` interactions and `renameTab` map remap; (2) a small integration test (mock kernelClient + sessionManager) covering the COMMAND_SAVE untitled branch.
  - **Risk:** silent dirty-state drift — if `setSessionRevision` is ever called for a tab that has no `savedRevision` entry, `isDirty` returns false (editorStore.ts:181 falls back to `current`), masking unsaved edits. Worth an assertion or an explicit branch.
  - **Tests required:** revision dirty matrix (no entry, equal, diverged, after rename), untitled-save acquire-failure path leaves the warning surfaced and the tab usable.

### Phase 8 — Cleanup + docs

  - **What's done:** docs/editor-transaction-architecture.md exists. The textarea code path is gone — `EditorView.tsx` uses CM exclusively (one bridge-mode mount, one local-mode mount).
  - **What's missing:** the §8 exit criterion `rg "setContent\(" shell/src/plugins/nexus/editor/` should return "only untitled-tab handling." It currently returns five hits (editorStore.ts:134,320 are the definition; editorStore.test.ts:106,134 are tests; EditorView.tsx:789 is the no-session fallback). All hits are legitimate, but the audit was never recorded. docs/leaf-architecture.md has not been updated with the MarkdownView-owns-session pointer (not verified — file not read; flagged as todo).
  - **Suggested closing work:** ~0.25 day. Run the grep, document the legitimate residual call sites in a comment in editorStore.ts (or remove `setContent` from the exported state interface and inline its body for the two callers), add the leaf-architecture pointer.
  - **Risk:** none — pure hygiene.
  - **Tests required:** none beyond a CI grep guard if you want to enforce the exit forever.

## 4. Cross-cutting findings

The implementation is markedly more complete than the Phase 2 plan implies (§3.2 says "Phases 0-2 claimed" / "Phase 3+ in progress") — every load-bearing handler, event, store mutation, batching pass, and downstream subscriber I could find is wired. The 6621 LOC the user noted in the editor plugin is real and reflects significant Phase 5–7 work plus *additional unplanned work*: cm/ holds five block-UX modules (blockHandle.ts:647, slashCommand.ts:607, inlineToolbar.ts:248, blockSelection.ts:121, inputRules.ts:74) implementing docs/notion-block-ux-plan.md phases 1–5, layered on top of the wiring plan. These tap the kernel via the same `editor_sync_content` reparse rather than precise transactions — a deliberate v1 simplification noted in their headers, but a follow-up risk (no granular undo for slash/handle/toolbar mutations). Test coverage is solid kernel-side (editor_ipc.rs covers open/apply/save/get_markdown/event-bus end-to-end) and good shell-side (sessionManager 304 lines, transactionBridge 385 lines, two `*.eventDriven.test.ts` files for downstream). The two thin spots are CodeMirrorHost (no tests) and editorStore (predates Phase 6 revision logic).

## 5. Estimated remaining effort

| Phase | Status | Days | Confidence |
|-------|--------|------|------------|
| 2 | done (test gap) | 0.25 | high |
| 6 | done (test gap + 1 latent bug to harden) | 0.5–0.75 | high |
| 8 | partial (hygiene) | 0.25 | high |
| **Total** | | **~1–1.25 days** | high |

This is dramatically lower than Phase 2 plan §3.2's "~10 days implementation" estimate (PHASE-2-IMPLEMENTATION-PLAN.md:306). The plan's estimate appears to have been formed without the audit it itself was scheduling. Note that §3.2 also lists three follow-on commits (`feat(editor): finalize block-tree sync phase 3`, `feat(editor): kernel-routed undo/redo phase 4`) — both of those are already shipped. If WI-03 is intended to also cover the *unplanned but adjacent* notion-block-ux-plan items (precise per-block transactions to replace the `sync_content`-on-reparse fallback, granular undo for slash/handle mutations), that's a different scope and could legitimately be a multi-day project — it is **not** in the wiring plan being audited.

## 6. Open questions for the implementation owner

1. **Is `apply_transaction` expected to be idempotent on retry?** core_plugin.rs:415-446 unconditionally calls `s.undo.execute(tx, &mut s.tree)`; a retried transaction with the same `tx.id` would double-apply. The shell currently fire-and-forgets in transactionBridge.ts:298-318; failure path drops the pending id and re-fetches markdown rather than retrying. Confirm we never retry, or add server-side dedupe by `tx.id`.
2. **Should `setContent` be removed from the editorStore public interface?** It survives only for the `local:` CM mount path (EditorView.tsx:789) and the untitled flow (editorStore.test.ts:106,134). Removing it would force a clearer contract; keeping it documents the legitimate fallback. Plan §8 implies the grep should be the source of truth.
3. **`isDirty` fallback when sessionRevision is set but savedRevision is missing** — editorStore.ts:181 defaults `saved` to `current`, returning false. Is that correct (newly-acquired session is clean) or a bug masking a missed `markSavedRevision`? sessionManager.acquire calls both setters (sessionManager.ts:135-140) so this should never happen — but no assertion guards it.
4. **Cursor preservation on full-doc reconcile** — transactionBridge.ts:286-293 replaces the whole doc on reconciliation; cursor is implicitly snapped (CM keeps it via its position-mapping for the `changes` spec, but no explicit selection restore). Plan §5 exit says "cursor/selection preserved if possible (at worst, snapped to end)." Current impl is closer to "left to CM's defaults." Acceptable for v1?
5. **Block-UX phases (notion-block-ux-plan)** — five unplanned modules in cm/ (blockHandle.ts, slashCommand.ts, inputRules.ts, inlineToolbar.ts, blockSelection.ts) sidecar the wiring plan. They mutate via plain `dispatch({ changes })` and rely on the transaction bridge to forward as `update_block_content` ops. Is the lossy-undo trade-off in their headers an accepted v1 stance, or in WI-03's scope to fix?
6. **Bridge dispatch concurrency** — transactionBridge.ts uses rAF batching but the kernel `applyTransaction` is async and reconciliation can lag. If a second rAF tick fires before the first reconcile lands, the second batch's `oldContent` may diverge from the kernel's view (because the kernel-applied version normalised whitespace). The plan accepts this in v1 (resolved decision #2). Is "accepted" still the call?
