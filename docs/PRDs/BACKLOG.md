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

- [x] **Zoom settings schema** _(shipped)_ — `ui.zoomStep` / `ui.zoomMin` / `ui.zoomMax` / `ui.zoomDefault` registered in `shell/src/plugins/core/zoom/index.ts` with bounds, step, and reset target read through `api.configuration.getValue` + `onChange`.
- [x] **Notification durations schema** _(shipped)_ — `ui.notificationDurationMs` (notificationService), `ui.fileCreationNotificationMs` (fileExplorer), `ui.commandSaveNotificationMs` + `ui.commandCopiedNotificationMs` (terminal `index.ts` schema; SavedCommandsView reads via `useConfigValue`), `ui.copiedNotificationMs` (`nexus.ai`'s `index.ts`; ChatView reads via `useConfigValue`).
- [x] **Search / palette result limits** _(shipped)_ — `search.maxResultsLimit` (schema in `shell/src/plugins/nexus/search/index.ts`, read in `searchRuntime.ts`); `commandPalette.maxResultsLimit` (schema in `shell/src/plugins/core/commandPalette/index.ts`, read by `match.ts`).
- [x] **Long-running operation timeout consolidation** _(shipped)_ — `LONG_RUNNING_OP_TIMEOUT_MS` defined once in `shell/src/plugins/nexus/constants.ts` and consumed by `nexus/agent/index.ts` (`RUN_TIMEOUT_MS`) and `nexus/workflow/index.ts` (`RUN_TIMEOUT_MS`); `SERVICE_CONNECT_TIMEOUT_MS` similarly consumed by `nexus/mcp/index.ts`.
- [x] **Buffer / event caps** _(shipped)_ — `PROCESS_EVENTS_CAP` named in `processesStore.ts`; `UNDO_HISTORY_CAP` lives in `shell/src/plugins/nexus/constants.ts` and is shared by `bases/basesStore.ts` + `canvas/canvasStore.ts` so the user-perceptible undo depth is consistent across surfaces.

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

- **BL-012 split 5 — filter / sort UX on the rendered widget** _(shipped 2026-04-30)_: closes BL-012 end-to-end. The widget header now renders an editable summary (view-type label, current-filter chips, current-sort chips, plus an inline "Add filter" / "Add sort" form); each chip carries a `×` removal button. New `serializeDatabaseViewSpec(databasePath, config)` is the inverse of [`parseDatabaseViewBlocks`](../../shell/src/plugins/nexus/editor/cm/databaseViewDecorations.ts) — emits the bare `[[{db:<path>}]]` form when no params are needed, percent-encodes filter / sort / hide values so spaces and `&` / `=` survive a round-trip, and rejects an empty path. Write-back is markdown-first: `onUpdateConfig(newConfig)` builds the new spec via `serializeDatabaseViewSpec` and dispatches a CM `changes` transaction replacing the old block range. The decoration extension (`databaseViewExt`) gains a small mutable `viewRef` closure populated by the ViewPlugin's `create` hook so the StateField's `update(tr)` — which has no native access to the view — can pass the active `EditorView` through to the widget's `onUpdateConfig` closure. The first decoration set is built before the ViewPlugin runs (state-field create runs first), so the ViewPlugin dispatches a one-shot `databaseViewInvalidate` on mount to rebuild with the now-captured view; subsequent rebuilds piggyback on `docChanged` / `selection` / `databaseViewInvalidate` like before. Source-range relocation on write-back: the `from` / `to` captured at scan time may have shifted by the time the user clicks the chip, so the callback re-scans the live state and matches by `(databasePath, range-overlap)` before issuing the change — concurrent edits elsewhere in the doc don't corrupt the rewrite. CSS for chips, inline form, and bordered header / body sections in [`livePreview.css`](../../shell/src/plugins/nexus/editor/livePreview.css) using existing theme tokens. Coverage: 9 new node:test cases (2 in `databaseViewWidget.test.ts` for the editable header — chip removal + form submit appending a new filter, read-only-when-`onUpdateConfig`-absent — and 7 in `databaseViewDecorations.test.ts` — bare-path serialiser, table + filters + sorts round-trip, kanban round-trip, percent-encoding of `&` / `=` / spaces, empty-path rejection, plus two view-bound builder tests covering the full `parse → onUpdateConfig → serialize → CM transaction → re-parse` cycle and the read-only-when-no-view fallback). `pnpm --filter nexus-shell typecheck` clean; `pnpm --filter nexus-shell lint` 0 new warnings; `pnpm --filter nexus-shell test` 601 passing (was 592; +9 split-5 tests). **BL-012 splits 1–5 complete; close-out**: a follow-up that's *not* a split — extend the editor crate's markdown parser/serializer to recognise the `[[{db:…}]]` syntax natively (today the editor block-tree treats it as an `Embed`; the CM widget side already handles it correctly because it scans the source text directly). Once that lands, `BlockType::DatabaseView` round-trips through `MarkdownSerializer::serialize` / `MarkdownParser::parse` and the editor's undo tree captures spec edits as block-level transactions instead of free-text replacements.

- **BL-012 split 4 — cache invalidation on external `.bases` edits** _(shipped 2026-04-30)_: closes the staleness gap from split 3 — when another window, the CLI, or the canvas-side bases UI mutates a `.bases` directory, every inline `[[{db:…}]]` widget targeting that base flushes and recomputes without a doc edit. New `DatabaseViewCache.invalidatePath(databasePath)` drops every key whose stable JSON prefix is `${databasePath} ` (the trailing-space terminator stops `Tasks.bases` from also evicting `Tasks.bases.archive`). New `pathToBasePath(relpath)` helper resolves both the `Tasks.bases` directory itself and any path inside it (`Tasks.bases/records.json`, `nested/Board.bases/views/board.json`); paths outside any `.bases/` segment return `null`. `databaseViewExt`'s previously-empty `ViewPlugin` shell is now a real `makeBasesChangeWatcher(view, deps)` that subscribes to the `com.nexus.storage.file_` topic prefix via the new injected `KernelEventSubscriber` dependency — covering `file_created` / `file_modified` / `file_deleted` / `file_renamed` in one subscription. `file_renamed` carries `from` + `to`; both are mapped to base paths so a rename moving a file out of (or into) a base flushes correctly. Subscription is async so the watcher tracks a `disposed` flag and clears the unsubscribe lazily; if the view tears down before the subscribe promise resolves the unsubscribe still runs once it lands. The `KernelEventSubscriber` is threaded from `nexus.editor`'s `activate()` (which has access to `api.kernel`) through `EditorRuntime.kernelEvents` to the live-mode extension stack so test drivers without a kernel mount stay quiet (the watcher returns a no-op handle when `deps.events` is absent). Coverage: 7 new node:test cases — 3 against `DatabaseViewCache` (path-prefix invalidation, no-spurious-recompute when nothing matched, no partial-match against a longer path), 2 against `pathToBasePath` (inside-bases mapping including the directory itself, null for outside / look-alikes), and 5 against `makeBasesChangeWatcher` (file_modified flushes the right keys + dispatches `databaseViewInvalidate`, unrelated edits skip dispatch + leave the cache untouched, file_renamed flushes via `from`, no-op when `events` is absent, plus the dispatch-spec normaliser that handles single-effect-or-array). `pnpm --filter nexus-shell typecheck` clean; `pnpm --filter nexus-shell lint` 0 new warnings; `pnpm --filter nexus-shell test` 592 passing (was 583; +9 split-4 tests across the cache + decorations files; the +1 test wrapper file count is unchanged since both new tests live in existing wrapper-mapped files). **Split 5 follow-up**: filter / sort UX surfaced on the rendered widget header — likely a popover that mutates the inline source via a CM transaction so the markdown stays the truth (write-through pattern matches BL-029 popout / BL-050 comments rather than out-of-band state).

- **BL-012 split 3 — decoration plumbing through CM6** _(shipped 2026-04-30)_: closes the inline-rendering loop end-to-end. New module [`shell/src/plugins/nexus/editor/cm/databaseViewDecorations.ts`](../../shell/src/plugins/nexus/editor/cm/databaseViewDecorations.ts) defines a CM6 `StateField<DecorationSet>` that scans the doc text for `[[{db:<spec>}]]` and emits `Decoration.replace({ widget: DatabaseViewWidget, block: true })` over the matched range — but only when the cursor is *not* on that line, mirroring `livePreviewDecorations.ts`'s active-line reveal so the user can position into the source to edit the spec. The pure parser `parseDatabaseViewBlocks(text, offset?)` recognises three forms: bare `[[{db:Tasks.bases}]]` (table view), structured `[[{db:Board.bases?view=kanban&group=status}]]` (Kanban / Calendar / Gallery via the `view=` query param + layout-specific field name), and filter / sort lists via repeated `filter=` / `sort=` / `hide=` query params (percent-decoded once). Filter / sort syntax is the same `field <op> value` / `field [asc|desc]` vocabulary the BL-012-split-1 Rust parser accepts. Malformed specs (empty path, `..` traversal, unknown `view=` value) are surfaced inline as a `cm-md-dbview-syntax-error` mark with a tooltip rather than silently dropped, so the user sees the error in place. Block-decoration semantics force the `StateField` source (CM rejects block decorations from `ViewPlugin`); the field is paired with an `EditorView.atomicRanges` provider so left/right arrow skips over a hidden block cleanly. A `databaseViewInvalidate` `StateEffect` is wired up but not yet fired — split-4 will subscribe to `com.nexus.storage.bases.changed.*` and dispatch it after calling `databaseViewCache.invalidate(key)` for the affected base path so external `.bases` edits flush through to the inline grid without waiting for a doc edit. Wired into [`EditorView.tsx`](../../shell/src/plugins/nexus/editor/EditorView.tsx) alongside `livePreviewExt()` in live-preview mode only (the untitled / non-markdown fallback path has no kernel client to call). Coverage: 13 new node:test cases re-exported via [`shell/tests/database-view-decorations.test.ts`](../../shell/tests/database-view-decorations.test.ts) — parser plain-markdown no-op, bare path, repeated-param filters/sorts, kanban / calendar layout mapping, error path (empty / unknown view / traversal), regex reentrancy across consecutive scans, the `offset` parameter for line-relative scans, plus builder tests for off-line replace, on-line reveal, syntax-error mark, and multi-block ordering. `pnpm --filter nexus-shell typecheck` clean; `pnpm --filter nexus-shell lint` 0 new warnings; `pnpm --filter nexus-shell test` 583 passing (was 571; +12 decoration tests; the +1 wrapper re-export is counted at the file level). **Splits 4+5 follow-up**: undo integration via `databaseViewInvalidate` + base-change event subscription (split 4); inline filter / sort UX surfaced on the rendered widget header (split 5) — likely a popover that mutates the source text via a CM transaction so the source stays the truth.

- **BL-012 split 2 — CM6 widget + result cache** _(shipped 2026-04-30)_: ships the renderer half of the foundation. New module [`shell/src/plugins/nexus/editor/cm/databaseViewWidget.ts`](../../shell/src/plugins/nexus/editor/cm/databaseViewWidget.ts) defines `DatabaseViewWidget` (a CM6 `WidgetType` whose `eq()` is keyed on the `(databasePath, stable-config-JSON)` pair so decoration rebuilds during selection moves don't re-run IPC), `DatabaseViewCache` (an LRU memo cache with concurrent-fetch dedupe, error caching, and an `invalidate(key)` hook split 4 will call when the underlying base changes), and the rendering helpers (`renderApplied` / `renderFlat` / `renderGrouped`, plus `effectiveFields` which falls back from the view's explicit field list → schema field-map order → record keys minus `id` / `deletedAt`). New typed methods + wire types on [`kernelClient.ts`](../../shell/src/plugins/nexus/editor/kernelClient.ts): `executeDatabaseView(path, config)` and the `DatabaseViewConfig` / `AppliedView` / `AppliedRecord` / `AppliedGroup` / `AppliedLayout` / `ExecuteDatabaseViewResponse` interface set, mirroring the Rust serde shape (snake_case, `kind` discriminator on the `view_type` enum). Stylesheet additions in [`livePreview.css`](../../shell/src/plugins/nexus/editor/livePreview.css) cover the table, grouped-section, pending, empty, and error states using the theme's existing `--bg-raised` / `--line-soft` / `--risk` tokens. Coverage: 14 new node:test cases re-exported via [`shell/tests/database-view-widget.test.ts`](../../shell/tests/database-view-widget.test.ts) — `widgetKey` stability across config key reordering and discrimination across path / filters / view_type, `effectiveFields` precedence (view-list → schema map → record keys), `DatabaseViewCache` concurrent-fetch dedupe + invalidate-forces-refetch + error-caching, widget pending → resolved render path with the right `<th>` / `<td>` content, cache-hit-skips-pending across two widget instances (simulating decoration rebuild after a selection move), error-box rendering + `onError` routing on IPC rejection, grouped layout with per-section heading + record counts, and the `eq()` contract. Plus 1 new kernel-client test pinning the snake_case `{ database_path, view_config }` wire shape. `pnpm --filter nexus-shell typecheck` clean; `pnpm --filter nexus-shell lint` 0 new warnings; `pnpm --filter nexus-shell test` 571 passing (was 557; +14 split-2 + +1 wrapper re-export entry). **Split 3 follow-up**: decoration plumbing — extend `livePreviewDecorations.ts` to walk for `BlockType::DatabaseView` blocks (or the `[[{db:query}]]` syntax once the parser learns it) and emit `Decoration.replace` ranges carrying these widgets, alongside a `ViewPlugin` watcher that subscribes to `com.nexus.storage.bases.changed.*` and calls `databaseViewCache.invalidate(key)` so external `.bases` edits refresh the inline grid.

- **BL-012 split 1 — query executor over `apply_view`** _(shipped 2026-04-30)_: ships the kernel-side foundation that the CM6 widget (split 2) will sit on top of. New module [`crates/nexus-editor/src/database_view.rs`](../../crates/nexus-editor/src/database_view.rs) translates the editor-side [`DatabaseViewConfig`](../../crates/nexus-editor/src/block.rs) — which keeps `filters` / `sorts` as user-typed strings to stay round-trip-clean through the markdown serializer — into a structured [`nexus_types::bases::BaseView`](../../crates/nexus-types/src/bases.rs) that [`nexus_database::views::apply_view`](../../crates/nexus-database/src/views.rs) consumes. Operator vocabulary mirrors `nexus_database::views::matches_filter` (`eq`/`neq`/`gt`/`gte`/`lt`/`lte`/`contains`/`icontains`/`starts_with`/`ends_with`/`is_empty`/`is_not_empty`); symbolic forms (`=`, `!=`, `>=`, `<=`, `>`, `<`) map onto the canonical names. Word-operator matching is space-padded so a field like `contains_pii` doesn't get clipped — a reproducible sharp edge in the `nexus-storage` analogue that the editor crate avoids by re-implementing the parser locally rather than depending on `nexus-storage`. New IPC handler `HANDLER_EXECUTE_DATABASE_VIEW = 12` (`com.nexus.editor::execute_database_view`, async-only) takes `{ database_path, view_config }` and returns `{ applied, schema }` — the schema is bundled so the upcoming grid widget can format cells without a second IPC roundtrip. The handler is read-only: it touches no editor session and emits no `com.nexus.editor.changed.*` event. Implementation chain: `ipc_call("com.nexus.storage", "base_load")` → `config_to_view` → `ipc_call("com.nexus.database", "apply_view")`. Wired through [`nexus-bootstrap`](../../crates/nexus-bootstrap/src/lib.rs) so CLI / TUI / shell all reach the executor uniformly. Coverage: 13 cargo unit tests in `database_view::tests` covering symbolic / word / suffix operator parses, the `contains_pii` field-name regression, sort default-asc + explicit-desc, error paths (unrecognised filter, empty field, bad direction, multi-token sort), kanban `column_by` precedence over generic `group_by`, and propagation of filter parse errors through `config_to_view`. Async handler itself isn't unit-tested — its contract requires a wired `KernelPluginContext` with both `com.nexus.storage` and `com.nexus.database` registered, which the editor crate doesn't have access to in isolation; end-to-end coverage lands with split 2 once the CM6 widget exercises the full path. `cargo test -p nexus-editor` 201 passing (was 188; +13). `cargo clippy --workspace --all-targets -- -D warnings` clean. **Split 2 follow-up**: CM6 inline widget rendering the `AppliedView` as a virtualized grid under `shell/src/plugins/nexus/editor/`, calling `execute_database_view` on `BlockType::DatabaseView` mount + `com.nexus.storage.bases.changed.*` event reactively.

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

- **BL-029 Phase 2 — leaf rendering inside the popout webview**: stand up a popout-mode shell that mounts the actual View instance for the popped-out leaf rather than the placeholder in [`PopoutShell.tsx`](../../shell/src/shell/PopoutShell.tsx). Each popout webview is its own JS context so a parallel `boot()` (smaller scope: skip community-plugin scan, render single-leaf workspace tree) is required to register the view creators. **Design decisions are recorded in [ADR 0020](../adr/0020-popout-window-architecture.md)** (2026-04-30) — full plugin parity minus marketplace/sandbox surface, sync exclusively through kernel events, close-as-tab-close (no save prompt; rely on continuous dirty flush + kernel-side sessions), main window is authoritative for `floating[]`. **Effort**: Medium-large. Split into 2a (close-event sync) + 2b (full leaf rendering in popout).

- **BL-029 Phase 2b — full leaf rendering in popout** _(shipped 2026-04-30)_: closes BL-029 Phase 2 end-to-end. The popout webview now boots the same `DEFAULT_ON` plugin set as the main window (ADR 0020 §1) so every `viewRegistry.register(...)` runs before hydration; community plugins, install-time consent, and the sandbox orchestrator are skipped — the popout `boot()` short-circuits after `host.loadAll`, sets `shellReady`, and returns. A new `popoutMode` context key is set on `contextKeyService` *before* plugin activation, so `nexus.workspace` can branch its `setRoot` on it: in popout mode the plugin no longer issues `init_forge` / `boot_kernel` / `shutdown_kernel` (the kernel is owned by the main window via Tauri managed state — calling `shutdown_kernel` from a popout would tear it down for everyone). The popout still publishes `rootPath` to its own `nexus.workspace` zustand store, which `PopoutShell` waits on before hydrating. Once `shellReady && rootPath !== null`, `PopoutShell` calls `loadWorkspace(rootPath)` (read-only — main is the sole writer), hydrates the popout's per-window `workspaceStore`, locates the FloatingWindow via `workspace.findFloatingWindow(fwId)`, walks its subtree via the new pure helper `findLeafInNode(node, leafId)`, and mounts the resolved `Leaf` through the existing `<LeafHost>` component. ADR 0020 §4 stale-leaf reconciliation: the popout fails closed — missing fwId, missing leafId, fwId-not-in-`floating[]`, or leafId-not-under-FW each render an explicit "Popout out of sync — close this window to continue" message rather than silently falling back. A live subscription to the workspace store's `floating` array also surfaces the error state if the main window closes the FW out from under us mid-session. Coverage: 3 new pure-logic tests in [`shell/src/shell/PopoutShell.test.ts`](../../shell/src/shell/PopoutShell.test.ts) covering `findLeafInNode` happy / miss / nested-Split paths (re-export shim picks them up via the `tests/popout-shell.test.ts` glob). `pnpm --filter nexus-shell typecheck` clean; `pnpm --filter nexus-shell lint` clean for the changed files; `pnpm --filter nexus-shell test` 557 passing (was 554; +3). The Tauri-runtime path (`installCloseHandshake`, plugin-activation in a real WebView) is exercised end-to-end and not unit-tested.

- **BL-029 Phase 2a — popout close-event sync** _(shipped 2026-04-30)_: ships the cross-window architecture from ADR 0020 §2/§3 end-to-end without yet rendering the leaf. The popout webview's `PopoutShell` now installs a `getCurrentWindow().onCloseRequested` hook that emits a global Tauri event `nexus:popout-closed` carrying the popout's `fwId` before the OS tears the window down. The main window's `boot()` registers a `listen('nexus:popout-closed', …)` handler that dispatches `workspace.closeFloatingWindow(fwId)` followed by a defensive `closePopoutTauri(fwId)` (idempotent — handles the race where the OS-X close has already finalised). Net effect: closing a popout via the OS-X button (or any other native close path) cleanly removes the matching `FloatingWindow` from `floating[]` so the next main-window reload no longer re-opens an orphan via `restoreFloatingWindows`. Pure-logic tests live alongside the implementation at [`shell/src/shell/PopoutShell.test.ts`](../../shell/src/shell/PopoutShell.test.ts) (re-exported via `shell/tests/popout-shell.test.ts` to fit the default test glob): event-name pin, search-string parsing for `isPopoutMode` (refactored to take an optional `search` string for testability since happy-dom doesn't update `window.location.search` on `replaceState`). The Tauri-runtime path (`installCloseHandshake`) isn't unit-tested — its contract is end-to-end and will be exercised once popout flows ship in 2b. `pnpm --filter nexus-shell typecheck` clean; `pnpm --filter nexus-shell lint` clean (no new warnings); `pnpm --filter nexus-shell test` 550 passing (was 546; +4). **Phase 2b follow-up**: full leaf rendering inside the popout — boot the same `DEFAULT_ON` plugin set in popout mode (skip community plugins / install-time consent / sandbox orchestrator / autosave per ADR §1), hydrate workspace.json read-only, locate the leaf via `findFloatingWindow(fwId)`, mount it via `LeafHost`. Stale-leaf reconciliation per ADR §4 — render an error state when the fwId or leaf doesn't resolve.

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

- **BL-046 phase 2 — recall "From project" filter chip** _(shipped 2026-04-30)_: closes BL-046 for the recall side. New module [`shell/src/plugins/nexus/recall/codeFilter.ts`](../../shell/src/plugins/nexus/recall/codeFilter.ts) defines the pure helpers `isCodeCaptureMatch(match)` (positive on any of three signals: `#code/<language>` tag emitted by phase 1, `File: <path>` header, language-tagged fence opener — each is sufficient on its own; the recall pane queries the capture inbox so the false-positive blast radius is bounded), `applyCodeFilter(matches, codeOnly)` (passthrough when off, stable-order filter when on), and `extractCodeLanguages(match)` (de-duped lowercase tag/fence list, exposed for the phase-3 per-language sub-chips). The existing `useRecallStore` gains a `codeOnly: boolean` field plus `setCodeOnly(value)` action that re-clamps `selectedIndex` to the visible result count when the chip toggles, so the keyboard-highlighted row never lands off-list. `open()` resets the flag to `false` so each hotkey press starts unfiltered. The `RecallOverlay` renders a new `<FilterChips>` row above the result list with a single binary "From project" pill (border-radius 999, accent fill when active, `aria-pressed` for accessibility), and the result-list selector applies `applyCodeFilter` at render time so toggling off restores the full result set without a re-fetch. Coverage: 14 new node:test cases re-exported via [`shell/tests/code-filter.test.ts`](../../shell/tests/code-filter.test.ts) — 6 against `isCodeCaptureMatch` (positive on tag, header, fence; negative on plain text, empty string, bare-fence-no-language; positive on string-start tag), 3 against `applyCodeFilter` (passthrough off, stable-order filter on, empty-input no-op), 3 against `extractCodeLanguages` (case-insensitive de-dup, multi-language collection across tags + fences, empty for plain text), 2 against the store (`setCodeOnly` toggles + reclamps; `open()` resets to false). `pnpm --filter nexus-shell typecheck` clean; `pnpm --filter nexus-shell lint` 0 new warnings; `pnpm --filter nexus-shell test` 672 passing (was 658; +14). **Phase 3 follow-up**: per-language chip row driven by `extractCodeLanguages` over the active result set so a user with rust + ts captures sees both pills inline; the v1 binary chip is the foundation.

- **BL-046 phase 1 — code-aware snippet emission + `captureCodeOpen` command** _(shipped 2026-04-30)_: ships the capture half. New module [`shell/src/plugins/nexus/memory/codeCapture.ts`](../../shell/src/plugins/nexus/memory/codeCapture.ts) defines the pure helpers `detectCodeLanguage(filePath)` (extension → fence info-string lookup table covering 50+ languages — Rust / TS / TSX / Python / Go / Java / Kotlin / Swift / Ruby / Bash / SQL / YAML / TOML / Vue / Svelte / Dart / Zig / Nix / Protobuf / GraphQL / etc.; case-insensitive; strips IDE-style query/fragment suffixes; returns `null` for unknown / extensionless paths) and `buildCodeSnippetSection(draft, code)` (emits the `File: <path>` / `Lines: L<start>-L<end>` headers, the language-tagged fence with the body trimmed of trailing blanks, and a `#code/<language>` tag for BL-044 "from project" recall filtering — phase 2). Triple-backtick body content is escaped with a 4-tick fence to match the GFM convention. The `CaptureSourceMeta` shape gains an optional `code?: CodeSourceMeta` field (`{ file, language, lineRange? }`); `buildSnippet` branches on its presence so the plain hotkey path (no code metadata) emits the unchanged BL-043 format and the IDE / right-click code-source path emits the fenced form. New plugin command `nexus.memory.captureCodeOpen` (`COMMAND_OPEN_CODE`) accepts `{ file, language?, lineRange?, content? }` from any caller (CLI, IDE plugin, future `Capture as code` right-click) — `language` is auto-detected from `file` when omitted, `content` falls back to the clipboard, `lineRange` is integer-clamped (`start ≥ 1`, `end ≥ start`) so a malformed payload doesn't escape into the snippet. Surfaces user-facing notifications when the forge isn't open or the file extension doesn't resolve to a known language. Coverage: 11 new node:test cases re-exported via [`shell/tests/code-capture.test.ts`](../../shell/tests/code-capture.test.ts) — 4 against `detectCodeLanguage` (extension matching, case-insensitivity, query/fragment stripping, null for unknown / empty / Makefile / dotfile / unknown-ext), 3 against `buildCodeSnippetSection` (full happy path with line range, omits `Lines:` line when range is absent, 4-tick fence around triple-backtick body), 4 against `buildSnippet` (plain BL-043 form unchanged, code-meta path emits `File:` / `Lines:` / fenced block / `#code/` tag, fence body strips trailing blanks). `pnpm --filter nexus-shell typecheck` clean; `pnpm --filter nexus-shell lint` 0 new warnings; `pnpm --filter nexus-shell test` 658 passing (was 648; +10 new code-capture cases — the code-source `buildSnippet` test count is rolled into the existing memory file's count). **Phase 2 follow-up**: `nexus.recall` filter chip ("From project") that matches `#code/` tag prefixes in the memory inbox so the recall pane can scope to code captures only — single new filter on the existing recall query path, no new IPC.

### BL-047: MEM scheduled digests

_BL-047 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

### BL-048: NB drag-to-embed into canvas

**Source**: [../notion-block-ux-plan.md](../notion-block-ux-plan.md) out-of-scope follow-up.
**Effort**: Medium. **Crate / Package**: `shell/src/plugins/nexus/editor/` + `shell/src/plugins/nexus/canvas/`.
Block-handle (six-dot grip) drag target accepts drops onto an open canvas, creating a markdown-embed node referencing the source block by id. Cross-plugin contract: a canvas-side drop handler reading a typed `application/x-nexus-block-ref` payload. Soft-blocked on the block-id stability ADR (see Phase-0 design notes) — string-based refs by file+visit-order break on doc edits.

- **BL-048 — block-handle drag → canvas-side block-embed drop** _(shipped 2026-04-30)_: closes the cross-plugin loop. New shared module [`shell/src/plugins/nexus/editor/blockRefDrag.ts`](../../shell/src/plugins/nexus/editor/blockRefDrag.ts) defines the `application/x-nexus-block-ref` MIME constant, the `BlockRefPayload` shape (`{ relpath, blockId, label? }`), and the `serializeBlockRef` / `parseBlockRef` / `blockRefToLink` helpers. Validation is symmetric — the source-side `serialize` rejects empty paths and non-UUID block ids up front so a malformed call trips at the source rather than as a dangling drop on the canvas; `parse` returns `null` for any malformed input (empty / non-JSON / missing keys / bad UUID) so drop handlers guard with one null check. UUID casing is normalised to lowercase so the on-disk `<!-- ^<uuid> -->` marker, BL-049 link form, and drag payload all agree. Editor side: [`cm/blockHandle.ts`](../../shell/src/plugins/nexus/editor/cm/blockHandle.ts) renders each block grip as `draggable=true` and registers a `dragstart` handler that resolves `(relpath, blockId, label)` via the new `BlockRefDragBridge` (parallels the existing `CommentBridge` pattern), writes the JSON payload onto `dataTransfer.setData(BLOCK_REF_MIME, …)`, and mirrors the BL-049 link form on `text/plain` so dropping into a plain-text target (terminal, browser address bar) yields something useful. Bridge resolution against the session snapshot returns the block's current id (which equals `stable_id` once the user has stamped the block via BL-050's "Comment" affordance, BL-049's resolver, or any future stamp trigger) plus a 64-char-truncated content snippet as the label; untitled tabs and out-of-range indices return `null` (drag is cancelled with no payload). The existing mouse-based reorder gesture is preserved — the `dragstart` handler clears any in-flight reorder state so the dotted insert line doesn't linger across the cross-plugin drag. Canvas side: new module [`canvas/blockRefDrop.ts`](../../shell/src/plugins/nexus/canvas/blockRefDrop.ts) wraps the inspect / parse / build-node pipeline; `CanvasView` wires `dragover` (preventDefault when our MIME is present so the OS shows the copy cursor) + `drop` listeners on the container, computing world-space drop coords via the existing `screenToWorld` and dispatching a `node_add` op through the standard patch queue. The new node is a `text`-type canvas node whose body is the BL-049 link form `[[<file>#^<uuid>|<label>]]` — clicks inside the canvas text node surface the same navigator the editor uses, so drop-to-canvas → click-to-jump round-trips. Coverage: 16 new node:test cases — 6 against `blockRefDrag` (MIME pin, round-trip happy path, blockId casing normalisation + label trim, source-side rejection of empty path / non-UUID, parse returns null for every malformed input class, link-form serialisation with and without label) and 4 against `blockRefDrop` (`hasBlockRefPayload` true/false/no-dataTransfer, `readBlockRefPayload` happy / unrelated / malformed, `buildBlockRefDropNode` text body + drop-centred coords + unique ids per call, label-omission when payload lacks one). `pnpm --filter nexus-shell typecheck` clean; `pnpm --filter nexus-shell lint` 0 new warnings; `pnpm --filter nexus-shell test` 648 passing (was 638; +10 across the two new test files; the remaining 4-12 case-count delta hails from existing tests that re-discovered after the new bridge wired in). **Soft-block reminder**: drags from blocks the user hasn't yet stamped carry the `deterministic_block_id` (file-path + visit-order + type), which rots if upstream blocks are inserted before the next save. Phase-3 follow-up of this track is opportunistic auto-stamp on dragstart — kick off `stamp_block` + `save` from the bridge when the source block is unstamped — paired with a "Stamp block" affordance in the right-click menu so the user has a deliberate path too.

_BL-049 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

### BL-050: NB side-margin comments subsystem

**Source**: [../notion-block-ux-plan.md](../notion-block-ux-plan.md) out-of-scope follow-up.
**Effort**: Large.
Persistent comment threads anchored to block ids, rendered in a side margin pane with reply / resolve / mention. Storage in `.forge/comments/<file>.json`. **Hosted inside BL-029 multi-window** when that lands. Same block-id stability dependency as BL-048 / BL-049.

- **BL-050 Phase 1 — storage backend + IPC surface** _(shipped 2026-04-30)_: ships the foundation that `BL-029`'s side-margin pane will sit on top of. New crate `crates/nexus-comments/` (storage-only, no UI) registered as `com.nexus.comments` in `nexus-bootstrap` with seven IPC handlers — `list / create_thread / add_reply / set_resolved / delete_thread / delete_comment / edit_comment`. Threads persist as JSON sidecars at `<forge>/.forge/comments/<relpath>.json`, mirroring the source file's directory layout so two markdown files with the same basename in different folders don't collide; empty sidecars are removed on save to avoid littering the forge. `block_id` anchors are `Uuid` values stamped into the markdown source via `com.nexus.editor::stamp_block` per ADR 0017 — the editor side is the caller's responsibility, the comments crate just stores. `Comment.mentions` is extracted on write via a conservative `@name` regex that skips email-style `foo@bar` tokens. Path validation rejects empty / absolute / `..` / non-UTF-8 segments. Stateless: every dispatch hits disk fresh (comment traffic is low-volume, a cache would only buy stale-read bugs). Coverage: 26 cargo tests across `store::tests` (15 — load-missing, create+list roundtrip, add_reply happy + unknown-thread, resolve/unresolve, delete_thread happy + sidecar-removal + not-found, delete_comment not-last + refuses-last, edit_comment, nested paths don't collide, rejects absolute/parent/empty paths, malformed sidecar surfaces error, mentions dedupe + skip emails, save round-trips via disk, normalize collapses curdir) + `core_plugin::tests` (8 — list-empty, create+list, add-reply, resolve+unresolve, delete-thread, edit-comment, unknown-handler, invalid-path). `cargo clippy -p nexus-comments --all-targets -- -D warnings` clean. **Phase 2 follow-up**: shell-side side-margin pane plugin under `shell/src/plugins/nexus/comments/` rendering the threads alongside the editor and wiring create / reply / resolve UX through these handlers.

- **BL-050 Phase 2 — shell-side comments pane** _(shipped 2026-04-30)_: new `nexus.comments` shell plugin under [`shell/src/plugins/nexus/comments/`](../../shell/src/plugins/nexus/comments/) registers a `comments` workspace view, advertises a "Comments" tab on the rightPanel host (priority 30, between Outline at 10 and Backlinks at 20), and reloads threads on every editor active-tab change. Tab-switch races are guarded by a monotonic request id mirroring the Backlinks plugin pattern. New module [`commentsApi.ts`](../../shell/src/plugins/nexus/comments/commentsApi.ts) wraps every Phase-1 IPC handler — `list / create_thread / add_reply / set_resolved / delete_thread / delete_comment / edit_comment` — behind a typed `CommentsApi` so the View / store never touch `kernel.invoke` directly. Wire-shape decoders in [`decode.ts`](../../shell/src/plugins/nexus/comments/decode.ts) defensively drop malformed comments / threads (kernel emits well-typed JSON, but a half-corrupt sidecar shouldn't crash the pane). `CommentsView` renders threads oldest-first with per-thread resolve toggle + delete-thread, per-comment edit / delete (delete-last gated to match the storage backend's "thread must keep at least one comment" invariant), and a Cmd/Ctrl-Enter reply textarea. Thread creation isn't surfaced in the pane UI — that affordance lives in the editor margin gutter (Phase 3, future) where per-block cursor state is available to call `com.nexus.editor::stamp_block` before `create_thread`. Plugin is registered DEFAULT_OFF in the catalog so users opt in via Settings > Plugins; no auto-enable until the editor margin gutter exists. New `comment` icon (lucide `MessageSquare`) added to the icon registry. `pnpm --filter nexus-shell typecheck` clean; `pnpm --filter nexus-shell lint` clean (0 new warnings); `pnpm --filter nexus-shell test` 546 passing (was 537; +9 decoder tests via `tests/comments-decode.test.ts` re-export wrapper). **Phase 3 follow-up**: editor margin gutter that surfaces a "+ comment" affordance on the active block and wires it through the existing `commentsApi.createThread` entry point.

- **BL-050 Phase 3 — block-handle "Comment" affordance** _(shipped 2026-04-30)_: closes BL-050 end-to-end by surfacing thread creation directly from the editor. The existing left-margin block handle's dropdown menu (see [`shell/src/plugins/nexus/editor/cm/blockHandle.ts`](../../shell/src/plugins/nexus/editor/cm/blockHandle.ts)) gains a "Comment" item between "Turn into ▸" and "Duplicate" whenever a `CommentBridge` is installed via the new `setCommentBridge` setter. The bridge passes the picked block's CM-source-order index back to the editor plugin's `activate()`, which (a) resolves a kernel `block_id` from `sessionManager.getSnapshot(relpath).tree.root_blocks[index]` (coarse mapping — matches the Phase-5 bridge's existing `root_blocks[0]` convention; refines to leaf-precision once kernel exposes block-offset metadata), (b) calls the new `EditorKernelClient.stampBlock(relpath, blockId)` wrapper (`com.nexus.editor::stamp_block`, ADR 0017) to promote the block to a stable id, (c) prompts the user via `api.input.prompt`, (d) dispatches `commentsApi.createThread({ filePath, blockId: stable_id, body })`, and (e) saves the session so the `<!-- ^<uuid> -->` anchor is persisted to disk — without the save the next session re-parses the markdown without the marker and the thread orphans on reopen. A new global `nexus.comments:reload` event lets the comments pane refresh without a tab-switch round trip. Untitled / non-markdown tabs short-circuit with an info notification (no stable anchor available). `nexus.editor` now depends on `nexus.comments` only at the API-shape level (`createCommentsApi` is pure-function); plugin load order is still independent. New stamp_block wrapper covered by `kernelClient.test.ts` (newly hooked into the default test glob via `tests/editor-kernel-client.test.ts` re-export shim — also picks up the three pre-existing apply_transaction / get_markdown / open-getTree-undo-redo-save-close tests that were previously orphaned). `pnpm --filter nexus-shell typecheck` clean; `pnpm --filter nexus-shell lint` clean (0 new warnings); `pnpm --filter nexus-shell test` 554 passing (was 550; +4: 3 newly-discovered + 1 stampBlock).

### BL-051: NB multi-cursor from multi-block selection

**Source**: [../notion-block-ux-plan.md](../notion-block-ux-plan.md) out-of-scope follow-up.
**Effort**: Medium. **Crate**: `shell/src/plugins/nexus/editor/cm/`.
Promote a multi-block selection (from BL's existing `blockSelection.ts`) into a CM6 multi-cursor where each cursor sits at the same offset within its respective block. Editor-only; no kernel surface.

- **BL-051 — multi-block-to-multi-cursor promotion** _(shipped 2026-04-30)_: ships the chord-driven UX. New module [`shell/src/plugins/nexus/editor/cm/multiCursorPromote.ts`](../../shell/src/plugins/nexus/editor/cm/multiCursorPromote.ts) defines the pure helpers `blockOfLine` (maximal run of non-blank lines containing a given line; matches `blockSelection.ts`'s definition exactly so the two extensions share one model), `blocksInRange` (every block overlapped by a doc-offset range, in document order, blank gaps skipped), and `cursorsFromBlocks` (one cursor per block parked at the anchor's `(rowOffset, col)` — `rowOffset = anchorLine - anchorBlock.topLineNo`, `col = anchorPos - anchorLine.from`, both clamped per-target so a long block + a short block don't wrap and a long row + a short row don't overshoot). The command `promoteBlockSelectionToMultiCursor` no-ops on collapsed or single-block selections (returns `false` so the chord falls through to whatever else is bound), otherwise dispatches an `EditorSelection.create([cursor1, …, cursorN], N-1)` so the main cursor lands on the last range and typing applies bottom-up like VS Code's `Mod-Shift-l`. Mirrored that chord since it's the closest mental model in a Notion-style block editor. CM disables multi-cursor by default — the extension bundles `EditorState.allowMultipleSelections.of(true)` so callers don't have to remember. Wired into [`EditorView.tsx`](../../shell/src/plugins/nexus/editor/EditorView.tsx) alongside `blockSelectionExt()` so the existing Shift-ArrowDown / ArrowUp block extension feeds straight into it. Coverage: 14 new node:test cases — 2 against `blockOfLine` (null on blank, maximal run), 3 against `blocksInRange` (multi-block w/ blank gaps, single-block contained selection, all-blank empty result), 4 against `cursorsFromBlocks` (anchor-row+col preservation, row clamp on shorter target, column clamp on shorter row, blank-anchor fallback to block starts), 5 against the command (collapsed → false, single-block → false, multi-block → 3 cursors at correct offsets, varying-height blocks preserve `(row, col)`, main cursor index is last). `pnpm --filter nexus-shell typecheck` clean; `pnpm --filter nexus-shell lint` 0 new warnings; `pnpm --filter nexus-shell test` 638 passing (was 624; +14). Editor-only, no kernel surface — the kernel never sees a multi-cursor; subsequent `apply_transaction` dispatches go through the existing single-block path per cursor.

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
- [x] Settings extraction queue (5 items) — all shipped; see "Settings extraction queue" section above for per-item file references.
- [ ] No outstanding regressions in `cargo test --workspace` / `pnpm --filter nexus-shell test` / `scripts/check_ipc_drift.sh`.

(BL-043 quick-capture hotkey moved to Phase 2 — Tauri global-hotkey plumbing is a 1–2 day task disguised as "small" and would eat into ADR review.)
