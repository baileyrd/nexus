# Nexus Feature Backlog

> Features identified in the [Growth Plan](Nexus_Growth_Plan.md) that are not fully covered by existing PRDs 01–17. Items are categorized by coverage gap and listed in suggested implementation order.
>
> **Only unfinished work lives here.** Completed items are archived verbatim (with their original section context) in [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Section headings with no listed items are preserved as structural placeholders — consult the archive for what landed under each, and add new follow-ups directly below the heading.

---

## New Features (not addressed in any PRD)

## Partially New Features (concept exists in PRDs but design is unspecified)

### BL-007: CRDT-over-Git Transport

**Source**: PRD 11, Section 4.4 (Level 3)
**Effort**: Large (2–3 weeks)
**Crate**: `nexus-git`, new `nexus-crdt`
**Related PRD**: PRD 11 (specified but deferred — requires collaborative editing layer)

Serialize Nexus CRDT state (rich text buffer) as JSON in `.nexus/crdt-state.json`, tracked in git. On push, CRDT state is included in commits. On pull with merge conflict in the CRDT file, apply CRDT merge semantics (operation-based or state-based) for automatic convergence. Fallback to content conflict if CRDT merge fails. Enables multi-user async collaboration via git push/pull without manual conflict resolution. Prerequisite: a CRDT-based editor engine (PRD 08) or collaborative editing layer.

---

## Architecture review (2026-04-16) — microkernel adherence

## UI architecture review (2026-04-16) — editor-shell pattern

### Code gaps

### PRD gap — no owner for plugin-contributed tab surfaces

## Editor-shell capability gaps (2026-04-16) — vs VS Code / Obsidian / IntelliJ

### Spec'd in a PRD, not yet implemented

- [ ] **`.bases` database renderer in the shell (PRD-10).** Kernel side of
  the Database Engine is shipped (`.bases` TOML parser + SQLite index +
  formula evaluator + CSV import/export behind `com.nexus.database`),
  but the current `shell/` UI has no renderer — `.bases` files fall
  through to CodeMirror as raw TOML. Plan: [docs/bases-shell-plan.md](../bases-shell-plan.md)
  (6 phases; routing skeleton → Table view → Board/List → Calendar/
  Gallery/Timeline → view persistence → polish). CRUD IPC handlers
  landed 2026-04-22 on `com.nexus.storage` (not `com.nexus.database`
  as originally planned — storage already owns `base_load` /
  `base_index` / `base_query` / `base_list` and has `forge_root`;
  colocating the mutators keeps the reindex atomic within a single
  dispatch). Handler ids 40–48: record CRUD
  (`base_record_create/update/delete`, 40–42), property CRUD
  (`base_property_create/update/delete`, 43–45), view CRUD
  (`base_view_create/update/delete`, 46–48). Only shell-side phases
  1–6 remain. Known follow-ups: property-rename + type-migration (the
  current `base_property_update` replaces the definition in place but
  does not walk records or rename the column); record soft-delete
  waits on a `deleted_at` slot on `BaseRecord`.
- [ ] **`.canvas` board renderer in the shell (PRD-06 §4).** Storage
  layer parses/serializes/indexes canvas files; CLI shipped; kernel
  IPC surface landed 2026-04-22 (`canvas_read` / `canvas_write` /
  `canvas_patch` / `canvas_nodes` / `canvas_edges` on
  `com.nexus.storage`, handler ids 35–39). Shell Phase 1 (routing +
  blank surface) landed 2026-04-22: `shell/src/plugins/nexus/canvas/`
  registers view type `canvas`, claims the `.canvas` extension
  (routed via the existing `viewTypeForFile` path), and renders a
  grey pane with a "N nodes · M edges" overlay sourced from
  `canvas_read`. Plan: [docs/canvas-shell-plan.md](../canvas-shell-plan.md)
  — remaining phases 2–6 (canvas renderer with zoom/pan →
  interactions → edges + inspector → rich node embeds → polish).
- [ ] **Notion-style block UX on top of the existing block-tree engine
  (PRD-08).** Block tree + transactions + annotations are shipped
  (3.7k LoC in `nexus-editor`); what's missing is the UI layer —
  slash menu, block selection, gutter handles + per-block menu +
  drag-to-reorder, inline annotation toolbar. Plan:
  [docs/notion-block-ux-plan.md](../notion-block-ux-plan.md) (6
  phases; phases 1–3 are the "feels like Notion" threshold).
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

Findings from `docs/MICROKERNEL-AUDIT.md` not yet tracked. Ordered by audit priority. The three 🔴 items and F-9.2.1 are blockers before any public plugin marketplace.

### 🔴 Red — blockers for untrusted plugin distribution

_None outstanding._ F-2.1.1 closed 2026-04-22 — see archive.

### 🟠 Orange — address before marketplace or next minor release

### 🟡 Yellow — quality / correctness improvements

## Suspected issues — not fully investigated

Threads from `docs/MICROKERNEL-AUDIT.md §Suspected Issues` that warrant a targeted code walk.

- [ ] **Hot-reload timing on macOS and Windows.** `notify-debouncer-mini` behaviour differs across platforms; F-4.3.1 covers one class of issue. A targeted cross-platform reliability pass on the hot-reload path would be worthwhile before shipping community plugin hot-reload as a feature. **Deferred** — requires running the shell on macOS and Windows hardware to reproduce and measure; this repo's test host is Linux/WSL only. Track for a dedicated cross-platform QA pass once a macOS/Windows CI runner or test machine is available.

## UI audit (2026-04-16) — follow-ups

Findings from `docs/UI-AUDIT.md` not yet tracked above. IDs reference the audit. The 🔴 items plus F-9.1.1 are blockers before any untrusted-plugin distribution.

### 🔴 Red — cannot ship to untrusted users without these

- [ ] **Move JS script plugin execution into a sandboxed iframe (UI F-8.1.1).** `app/src/plugins/scriptRuntime.ts:61-67` loads plugin modules via `URL.createObjectURL(new Blob([...], { type: "application/javascript" }))` + `import(url)` straight into the main WebView thread. Combined with F-5.1.2 (no CSP) and F-2.2.1 (JS caps unenforced), an untrusted plugin has full DOM + `invoke` + `localStorage` + `fetch` access. Fix: host JS plugins in an `<iframe sandbox="allow-scripts">` (no `allow-same-origin`) with a typed `postMessage` protocol; expose `NexusPluginContext` as a message-passing proxy. Large effort (1–2 eng-months) but required by the stated trust model.
- [ ] **Bind `pluginId` at the sandbox boundary, not in the JS context (UI F-8.1.2).** `app/src/plugins/nexusContext.ts:184` — `createNexusContext(pluginId)` trusts the string; any plugin can re-create a context claiming another plugin's id, affecting `ctx.events.emit`, `ctx.ui.notify` `source`, settings namespace, and per-plugin disposables. Fix: derive identity at the iframe/worker boundary (depends on F-8.1.1); reject any host call whose asserted id disagrees with the boundary id.

> F-9.1.1 (validate `api_version` at load time) is the UI twin of the microkernel 🟠 item of the same ID already tracked above — no duplicate entry.

### 🟠 Orange — substantive design gaps, schedule before next external release

- [ ] **Memory budget / accounting for script plugins (UI F-8.3.1).** WASM plugins have `memory_mb = 8` in their manifest; script plugins have no equivalent and allocate against the WebView heap directly. A plugin that accumulates a 500 MB structure OOMs the whole shell. **Deferred — blocked on UI F-8.1.1 iframe sandbox.** `performance.measureUserAgentSpecificMemory()` is per-frame, so meaningful accounting requires the per-plugin iframe boundary to land first. Today a misbehaving script plugin's RSS is indistinguishable from the shell's. Re-open this item when F-8.1.1 ships.

### 🟡 Yellow — rough edges to fix opportunistically

### Suspected issues — UI audit §6 spike candidates

Threads from `docs/UI-AUDIT.md §6` not yet confirmed. Each is a 1–2 day targeted code walk or runtime probe.

- [ ] **SI-1 — Blob-URL same-origin inheritance.** **Deferred — verified as expected, conclusion pending UI F-8.1.1.** The MDN spec on blob URLs is clear: a `blob:` URL inherits the origin of the page that created it, so a plugin module loaded via `URL.createObjectURL` + `import()` runs in the shell's origin and can read `window.top`, `document.cookie`, and invoke any Tauri command the allowlist exposes. This is precisely the hole the UI F-8.1.1 iframe sandbox closes. No separate mitigation is tractable without that boundary; track as duplicate of F-8.1.1 for closure.
- [ ] **SI-6 — `PluginManager` Mutex contention.** **Deferred — requires a dedicated load-test harness that doesn't exist yet.** Measuring requires 20+ chatty plugins and wall-clock profiling while a human drives the UI, which this environment cannot replicate. Hypothesis: per-plugin dispatch already uses `try_lock` + reentrancy guard + per-plugin backend mutex, so the `PluginManager` top-level mutex is only held during scan/load/unload/reload — not during steady-state dispatch. If the hypothesis holds this is cosmetic; if not, the fix is likely `RwLock<HashMap<id, …>>` inside the loader with per-plugin reader locks. Track as an explicit Phase-3 stability task once the load-test tooling exists.

## Decisions — PRD-04 audit (2026-04-17)
