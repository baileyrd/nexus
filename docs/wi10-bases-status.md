# WI-10 Bases — Validation Audit

**Source plan:** docs/planning/PHASE-2-IMPLEMENTATION-PLAN.md §4.2
**Sub-plan:** docs/bases-shell-plan.md
**Audited against:** shell/src/plugins/nexus/bases/, crates/nexus-storage/src/core_plugin.rs (base_*)
**Date:** 2026-04-23
**Auditor:** validation-only, static analysis (no runtime tests run)

## 1. Plugin overview

The `nexus.bases` shell plugin (5,142 LOC across 21 files under `shell/src/plugins/nexus/bases/`) registers a `'bases'` view type and the `.bases` extension so Files-tree clicks on a `.bases` directory mount a dedicated leaf instead of falling through to CodeMirror. The plugin loads a base via `base_load` on mount, then renders one of six views (Table / Board / List / Calendar / Gallery / Timeline) sharing a single zustand-backed selection model. CRUD round-trips through the kernel's seventeen `base_*` handlers; CSV import/export and formula evaluation hit the legacy `com.nexus.database` plugin.

## 2. Phase-by-phase status matrix

| Phase | Title | Status | Code paths | IPC handlers used | Tests | Closing work |
|-------|-------|--------|------------|-------------------|-------|--------------|
| 1 | View registration + routing | done | `index.ts:21-91`, `BasesPaneView.tsx`, `BasesView.tsx:32-188` | `base_load` | none (shell-side) | — |
| 2 | Table view (virtualized + per-type cells) | done | `BasesTable.tsx` (927 LOC), uses `@tanstack/react-virtual` at lines 240-245 | `base_record_create/update/delete` (40-42), `formula_eval` | none | — |
| 3 | Board + List views | done | `BasesBoard.tsx` (303 LOC), `BasesList.tsx` (284 LOC); HTML5 drag-drop in BasesBoard | `base_record_update` | none | — |
| 4 | Calendar + Gallery + Timeline | done | `BasesCalendar.tsx` (341 LOC), `BasesGallery.tsx` (206 LOC), `BasesTimeline.tsx` (469 LOC) | `base_record_create/update` | none | — |
| 5 | View switcher + persistence | partial | `BasesViewBar.tsx:64,85,86,102,113`, `viewMapping.ts` | `base_view_create/update/delete` (46-48) | none | View `fields[]` and `filter[]` arrays are dropped on snapshot — `viewFromTabState` (`viewMapping.ts:58-85`) only emits `name`, `type`, `sort`, `groupField`, `dateField`, `endField`. Hidden columns and per-view filter chips don't survive a save → reload (~1 day to wire). |
| 6 | Polish + edge cases | partial | CSV: `BasesTable.tsx:158-231`; undo/redo: `basesStore.ts:291-350`; formula preview: `BasesTable.tsx:626+`; virtualization: `BasesTable.tsx:240-245`; templates: `NewBaseDialog.tsx`, `templates.ts`; SchemaEditor: `SchemaEditor.tsx` (488 LOC) | `csv_import/export`, `formula_eval`, `base_create` (49), `base_property_rename` (50), `base_property_update` w/ `migrate_values` | none | **Soft-delete UX missing — see §3.** |

## 3. Soft-delete / restore — UX path audit

The plan §4.2 names this as a key acceptance gate ("verify records can be soft-deleted AND restored via UI, not just API"). **The kernel surface ships, but the UI never calls it.**

- **Kernel:** `base_record_soft_delete` (id 51) and `base_record_restore` (id 52) wired in `crates/nexus-storage/src/core_plugin.rs:629-651`. Both functions exist in `StorageEngine`.
- **Shell client:** thin wrappers `softDeleteRecord` and `restoreRecord` typed and implemented in `kernelClient.ts:108-112, 181-192`.
- **UI invocations:** zero. `grep -nE "softDelete|restoreRecord"` across `shell/src/plugins/nexus/bases/` returns only the kernelClient definitions and a single read of `r.deletedAt` in `BasesView.tsx:118`.
- **What the Table does instead:** every `Backspace`/`Delete` keypress and the row-context delete fires the **hard-delete** handler `client.deleteRecord` (`BasesTable.tsx:122, 138, 143, 212`). The undo entry resurrects via `createRecord` with the snapshotted record — workable, but bypasses the soft-delete model entirely.
- **Filter behaviour:** `BasesView.tsx:114-119` filters `r.deletedAt` records out of the visible base passed to every child view. SchemaEditor receives the unfiltered base, but it only enumerates schema columns — no record list, no restore button. There is **no trash view, no "show deleted" toggle, and no per-row restore action** anywhere in the plugin.
- **Net assessment:** Phase-6 status banner in `bases-shell-plan.md:84-87` says "leaving them on the base for the SchemaEditor (future trash-view surface)" — i.e. the plan itself acknowledges the hole. WI-10 §4.2 acceptance text demands closing it.

**Closing work for §3:** ~1.5 days. Re-route the Table delete + Backspace path through `softDeleteRecord`; add a `viewMode === 'trash'` (or a checkbox in `BasesViewBar`) that surfaces deleted records with a Restore action calling `restoreRecord`; preserve the existing hard-delete via a separate "Permanently delete" affordance.

## 4. Property rename — schema editor audit

The rename flow per §4.2 is **fully present** and arguably the strongest part of the plugin.

- **Rename UI:** `SchemaEditor.tsx:82-100`. The "rename" link on each schema row calls `api.input.prompt`, validates against `id` and empty/duplicate names, and (when records exist) calls `api.input.confirm` showing the record count before issuing the rename.
- **Kernel call:** `client.renameProperty` → `base_property_rename` (handler id 50). Kernel renames the schema key and walks every record updating its fields map (`crates/nexus-storage/src/core_plugin.rs:655-672`).
- **Retype with migration:** `SchemaEditor.tsx:102-122`. Confirmation prompt fires before issuing `updateProperty(..., migrateValues=true)`. Coercion rules are kernel-side; uncoercible values drop to null per the plan.
- **Preview/confirmation:** present (`api.input.confirm`), but **no live preview of the migration result before commit**. The user sees a record count and a yes/no — they don't see "the value '12.5' will become 12 in 7 records, and 'banana' will become null in 3 records." Plan §4.2 says "Schema editor rename flow + data migration preview" — the rename is solid; the preview-before-commit for retype is what's thin.
- **Data preservation on rename:** confirmed at the kernel level by `base_property_rename` walking record fields; no shell-side regression risk because the SchemaEditor reloads via `client.loadBase` after every op (`SchemaEditor.tsx:62-65`).

**Closing work for §4:** ~0.5 day. Optional polish — render a small preview table in the confirm prompt for retype operations summarising old→new values for the first 10 records.

## 5. IPC coverage matrix

All seventeen kernel handlers, with shell adoption status. Plugin id is `com.nexus.storage` for everything in this table; `com.nexus.database` rows are listed separately at the bottom.

| ID | Handler | Wired in plugin? | Where (file:line) |
|----|---------|------------------|-------------------|
| 24 | `base_index` | **no** | kernel-only (vault scan path) |
| 25 | `base_list` | **no** | kernel-only |
| 26 | `base_query` | **no** | shell loads full base via `base_load` and filters/sorts client-side |
| 32 | `base_load` | yes | `kernelClient.ts:152-154`, called from `BasesView.tsx:47`, `SchemaEditor.tsx:63` |
| 40 | `base_record_create` | yes | `kernelClient.ts:162-167`, called from `BasesTable.tsx:118, 147, 193, 206`, undo paths |
| 41 | `base_record_update` | yes | `kernelClient.ts:168-174`, called from Table inline cell editor, Board drag-drop |
| 42 | `base_record_delete` | yes | `kernelClient.ts:175-180`, called from `BasesTable.tsx:122, 138, 143, 212` (hard-delete only) |
| 43 | `base_property_create` | yes | `kernelClient.ts:193-199`, called from `SchemaEditor.tsx:164` |
| 44 | `base_property_update` | yes | `kernelClient.ts:200-207`, called from `SchemaEditor.tsx:121, 126, 135, 140` (with `migrateValues`) |
| 45 | `base_property_delete` | yes | `kernelClient.ts:215-220`, called from `SchemaEditor.tsx:150` |
| 46 | `base_view_create` | yes | `kernelClient.ts:221-226`, called from `BasesViewBar.tsx:64, 86, 102` |
| 47 | `base_view_update` | yes | `kernelClient.ts:227-232` (no callers — rename routes through delete+create) |
| 48 | `base_view_delete` | yes | `kernelClient.ts:233-238`, called from `BasesViewBar.tsx:85, 113` |
| 49 | `base_create` | yes | `kernelClient.ts:155-161`, called from `NewBaseDialog.tsx` via `useNewBaseStore` |
| 50 | `base_property_rename` | yes | `kernelClient.ts:208-214`, called from `SchemaEditor.tsx:99` |
| 51 | `base_record_soft_delete` | **client-only** | typed in `kernelClient.ts:181-186` — **zero UI callers** |
| 52 | `base_record_restore` | **client-only** | typed in `kernelClient.ts:187-192` — **zero UI callers** |

`com.nexus.database` adoption: `csv_import` and `csv_export` (`BasesTable.tsx:158-231`), `formula_eval` (`BasesTable.tsx:626+`, `SchemaEditor.tsx:138-141`). `apply_view` (handler id 4) is **unused** — sort/filter happens client-side.

**Coverage summary:** 12 of 17 handlers actively used by UI; 3 kernel-only (`base_index`, `base_list`, `base_query`) which is fine for a single-file editor; 2 (`base_record_soft_delete`, `base_record_restore`) wired in the client but never invoked from the UI; 1 (`base_view_update`) defined but unused because rename = delete + create.

## 6. Cross-cutting findings + closing work

The plugin is **substantially more complete than §4.2 implies** — every advertised view type renders with edit-in-place and round-trips a real record through the kernel; the SchemaEditor is a real schema-migration UI with confirmation prompts; CSV import/export and formula previews work; virtualization is wired. The areas where the plan's claim outpaces reality:

- **Soft-delete / restore UI is missing entirely** (§3) — the named edge case is unmet despite the kernel + client being ready. ~1.5d.
- **View `fields[]` and `filter[]` don't round-trip** — `viewFromTabState` discards them on save. ~1d.
- **No automated tests on the shell side** — `find shell -name "*.test.*"` returns zero hits under `bases/`. The kernel side has good coverage (`bases/schema.rs:6 tests`, `bases/query.rs:24 tests`, `bases/relation.rs:7 tests`, `bases/mod.rs:2 tests`). A vitest harness mounting `BasesView` against a `MockKernel` would prevent regressions. ~1.5d.
- **Property rename has no value-migration preview** — §4 above. ~0.5d optional polish.
- **`base_view_update` handler is dead code in the shell** — rename = delete + create. Either kill the wrapper or use it. ~0.25d.

**Total estimated closing effort:** ~4.75 person-days against §4.2's "M ~1 week" budget. Confidence **medium-high** — the soft-delete UX is the only must-have; everything else is polish. The plan budget is realistic if the team prioritises §3 + the missing test harness.

## 7. Open questions

1. Should hard-delete remain accessible behind a "Permanently delete" affordance, or fully replaced by soft-delete + manual purge from a trash view?
2. Where should the "show deleted" toggle live — `BasesViewBar` (per view) or `BasesView` header (per tab)? The latter matches the existing SchemaEditor toggle pattern.
3. Is `base_query` (handler 26) intended for a future "hosted view" mode, or genuinely vestigial? The shell currently loads the entire base into memory which scales fine to 50k rows but won't to 500k.
4. Does `base_view_update` actually work for a rename in the kernel, or does it key strictly by name? `bases-shell-plan.md:120-122` claims "rename = delete + create because the kernel's view-update keys by name" — worth verifying so the wrapper isn't shipped as a footgun.
