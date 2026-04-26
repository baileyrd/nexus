> **Archived 2026-04-26** — Point-in-time validation audit (2026-04-23) of the `nexus.agent` plugin slices. Useful as history; the live agent plugin has evolved past this snapshot.

# WI-07 Agent Panel — State Audit

**Source plan:** docs/planning/PHASE-2-IMPLEMENTATION-PLAN.md §4.1
**Audited against:** shell/src/plugins/nexus/agent/ (AgentView, agentStore — ported from the legacy `AgentHistoryPanel.tsx`), crates/nexus-agent/src/core_plugin.rs
**Date:** 2026-04-23

## 1. Plugin overview

`nexus.agent` is a fully wired pane-mode workspace. The user opens it from the activity bar, types a goal, picks `Auto` or `Step` mode, and either fires `Plan` (LLM round-trip only), or `Run`. Auto mode dispatches `com.nexus.agent::run`, watches `com.nexus.agent.*` topics for live per-step status, then backfills the full plan from `history_get` after the observation lands. Step mode loops `execute_step` per step with explicit Approve / Skip / Stop buttons. The left column lists persisted history (newest first) with click-to-load and trash-to-delete behind a confirm modal. Workspace open/close drives subscribe/unsubscribe of the kernel topic prefix.

## 2. Slice-by-slice status matrix

| Slice | Title | Status | Code paths (file:line) | Tests | Closing work |
|-------|-------|--------|------------------------|-------|--------------|
| A | Plan rendering from `agent_plan` | **Done** | index.ts:175-198 (`planOnly`), index.ts:66-90 (`decodePlan`), AgentView.tsx:417-484 (`PlanView`), AgentView.tsx:486-590 (`StepRow`) | None | Add a unit test for `decodePlan` and a snapshot/component test for `PlanView`; optionally surface `archetype` selector in the composer (~0.5d). |
| B | Step-by-step execution + approval dialog | **Done (mostly)** | index.ts:261-390 (state machine: `planThenAwaitApproval`, `handleApproveStep`, `handleSkipStep`, `handleStopRun`, `advanceStep`, `finishStepRun`), AgentView.tsx:592-608 (`ApprovalCluster`), agentStore.ts:96-99,154,156-157 (`pendingApprovalIndex`) | None | Surface step `response` in the row (currently discarded at index.ts:341-343); decide if step-mode needs history persistence (kernel `execute_step` doesn't save — see index.ts:32-33). Add tests covering Approve/Skip/Stop transitions. (~1d). |
| C | History list + restore | **Done** | index.ts:147-173 (`refreshHistory`), index.ts:392-411 (`loadPlanIntoState`), index.ts:418-445 (`handleDeleteHistory`), AgentView.tsx:73-249 (`HistoryColumn`/`HistoryItem`), agentStore.ts:59-66 (HistoryRow) | None | Add a tiny test for `decodeHistoryList` ordering + the workspace-closed reset path; consider rendering observation `response` payloads (legacy AgentHistoryPanel.tsx:230-274 shows them). (~0.5d). |
| D | Streaming `com.nexus.agent.*` events | **Done** | index.ts:447-508 (`handleAgentTopic` + sub/unsub), index.ts:555-566 (lifecycle wire-up), agentStore.ts:168-177 (`setStepStatus`), AgentView.tsx:428-481 (live runtime → `PlanView`) | None | `run_start` is a no-op (index.ts:459-465) — could pre-populate placeholder rows so the user sees the step count before plan backfill, but acceptance criteria are met. Add a topic-routing unit test against the four event shapes. (~0.5d). |

Summary: **Done: 4, Partial: 0, TBD: 0 of 4** (with caveats below).

## 3. Slice deep-dives — partial or TBD only

None of the four slices are blockingly partial. All four "acceptance" sub-flows from §4.1 ("submits goal, sees plan, approves steps one at a time, sees live progress, can re-open from history") are wired end-to-end. The remaining work is tests, polish, and a couple of v1 limitations that the code itself flags. Treating them as Slice E (tests + polish) below.

### Slice E — closing-bay (testing + minor polish)

  - **What's done** (everything from A–D above)
  - **What's missing**
    - **No tests at all** for the agent plugin (`find shell -path '*agent*' -name '*.test.*'` returns nothing). Phase 1 audits flagged this same gap on shell plugins.
    - **Stale doc comment.** AgentView.tsx:34-37 says "Per-step approval (HANDLER_EXECUTE_STEP) and archetype picker are intentionally out of v1 — the kernel surface is ready when the UI lands." Per-step approval IS implemented; only the archetype picker is genuinely absent.
    - **Step `response` payload is dropped.** index.ts:341-343 explicitly comments "the full response shape (response field) isn't surfaced in v1". Legacy `AgentHistoryPanel.tsx:257-270` rendered it as a JSON `<pre>` — current shell drops it on the floor for both step-mode and history-load.
    - **Archetype selector missing.** The kernel `plan` and `run` handlers accept `{ goal, archetype? }` (index.ts:27-28) but the composer hard-codes `{ goal }` (index.ts:185-186, 226-230, 268-271). No `<select>` for archetype anywhere in AgentView.
    - **Step-mode runs aren't persisted.** Documented limitation (agentStore.ts:75-78, index.ts:259-260, 290-293). The shell builds an Observation locally to render the success summary but the kernel never writes a history row, so step-mode runs vanish on reload.
    - **`run_start` topic ignored** (index.ts:459-465). Auto-mode users don't see step rows until either the plan backfills (post-`run`) or `step_start` arrives. Plan view shows "Enter a goal…" placeholder during the planning window.
  - **Suggested closing work** (sized in days, concrete)
    - Tests (decodePlan/decodeObservation/decodeHistoryList; topic-routing; step-mode state machine reducer; history delete confirm flow): ~1.5d
    - Surface step `response` in `StepRow` (re-use `previewJson` from legacy file): ~0.5d
    - Archetype dropdown (fetch list / hardcode set, thread through `plan`/`run` args): ~0.5d
    - Refresh stale doc comment + decide on step-mode persistence (likely "leave as-is, document"): ~0.25d
    - Optional: render placeholder rows on `run_start`: ~0.25d
  - **Risk**
    - The `run` invoke (auto mode) is awaited up to 5 minutes (`RUN_TIMEOUT_MS`, index.ts:46-48). If the user closes the workspace during a run, `unsubscribeAgentTopics()` fires (index.ts:559-562) and `reset()` wipes the store — but the awaiting `invoke` promise still resolves into store-mutation calls that race the reset. Low-impact (the next workspace_opened resets again), but worth a guard.
    - `handleApproveStep` flips status to `running` then awaits — if the user clicks Stop while `execute_step` is in flight, `handleStopRun` will mark the running step `skipped` (index.ts:382-388) but the kernel call is still pending and its return value will overwrite the skipped status (index.ts:347-350). This is a small race with cosmetic impact.
    - `loadPlanIntoState` (index.ts:392-411) overwrites the goal textarea when a history row is clicked. If the user has typed a new goal but not run it yet, that draft is lost.
  - **Tests required**
    - Pure decoders: `decodePlan`, `decodeObservation`, `decodeHistoryList` against representative payloads.
    - Topic router: feed `run_start`/`step_start`/`step_done`/`run_done` payloads through `handleAgentTopic` and assert store state (mocked invoke/subscribe).
    - State machine: drive `planThenAwaitApproval` → `handleApproveStep` × N → `finishStepRun`; assert `pendingApprovalIndex`, `stepRuntime`, `phase`, and built Observation.
    - History flow: load → render → delete (with confirm modal stubbed true/false).

## 4. Cross-cutting findings

The plugin's plan claim ("plan execution UI not fully connected") is **stale**, just like WI-03 and WI-24. The `RunPhase` machine in `agentStore.ts` (`idle → planning → planned/running/awaiting → done/error`) is fully threaded through both modes; `pendingApprovalIndex` correctly disables the composer (`busy = planning || running || awaiting` at AgentView.tsx:288); the topic-stream subscription model from Phase 1 (single prefix subscribe in `activate`, unsubscribe on `workspace:closed`) is in use at index.ts:447-508,555-562; the `paneMode` slot + activity-bar item + `EVENT_ACTIVITY_BAR_ACTIVE_CHANGED` routing dance matches the Phase 1 pattern verbatim. The kernel side (run lifecycle events at core_plugin.rs:217-319; all seven handlers at core_plugin.rs:116-122) was already complete before WI-07 was scoped. The remaining gaps (response display, archetype picker, step-mode persistence) are surface-level v1 cuts that the code self-documents, not architectural holes. There are zero tests for the plugin — this is the biggest gap and the same gap surfaced in earlier Phase 1 audits.

## 5. Estimated remaining effort

| Item | Days | Confidence |
|------|------|------------|
| Test suite for decoders + topic router + state machine | 1.5 | High |
| Surface step `response` in StepRow + history detail | 0.5 | High |
| Archetype dropdown wired through plan/run | 0.5 | Medium (need to settle archetype list source) |
| Stale-comment cleanup + step-mode persistence decision | 0.25 | High |
| Optional: `run_start` placeholder rendering | 0.25 | High |
| **Total** | **~3 days** | High overall |

§4.1 estimates **M (~1 week / ~5 days)**. The plan's estimate is **pessimistic by ~2 days** — slices A/B/C/D are already implemented; only tests + small polish remain. Even assuming the archetype work needs UX discussion (push to ~1d), total stays well under the 5d budget. Same pattern as WI-03 and WI-24: plan was written before re-audit.

If the archetype picker turns into a "design archetype browser" sub-feature (likely scope creep), add 2–3 days; otherwise hold at ~3 days.

## 6. Open questions for the implementation owner

1. **Step-mode persistence:** keep current behaviour (no history row written) and document, or extend `execute_step` kernel handler to persist after the final step? Affects acceptance interpretation of "can re-open from history" for Approve-mode runs.
2. **Archetype picker:** is the list of archetypes static (read from `crates/nexus-agent/src/archetypes.rs`), or is there a kernel command to enumerate them? Plan §4.1 doesn't mention archetypes; legacy `AgentHistoryPanel.tsx` doesn't either.
3. **Response payload UX:** legacy used a `<pre>` with `previewJson` truncation at 400 chars. Acceptable here, or do we want collapsible/expandable JSON?
4. **Race-window guards:** are the two cosmetic races (workspace-close-during-run, Stop-during-execute_step) worth fixing in this WI, or punt to a separate hardening pass?
5. **Test framework:** does the shell already have a pinned test runner for plugin units (vitest? jest?), or does WI-07 introduce one alongside the agent tests? Phase 1 audits suggested testing infra was the larger missing piece.
6. **Goal-draft preservation on history-load:** intended (current behaviour overwrites) or a bug to fix?
