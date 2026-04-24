# Phase 2 Implementation Plan — Feature Parity Migration

> **Historical document** — Written before the `app/` → `shell/` migration (Phase 4 WI-37, 2026-04-24). Paths below reference the legacy `app/` and `crates/nexus-app/` tree that has since been deleted. For current locations see `docs/legacy-shell-retirement.md`.

**Status:** Plan only (no code changes yet)
**Date:** 2026-04-23
**Author:** Claude (audit + planning run)
**Phase:** 2 of 6 in the shell-migration roadmap (per [`INTEGRATION-REVIEW.md`](./INTEGRATION-REVIEW.md) §5 and [ADR 0011](./adr/0011-adopt-plugin-first-shell.md))
**Prerequisite:** [Phase 1](./PHASE-1-IMPLEMENTATION-PLAN.md) acceptance complete.
**Source of truth for scope:** [`PARITY-CHECKLIST.md`](./PARITY-CHECKLIST.md) + [`Parity-Checklist.xlsx`](./Parity-Checklist.xlsx) (23 work items).

---

## 1. Executive summary

Phase 2 is the largest item on the roadmap: ~19.25 engineer-weeks of work porting the ~95 legacy Tauri commands' UX to the plugin-first shell, finishing partial plugins, and closing the architectural-diff decisions. Every work item's backing kernel IPC already exists — this is almost entirely frontend-plus-a-handful-of-kernel-gaps work.

**Readiness corrections from the audit** (IPC surfaces verified, not just reported):

| Work item | Checklist said | Actual state |
|---|---|---|
| WI-01 AI | 7 IPC commands | **11** (`ask`, `index_file`, `vectorstore_count`, `status`, `config`, `stream_chat`, `stream_ask`, `session_{load,save,list,delete}`) |
| WI-02 Theme | 8 IPC commands | **11** (includes `apply_config`, `set_plugin_overrides`, `reload`) |
| WI-10 Bases | 13 `base_*` handlers | **17** (adds `base_index`, `base_list`, `base_query`, `base_property_rename`) |
| WI-11 Canvas | 4 IPC commands (checklist) | **5** (`canvas_read`, `canvas_write`, `canvas_patch`, `canvas_nodes`, `canvas_edges`) |
| WI-03 Editor | Phases 0-2 claimed | CM6 mount + transaction bridge shipped; Phase 3+ block UX in progress |
| WI-10 Bases | Shell "partial" | **All five views shipped** (Table, Board, Calendar, Gallery, Timeline — 5138 LOC). Soft-delete/restore are the edges to close. |
| WI-11 Canvas | Shell "partial" | **5312 LOC shipped** (CanvasView, overlay, minimap, inspector, auto-layout, export). Validation-only item. |
| WI-20 extension-api | Phase 2 item | **Moved into Phase 1** ([PHASE-1-IMPLEMENTATION-PLAN.md §5](./PHASE-1-IMPLEMENTATION-PLAN.md)) |
| WI-06 kernel_subscribe | Phase 2 item | **Moved into Phase 1** |
| WI-22, WI-23 guardrails | Phase 2 items | **Moved into Phase 1** |

Net effect: Phase 2 gets ~4 WIs shifted to Phase 1 where they provide foundations; Phase 2's remaining scope is **19 work items** (originally 23 in the checklist). Estimated effort drops from 19.25 to ~15 weeks (mostly because AI/theme/editor are bigger than they looked).

**Treatment depth in this plan:** P0/P1 items (12 remaining, the spine of Phase 2) get full per-WI sketches — design, subagent pattern, commit plan, acceptance. P2/P3 items (7) get lighter grouped treatment since their contours are clearer once P0/P1 land.

**Acceptance for Phase 2 as a whole:**
1. Every legacy Tauri command in `crates/nexus-app/src/` is reachable through the new shell with equivalent UX, verified by a side-by-side workflow test (open forge → edit note → search → ask AI → run workflow → close).
2. Every P0/P1 work item has `Status = done` on `Parity-Checklist.xlsx`.
3. The three architectural-diff decisions (WI-15 layout presets, WI-16 menu bar, WI-17 ribbon) are closed via ADRs regardless of which way they go.
4. No new `#[tauri::command]` in `crates/nexus-app/` (WI-22 Phase 1 guardrail holds).
5. All shell plugins import only from `@nexus/extension-api` (WI-23 Phase 1 guardrail holds).

---

## 2. Scope summary

### 2.1 Work items moved to Phase 1

Removed from this plan's scope; tracked in [PHASE-1-IMPLEMENTATION-PLAN.md](./PHASE-1-IMPLEMENTATION-PLAN.md):

- WI-06 (kernel_subscribe generalization)
- WI-20 (@nexus/extension-api via ts-rs)
- WI-22 (Rust freeze guardrail)
- WI-23 (shell import-hygiene guardrail)

### 2.2 Phase 2 work items — P0/P1 (full treatment below)

| ID | Title | Size | Priority | Phase |
|---|---|---|---|---|
| WI-01 | Port AI chat panel to shell (streaming + sessions) | L | P0 | 2a |
| WI-03 | Finish editor transaction wiring (Phases 0–8) | L | P0 | 2a |
| WI-02 | Re-wire theme engine (brand-neutral redesign) | L | P1 | 2a |
| WI-04 | Keybinding overrides UI (HotkeysTab) | S | P1 | 2a |
| WI-21 | File tree parity (context menu, drag-drop) | S | P1 | 2a |
| WI-07 | Agent panel: approval loop + streaming progress | M | P1 | 2b |
| WI-14 | Persistence migration script (legacy → shell-state.json) | S | P1 | 2c |
| WI-18 | Plugin capability listing in shell settings | S | P1 | 2c |

### 2.3 Phase 2 work items — P2/P3 (lighter treatment, grouped)

| ID | Title | Size | Priority | Phase |
|---|---|---|---|---|
| WI-05 | Saved terminal commands sidebar | S | P2 | 2a |
| WI-08 | Skills browser: render with params | S | P2 | 2b |
| WI-09 | Workflow: list/get/reload/validate UI | S | P2 | 2b |
| WI-10 | Bases: validate granular IPC (17 handlers) | M | P2 | 2b |
| WI-11 | Canvas: live-data validation | M | P2 | 2b |
| WI-12 | Terminal: upgrade from poll to streaming | M | P2 | 2b |
| WI-13 | URI handler registry + dispatch_uri port | S | P2 | 2c |
| WI-19 | Activation events (deferred plugin load) | M | P2 | 2c |
| WI-15 | DECISION: Layout presets | M | P3 | 2c |
| WI-16 | DECISION: Menu bar vs. palette-only | M | P3 | 2c |
| WI-17 | DECISION: Ribbon vs. activity bar API alignment | XS | P3 | 2c |

Total: **19 WIs**, ~15 weeks for one engineer.

---

## 3. Phase 2a — Leverage wave (P0/P1 foundations, ~4 weeks)

These unblock the biggest user-visible wins and the remaining parity surface. They're independent and can fan out to two engineers.

---

### 3.1 WI-01 — Port AI chat panel to shell (L, P0)

**Intent.** Bring the legacy shell's streaming AI chat UI (+ RAG "Ask" + multi-session persistence + per-session history) into the new shell. Biggest user-visible gap today.

**Current state.**
- Kernel `com.nexus.ai` IPC: 11 handlers already live (`ask`, `index_file`, `vectorstore_count`, `status`, `config`, `stream_chat`, `stream_ask`, `session_load`, `session_save`, `session_list`, `session_delete`).
- Kernel event topics live: `com.nexus.ai.stream_start`, `stream_chunk`, `stream_done`. Forwarded as `ai:stream_*` Tauri events in legacy.
- Legacy UI: `app/src/components/panels/ChatPanel.tsx` — 1275 LOC. Turn-based conversation, RAG source chips, agent-approval flow, session load/save UI, pauseable streaming.
- Shell plugin: `shell/src/plugins/nexus/ai/` — 89 LOC across `index.ts`, `aiStore.ts`, `aiRuntime.ts`. Skeleton: activity-bar registration, view registry stubbed, focus/clear commands. **No UI, no streaming wiring.**

**Design sketch.**

Split the port into three vertical slices instead of porting 1275 LOC in one go:

1. **Slice A — Config + plain Q&A.** Wire `ai_config`, render minimal "ask a question, get a streamed response" UI. Validates end-to-end streaming through `api.kernel.on('com.nexus.ai.stream_chunk', ...)`. ~300 LOC.
2. **Slice B — Conversation model.** Turn-based chat, scroll behavior, copy-paste, markdown rendering of assistant replies. Source chips for RAG. ~400 LOC.
3. **Slice C — Sessions.** Session list, load/save/delete/rename, session title auto-gen. ~300 LOC.

Each slice is a commit; the plugin ships progressively more capable. This lets us catch streaming architecture bugs in Slice A before compounding them in B/C.

**Architecture:**
- View type: `nexus.ai.chat` registered via `viewRegistry.register` (pattern from Phase 1 WI-24).
- Store: `aiStore.ts` grows to track `{ sessions, activeSessionId, turns, streamingTurnId, status }`.
- Streaming: subscribe `com.nexus.ai.stream_chunk` on plugin activate; match incoming chunks by `request_id` (kernel-assigned); buffer into active turn until `stream_done`.
- Error handling: `IpcErrorEnvelope` from WI-06 gives typed errors; show "Retry" button for `retryable: true`, settings link for `capability_denied`.

**Subagent pattern.**

**Agent 1 (once, upfront) — reference extraction.** Prompt: *"Read `app/src/components/panels/ChatPanel.tsx` end-to-end. Produce a structured extraction: (a) component tree, (b) state shape (exact Zustand/useState names), (c) IPC call sites with kernel command and argument shape, (d) event subscription patterns, (e) UX details that will be non-obvious to re-implement (scroll behavior, keyboard shortcuts, loading states, RAG source rendering). Don't propose a new design — just catalog what's there. ~800 words."*

**Agent 2 per slice (3 sequential) — implementation.** Prompt: *"Implement Slice <A|B|C> of `shell/src/plugins/nexus/ai/` per [design reference]. Use the legacy extraction from Agent 1 as a reference but write idiomatic new-shell code (Leaf View, `@nexus/extension-api` imports, `api.kernel.*` for all IPC). Ship: diff, tests for new code paths, one-line summary for commit message."*

Main thread: review each slice, run it, commit.

**Commit plan.** 4 commits:
1. `feat(ai): extract legacy ChatPanel reference notes` (markdown artifact, no code change)
2. `feat(ai): slice A — config surface + streamed Q&A`
3. `feat(ai): slice B — conversation model + RAG sources`
4. `feat(ai): slice C — session persistence + list UI`

**Files touched:**
- `shell/src/plugins/nexus/ai/{index.ts, aiStore.ts, aiRuntime.ts, ChatView.tsx, SessionList.tsx, TurnRenderer.tsx}` — new components.
- Tests for store reducers and the streaming buffer logic.

**Acceptance.**
- User opens AI view, types a question, sees streamed response with chunks arriving <200ms after first byte.
- RAG "Ask" mode returns sources rendered as clickable chips that open source files.
- Session list shows named sessions; "New chat" / "Delete" / "Rename" all work.
- Plugin deactivation cleans up active stream subscriptions (inherits from WI-06).

**Risks.** Streaming + reconnection UX is the hard part — if `stream_chunk` events race with a plugin reload (e.g. hot-reload during dev), turns can lose data. Mitigate with per-request `request_id` tagging and stale-request dropping in the store reducer. Slice A is explicitly where we validate this before going deeper.

**Size:** L (~2 weeks). Extraction (1d) + Slice A (4d) + Slice B (4d) + Slice C (3d).

---

### 3.2 WI-03 — Finish editor transaction wiring (L, P0)

**Intent.** Close out `editor-transaction-wiring-plan.md` Phases 3–8 so the editor plugin has kernel-authoritative undo, echo-suppressed change propagation, and block-aware session state. Everything downstream (Notion blocks, canvas embeds, collaborative edit) assumes this is solid.

**Current state.**
- Kernel `com.nexus.editor` IPC: 10+ handlers (`open`, `close`, `get_tree`, `save`, `apply_transaction`, `undo`, `redo`, `list_open`, `sync_content`, `get_markdown`).
- Shell plugin: `shell/src/plugins/nexus/editor/` — 3748 LOC across `cm/`, views, store, runtime. CM6 mount + transaction bridge shipped (Phases 1–2 of wiring plan). Block-aware features stubbed.
- Plan reference: `docs/editor-transaction-wiring-plan.md` — eight phases defined.

**Design sketch.**

This WI is more validation + completion than new-build. Approach:

1. **Read the plan, mark phases done/partial/TBD** against the current shell code. Produce a definitive phase-status matrix.
2. **For each TBD/partial phase**, design the minimal closing work. Phases 3–8 roughly cover: block-tree sync with outline, save/dirty indicator, undo/redo kernel routing, multi-cursor integration, transaction batching for large paste operations, performance (>500-node docs).
3. **Implement phase by phase**, one commit per phase. Tests for each.

**Subagent pattern.**

**Agent 1 — phase audit.** Prompt: *"Walk through `docs/editor-transaction-wiring-plan.md` end-to-end. For each numbered phase (0 through 8), inspect the corresponding implementation in `shell/src/plugins/nexus/editor/` and `shell/src-tauri/src/` and report: (a) implementation status [done/partial/TBD], (b) file paths where relevant code lives, (c) concrete closing work remaining if partial, (d) tests that exist vs. missing. Don't propose — report. ~1200 words."*

**Main thread** consumes the audit, plans the 6-ish remaining phases as commits, implements. Delegating actual editor-plugin code to agents is risky — it's a complex surface where subtle bugs hide. Do this one in the main loop.

**Commit plan.** 1 audit doc + one commit per remaining phase (~5-7 commits):
1. `docs(editor): phase-by-phase audit against wiring plan`
2. `feat(editor): finalize block-tree sync phase 3`
3. `feat(editor): kernel-routed undo/redo phase 4`
4. (etc. per audit output)

**Acceptance.**
- All phases of `editor-transaction-wiring-plan.md` marked complete.
- Ctrl+Z routes to kernel `com.nexus.editor::undo`, not CM6 local history.
- Paste of 500+ blocks stays responsive (<500ms for transaction apply).
- Dirty indicator matches kernel-side session revision.
- Regression: editing a file in one tab and opening it in another tab shows same content with no desync.

**Risks.** CM6's own history state is legacy and subtle; disabling it cleanly without breaking keyboard shortcuts (Cmd+Z in a text input that's not the editor itself) needs care. The audit will catch this.

**Size:** L (~2 weeks). 1d audit + 1.5 weeks implementation.

---

### 3.3 WI-02 — Re-wire theme engine (L, P1)

**Intent.** Bring theming back to the shell, brand-neutral, so user preferences (light/dark/system, theme choice, snippet cascade) work end-to-end. Without this, the shell is locked to dark.

**Current state.**
- Kernel `com.nexus.theme` IPC: 11 handlers live (`get_available_themes`, `apply_theme`, `compute_variables`, `get_available_snippets`, `toggle_snippet`, `reorder_snippets`, `get_theme_config`, `set_mode`, `apply_config`, `set_plugin_overrides`, `reload`).
- Kernel event: `com.nexus.theme.changed` fires on any theme mutation.
- Shell plugin: `shell/src/plugins/core/themeService/` — **intentionally excluded from boot** per `shell/src/main.tsx:37` comment ("template's plugins/core/* UI files ship with hardcoded Nexus product content — 'Forge Ember', 'Forge Paper'"). Dark enforced via `shell.css :root`.
- Legacy: `app/src/stores/theme.ts` — 154 LOC Zustand store, syncs with kernel events, hot-applies CSS variables to `:root`.

**Design sketch.**

Three sub-workstreams:

1. **Kernel-side rename.** The theme manifests themselves carry "Forge Ember" / "Forge Paper" product names. Rename to generic "Default Dark" / "Default Light" / "High Contrast" or similar. This is a data-change in `crates/nexus-theme/themes/*.toml` or wherever manifests live. Low risk, high visibility fix.

2. **Shell theme store.** Create `shell/src/stores/themeStore.ts` (or extend the existing one) modeled on the legacy pattern: mirror the kernel state (current theme id, enabled snippets, mode, resolved variables). Hydrate on boot via `com.nexus.theme::get_theme_config`. Subscribe to `com.nexus.theme.changed`; on event, call `compute_variables` and apply to `:root`.

3. **Settings UI.** Add theme/mode picker + snippet list to the `core.settings` plugin (already loaded). Match legacy layout: theme dropdown, mode radio (light/dark/system), snippet checkbox list with drag-to-reorder.

**Subagent pattern.**

**Agent 1 — rename.** Prompt: *"In `crates/nexus-theme/`, find every user-facing string that references 'Forge Ember', 'Forge Paper', or 'Nexus' as a brand (theme manifest fields, README examples). Replace with brand-neutral equivalents: 'Default Dark', 'Default Light', 'High Contrast'. Don't touch code logic. Report: diff, any strings you were unsure about."*

**Agent 2 — store.** Prompt: *"Port `app/src/stores/theme.ts` to `shell/src/stores/themeStore.ts`. Use the Zustand pattern consistent with other shell stores (`configStore`, `paneModeStore`). Imports from `@nexus/extension-api` only. Subscribe to `com.nexus.theme.changed` via `api.kernel.on` with cleanup on plugin deactivate. Write unit tests for the state transitions. Don't port the UI — separate task."*

**Main thread** does the settings UI work — it's integration with the existing settings plugin and benefits from hands-on iteration.

**Commit plan.** 3 commits:
1. `chore(theme): brand-neutral theme manifest names`
2. `feat(theme): shell-side theme store with kernel sync`
3. `feat(settings): theme + snippet picker UI`

**Acceptance.**
- User opens Settings → Appearance; sees theme dropdown, mode radio, snippet list.
- Changing any of them updates the UI live (no reload needed).
- Change persists across app restart.
- Kernel-initiated theme change (e.g. from CLI/plugin) propagates to the UI.

**Risks.** Forced-dark via `shell.css` is a bootstrapping choice. During migration, removing that while the theme store isn't fully wired = flash of white. Mitigate by keeping `shell.css` default dark and only applying theme-store-resolved variables on top (CSS variables cascade).

**Size:** L (~1.5 weeks). Kernel rename (1d) + store (3d) + settings UI (3d) + tests (2d).

---

### 3.4 WI-04 — Keybinding overrides UI (S, P1)

**Intent.** Give users the ability to see, rebind, and clear keybinding overrides. Legacy shell has this in Settings > Hotkeys; new shell has a `KeybindingRegistry` scaffold but no persistence or UI.

**Current state.**
- Shell: `shell/src/registry/KeybindingRegistry.ts` — registry exists, no persistence, no override UI.
- Legacy: `app/src/components/settings/tabs/HotkeysTab.tsx` — 176 LOC. Lists all bindings with chord normalization, shows `when` context evaluation, edit/clear flow.

**Design sketch.**

1. **Persistence layer.** Add `getOverrides()` / `setOverride(commandId, chord)` / `clearOverride(commandId)` to `KeybindingRegistry`, backed by shell-side JSON in `<app_config_dir>/keybindings.json`. Save on mutation.
2. **Registry merge.** When a plugin registers a keybinding, check the overrides map; prefer override chord over default.
3. **UI.** Settings tab with same shape as legacy: table of (command label, current chord, default chord if overridden, clear button). Edit button opens a chord-capture input.

**Subagent pattern.**

**Agent 1 — registry extension.** Prompt: *"Extend `shell/src/registry/KeybindingRegistry.ts` with persistence of user overrides to a JSON file managed by the shell's `core.configuration-service` plugin. Shape: `{ [commandId]: chord }`. On registry lookup, merge override over default. Write unit tests for override set/clear/merge. Do not change the UI."*

**Agent 2 — Settings UI port.** Prompt: *"Port `app/src/components/settings/tabs/HotkeysTab.tsx` into a shell settings tab. Use the shell's settings contribution API. The chord-capture UI is the interesting bit — preserve legacy behavior (capture modifier + keycode, display as Cmd-based on platform)."*

**Commit plan.** 2 commits:
1. `feat(keybindings): persist overrides to disk with registry merge`
2. `feat(settings): hotkeys tab for override management`

**Acceptance.**
- User opens Settings → Keybindings; sees all registered commands with current chord.
- Click "Edit", press new chord, see it saved and active.
- Click "Reset" restores default. Override persists across restart.
- Plugin registering a keybinding that the user has overridden uses the override, not the default.

**Size:** S (~3d).

---

### 3.5 WI-21 — File tree parity (S, P1)

**Intent.** Match legacy file tree's right-click context menu, keyboard shortcuts, and drag-drop move. Without these, power users notice the regression immediately.

**Current state.**
- Shell: `shell/src/plugins/nexus/files/` — 1067 LOC. Tree renders, file ops (`create_file`, `rename_entry`, `delete_entry`) are wired. Context menu stubbed, keyboard shortcuts missing, drag-drop stubbed.
- Legacy: `app/src/components/panels/FileTree.tsx` — 414 LOC. Right-click menu (new file / new folder / rename / delete / reveal / copy path), Del + F2 shortcuts, drag-to-move.

**Design sketch.**

Port the three legacy affordances:

1. **Context menu.** Use the existing `nexus.confirm` + `nexus.paneMode` patterns for overlay rendering. Right-click on tree node → show contextual menu; items are command invocations (`nexus.files.create`, `nexus.files.rename`, `nexus.files.delete`, `nexus.files.reveal`).
2. **Keyboard shortcuts.** Register via `api.keybindings.register` with `when: 'nexus.files.focused'` context key so they don't fire globally.
3. **Drag-drop.** HTML5 drag-drop API; on drop, call `com.nexus.storage::rename_entry` with new parent path.

**Subagent pattern.**

**Agent 1 — context menu.** Prompt: *"Add a right-click context menu to `shell/src/plugins/nexus/files/FilesTree.tsx` matching the legacy menu at `app/src/components/panels/FileTree.tsx`. Use a reusable overlay primitive (look at `shell/src/primitives/` for an existing pattern, or create a minimal one). Menu items invoke commands registered in the plugin. Test: right-click opens menu, item click fires command, click-outside closes."*

**Agent 2 — shortcuts + drag-drop.** Prompt: *"In the same plugin, register Del and F2 keybindings under `when: 'nexus.files.focused'` that invoke delete and rename commands on the selected node. Implement HTML5 drag-drop: draggable nodes, droppable folders, on drop call `com.nexus.storage::rename_entry`. Ensure focus tracking sets/clears the context key."*

**Commit plan.** 2 commits.

**Acceptance.** Right-click any file → menu appears with correct items. Select file, press F2 → rename input. Drag file to folder → moved on disk.

**Size:** S (~3d).

---

### 3.6 Phase 2a summary

| ID | Sub | Days | Can parallelize with |
|---|---|---|---|
| WI-01 | Slice A/B/C | ~10 | WI-02, WI-04, WI-21 |
| WI-03 | Editor completion | ~10 | WI-02, WI-04, WI-21 |
| WI-02 | Theme rewire | ~7 | WI-01, WI-03, WI-04 |
| WI-04 | Keybindings UI | ~3 | any |
| WI-21 | File tree polish | ~3 | any |

One engineer serial: ~5.5 weeks for Phase 2a.
Two engineers: WI-01 + WI-03 on A (hardest, 4 weeks), WI-02 + WI-04 + WI-21 on B (~2.5 weeks) → Phase 2a done in ~4 calendar weeks with slack.

---

## 4. Phase 2b — Features wave (P1/P2, ~4 weeks)

### 4.1 WI-07 — Agent panel: approval loop + streaming progress (M, P1)

**Intent.** Close the last partial flows in the agent plugin: streaming step progress, approval UI, history restore.

**Current state.** `shell/src/plugins/nexus/agent/` is 1515 LOC — skeleton is there, kernel IPC calls wired, but the plan execution UI with streaming step progress isn't fully connected.

**Design.** Similar split to WI-01 slices — small vertical slices rather than one big port:
- Slice A: plan rendering from `agent_plan` response.
- Slice B: step-by-step execution with `agent_execute_step` + approval dialog.
- Slice C: history list + restore from `agent_history_*`.
- Slice D: streaming step-start/step-done events from `com.nexus.agent.*`.

**Subagent pattern.** Same as WI-01: Agent 1 extracts from `app/src/components/panels/AgentHistoryPanel.tsx` (316 LOC), then sequential implementation agents per slice.

**Commit plan.** 4 commits, one per slice.

**Acceptance.** User submits goal, sees plan, approves steps one at a time, sees live progress, can re-open from history.

**Size:** M (~1 week).

### 4.2 WI-10 — Bases: validate granular IPC (M, P2)

**Intent.** The bases plugin is 5138 LOC, claims Phases 1–6 complete per `bases-shell-plan.md`. Validate all 17 `base_*` IPC handlers work end-to-end against real data; close any gaps around soft-delete / restore / property rename.

**Design.** Validation-heavy, implementation-light.
1. **Integration test harness.** A test fixture with a real `.bases` file exercising each handler.
2. **Soft-delete UX.** Verify records can be soft-deleted and restored via UI (not just API).
3. **Property rename validation.** Schema editor rename flow + data migration preview.
4. Any discovered gaps → bug-fix commits.

**Subagent pattern.** One Plan subagent to map the validation plan + gap-finding pass, then implementation in main thread because bases is a high-risk surface.

**Commit plan.** 1 validation commit + N fix commits (expect 2–4).

**Acceptance.** Integration test fixture passes; soft-delete/restore works UI-end; property rename preserves data.

**Size:** M (~1 week).

### 4.3 WI-11 — Canvas: live-data validation (M, P2)

**Intent.** Same shape as WI-10. Canvas is 5312 LOC, Phases 1–6 claimed complete. Validate against real `.canvas` files, close edge cases around `canvas_patch` debounce and concurrent-edit scenarios.

**Design.** Primarily a test harness + smoke pass. Load real files, exercise CRUD + drag + link + minimap + export. Fix bugs found.

**Subagent pattern.** Same as WI-10.

**Commit plan.** 1 validation + N fixes (expect 1–3).

**Acceptance.** Open real `.canvas`, edit, save, reload, see the edits. Export PNG/SVG/PDF succeeds. Minimap stays in sync.

**Size:** M (~1 week).

### 4.4 WI-12 — Terminal: poll → streaming (M, P2)

**Intent.** Legacy pumps PTY output on 100ms poll via `term_pump`. Upgrade to streaming via kernel event bus so CPU drops to near-zero when idle and latency improves.

**Design.**
- Add kernel event topic `com.nexus.terminal.output.<session_id>` emitted from the PTY reader loop in `crates/nexus-terminal/`.
- Shell plugin subscribes via `api.kernel.on(prefix='com.nexus.terminal.output.', ...)`.
- Keep `term_pump` as fallback for very slow subscribers or catch-up scenarios.

**Subagent pattern.** Agent 1 Rust-side event emission, Agent 2 TS-side subscription + buffer. Sequential.

**Commit plan.** 2 commits.

**Acceptance.** Terminal idle → no CPU. Output appears <16ms after emission. Poll path still works if subscribed handler errors.

**Size:** M (~1 week).

### 4.5 WI-05 — Saved terminal commands (S, P2)

**Intent.** Port legacy `SavedCommandsPanel.tsx` (313 LOC) into a sub-view of `nexus.terminal`.

**Design.** Straightforward port — kernel IPC is there (5 `saved_*` handlers already implemented). Add `SavedCommandsView` as a leaf under the terminal view, with CRUD + drag-reorder + click-to-execute.

**Subagent pattern.** Single Explore agent extracts the legacy UI shape, single implementation agent ports it.

**Commit plan.** 1 commit.

**Acceptance.** Sidebar shows saved commands, CRUD works, drag reorders, click executes in active terminal session, persists across restart.

**Size:** S (~3d).

### 4.6 WI-08, WI-09 — Skills and workflow UI flows (S+S, P2)

Grouped because the shape is identical: both have browsers in the shell that render lists, but partial flows (skills `render` with params; workflow `validate`) need completion.

**Design.** For each:
1. Inspect what's already in `shell/src/plugins/nexus/{skills,workflow}/`.
2. Implement the missing action (parameter form for skills render; validation error display for workflow).
3. Wire to the corresponding IPC.

**Subagent pattern.** Two parallel agents (one per plugin), each does the same audit-then-implement flow.

**Commit plan.** 2 commits (one per plugin).

**Acceptance.** Skills: user picks skill, fills params, sees rendered output. Workflows: user pastes TOML, clicks Validate, sees errors inline.

**Size:** S + S (~1 week combined).

---

## 5. Phase 2c — Polish, peripherals, and decisions (~4 weeks)

### 5.1 WI-14 — Persistence migration script (S, P1)

**Intent.** Users upgrading from the legacy shell should see their layout and recents preserved.

**Design.** Standalone script `scripts/migrate-shell-state.ts` that reads `layout-persistence.json` (legacy) + legacy tab state and emits `shell-state.json` + `workspace.json` (new).

**Subagent pattern.** One implementation agent, given both schemas. Testable via fixture JSON files.

**Commit plan.** 1 commit with script + fixture + test.

**Acceptance.** Run on a legacy install fixture → output files match expected new-shell state.

**Size:** S (~3d).

### 5.2 WI-18 — Plugin capability listing (S, P1)

**Intent.** Precursor to Phase 3's install-time capability prompt. Settings > Plugins tab should show each plugin's declared capabilities with risk-level labels.

**Design.** Read capabilities from plugin manifests (already parsed by `ExtensionHost`); extend `shell/src/plugins/nexus/pluginsMgmt/PluginsMgmtView.tsx` to render them per plugin.

**Subagent pattern.** Single implementation agent.

**Commit plan.** 1 commit.

**Acceptance.** Settings > Plugins shows each plugin with its capabilities listed and risk color-coded (low/medium/high).

**Size:** S (~2d).

### 5.3 WI-13 — URI handler registry (S, P2)

**Intent.** Port the `dispatch_uri` + `list_plugin_uri_handlers` legacy pattern so deep links (`nexus://note/...`) work.

**Design.** New registry `shell/src/registry/UriHandlerRegistry.ts` + API surface `api.uri.register(scheme, handler)`. Tauri deep-link plugin routes to it.

**Subagent pattern.** One agent for registry + API, one for Tauri-side wiring.

**Commit plan.** 2 commits.

**Acceptance.** External `nexus://` link opens the app and routes to the correct handler.

**Size:** S (~3d).

### 5.4 WI-19 — Activation events (M, P2)

**Intent.** Plugins currently all load on boot. Introduce activation events (`onView:<viewId>`, `onCommand:<cmdId>`, `onUri:<scheme>`, `onLanguage:<lang>`) so cold-start cost scales with used-plugins, not installed-plugins.

**Design.** Modify `ExtensionHost` to defer plugin activation until its declared events fire. Plugins declare `activationEvents: [...]` in manifest; host registers triggers rather than activating.

**Subagent pattern.** One agent for the host change + migration of existing plugin manifests to declare events. Main thread reviews.

**Commit plan.** 1 commit for host + 1 commit per ported plugin cohort.

**Acceptance.** Shell cold-boot measurably faster with 32 plugins installed but few activated. Activating a plugin at runtime (e.g. opening its view) still works.

**Size:** M (~1 week).

### 5.5 WI-15, WI-16, WI-17 — Architectural decisions (XS–M, P3)

Grouped because these are **ADR-shaped work**, not code-shaped.

**WI-15 (Layout presets).** Decide: keep Obsidian/Vibe/Dev preset concept as named Leaf snapshots, or drop. Propose **drop** for v1 (low usage, Leaf tree is arbitrary anyway); revisit if users ask. Deliverable: `docs/adr/0012-drop-named-layout-presets.md` (or `...retain-as-snapshots.md`).

**WI-16 (Menu bar).** Decide: reintroduce native MenuBar as a plugin, or commit to palette-first. Propose **palette-first** for v1 on non-macOS; a macOS-specific minimal menu bar (File/Edit/View/Window) for platform conformance. Deliverable: `docs/adr/0013-menu-bar-strategy.md`.

**WI-17 (Ribbon vs. activity bar).** Minor naming/API alignment. Deliverable: an ADR or a brief note in `packages/nexus-extension-api/README.md` documenting the mapping.

**Subagent pattern.** Each is a short ADR + (if adopted) a small implementation. Agent writes ADR draft; main thread reviews and merges.

**Commit plan.** 3 commits (one ADR each) + 0–2 implementation commits if WI-16 lands a macOS menu bar.

**Acceptance.** All three ADRs committed with Status = Accepted or Rejected; code matches the decision.

**Size:** M combined (~3d).

---

## 6. Dependency graph

```
 Phase 1 complete (extension-api, bridge subscriptions, guardrails)
                              │
                              ▼
             ┌──────── Phase 2a (leverage) ────────┐
             │                                       │
             ▼                                       ▼
  WI-01 (AI chat) ──┐                    WI-02 (theme) ──┐
  WI-03 (editor) ───┤ unblocks           WI-04 (keybinds)┤
  WI-21 (filetree) ─┘                    WI-05 (saved)  ─┘
             │                                       │
             └────────────────┬──────────────────────┘
                              ▼
             ┌──────── Phase 2b (features) ────────┐
             │                                       │
             ▼                                       ▼
  WI-07 (agent streaming)                WI-12 (terminal stream)
  WI-10 (bases validate)                 WI-11 (canvas validate)
  WI-08 (skills)                         WI-09 (workflow)
             │                                       │
             └────────────────┬──────────────────────┘
                              ▼
             ┌─── Phase 2c (polish + decisions) ────┐
             │                                       │
             ▼                                       ▼
  WI-14 (persistence migrate)            WI-15 (layout preset ADR)
  WI-18 (capability listing)             WI-16 (menu bar ADR)
  WI-13 (URI handlers)                   WI-17 (ribbon API ADR)
  WI-19 (activation events)
```

### 6.1 Single-engineer serialization (~15 weeks)

- Weeks 1–2: WI-01 (AI chat, 3 slices)
- Weeks 3–4: WI-03 (editor completion)
- Week 5: WI-02 (theme)
- Week 6: WI-04 + WI-21 (keybinds + filetree)
- Week 7: WI-05 + WI-08 + WI-09 (small items)
- Week 8: WI-07 (agent)
- Week 9: WI-10 + WI-11 (bases + canvas validation)
- Week 10: WI-12 (terminal streaming)
- Weeks 11–12: WI-19 (activation events)
- Week 13: WI-14 + WI-18 + WI-13 (migration script + capability UI + URI)
- Weeks 14–15: WI-15 + WI-16 + WI-17 decisions + polish + acceptance

### 6.2 Two-engineer parallelization (~8–9 calendar weeks)

- Engineer A (big items): WI-01 → WI-03 → WI-07 → WI-19
- Engineer B (wide items): WI-02 → WI-04 → WI-21 → WI-05 → WI-08/9 → WI-10/11 → WI-12 → WI-14 → WI-18 → WI-13 → WI-15/16/17

Critical path runs through A (WI-01 + WI-03 + WI-07 + WI-19 = ~8 weeks).

### 6.3 Agent-heavy run (one engineer + Claude agents, ~6–7 weeks)

Most Phase 2 WIs have well-identified fan-out points (extraction agents, per-slice implementation agents, per-plugin port agents). Realistic throughput with prompt caching: 2–3 WIs per week, with main-thread review as the bottleneck.

---

## 7. Risks & mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| WI-01 slice A uncovers streaming architecture bugs that delay subsequent slices | Medium | Keep each slice behind a feature flag; slice A is where we validate the streaming contract. Retry logic in the store. |
| WI-03 editor audit reveals more unfinished phases than expected | Medium | Treat the audit as its own commit; re-estimate remaining work before committing to week 3 of Phase 2a. |
| Legacy UI has subtle UX details (scroll restoration, keyboard focus) that get lost in the port | High | Agent 1 (extraction) catches these explicitly; keep side-by-side workflow test through Phase 2. |
| WI-10/11 validation reveals kernel bugs in bases/canvas IPC | Medium | Fix in the service crate; Phase 1 guardrails (WI-22) don't block this because it's service-layer work, not legacy shell. |
| WI-19 activation-event system breaks plugins that implicitly assumed sync load | Low | Migrate plugins one cohort at a time behind a feature flag; fall back to sync load on any error. |
| ADR decisions (WI-15/16/17) drag out into bikeshedding | Low | Time-box each to 2 days; defaults are already in the plan. Write the ADR, ship it. |

---

## 8. Open questions for user before execution

Plan-level calls that can be settled at Phase 2a kickoff:

1. **Do you want Slice A of WI-01 to ship independently** (usable but minimal AI chat) or bundle slices A+B+C? Recommendation: ship A to get streaming UX in users' hands fast.

2. **For WI-03 editor audit**, do you want me to produce a `docs/editor-phase-status.md` artifact first, or dive directly into implementation from an internal audit? Recommendation: artifact — it doubles as design review material.

3. **For WI-15/16 decisions** — are you open to dropping named layout presets and the native MenuBar for v1? The recommendations above argue yes; want to pre-approve or decide per-ADR as they land?

4. **Scope of Phase 2 acceptance test** — lightweight unit-test harness, or full Tauri window E2E? (Same question as Phase 1 §11.) Recommendation: E2E for Phase 2 acceptance since this is the shell-readiness gate; Phase 1 unit-test harness was fine for structural work.

Flag these at kickoff; defaults in the plan apply otherwise.

---

## 9. What this plan does NOT cover

- **Phase 3 security hardening.** JS plugin sandbox, install-time capability prompt, TOCTOU fix, `api_version` enforcement. Separate plan.
- **Phase 4 frontend unification and retiring `crates/nexus-app`.** After Phase 2 parity is verified.
- **Phase 5 v1 polish.** Updater, marketplace, crash reporting.
- **Any new capability.** Phase 2 is parity only; new features go to a separate backlog.

---

## 10. Next action

Review this plan. If approved, execution order:

1. **Kickoff check** — resolve the four §8 open questions.
2. **WI-01 Slice A** — highest-leverage user-visible win; validates streaming architecture.
3. **WI-03 editor audit** — produces the sub-plan for the largest remaining item.
4. **Parallelize Phase 2a** per §6.2 if two engineers, else serial per §6.1.
5. **Checkpoint every week** against this plan's estimates; reforecast at the Phase 2a → 2b boundary.

Each WI has its own commit plan in §3–§5; land them incrementally per the Phase 0 workflow.
