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

### BL-009: Whole-File `.mermaid` Viewer

**Source**: BL-008 follow-up (2026-04-28)
**Effort**: Small (~0.5 day)
**Crate / Package**: `shell/src/plugins/community/mermaid/` (extension to BL-008's plugin)
**Related**: BL-008 (fenced-code-renderer registry)

Render standalone `.mermaid` files (whole-file mermaid source, no markdown wrapper) as SVG diagrams in the editor. BL-008 only handles fenced `mermaid` code blocks inside markdown documents — it hooks into the markdown live-preview/preview pipeline, which doesn't run for non-markdown files. Implement by extending `community.mermaid`'s `activate()` to also call `viewRegistry.register('mermaid', creator)` + `viewRegistry.registerExtensions(['mermaid'], 'mermaid')` (same pattern as `nexus.canvas`'s `.canvas` claim and `nexus.bases`'s `.bases` claim — see `shell/src/plugins/nexus/canvas/index.ts:130`). The view reads the file via `api.fs`, calls the same `mermaid.render` path the fenced renderer uses, and shows the SVG with a "View Source" toggle to fall back to the raw text. Edit-in-place is out of scope for v1 — open in CodeMirror via "View Source" if the user needs to edit. Plugin stays default-off; users opt in via Settings → Plugins.

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

## Future directions (exploratory, not phased)

Design-only docs without committed timelines. Treat as inspiration / option pool, not as work in flight. If any of these get scoped into a phase, mint an ID here and link the doc as the design rationale.

- [ ] **AI integration directions** — 8 ordered directions (inline rewrite/summarize, auto-link suggestions, semantic search, per-surface chat, skills as prompts, agent loops, MCP exposure, background indexing). See [../AI-INTEGRATION-DIRECTIONS.md](../AI-INTEGRATION-DIRECTIONS.md).
- [ ] **Ambient copilot UX patterns** — 10 patterns (Cmd+I overlay, context chips, model switcher, ghost suggestions, right-click AI actions, margin suggestions, activity timeline, citations, inline correction, capture → AI). See [../AI-AMBIENT-COPILOT-PLAN.md](../AI-AMBIENT-COPILOT-PLAN.md).
- [ ] **AI memory layer (Pieces.app-style)** — 6-piece build plan (quick-capture hotkey, auto-enrichment on save, recall hotkey, implicit chat context, code-aware capture, scheduled digests). See [../AI-MEMORY-LAYER-PLAN.md](../AI-MEMORY-LAYER-PLAN.md).
- [ ] **Notion-style block UX — out-of-scope follow-ups.** Phases 1–6 of the plan landed 2026-04-22 (see "Spec'd in a PRD, not yet implemented" below for the entry). The plan itself enumerates explicit out-of-scope items: drag-to-embed into canvas, block-links navigator (`[[…#^block-id]]`), side-margin comments subsystem, block AI actions via `com.nexus.ai`, multi-cursor from multi-block selection. See `docs/notion-block-ux-plan.md`.

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

- [ ] **Move JS script plugin execution into a sandboxed iframe (UI F-8.1.1).** The script-runtime loader (`shell/src/host/ExtensionHost.ts` + `shell/src/host/communityPluginLoader.ts` + `shell/src/host/sandbox/`, superseding the legacy shell's `scriptRuntime.ts:61-67`) loads plugin modules via `URL.createObjectURL(new Blob([...], { type: "application/javascript" }))` + `import(url)` straight into the main WebView thread. Combined with F-5.1.2 (no CSP) and F-2.2.1 (JS caps unenforced), an untrusted plugin has full DOM + `invoke` + `localStorage` + `fetch` access. Fix: host JS plugins in an `<iframe sandbox="allow-scripts">` (no `allow-same-origin`) with a typed `postMessage` protocol; expose `NexusPluginContext` as a message-passing proxy. Large effort (1–2 eng-months) but required by the stated trust model.
- [ ] **Bind `pluginId` at the sandbox boundary, not in the JS context (UI F-8.1.2).** The host plugin API (`shell/src/host/PluginAPI.ts` + `shell/src/host/shellRegistry.ts`, superseding the legacy shell's `nexusContext.ts:184`) — `createNexusContext(pluginId)` trusts the string; any plugin can re-create a context claiming another plugin's id, affecting `ctx.events.emit`, `ctx.ui.notify` `source`, settings namespace, and per-plugin disposables. Fix: derive identity at the iframe/worker boundary (depends on F-8.1.1); reject any host call whose asserted id disagrees with the boundary id.

> F-9.1.1 (validate `api_version` at load time) is the UI twin of the microkernel 🟠 item of the same ID already tracked above — no duplicate entry.

### 🟠 Orange — substantive design gaps, schedule before next external release

- [ ] **Memory budget / accounting for script plugins (UI F-8.3.1).** WASM plugins have `memory_mb = 8` in their manifest; script plugins have no equivalent and allocate against the WebView heap directly. A plugin that accumulates a 500 MB structure OOMs the whole shell. **Deferred — blocked on UI F-8.1.1 iframe sandbox.** `performance.measureUserAgentSpecificMemory()` is per-frame, so meaningful accounting requires the per-plugin iframe boundary to land first. Today a misbehaving script plugin's RSS is indistinguishable from the shell's. Re-open this item when F-8.1.1 ships.

### 🟡 Yellow — rough edges to fix opportunistically

### Suspected issues — UI audit §6 spike candidates

Threads from `docs/archive/planning/UI-AUDIT.md §6` not yet confirmed. Each is a 1–2 day targeted code walk or runtime probe.

- [ ] **SI-1 — Blob-URL same-origin inheritance.** **Deferred — verified as expected, conclusion pending UI F-8.1.1.** The MDN spec on blob URLs is clear: a `blob:` URL inherits the origin of the page that created it, so a plugin module loaded via `URL.createObjectURL` + `import()` runs in the shell's origin and can read `window.top`, `document.cookie`, and invoke any Tauri command the allowlist exposes. This is precisely the hole the UI F-8.1.1 iframe sandbox closes. No separate mitigation is tractable without that boundary; track as duplicate of F-8.1.1 for closure.
- [ ] **SI-6 — `PluginManager` Mutex contention.** **Deferred — requires a dedicated load-test harness that doesn't exist yet.** Measuring requires 20+ chatty plugins and wall-clock profiling while a human drives the UI, which this environment cannot replicate. Hypothesis: per-plugin dispatch already uses `try_lock` + reentrancy guard + per-plugin backend mutex, so the `PluginManager` top-level mutex is only held during scan/load/unload/reload — not during steady-state dispatch. If the hypothesis holds this is cosmetic; if not, the fix is likely `RwLock<HashMap<id, …>>` inside the loader with per-plugin reader locks. Track as an explicit Phase-3 stability task once the load-test tooling exists.

## Audit findings (2026-04-28)

> Cross-PRD docs audit ([DOCS_AUDIT_2026-04-28.md](DOCS_AUDIT_2026-04-28.md)) — items spec'd in a PRD that are not yet built and were not previously assigned a backlog ID. Each cites the PRD section, target crate, and estimated effort. Effort scale: small ≈ ½–2 days, medium ≈ 3–10 days, large ≈ 2+ weeks.

### BL-010: `nexus ai chat` interactive REPL

**Source**: PRD-05 §3.5.2, §10.2 — readline history, multi-turn session, `--context <FILE>`, `--model <MODEL>`.
**Effort**: Medium. **Crate**: `nexus-cli` (new subcommand under existing `AiCommand` enum at `crates/nexus-cli/src/main.rs:314`).
Wraps existing `com.nexus.ai::stream_chat` (handler id 6); persistence reuses `session_load`/`session_save` (ids 8/9). Today the CLI exposes `ask | embed | status | config` only.

### BL-011: `nexus ai complete` CLI

**Source**: PRD-05 §3.5.3, §10.3 — `nexus ai complete <FILE> [--line <N>] [--col <N>] [--context <NUM_LINES>]`.
**Effort**: Medium. **Crate**: `nexus-cli`. Editor-side equivalent (Mod+Shift+Space) shipped per PRD-08 §9 / IMPLEMENTATION_STATUS.md line 84; the CLI surface is the headless twin and is not yet present.

### BL-012: Database query blocks in the editor (`[[{db:query}]]`)

**Source**: PRD-08 §8.1 (`Block::DatabaseView`, `DatabaseViewConfig`).
**Effort**: Large (1–2 weeks). **Crate**: `nexus-editor` (executor) + `shell/src/plugins/nexus/editor/` (grid renderer).
Last functional gap in PRD-08; previously listed in IMPLEMENTATION_STATUS.md gaps without a BL ID. Needs (1) query executor over `com.nexus.database::apply_view`, (2) virtualized inline grid widget, (3) decoration plumbing through CM6, (4) undo integration, (5) filter/sort UX surfaced in the block.

### BL-013: Terminal event subscription over plugin IPC

**Source**: PRD-09 §11 — async `subscribe_events(&self, tx) -> Result<()>`.
**Effort**: Medium. **Crate**: `nexus-terminal`.
Today `InMemoryTerminalServer::subscribe_events` returns a `std::sync::mpsc::Receiver` that does not cross plugin boundaries. The Tauri panel works because it polls `pump`; the TUI does the same. Closing this gap unblocks remote terminals and lets community plugins react to terminal output without re-implementing pump loops. Probably needs a generic plugin-host stream convention first (currently no other plugin streams either).

### BL-015: Soft-deleted bases — trash view UI

**Source**: PRD-10 / [BACKLOG.md](#)'s `.bases` follow-up at line 137 ("a trash-view UI surfacing soft-deleted records").
**Effort**: Small. **Crate**: `shell/src/plugins/nexus/bases/`.
`BaseRecord.deletedAt` is wired end-to-end (table actions at `BasesTable.tsx:153-188`, kernel filter at `BasesView.tsx`, IPC handlers `base_record_soft_delete` / `base_record_restore`). Missing piece is a **Trash** filter chip / dedicated view that shows deleted-only records with a "Restore" action.

### BL-016: AI tool registration for LLM function-calling

**Source**: PRD-12 §8.1 (`ToolRegistry`, `ToolExecutor`, built-in `read_file` / `write_file` / etc.).
**Effort**: Medium. **Crate**: `nexus-ai`.
Distinct from the agent's MCP discovery (which appends *tool descriptions* to the planner prompt). Native function-calling means surfacing Anthropic / OpenAI `tools` and Ollama tool-call format from `stream_chat`, dispatching the model's tool-calls back through `ipc_call`. Today providers strip tool params before the request.

### BL-017: AI PII / secret egress filter

**Source**: PRD-12 §15.1 (`DataClassifier`, `DataAnonymizer`, `send_file_contents_to_cloud` privacy switch).
**Effort**: Medium. **Crate**: `nexus-ai` (new `privacy` module) or `nexus-security` (if scanner is reused).
Privacy config struct exists but no filtering occurs before context assembly hits a remote provider. Minimum viable: regex set for AWS keys / private keys / common API tokens; redact-and-warn before egress when `policy.privacy = strict`.

### BL-019: AI local embeddings backend

**Source**: PRD-12 §9.1 (`EmbeddingModel` with cache; sentence-transformer-quality offline embeddings).
**Effort**: Large. **Crate**: `nexus-ai`.
RAG today calls remote embedding endpoints. A local backend (e.g. fastembed-rs, candle, or sqlite-vec's bundled gguf path) would unblock fully-offline forges. Nice-to-have for personal-tool scope; medium priority.

### BL-021: Skill `depends_on` composition resolver

**Source**: PRD-13 §5 (depends-on stacking, conflict resolution rules).
**Effort**: Large. **Crate**: `nexus-skills`.
`SkillMeta.depends_on` parses but is never resolved or layered. Need: topological sort, cycle detection, prompt fragment merging in well-defined order, conflict warning surface. Aligns with the existing skill-aware planner prompt assembly in `nexus-agent`.

### BL-022: Skill in-app editor UI

**Source**: PRD-13 §16 (live-preview in-app editor with frontmatter form + body markdown editor).
**Effort**: Medium. **Crate**: `shell/src/plugins/nexus/skills/`.
Today `SkillsPanel` is read-only; mutations require editing `.skill.md` on disk and calling `reload`. Editor would round-trip through the existing `com.nexus.storage::write_file` + `com.nexus.skills::reload`.

### BL-023: MCP WebSocket + HTTP+SSE transports

**Source**: PRD-14 §4.2.2 (HTTP+SSE), §4.2.3 (WebSocket).
**Effort**: Medium. **Crate**: `nexus-mcp` (transport abstraction).
Stdio is the only transport today (`McpClient` over `TokioChildProcess`). Remote MCP servers need at least one of these. WebSocket gets the lower-latency path; HTTP+SSE is the broader-compat fallback.

### BL-024: MCP reconnection + connection pool

**Source**: PRD-14 §4.2.4 (exponential backoff, max 10 concurrent/server, idle timeout).
**Effort**: Medium. **Crate**: `nexus-mcp`.
`com.nexus.mcp.host` exposes manual `connect` / `disconnect` (ids 6/7). Production-grade clients reconnect on transient errors and idle-out long-lived stale connections.

### BL-025: MCP authentication

**Source**: PRD-14 §8 (API key, bearer, OAuth client-credentials).
**Effort**: Medium. **Crate**: `nexus-mcp`.
`McpServerSpec.env` already accepts a string map; the auth flow itself (token exchange, refresh, keychain lookup) is unbuilt. Pairs with ADR-0009 (keyring hard-fail) once that policy is enforced at bootstrap.

### BL-026: MCP resource enumeration

**Source**: PRD-14 §7 (`mcp://nexus/notes/...`, `mcp://nexus/db/...`).
**Effort**: Small. **Crate**: `nexus-mcp` (server side).
`list_resources` exists on the host plugin (id 4) and forwards to external servers; the **Nexus-side** server (`serve_stdio`) advertises tools but no resources. Wire up at least `mcp://nexus/notes` listing so external clients can browse the forge.

### BL-027: Multi-agent orchestration / delegation

**Source**: PRD-15 §10 (`AgentOrchestrator::delegate / parallel / pipeline`).
**Effort**: Large. **Crate**: `nexus-agent`.
Today a `LlmAgent` plans + executes solo. Spec calls for a coordinator that hands subtasks between archetypes (Researcher → Writer → Coder) with shared scratch state. PRD §6.3 reactive rules and §12 debugger/replayer are in the same neighbourhood — defer those until orchestration lands and exposes the right hooks.

### BL-028: Workflow trigger expansion + control flow

**Source**: PRD-16 §6 (webhook / git_event / mcp_event), §9.2-9.4 (parallel, retry/backoff, AI steps), §7 (template library).
**Effort**: Large (umbrella — split when scoped). **Crate**: `nexus-workflow`.
Cron + file_event triggers + condition evaluator + variable interpolation + manual executor are all live. Outstanding: webhook trigger (HTTP listener), git_event trigger (subscribe to `com.nexus.git.*` bus topics — straightforward now that those events exist), mcp_event trigger, parallel step scheduler (`Workflow.parallel: bool` parsed but ignored at executor), per-step retry with exponential backoff, AI step types (`ai_prompt`, `ai_decision`), and a built-in workflow templates library. Track as one umbrella; mint sub-IDs when prioritised.

### BL-029: Multi-window / detachable panels

**Source**: PRD-17 §6.
**Effort**: Medium. **Crate**: `shell/src-tauri/` + `shell/src/host/`.
Spec calls for per-Leaf detachment into a separate `WebviewWindow`. Today the shell ships single-window. Not in REQUIRED-FOR-FORMAL-RELEASE.md, so it lands here. Web/mobile platform targets (also PRD-17) remain explicitly deferred and are not BL items.

### Verification notes (no BL ID — informational)

- **ADR-0009 keyring hard-fail enforcement** — ADR mentions a `NEXUS_NO_KEYRING=1` escape hatch, but bootstrap-side enforcement was not located in this audit. Either confirm enforcement (and document the location) or file as a follow-up in OPEN-ITEMS.md if real.
- **PRD-04a MockPluginContext / MockEventBus** — referenced in template tests as TODO but not yet exposed from `nexus-plugin-api`. Low priority; community plugin authors are not yet writing many tests, and the issue surfaces only when someone tries.

## Decisions — PRD-04 audit (2026-04-17)
