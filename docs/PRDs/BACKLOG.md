# Nexus Feature Backlog

> **Single source of truth for unfinished work.** This file is the index every other planning doc points to.
>
> - **Per-PRD status** lives in [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md).
> - **Completed items** are archived verbatim in [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md).
> - **Full descriptions of OI-\*** items live in [../OPEN-ITEMS.md](../OPEN-ITEMS.md); this file cross-lists by ID.
> - **Formal-release work** (auto-updater, telemetry, marketplace, beta→GA) is deferred to [../REQUIRED-FOR-FORMAL-RELEASE.md](../REQUIRED-FOR-FORMAL-RELEASE.md); the WI-IDs are indexed below for completeness.
> - **Exploratory / unscoped design docs** (AI directions, ambient copilot, memory layer, settings extraction inventory) are linked under "Future directions" — they do not have committed timelines.
>
> Section headings with no listed items are preserved as structural placeholders — consult the archive for what landed under each, and add new follow-ups directly below the heading.

---

## New Features (not addressed in any PRD)

_BL-009 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

## Partially New Features (concept exists in PRDs but design is unspecified)

### BL-007: CRDT-over-Git Transport

**Source**: PRD 11, Section 4.4 (Level 3)
**Effort**: Large (2–3 weeks)
**Crate**: `nexus-git`, new `nexus-crdt`
**Related PRD**: PRD 11 (specified but deferred — requires collaborative editing layer)

Serialize Nexus CRDT state (rich text buffer) as JSON in `.nexus/crdt-state.json`, tracked in git. On push, CRDT state is included in commits. On pull with merge conflict in the CRDT file, apply CRDT merge semantics (operation-based or state-based) for automatic convergence. Fallback to content conflict if CRDT merge fails. Enables multi-user async collaboration via git push/pull without manual conflict resolution. Prerequisite: a CRDT-based editor engine (PRD 08) or collaborative editing layer.

---

## Post-migration carryover gaps (2026-04-24)

Capabilities described in legacy `app/` documentation that were not carried over to `shell/` during the Phase 4 WI-37 retirement. Full descriptions and acceptance criteria in [../OPEN-ITEMS.md](../OPEN-ITEMS.md). Resolved entries are archived in [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md).

### Open

- [ ] **OI-05: Rust dep duplication** — Blocked on upstream. 34 crates with duplicated versions all trace through `wasmtime 42` (toml/sha2/digest/rand_core/reqwest/rustix/nix/hashbrown) or `portable-pty → filedescriptor` (`thiserror 1`). Revisit after the next wasmtime major release.
- [ ] **OI-15: Manifest signature / provenance** — Optional `manifest.toml.sig` Ed25519 over manifest bytes, verified against trusted-publisher keyring. Marketplace prerequisite (paired with WI-44).
- [ ] **OI-18: Snippet trigger collision detection** — Same hazard as OI-10 but for snippets; emit `plugins:snippet-conflict` and surface a "which plugin wins" control. **Blocked: no snippet registry exists yet.** `Snippet` type + `editor.registerSnippet` are declared in [`@nexus/extension-api`](../../packages/nexus-extension-api/src/index.ts#L101) but never implemented in the shell — every existing "snippet" reference is the unrelated CSS theme-snippet system. Doing this properly means building the script-plugin code-snippet registry first; closer to OI-15 than OI-10 in scope. Reopen when `registerSnippet` lands.

### Resolved (preserved here for cross-reference; full notes in [../OPEN-ITEMS.md](../OPEN-ITEMS.md))

- [x] OI-01 — Settings modal + `registerSettingsTab` API _(2026-04-24)_
- [x] OI-02 — Split-size persistence (editor splits gained drag handles + `setSplitSizes` mutator) _(2026-04-24)_
- [x] OI-03 — Workspace-wide clippy `-D warnings` sweep _(2026-04-24)_
- [x] OI-04 — Kernel-contract promotion TODOs (`SlotId` and `list_archetypes` IPC) _(2026-04-24)_
- [x] OI-06 — ESLint 8 → 9 + typescript-eslint 7 → 8 + xterm → `@xterm/*` scoped _(2026-04-24)_
- [x] OI-07 — Capability grants/denials/path-traversal routed through `audit::*` _(2026-04-24)_
- [x] OI-08 — "Running Extensions" Settings tab (live plugin state + errors + Disable) _(2026-04-26)_
- [x] OI-09 — `pluginsStatusStore` aggregates plugin lifecycle events into a per-plugin `{ state, lastError }` map _(2026-04-26)_
- [x] OI-10 — `KeybindingRegistry.getConflicts()` + `plugins:keybindings-conflict` event with signature-dedup; per-row `!` badge + summary banner in Settings → Keybindings _(2026-04-27)_
- [x] OI-11 — `CommandRegistry.execute` races handlers against a configurable cancel deadline (`shell.command.timeoutCancelMs`, default 5s) with a soft warn at `shell.command.timeoutWarnMs` (default 250ms); emits `command:cancelled` and throws `CommandCancelledError` so the palette can dismiss in-flight state _(2026-04-27)_
- [x] OI-12 — Auto-promotion was already gone on the kernel side; this pass tightened the `confine_path` / `read_file` doc comments to spell out the contract, documented the script-plugin `PlatformFsAPI` path-semantics in `@nexus/extension-api`, and added two kernel tests that pin the loud `PermissionDenied` + traversal-message AC for absolute reads / writes _(2026-04-27)_
- [x] OI-13 — Deleted dead `nexus_kernel::PluginRegistry` + `Kernel::plugins()` (zero callers; `PluginLoader::loaded` is authoritative) _(2026-04-26)_
- [x] OI-16 — `ExtensionHost.deactivateAllForShutdown(perPluginCapMs)` runs every active plugin's `deactivate()` in parallel with a per-plugin soft cap; wired from a `beforeunload` listener in `main.tsx` so flush-on-stop hooks get one last shot before the WebView tears down _(2026-04-27)_
- [x] OI-17 — Deprecation policy lands as a three-way handshake — `@deprecated` JSDoc on the symbol + an entry in `packages/nexus-extension-api/DEPRECATED.md` + an `importNames` row in `shell/eslint.config.js`'s `no-restricted-imports` block. CI gate works without enabling type-aware lint (kept defer-decision intact); empty list today, table headers + protocol ready for the first deprecation _(2026-04-27)_
- [x] OI-20 — Terminal copy/paste — `attachCustomKeyEventHandler` claims `Ctrl+Shift+C/V` (Linux/Windows) and `Cmd+C/V` (macOS) without disturbing plain `Ctrl+C` SIGINT, right-click pastes from clipboard, paste honours bracketed-paste mode (`\e[200~ … \e[201~`) when xterm signals it. Uses `navigator.clipboard.{read,write}Text` from user-gesture handlers; denial logs a follow-up note pointing at `@tauri-apps/plugin-clipboard-manager` _(2026-04-27)_
- [x] OI-14 — `api.workspace.forgeRoot()` + `api.editor.active()/onChange()` exposed via `@nexus/extension-api` _(2026-04-26)_
- [x] OI-19 — Deferred createRoot/unmount in `TerminalPaneView` + `EmptyView`; React 18 commit-phase warnings on drawer collapse + StrictMode double-mount cleared _(2026-04-27)_

---

## Formal release scope (deferred)

Tracked in full in [../REQUIRED-FOR-FORMAL-RELEASE.md](../REQUIRED-FOR-FORMAL-RELEASE.md). Out of scope for personal-tool use; surface here so the IDs are findable.

- [ ] **WI-41: Tauri auto-updater + code-signing + release channel.** ~5–7 eng-days plus 1–3 weeks calendar for signing-cert procurement.
- [ ] **WI-42: Crash reporting & telemetry.** ~5 eng-days, opt-in via Settings.
- [ ] **WI-44: Minimal marketplace.** ~5 eng-days; index schema + shell UI + CLI install + tarball publishing. Paired with **OI-15** (manifest signing) and **F-8.1.1 / F-8.1.2** (iframe sandbox + boundary-bound `pluginId`) before opening to untrusted plugins.
- [ ] **WI-46: Beta → GA logistics.** Triage rubric, test-group recruitment, ship criteria. ~3 eng-days plus 2-week calendar.

---

## Future directions (scoped 2026-04-28)

Previously: design-only docs without committed timelines. **Scoped into the implementation plan on 2026-04-28** — each FD piece now has a BL-* ID (see "Future-direction items minted into the backlog" above) and the docs themselves remain authoritative for design rationale.

- **AI integration directions** — see [../AI-INTEGRATION-DIRECTIONS.md](../AI-INTEGRATION-DIRECTIONS.md). Mapping: "inline rewrite/summarize" → BL-034 (engine) + BL-035 (action surface); "auto-link suggestions" → BL-039; "semantic search" → BL-040; "per-surface chat" → merged into BL-010 (reshape note); "skills as prompts" → composed via BL-021 / BL-022; "agent loops" → merged into BL-027 (same surface); "MCP exposure" (Nexus-as-server) → BL-042; "background indexing" → BL-041. Direction "tool-calling" was already BL-016.
- **Ambient copilot UX patterns** — see [../AI-AMBIENT-COPILOT-PLAN.md](../AI-AMBIENT-COPILOT-PLAN.md). Mapping: Cmd+I overlay → BL-032; context chips + model switcher → BL-033; ghost suggestions → BL-034; right-click AI actions → BL-035 (shared with NB block AI actions); margin suggestions + inline correction → BL-036; activity timeline → BL-037; citations → BL-038; capture → AI → folded into BL-043 (memory quick-capture).
- **AI memory layer** — see [../AI-MEMORY-LAYER-PLAN.md](../AI-MEMORY-LAYER-PLAN.md). Mapping: quick-capture → BL-043; auto-enrichment on save → BL-045; recall hotkey → BL-044; implicit chat context → merged into BL-010 (reshape note); code-aware capture → BL-046; scheduled digests → BL-047.
- **Notion-style block UX out-of-scope follow-ups** — see [../notion-block-ux-plan.md](../notion-block-ux-plan.md). Mapping: drag-to-embed into canvas → BL-048; block-links navigator → BL-049 (gated on block-id stability ADR); side-margin comments → BL-050; block AI actions → merged into BL-035; multi-cursor from multi-block → BL-051.

---

## Settings extraction queue

Inventory of named-constant / hardcoded settings candidates lives in [../../shell/HARDCODED_SETTINGS_AUDIT.md](../../shell/HARDCODED_SETTINGS_AUDIT.md). Pickable in any order; each is a 1–2 hour change.

- [ ] **Zoom settings schema** — `ui.zoomStep`, `ui.zoomMin`, `ui.zoomMax`, `ui.zoomDefault`. Constants already named in `shell/src/plugins/core/zoom/index.ts:15–18`.
- [ ] **Notification durations schema** — 5 hardcoded ms values in `notificationService` + ChatView + SavedCommandsView.
- [ ] **Search / palette result limits** — `search.maxResultsLimit`, `commandPalette.maxResultsLimit`.
- [ ] **Long-running operation timeout consolidation** — 3 independent `5 * 60_000` literals in AI / agent / workflow crates; consolidate into `LONG_RUNNING_OP_TIMEOUT_MS`.
- [ ] **Buffer / event caps** — Name `PROCESS_EVENTS_CAP`, `BASES_HISTORY_CAP`, `CANVAS_HISTORY_CAP`, etc.; consider a shared `UNDO_HISTORY_CAP`.

---

## Architecture review (2026-04-16) — microkernel adherence

## UI architecture review (2026-04-16) — editor-shell pattern

### Code gaps

### PRD gap — no owner for plugin-contributed tab surfaces

## Editor-shell capability gaps (2026-04-16) — vs VS Code / Obsidian / IntelliJ

### Spec'd in a PRD, not yet implemented

- [ ] **`.bases` database renderer in the shell (PRD-10).** Phases
  1–6 landed 2026-04-22 along with every deferred tail item.
  Kernel surface: `com.nexus.storage::base_*` ids 40–52
  (including 49 `base_create`, 50 `base_property_rename`, 51
  `base_record_soft_delete`, 52 `base_record_restore`);
  `com.nexus.database::csv_import` / `csv_export` / `formula_eval`.
  Wire schema grew two slots in the same pass: `ViewType` now
  includes `List` + `Timeline`, `BaseView` gained `end_field`
  (timeline end-date pairing with the existing `date_field` as
  start), and `BaseRecord` gained `deleted_at: Option<i64>` to
  carry the soft-delete state. Shell code under
  `shell/src/plugins/nexus/bases/`. Phase-6 work: CSV + undo +
  formula live preview, `@tanstack/react-virtual` windowing,
  "New base" Files-toolbar action → `NewBaseDialog` template
  picker, `SchemaEditor` side panel with rename / retype
  (migrate_values) / formula editor, list + timeline view
  persistence via `viewMapping.ts`, and soft-delete: `BasesView`
  filters `deletedAt != null` out of every live view's visible
  set but keeps them on the base for the SchemaEditor and a
  future trash view. Only truly open follow-ups now: a trash-view
  UI surfacing soft-deleted records and a table-virtualization
  retest when very large (>50k) bases land.
- [ ] **`.canvas` board renderer in the shell (PRD-06 §4).** Phases
  1–6 complete 2026-04-22 — every deferred Phase-6 item closed in
  the same session. Kernel surface:
  `com.nexus.storage::canvas_read` / `canvas_write` /
  `canvas_patch` / `canvas_nodes` / `canvas_edges` (handler ids
  35–39), with `SetBackground` added to `CanvasPatchOp`. Shell
  code under `shell/src/plugins/nexus/canvas/` covers the full
  editing loop (selection / marquee / resize / drag / delete /
  undo-redo / edge drag / inspector) plus the Phase-5 DOM overlay
  (`CanvasOverlay` — markdown, file embeds, OG cards, `.bases`
  mini-grid, terminal sessions) and the Phase-6 polish tier
  (minimap, Tidy auto-layout, grid toggle, zoom-to-fit / zoom-to-
  selection, help overlay). Phase-6 closers (2026-04-22):
  `exportFormats.ts` uses `html-to-image` + `jspdf` to produce
  overlay-inclusive PNG / SVG / PDF (the Export control-strip
  button opens a 3-option popover); a new optional
  `CanvasBackground { color, pattern? }` field on `CanvasFile`
  drives per-canvas background color + dots/grid/lines pattern,
  edited from the Inspector's `CANVAS` section behind a `BG`
  control-strip button; canvas shortcuts now route through the
  shell `KeybindingRegistry` via manifest contributions with a
  `canvas.focused` context-key gate, and every shortcut is also
  a palette-accessible `canvas.*` command.
- [ ] **Notion-style block UX on top of the existing block-tree engine
  (PRD-08).** Phases 1–6 of the plan landed 2026-04-22. Shell-only:
  every mutation drives the doc through plain CM
  `dispatch({ changes })` + the existing `editor_sync_content`
  reparse — no new kernel IPC. Five CodeMirror extensions under
  `shell/src/plugins/nexus/editor/cm/`: **`slashCommand.ts`** (typing
  `/` at block start opens a categorised palette with
  filter-as-you-type + runtime registry for plugin-contributed
  commands); **`blockSelection.ts`** (Cmd/Ctrl+A expands caret →
  block → document; Shift+Arrow at block edges steps by whole
  blocks); **`blockHandle.ts`** (6-dot grip overlay per block,
  click-menu with Turn-into submenu + Duplicate / Move up / Move
  down / Delete, drag-to-reorder with a live drop-line indicator,
  `Alt-ArrowUp/Down` keyboard equivalents); **`inputRules.ts`**
  (`[]`/`[x]`/`*`/`+` space-normalization rules that fill the gap
  where user expectation diverges from raw markdown);
  **`inlineToolbar.ts`** (floating Bold/Italic/Code/Link toolbar
  above non-empty single-block selections plus
  `Mod-b/i/e/k` shortcuts with wrap/unwrap toggle).
  Explicitly out of scope for this pass and tracked as separate
  follow-ups: drag-to-embed into canvas (cross-plugin), block
  links navigator (`[[…#^block-id]]`), side-margin comments
  subsystem, block AI actions via `com.nexus.ai`, multi-cursor
  from multi-block selection.
  Kernel asks addressed 2026-04-22: (1) `Transaction::move_block(tree,
  id, new_parent, new_index, metadata)` constructor landed — single
  `ReparentBlock` op = single undo step for block-drag. Fixed an
  incidental `ReparentBlock::reverse` bug where same-parent backward
  moves couldn't be cleanly reversed (the existing
  `reparent_roundtrip` test was cross-parent and missed it). (2)
  Block-id stability over save+reopen: `deterministic_block_id` keys
  on `(file_path, visit_order, block_type)`, so ids are stable for
  files that round-trip unchanged. An insert mid-document shifts
  `visit_order` for every downstream block and produces new ids on
  reload — the plan's proposed fixes (HTML-comment stamping in
  markdown or an out-of-band `.forge/blocks.json` sidecar) remain
  the options. Left deferred until the Phase-6 block-link UX forces
  a choice, since today no feature depends on cross-session block
  id stability under edits.

### Half-specced: manifest keys exist, but no UI/wiring spec in PRD-07

### Not in any PRD — new spec work needed

## Architecture audit (2026-04-16) — follow-ups

Findings surfaced by the microkernel + editor-shell audit that weren't already tracked above.

## Microkernel hardening — 2026-04-16 audit findings

Findings from `docs/archive/planning/MICROKERNEL-AUDIT.md` not yet tracked. Ordered by audit priority. The three 🔴 items and F-9.2.1 are blockers before any public plugin marketplace.

### 🔴 Red — blockers for untrusted plugin distribution

_None outstanding._ F-2.1.1 closed 2026-04-22 — see archive.

### 🟠 Orange — address before marketplace or next minor release

### 🟡 Yellow — quality / correctness improvements

## Suspected issues — not fully investigated

Threads from `docs/archive/planning/MICROKERNEL-AUDIT.md §Suspected Issues` that warrant a targeted code walk.

- [ ] **Hot-reload timing on macOS and Windows.** `notify-debouncer-mini` behaviour differs across platforms; F-4.3.1 covers one class of issue. A targeted cross-platform reliability pass on the hot-reload path would be worthwhile before shipping community plugin hot-reload as a feature. **Deferred** — requires running the shell on macOS and Windows hardware to reproduce and measure; this repo's test host is Linux/WSL only. Track for a dedicated cross-platform QA pass once a macOS/Windows CI runner or test machine is available.

## UI audit (2026-04-16) — follow-ups

Findings from `docs/archive/planning/UI-AUDIT.md` not yet tracked above. IDs reference the audit. The 🔴 items plus F-9.1.1 are blockers before any untrusted-plugin distribution.

### 🔴 Red — cannot ship to untrusted users without these

_F-8.1.1 (sub-tasks 1–5: iframe scaffold + sandbox flags, postMessage protocol, `NexusPluginContext` proxy, per-plugin manifest `sandboxed` flag, CSP + tests), **F-8.1.1-fo1** (precompiled `bootstrapSandboxedPlugin` runtime bundle + hello-world migration), and **F-8.1.2** (boundary-bound `pluginId` — orchestrator builds a per-plugin `PluginAPI` from the handshake-set id; `assertValidPluginId` rejects empty / colon-bearing ids) shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). All red-tier UI items now closed; remaining gating for community marketplace launch is **WI-44** (marketplace UI / index / signing) and **OI-15** (manifest signing) at the orange tier._

> F-9.1.1 (validate `api_version` at load time) is the UI twin of the microkernel 🟠 item of the same ID already tracked above — no duplicate entry.

### 🟠 Orange — substantive design gaps, schedule before next external release

- [ ] **Memory budget / accounting for script plugins (UI F-8.3.1).** WASM plugins have `memory_mb = 8` in their manifest; script plugins have no equivalent and allocate against the WebView heap directly. A plugin that accumulates a 500 MB structure OOMs the whole shell. **Now unblocked** — F-8.1.1 shipped 2026-04-28 (per-plugin iframe boundary in `shell/src/host/sandbox/SandboxOrchestrator.ts`). `performance.measureUserAgentSpecificMemory()` is per-frame, so the orchestrator can poll each iframe and attribute usage by `data-sandbox-plugin`. Today still unimplemented; a misbehaving script plugin's RSS is indistinguishable from the shell's. Track as a sandboxed-plugin watchdog enhancement.

### 🟡 Yellow — rough edges to fix opportunistically

### Suspected issues — UI audit §6 spike candidates

Threads from `docs/archive/planning/UI-AUDIT.md §6` not yet confirmed. Each is a 1–2 day targeted code walk or runtime probe.

- [x] **SI-1 — Blob-URL same-origin inheritance.** **Closed 2026-04-28** as a duplicate of F-8.1.1. The blob-URL same-origin inheritance behaviour is confirmed (MDN spec — a `blob:` URL inherits the origin of its creator), but it no longer matters for sandboxed plugins: `manifest.sandboxed === true` routes the plugin through `SandboxOrchestrator`, which mounts a null-origin iframe (`sandbox="allow-scripts"`, no `allow-same-origin`). Inside that iframe the host's blob URL is reachable for the bundle import but the iframe runs at `event.origin === "null"` so it can't read `window.parent.document` / `document.cookie` / Tauri's IPC bridge. Legacy non-sandboxed plugins still inherit the shell's origin — that's the "first-party only" trust posture documented in `DEPRECATED.md`.
- [ ] **SI-6 — `PluginManager` Mutex contention.** **Deferred — requires a dedicated load-test harness that doesn't exist yet.** Measuring requires 20+ chatty plugins and wall-clock profiling while a human drives the UI, which this environment cannot replicate. Hypothesis: per-plugin dispatch already uses `try_lock` + reentrancy guard + per-plugin backend mutex, so the `PluginManager` top-level mutex is only held during scan/load/unload/reload — not during steady-state dispatch. If the hypothesis holds this is cosmetic; if not, the fix is likely `RwLock<HashMap<id, …>>` inside the loader with per-plugin reader locks. Track as an explicit Phase-3 stability task once the load-test tooling exists.

## Audit findings (2026-04-28)

> Cross-PRD docs audit ([DOCS_AUDIT_2026-04-28.md](DOCS_AUDIT_2026-04-28.md)) — items spec'd in a PRD that are not yet built and were not previously assigned a backlog ID. Each cites the PRD section, target crate, and estimated effort. Effort scale: small ≈ ½–2 days, medium ≈ 3–10 days, large ≈ 2+ weeks.

_BL-010 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-011 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

### BL-012: Database query blocks in the editor (`[[{db:query}]]`)

**Source**: PRD-08 §8.1 (`Block::DatabaseView`, `DatabaseViewConfig`).
**Effort**: Large (1–2 weeks). **Crate**: `nexus-editor` (executor) + `shell/src/plugins/nexus/editor/` (grid renderer).
Last functional gap in PRD-08; previously listed in IMPLEMENTATION_STATUS.md gaps without a BL ID. Needs (1) query executor over `com.nexus.database::apply_view`, (2) virtualized inline grid widget, (3) decoration plumbing through CM6, (4) undo integration, (5) filter/sort UX surfaced in the block.

_BL-013 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-015 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

### BL-016: AI tool registration for LLM function-calling

**Source**: PRD-12 §8.1 (`ToolRegistry`, `ToolExecutor`, built-in `read_file` / `write_file` / etc.).
**Effort**: Medium. **Crate**: `nexus-ai`.
Distinct from the agent's MCP discovery (which appends *tool descriptions* to the planner prompt). Native function-calling means surfacing Anthropic / OpenAI `tools` and Ollama tool-call format from `stream_chat`, dispatching the model's tool-calls back through `ipc_call`. Today providers strip tool params before the request.
**Cross-references (2026-04-28)**: prerequisite for BL-010, BL-011, BL-027, BL-035 (right-click AI actions / block AI actions), BL-036 (margin / inline correction), and the memory layer's "recall as a tool" flow (BL-044). Treat as Phase-1 foundation; downstream agents queue behind it. Split per the implementation plan: (1) `ToolRegistry` + `ToolExecutor` core, (2) Anthropic + OpenAI tool-call wire format, (3) Ollama tool-call format + dispatch loop.

_BL-019 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-021 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-022 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-023 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-025 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-027 shipped 2026-04-29 — see BACKLOG_COMPLETED.md._

### BL-028: Workflow trigger expansion + control flow _(complete 2026-04-29)_

**Source**: PRD-16 §6 (webhook / git_event / mcp_event), §9.2-9.4 (parallel, retry/backoff, AI steps), §7 (template library).
**Effort**: Large (umbrella). **Crate**: `nexus-workflow`.
Every sub-item under this umbrella has shipped: cron + file_event + git_event + mcp_event + webhook triggers, condition evaluator, variable interpolation, manual executor, per-step retry/backoff, parallel step scheduler, AI step types (`ai_prompt` / `ai_decision`), and a built-in templates library with CLI surface.

Sub-items:
- **BL-028a — git_event trigger** _(shipped 2026-04-29)_: subscribes to `com.nexus.git.{state,commit,branch_changed,dirty_changed}` with optional `events` / `branch` / `branch_pattern` filters; defaults exclude `state` (snapshot, fires on every boot). Exposes `trigger.{event_type,branch,head,prev_head?,from?,is_dirty?}` to the workflow.
- **BL-028b — per-step retry with exponential backoff** _(shipped 2026-04-29)_: per-step `max_retries` / `retry_backoff` (`constant|linear|exponential`) / `retry_initial_delay_ms` / `retry_max_delay_ms` / `retry_jitter` (full-jitter, default true). Step-level fields shadow workflow `error_handling` defaults. `StepOutcome.attempts` records the final attempt count. Each parallel branch will retry independently when parallel scheduling lands.
- **BL-028e — `mcp_event` trigger** _(shipped 2026-04-29)_: parallel construct to `git_event` / `file_event`. Subscribes to `com.nexus.mcp.*` on the kernel bus, filters by an `events: [String]` allow-list against currently-known short names (today: `host_started`; more land here as `nexus-mcp` publishes them — no executor change needed). `host_started` is excluded by default since it's a one-shot snapshot fired at MCP plugin boot, mirroring the git `state` rationale. The trigger exposes `trigger.event_type` plus every top-level payload key (e.g. `trigger.configured_servers` on `host_started`) to the workflow. Spec parsing + topic mapping live in `core_plugin.rs` (`McpEventSpec::from_trigger`, `mcp_event_type_for_type_id`); accept loop is `mcp_event_loop`. 4 unit tests cover defaults, opt-in, unknown-event rejection, and topic mapping coverage.
- **BL-028f — built-in templates library** _(shipped 2026-04-29)_: ships 5 starter templates embedded in the binary via `include_str!`, exposed through three new IPC handlers on `com.nexus.workflow` and a `nexus workflow template list / show / init` CLI surface. Templates: `daily-journal` (cron + ipc), `commit-summary` (git_event + ai_prompt + ipc chain), `note-classifier` (file_event + ai_decision + kv ipc), `parallel-fetch` (manual + parallel ipc steps demonstrating BL-028c), `research-prompt` (manual + inputs + ai_prompt RAG). New module [`crates/nexus-workflow/src/templates.rs`](../../crates/nexus-workflow/src/templates.rs) holds the catalog + `find` + `parse` helpers. New IPC handlers (append-only ids on `com.nexus.workflow`): `HANDLER_TEMPLATES_LIST = 8` (returns `[{ slug, description, tags, filename }]`), `HANDLER_TEMPLATES_GET = 9` (returns same plus `body`), `HANDLER_TEMPLATES_INIT = 10` (writes the body to `<forge>/.workflows/<filename>`; refuses to clobber unless `overwrite = true`; sanitises `filename` via `sanitize_filename` to reject path separators / `..`). CLI subcommand wired in [`crates/nexus-cli/src/commands/workflow.rs`](../../crates/nexus-cli/src/commands/workflow.rs) (`template_list / template_show / template_init`) and [`crates/nexus-cli/src/main.rs`](../../crates/nexus-cli/src/main.rs) (`WorkflowTemplateCommand::List|Show|Init`). Bootstrap manifest entries added in [`crates/nexus-bootstrap/src/lib.rs`](../../crates/nexus-bootstrap/src/lib.rs). 6 unit tests in `templates::tests` (catalog has ≥ 5, slugs unique + kebab-case, filenames mirror slugs, every embedded body parses + validates as a workflow, `find` round-trips, `Template.description` matches `[workflow].description` inside the body) plus 6 IPC handler tests in `core_plugin::tests` (list returns catalog, get returns body, get rejects unknown slug, init writes file + refuses clobber + honours overwrite, init rejects path traversal, init then reload picks up the new workflow).
- **BL-028g — webhook trigger (HTTP listener)** _(shipped 2026-04-29)_: a single hand-rolled HTTP/1.1 listener (no new deps — uses tokio's `TcpListener` and ~150 lines of by-hand parsing in [`crates/nexus-workflow/src/webhook.rs`](../../crates/nexus-workflow/src/webhook.rs)) accepts `POST` requests, matches against registered `webhook` workflows, optionally validates a shared-secret header, and dispatches `com.nexus.workflow::run` with `trigger.{path, method, body, remote_addr}` variables. Workflow shape: `[trigger] type = "webhook" path = "/gitlab-push" method = "POST" secret = "shhh"` (path required + must start with `/`; method defaults to `POST` and v1 only accepts POST; secret is optional but, when set, requires `X-Webhook-Secret` header — compared via constant-time equality so timing doesn't leak prefix info). Forge config: `<forge>/.forge/config.toml` `[webhooks] enabled = false bind = "127.0.0.1:18080"` — disabled by default (explicit opt-in for binding ports), loopback by default (binding non-loopback is the user's call). The listener only spawns when `enabled = true` *and* at least one workflow declares a `webhook` trigger. Body capped at 64KB (`MAX_BODY_BYTES`); request preamble capped at 16KB (`MAX_HEADER_BYTES`); per-connection 5s read timeout. Routing returns 404 (unknown path), 405 (path matches but method differs), 401 (missing/wrong secret), 200 (dispatched), 400 (malformed), 408 (timeout), 413 (too large), 500 (run failed). New `WebhookCorePlugin::open_full(workflows_dir, digest_config, webhook_config)` constructor; legacy `open` / `open_with_digest_config` delegate with `WebhookConfig::default()`. Bootstrap loads `[webhooks]` via new `load_webhook_config` mirroring `load_digest_config`. Coverage: 18 unit tests in `webhook::tests` (spec parsing happy + 5 rejection cases, `parse_request` happy + query-strip + oversized + missing-content-length + malformed-header, `route_request` 4 outcomes, `constant_eq` correctness, `build_trigger_vars` shape, `WebhookConfig::default`). End-to-end accept loop is exercised through the same `ipc_call` path the other triggers use.
- **BL-028d — AI step types `ai_prompt` / `ai_decision`** _(shipped 2026-04-29)_: two new `step.step_type` arms in `KernelActionDispatcher`. **`ai_prompt`** routes through `com.nexus.ai::ask` with `{ question: <prompt>, limit? }` and returns the full `RagResponse` JSON (`answer` is the primary text). **`ai_decision`** asks the AI to pick one label from a fixed `choices: [String]` list — composes a tightly-instructed prompt, sends it through `ask` with `limit = 0` (no RAG context for a classifier call), parses the response via `pick_choice` (exact case-insensitive match → fall back to substring with longest-label tiebreak), and returns `{ choice, raw, model }`. No-match surfaces as `Err` so the step's retry/backoff and `on_error` policy apply uniformly. Pure logic lives in [`crates/nexus-workflow/src/ai_steps.rs`](../../crates/nexus-workflow/src/ai_steps.rs) (`AiPromptArgs::from_step`, `AiDecisionArgs::from_step`, `build_decision_prompt`, `pick_choice`) so it's unit-testable without an `ipc_call` stub. Coverage: `cargo test -p nexus-workflow` 114 passing (was 102; +12 net) — round-trip for required + optional fields, missing-prompt rejection, empty / blank / non-string choice rejection, prompt builder includes question + each choice + reply instruction, exact case-insensitive pick, quote / punctuation stripping, longest-substring tiebreak (`"chore"` vs `"chore-major"`), no-match returns `None`. `cargo clippy -p nexus-workflow --all-targets -- -D warnings` clean. Workflow authors can now write end-to-end pipelines like *"on commit → ai_prompt summarise diff → ai_decision tag as bug/feature/chore → ipc step writes the digest"* without leaving TOML.
- **BL-028c — parallel step scheduler** _(shipped 2026-04-29)_: a maximal contiguous run of `[[steps]]` with `parallel = true` forms a *parallel group*. Every branch starts together, the executor awaits all of them via `futures::future::join_all`, and only then proceeds to the next sequential step (or the next group). Outcomes are recorded in source order regardless of completion order. Each branch retries independently using its own per-step retry config (BL-028b). If any branch fails with an `on_error` policy that does *not* allow continuation (`continue` / `log_warn`), the executor aborts *after* the group completes — in-flight siblings are not cancelled, but the next sequential step (and everything after it) is emitted as `Skipped`. `[[steps.tasks]]` nested-array shape from PRD-16 §9.3 is intentionally not adopted here — it would require a `Step` schema change and IPC drift; the existing flat `Step.parallel: bool` field (already parsed in `lib.rs:170` since the original scaffold) is the wire shape. Shipped in `crates/nexus-workflow/src/executor.rs` (`run_workflow_with_variables` walker + `run_step` helper + `skipped_outcome` / `continue_on_error` helpers). New dep: `futures = "0.3"` on nexus-workflow (mirrors nexus-agent's existing usage). Coverage: `cargo test -p nexus-workflow` 102 passing (was 93; +9 net) — `parallel_group_runs_concurrently` (paused-time check that two 200ms branches complete in ~200ms not ~400ms), `parallel_outcomes_preserve_source_order` (slow first / fast second; finish order ≠ outcome order), `parallel_branch_failure_aborts_subsequent_sequential_step`, `parallel_branches_run_even_when_a_sibling_fails` (no mid-flight cancellation), `parallel_branch_failure_with_continue_does_not_abort`, `parallel_branch_retries_independently` (flaky branch retries 3x while steady branch ran once), `mixed_sequential_and_parallel_groups_walk_correctly` (seq → group → seq → group), `sequential_step_failure_skips_subsequent_parallel_group`. `cargo clippy -p nexus-workflow --all-targets -- -D warnings` clean.

### BL-029: Multi-window / detachable panels

**Source**: PRD-17 §6.
**Effort**: Medium. **Crate**: `shell/src-tauri/` + `shell/src/host/`.
Spec calls for per-Leaf detachment into a separate `WebviewWindow`. Today the shell ships single-window. Not in REQUIRED-FOR-FORMAL-RELEASE.md, so it lands here. Web/mobile platform targets (also PRD-17) remain explicitly deferred and are not BL items.

- **BL-029 Phase 2 — leaf rendering inside the popout webview**: stand up a popout-mode shell that mounts the actual View instance for the popped-out leaf rather than the placeholder in [`PopoutShell.tsx`](../../shell/src/shell/PopoutShell.tsx). Each popout webview is its own JS context so a parallel `boot()` (smaller scope: skip community-plugin scan, render single-leaf workspace tree) is required to register the view creators. Open design questions to resolve before coding: (a) which plugins must boot inside the popout — likely the editor, search, file-explorer, and any plugin contributing the popped-out viewType, but full parity is the safer default; (b) cross-window sync semantics — file edits in the popout need to invalidate the main window's preview; the shared kernel handles file-watcher events but the active-leaf event bus is per-window; (c) close-while-editing — if the popout has unsaved state and the user closes via OS-X, what's the rescue? (d) main-window reload while popouts are alive — `window-state` plugin already restores the main window; popouts have to reconcile via `restoreFloatingWindows` against potentially-stale leaf ids. **Effort**: Medium-large.

- **BL-029 Phase 1 — window-management primitives + workspace-store API** _(shipped 2026-04-29)_: ships the foundational multi-window plumbing without touching the per-leaf rendering path inside the popout webview (deferred to Phase 2). Five new Tauri commands in [`shell/src-tauri/src/windows.rs`](../../shell/src-tauri/src/windows.rs) — `popout_window`, `close_popout_window`, `list_popout_windows`, `get_popout_window_bounds`, `set_popout_window_bounds` — drive `WebviewWindowBuilder` to spawn / close child webviews under a `popout-<id>` label namespace. Ids are validated against `[A-Za-z0-9_-]{1,128}` so a hostile / corrupt id can't smuggle path or query separators into the webview URL. `popout_window` builds `index.html?popout=<fwId>&leaf=<leafId>` and the frontend's `main.tsx` short-circuits the full plugin-load path when `?popout` is present, mounting [`PopoutShell.tsx`](../../shell/src/shell/PopoutShell.tsx) instead. Workspace-store side: `WorkspaceJSON.floating?: SerializedFloating[]` is now serialized + hydrated (kept optional so existing `.forge/workspace.json` files are unaffected); new pure mutations `workspace.popoutLeaf(leafId, bounds?)`, `workspace.closeFloatingWindow(id)`, `workspace.setFloatingWindowBounds(id, bounds)`, `workspace.findFloatingWindow(id)` move a leaf out of its parent Tabs into a `FloatingWindow` node with bounds metadata; persistence schema validates that every entry in `floating[]` has `kind === 'floating'` so a stray Tabs / Split is rejected outright. Tauri-side I/O is isolated in [`popoutWindowBridge.ts`](../../shell/src/workspace/popoutWindowBridge.ts) (`popoutLeaf`, `closePopout`, `reportPopoutBounds`, `restoreFloatingWindows`) so the workspaceStore stays free of `@tauri-apps/api` imports and unit-tests run under `node:test` without a Tauri runtime. `restoreFloatingWindows` reconciles persisted state against `list_popout_windows` on boot: missing popouts are reopened, orphan windows (alive in Tauri but absent from the store) are closed. Coverage: 7 cargo unit tests in `windows.rs` (id validation happy + 7 rejection cases incl. URL-smuggle separators, label prefix, URL builder with/without leaf, empty leaf coerced to absent, bounds + snapshot serde shape) plus 11 new node:test cases across `workspaceStore.test.ts` + `persistence.test.ts` (popoutLeaf round-trip + idempotency + bounds + reparenting, setFloatingWindowBounds emits change exactly when bounds differ, closeFloatingWindow disposes leaves + clears active id, serialize omits empty `floating`, hydrate round-trips bounds, schema rejects non-floating entries / non-array field). `pnpm --filter nexus-shell typecheck` clean; `pnpm --filter nexus-shell test` 537 still passing (in-tree harness untouched); `node --test src/workspace/workspaceStore.test.ts` 9 → 20 passing; `node --test src/workspace/persistence.test.ts` 8 → 11 passing. **Phase 2 follow-up**: render the actual leaf View inside the popout's React tree (currently `PopoutShell.tsx` is a placeholder) and add a "Pop out" affordance to the tab context menu.

_BL-030 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-031 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

## Future-direction items minted into the backlog (2026-04-28)

> The four future-direction tracks were brought into the implementation plan on 2026-04-28. The IDs below carry their FD doc as design rationale; the original entries in the "Future directions" section now point here. Effort scale: S ≈ ½–2 days, M ≈ 3–10 days, L ≈ 2+ weeks.

### BL-032: Cmd+I command-anywhere AI overlay

**Source**: [../AI-AMBIENT-COPILOT-PLAN.md](../AI-AMBIENT-COPILOT-PLAN.md) pattern 1.
**Effort**: Medium. **Crate / Package**: `shell/src/plugins/nexus/ai/` (new) + `shell/src/host/` (registry hook).
A modal overlay invocable from any focused surface (editor, bases, canvas, terminal, files) that takes a free-form prompt and routes through the shared context-assembly service. Surface-specific context contributors (current selection, current file, current row, current canvas node) register adapters at activation. **Gates BL-010 / BL-011 UX** — land Cmd+I before the CLI surfaces so they share UX and engine. Scope excludes citations (BL-038) and right-click actions (BL-035), which compose on top.

_BL-033 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-034 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

### BL-035: Right-click AI actions + block AI actions (shared registry)

_BL-035 shipped 2026-04-29 — see BACKLOG_COMPLETED.md._

### BL-036: AMB margin suggestions + inline correction

**Source**: [../AI-AMBIENT-COPILOT-PLAN.md](../AI-AMBIENT-COPILOT-PLAN.md) patterns 6 + 9.
**Effort**: Medium.
Background pass over the active document surfaces non-blocking margin glyphs ("rephrase", "fact-check", "tighten") that expand into accept/dismiss diffs in the gutter; inline correction is the same engine for typos / grammar with squiggle decoration. Hard prereq: BL-016 tool-calling so suggestions can call domain tools (e.g. dictionary).

_BL-037 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-038 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-039 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-040 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-041 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-042 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-043 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-044 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

### BL-045: MEM auto-enrichment on save

_BL-045 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

### BL-046: MEM code-aware capture

**Source**: [../AI-MEMORY-LAYER-PLAN.md](../AI-MEMORY-LAYER-PLAN.md) piece 5.
**Effort**: Medium.
Extends BL-043 capture: when the source is detectably code (file path with code extension, syntax-highlightable content, IDE selection), capture preserves language fence + path + line range. Adds a "from project" filter to BL-044 recall. No AI dependency at capture time.

### BL-047: MEM scheduled digests

_BL-047 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

### BL-048: NB drag-to-embed into canvas

**Source**: [../notion-block-ux-plan.md](../notion-block-ux-plan.md) out-of-scope follow-up.
**Effort**: Medium. **Crate / Package**: `shell/src/plugins/nexus/editor/` + `shell/src/plugins/nexus/canvas/`.
Block-handle (six-dot grip) drag target accepts drops onto an open canvas, creating a markdown-embed node referencing the source block by id. Cross-plugin contract: a canvas-side drop handler reading a typed `application/x-nexus-block-ref` payload. Soft-blocked on the block-id stability ADR (see Phase-0 design notes) — string-based refs by file+visit-order break on doc edits.

### BL-049: NB block-links navigator (`[[…#^block-id]]`)

**Source**: [../notion-block-ux-plan.md](../notion-block-ux-plan.md) out-of-scope follow-up.
**Effort**: Medium. **Crate / Package**: `nexus-editor` (resolver) + `shell/src/plugins/nexus/editor/` (navigation UX).
Click-through and back-link surfacing for block-anchored links. **Hard-gated on the block-id stability ADR** — without stable cross-session ids, block links rot on every edit downstream of an insert.

### BL-050: NB side-margin comments subsystem

**Source**: [../notion-block-ux-plan.md](../notion-block-ux-plan.md) out-of-scope follow-up.
**Effort**: Large.
Persistent comment threads anchored to block ids, rendered in a side margin pane with reply / resolve / mention. Storage in `.forge/comments/<file>.json`. **Hosted inside BL-029 multi-window** when that lands. Same block-id stability dependency as BL-048 / BL-049.

### BL-051: NB multi-cursor from multi-block selection

**Source**: [../notion-block-ux-plan.md](../notion-block-ux-plan.md) out-of-scope follow-up.
**Effort**: Medium. **Crate**: `shell/src/plugins/nexus/editor/cm/`.
Promote a multi-block selection (from BL's existing `blockSelection.ts`) into a CM6 multi-cursor where each cursor sits at the same offset within its respective block. Editor-only; no kernel surface.

### Verification notes (no BL ID — informational)

- **ADR-0009 keyring hard-fail enforcement** — ADR mentions a `NEXUS_NO_KEYRING=1` escape hatch, but bootstrap-side enforcement was not located in this audit. Either confirm enforcement (and document the location) or file as a follow-up in OPEN-ITEMS.md if real.
- **PRD-04a MockPluginContext / MockEventBus** — referenced in template tests as TODO but not yet exposed from `nexus-plugin-api`. Low priority; community plugin authors are not yet writing many tests, and the issue surfaces only when someone tries.

## Decisions — PRD-04 audit (2026-04-17)

## Design notes — 2026-04-28

- **Global cross-surface undo is a non-goal.** Considered alongside BL-030. Per-surface undo is the idiom in VS Code / Obsidian / IntelliJ; a unified Cmd+Z spanning editor + canvas + bases + file ops creates ambiguous "what does this undo right now" behaviour and would require every mutating IPC handler to register an inverse op against the file-as-truth + IPC-only invariants. The right primitive for cross-surface time-travel in this architecture is git-based history (point-in-time restore via the existing commit graph) rather than a unified action stack. New BL items for undo should be scoped to a single surface.

### Phase-0 ADRs (gating the implementation plan)

Two design decisions sit on the critical path of the multi-phase rollout. Both are Phase-0 deliverables — the rest of the plan depends on the answers.

- **ADR-pending: block-id stability strategy.** Today `deterministic_block_id` keys on `(file_path, visit_order, block_type)`, so an insert mid-document re-numbers every downstream block on reload. **Gates BL-048 (drag-to-embed), BL-049 (block-links navigator), BL-050 (side-margin comments)** — all rely on cross-session stable block ids. Two viable approaches were enumerated in the Notion-block-UX work: (a) HTML-comment stamping inside markdown (visible in source, survives raw-text edits, ugly), (b) out-of-band `.forge/blocks.json` sidecar (clean source, but needs reconciliation when files are edited outside Nexus). Neither has been chosen because no current feature forces the issue. Choose at Phase-0; a half-day decision unblocks three downstream tracks.

- **ADR-pending: embedding backend selection.** BL-019 was previously "nice-to-have"; it now gates **nine downstream tracks** (BL-038 / BL-039 / BL-040 / BL-041 / BL-044 / BL-045 / BL-047 plus the BL-010 reshape and BL-011 / BL-034 retrieval-augmented variants). Candidates from the BL-019 entry: fastembed-rs, candle, sqlite-vec's bundled gguf path. Choosing wrong (e.g. a backend that doesn't ship cleanly cross-platform, or that bloats the binary past acceptable, or whose model quality is too low to make BL-040 useful) costs the schedule weeks. Compare on (1) model quality vs sentence-transformers baseline, (2) RAM footprint at idle and under indexing load, (3) cold-start time, (4) cross-platform binary cost (Linux/macOS/Windows; consider WebView constraints for shell), (5) license. Phase-0 deliverable.

---

## Implementation plan (2026-04-28)

> Phased rollout for every non-deferred BL item including the future-direction items minted as BL-032..BL-051 above. Cross-references all live in those entries; this section is the schedule.

### Agent-load assumptions

- **One agent ≈ 1–3 days of focused work, single tractable PR.** Items rated >medium must split into multiple agent-sized chunks (splits are listed per-item below).
- **2 concurrent foreground agents + 1 background long-runner.** The fg slots are sized so the human review queue stays drainable; the bg slot is reserved for multi-week work (F-8.1.1 in particular).
- **Agents that overlap files waste work in merges**, so file-conflict groups must serialize within their group.
- Retune assumptions: 1 fg + 0 bg roughly doubles the timeline; 3 fg + 1 bg lets BL-022 / BL-029 / BL-037 land earlier and compresses Phases 3–6 by ~3 weeks.

### File-conflict groups (serialize within group)

| Group | Items |
|---|---|
| Bases plugin | BL-015 → BL-030 → BL-031 |
| nexus-cli AI subcommands | BL-010 → BL-011 |
| nexus-mcp client | BL-023 → BL-025 |
| nexus-mcp server | BL-042 (distinct from client group above) |
| Skills | BL-021 → BL-022 |
| nexus-ai (Cargo + provider mods) | BL-016, BL-019 — keep one full PR apart |
| Shell host / sandbox | F-8.1.1 → F-8.1.2 |
| AI overlay surface | BL-032 → BL-033 → BL-034 |
| Memory inbox surface | BL-043 → BL-046 |

### Hard dependency chain

| Prereq | Unblocks |
|---|---|
| BL-016 tool-calling | BL-010, BL-011, BL-027, BL-035, BL-036, BL-044 |
| BL-019 embeddings | BL-038, BL-039, BL-040, BL-041, BL-044, BL-045, BL-047, plus BL-010/11/34 retrieval variants |
| BL-013 stream convention | future plugin streaming work |
| BL-015 trash view | BL-030 (reuses row-restore code path) |
| BL-030 undo stack | BL-031 (paste = one undo step) |
| BL-032 Cmd+I overlay | BL-010 / BL-011 / BL-033 / BL-044 (shared UX) |
| BL-041 indexing daemon | BL-045 (auto-enrichment reads the index) |
| F-8.1.1 iframe sandbox | F-8.1.2, marketplace |
| Block-id stability ADR | BL-048, BL-049, BL-050 |

### Phased rollout

| Phase | Wks | Agent A (fg) | Agent B (fg) | Agent C (bg) | Phase exit criteria |
|---|---|---|---|---|---|
| **0 — Quick wins + ADRs** | 1.5 | settings ×5 + BL-009 + BL-015 | (idle / pulls Phase-1 prep) | block-id ADR + embedding-backend ADR | both ADRs signed and recorded under "Decisions"; trash view live in bases; foundations clear for Phase 1 |
| **1 — Foundations** | 6 | **BL-016** (split ×3) | **BL-013** stream convention + **BL-032** Cmd+I overlay | **F-8.1.1** kickoff (split ×5; per-plugin migration posture — see below) | BL-016 merged → unblocks AI surfaces; BL-032 lands → unblocks BL-010/11; F-8.1.1 sandbox scaffold reachable |
| **2 — Bases + AI CLI/UI** | 4 | BL-030 → BL-031 → **BL-043** quick-capture hotkey | BL-010 + BL-034 ghost suggestions (paired engine) → BL-011 | F-8.1.1 cont. | bases polish complete; shared chat + completion engine live in CLI and editor; global capture hotkey live |
| **3 — Skills + MCP client + small AMB** | 5 | BL-021 (split ×4) → BL-022 | BL-023 → BL-025; BL-033 chips/switcher slots in | F-8.1.1 wraps; **F-8.1.2** | skills composition lands; MCP client gains WS/SSE + auth |
| **4 — Heavy AI core** | 8 | **BL-019** (split ×4) | **BL-027** agent loops (split ×5) | BL-035 right-click + block-AI actions | BL-019 unblocks all retrieval consumers; BL-027 unlocks orchestrated agents |
| **5 — Retrieval consumers** | 5 | BL-040 semantic search → BL-039 auto-links → BL-038 citations | BL-041 indexing daemon → BL-045 auto-enrichment → BL-044 recall | BL-047 scheduled digests | the BL-019 dependency tail drains |
| **6 — Heavyweights + multi-window** | 8 | BL-028 workflow umbrella (split ≥6) | BL-029 multi-window → BL-037 timeline → BL-050 side-margin comments | BL-042 Nexus-as-MCP-server | multi-window opens, panes follow; workflow gains every spec'd trigger |
| **7 — Editor + Notion polish** | 6 | BL-012 DB query blocks (split ×5) | BL-049 block-links → BL-051 multi-cursor → BL-048 drag-to-embed | BL-046 code-aware capture; BL-036 margin / inline correction | tail polish; backlog drained to deferred-only items |

Cumulative: ~44 weeks raw, ~50–55 with PR-review buffer at the assumed 2 fg + 1 bg slot budget.

### Sub-task splits (items >medium)

| BL | Split |
|---|---|
| BL-016 | (1) `ToolRegistry` + `ToolExecutor` core, (2) Anthropic + OpenAI tool-call wire format, (3) Ollama tool-call format + dispatch loop |
| BL-019 | (1) backend impl (per ADR), (2) `EmbeddingModel` trait + cache, (3) RAG wire-up, (4) batch indexer hook for BL-041 |
| BL-021 | (1) parse `depends_on`, (2) topo + cycle detection, (3) prompt-fragment merge order, (4) conflict-warning UX |
| BL-027 | (1) `AgentOrchestrator` skeleton, (2) `delegate`, (3) `parallel`, (4) `pipeline`, (5) shared scratch state + replay hooks |
| BL-028 | one agent per primitive: webhook trigger → git_event → mcp_event → parallel scheduler → retry/backoff → AI step types → templates |
| BL-012 | (1) executor over `apply_view`, (2) CM6 widget, (3) decoration plumbing, (4) undo integration, (5) filter/sort UX |
| F-8.1.1 | (1) iframe scaffold + sandbox flags, (2) postMessage protocol, (3) `NexusPluginContext` proxy, (4) per-plugin migration via `manifest.toml` `sandbox: "iframe" \| "legacy"` flag, (5) CSP + tests. Per-plugin migration posture (decided 2026-04-28) — community plugins keep working during the multi-week build window; cost is +1–2 wks vs hard cutover. |

### Risks tracked

1. **Phase-2 lock-in.** BL-010 / BL-011 / BL-034 share an engine. If BL-032 (Cmd+I) shifts after Phase-1, three tracks rework.
2. **BL-019 is the single biggest schedule bet.** Nine tracks queue behind it; a backend mistake costs weeks. The Phase-0 ADR is non-negotiable.
3. **BL-029 promotion** means earlier multi-window, which means earlier per-window plumbing problems for plugin lifecycle. Worth a lightweight design pass before Phase-6 begins.
4. **F-8.1.1** runs 1–2 eng-months in the background. If it slips into Phase-4, BL-035 (right-click in iframe-sandboxed plugins) gets harder to test.
5. **BL-022 absorbs MEM "code-aware capture" UI patterns** in Phase 3 — make sure the skill-editor surface is pluggable enough to host them rather than blocking on a separate capture UI.

### Phase-0 entry / exit checklist

- [ ] Block-id stability ADR drafted, reviewed, recorded under "Decisions".
- [ ] Embedding-backend ADR drafted with the 5-axis comparison (quality / RAM / cold-start / binary cost / license), recorded under "Decisions".
- [ ] BL-009 mermaid whole-file viewer merged.
- [ ] BL-015 bases trash view merged.
- [ ] Settings extraction queue (5 items) merged as one PR.
- [ ] No outstanding regressions in `cargo test --workspace` / `pnpm --filter nexus-shell test` / `scripts/check_ipc_drift.sh`.

(BL-043 quick-capture hotkey moved to Phase 2 — Tauri global-hotkey plumbing is a 1–2 day task disguised as "small" and would eat into ADR review.)
