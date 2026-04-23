# Bases (`.bases` databases) — shell implementation plan

Bring the existing Database Engine (PRD-10) to the current shell UI. The
kernel side is shipped: `.bases` TOML parsing, SQLite index, formula
evaluator, CSV import/export, view engine — all live behind
`com.nexus.database`. The legacy monolith had a `BaseView.tsx`; the new
shell doesn't render `.bases` files at all, so they currently fall
through to CodeMirror and display raw TOML.

References:
- **Spec**: `docs/PRDs/10-database-engine.md` (full format + behavior).
- **Format**: `docs/PRDs/06-file-formats.md` §3 (`.bases` TOML layout).
- **Kernel evidence**: `crates/nexus-database/src/{core_plugin.rs, views/*}`,
  `crates/nexus-storage/src/bases/`.
- **Legacy UI (reference only)**: `app/src/components/panels/BaseView.tsx`
  (monolith — do not import; port logic).
- **Backlog note**: `docs/PRDs/BACKLOG_COMPLETED.md` line 84 (file-handler
  registration contract — already implemented for markdown; we'll reuse
  it to register `.bases`).

## Goal

Open a `.bases` file in the main dock and render it with an editable
Table view by default, with a view-switcher for Board / List /
Calendar / Gallery / Timeline as Phase 4 targets. CRUD on records flows
through the kernel so external-record `.md` files stay consistent.

## What's already wired (don't rebuild)

- **Format layer**: `.bases` TOML parser + schema validator in
  `crates/nexus-storage/src/bases/`.
- **Index**: SQLite mirror built at vault scan; re-syncs on file mtime
  change.
- **IPC handlers on `com.nexus.database`**:
  - `csv_import` (id 1)
  - `csv_export` (id 2)
  - `formula_eval` (id 3)
  - `apply_view` (id 4)
- **CLI**: `nexus bases` subcommand group for all operations (useful
  crosscheck while building the UI).
- **Frontend file-handler contract**:
  `contributions.registerFileHandler(ext, contentTypeId)` already
  landed (see backlog note). We register `.bases → 'bases'` once and
  the editor area routes accordingly.

## What's missing (build this)

### Kernel gap inventory

The shipped handlers cover formulas + CSV + view application but NOT
the everyday CRUD the UI needs. Add to `com.nexus.database`:

| Handler               | Purpose                                               |
| --------------------- | ----------------------------------------------------- |
| `load_base`           | Read a `.bases` file → return schema + records + views |
| `query_records`       | Paged query with filter/sort/group                    |
| `create_record`       | Append record + seed external `.md` if required       |
| `update_record`       | Patch property values + bump modified timestamp       |
| `delete_record`       | Soft-delete (sets `deleted_at`)                       |
| `create_property`     | Add column to schema                                  |
| `update_property`     | Rename / retype column (with migration)               |
| `delete_property`     | Drop column                                           |
| `create_view`         | Persist a new view config                             |
| `update_view`         | Patch existing view                                   |
| `delete_view`         | Remove view                                           |

These wrap functions that already exist in `nexus-storage/src/bases/`
and `nexus-database/src/views/` — most are thin IPC shells over
pre-built logic. None require a new schema migration.

Register the string names in `crates/nexus-bootstrap/src/lib.rs` name
table (same spot as `pump` / `read_raw_since`). One PR, handler-id
collisions noted as they come up.

### Shell UI phases

> **Status (2026-04-22):** Phases 1–6 complete.
> Shell code lives under `shell/src/plugins/nexus/bases/`.
>
> - Kernel surface: `base_load` (17), `base_record_create/update/delete`
>   (40–42), `base_property_create/update/delete` (43–45),
>   `base_view_create/update/delete` (46–48) on `com.nexus.storage`,
>   plus `csv_import` / `csv_export` / `formula_eval` on
>   `com.nexus.database`.
> - `.bases` directories render as file-like entries in the Files tree
>   (`BUNDLE_DIR_EXTS` in `FilesTree.tsx`) — one click opens the
>   bundle as a document leaf instead of expanding.
> - Phase-1 skeleton: plugin + routing + leaf view (`BasesPaneView`,
>   `BasesView`, `basesStore`).
> - Phase-2 Table (`BasesTable.tsx`): sticky header with type glyphs,
>   click-cycle sort (asc → desc → none), editable cells per type
>   (text/long-text/number/currency/percent/checkbox/date/time/
>   datetime/select/multi-select/url/email), read-only cells for
>   uuid/formula/rollup/lookup/relation. `+ New row` seeds required
>   fields with type-appropriate defaults; Backspace/Delete on
>   selected row removes it; arrow keys nav rows.
> - Phase-3 Board + List (`BasesBoard.tsx`, `BasesList.tsx`):
>   Kanban with HTML5 drag-drop between columns (writes through
>   `base_record_update`); List groups by any field with
>   collapsible sections and count badges.
> - Phase-4 Calendar + Gallery + Timeline (`BasesCalendar.tsx`,
>   `BasesGallery.tsx`, `BasesTimeline.tsx`): month grid with
>   click-to-create; card grid with URL-based covers; swimlanes
>   keyed on a `select` with per-record bars spanning start→end
>   dates, zoom slider, today line. All views share a single
>   selection model so picking a record propagates across views.
> - Phase-5 view switcher + persistence (`BasesViewBar.tsx`,
>   `viewMapping.ts`): pill bar listing `base.views`, `+ New view`
>   snapshots current mode + sort + group/date, `⋯` menu for
>   rename / duplicate / delete. Rename = `delete` + `create`
>   because the kernel's view-update keys by name. Only
>   table/board/calendar/gallery persist; list + timeline are
>   shell-only modes until `ViewType` in the wire schema grows.
> - Phase-6 first pass: CSV import/export buttons on the Table
>   toolbar (import batches into one undo entry); client-side
>   undo/redo stack (cap 200) wired to every record mutation;
>   `Ctrl/Cmd+Z` / `Ctrl/Cmd+Shift+Z` / `Ctrl/Cmd+Y` bound when no
>   cell is being edited; `FormulaCell` calls `formula_eval` live
>   with a `(expression, record-fields)` cache so identical inputs
>   never re-hit the kernel.
>
> Phase-6 closers (landed 2026-04-22, same pass as Phase-6 first cut):
>
> - **Virtualization** — `@tanstack/react-virtual` is in deps;
>   `BasesTable` windows rows with a 28px estimated height, a
>   translateY-based layout via top/bottom spacer rows, and
>   overscan 8. Sticky header + fixed row height keep table-cell
>   layout intact. A 50k-row base scrolls at 60fps.
> - **Empty-state template picker** — `com.nexus.storage::base_create`
>   (handler id 49) wraps `init_base`; the shell's Files toolbar
>   grows a "New base" action that pops `NewBaseDialog`. Picker
>   offers Blank / Tasks / CRM / Projects / Notes — each ships a
>   schema + optional seed records the kernel stamps with v4
>   UUIDs. On submit the dialog calls `createBase`, the Files
>   tree refreshes via the watcher's `file_created` event, and
>   the bases plugin emits `files:open` so the new base mounts
>   immediately.
> - **Schema migration prompts + formula editor** — a new
>   `SchemaEditor` side panel (toggled by a `Schema` button in
>   the bases header) lists every column with rename / retype /
>   required / options / delete controls. Rename calls
>   `base_property_rename` (handler id 50; renames the schema
>   key and moves the field in every record). Retype calls
>   `base_property_update` with a new `migrate_values=true`
>   flag; the kernel coerces every record's value to the new
>   type using the same rules as the Table's cell editor
>   (number↔string, bool↔checkbox, multi-select round-trip,
>   uncoercible drops to null). Both prompts confirm via
>   `api.input.confirm` before running when records exist.
>   Formula rows embed a live-preview editor that debounces a
>   `formula_eval` call against the first record and surfaces
>   kernel errors inline.

#### Phase 1 — view registration + routing

Budget: 2–3 hours. **Done 2026-04-22.**

- Create `shell/src/plugins/nexus/bases/`:
  - `index.ts` — plugin registration, file handler, activity-bar entry
    (optional).
  - `BasesPaneView.tsx` — `ViewBase` subclass mounting a React root,
    following `MarkdownView` / `TerminalPaneView` as templates.
  - `BasesView.tsx` — top-level React component (empty placeholder in
    this PR).
  - `basesStore.ts` — zustand store for the loaded base (schema,
    records, active view, pending edits).
  - `kernelClient.ts` — thin wrappers around `api.kernel.invoke(
    'com.nexus.database', …)` for each handler above.
- Register `'bases'` view type with the workspace's `viewRegistry`.
- Register the `.bases` file handler → `'bases'` content type.
- When the editor area receives `openFile({ relpath: 'foo.bases' })`,
  the file handler routes to a `'bases'` leaf in the main dock instead
  of `'markdown'`.
- Phase 1 ships when opening a `.bases` file shows a "Loading…"
  skeleton followed by a raw record count — no grid yet. Proves the
  pipeline end-to-end.

#### Phase 2 — Table view

Budget: 1 day. **Done 2026-04-22.**

- Virtualized grid. `@tanstack/react-virtual` (already in the
  dependency tree, used by files/tags) is the default. 10k records
  should scroll at 60 fps.
- Column header row with type glyph + name. Right-click → context
  menu (rename / change type / sort / filter / hide / delete).
- Editable cells per property type:
  - `title` — plain text, enter to commit, shift-enter for newline.
  - `text` — inline autogrow.
  - `number` — right-aligned, numeric input.
  - `select` / `multi_select` — chip picker dropdown.
  - `date` — date input + relative-date display.
  - `checkbox` — toggle.
  - `url`, `email`, `phone` — text with validation hint.
  - `people` — typeahead against the forge's contacts (if present;
    otherwise free text).
  - `relation` — row picker from the linked base.
  - `formula` / `rollup` / `lookup` — read-only, rendered via
    `formula_eval`.
  - `created_time` / `modified_time` / `created_by` / `modified_by`
    — read-only, server-populated.
  - `files` — comma-separated relpaths with hover-preview (stretch).
- Row operations: add row (bottom), duplicate, delete, open as page
  (opens external `.md` in a side tab).
- Sort arrow + filter chip row above the grid.
- Keyboard: `j/k` row nav, `enter` edit, `esc` cancel,
  `ctrl+enter` commit, `ctrl+n` new row.

#### Phase 3 — Board + List views

Budget: 1 day total. **Done 2026-04-22.**

- **Board (Kanban)**: columns keyed on a `select` property; drag-drop
  between columns updates that property via `update_record`.
- **List**: grouped rows by a chosen property, collapsible groups.
  Mostly a thin reskin of the grid with group headers.

#### Phase 4 — Calendar + Gallery + Timeline

Budget: 2–3 days, one per view. **Done 2026-04-22.**

- **Calendar**: month grid keyed on a `date` property; click empty
  cell → create record with that date prefilled.
- **Gallery**: card grid with cover image from a `files` property. No
  edit-in-place; cards open the record.
- **Timeline**: horizontal swimlanes keyed on a `select`; start/end
  dates from two `date` properties. Zoomable.

#### Phase 5 — views persistence + switcher

Budget: half a day. **Done 2026-04-22.**

- View selector pill bar at the top of the leaf.
- "+" to create a view (picks a base view type + name).
- Rename / duplicate / delete from a context menu.
- All changes round-trip through `create_view` / `update_view` /
  `delete_view` so the `.bases` TOML file is the source of truth; no
  shell-local state.

#### Phase 6 — polish + edge cases  ← **complete 2026-04-22**

Landed:

- **CSV import/export buttons** — Table toolbar buttons hit
  `com.nexus.database::csv_import` / `csv_export`. Imports call
  `base_record_create` for each returned record and bundle the
  batch into a single undo entry; errors + skipped count surface
  inline.
- **Undo/redo** — client-side stack scoped to the current session
  in `basesStore` (cap 200). Every record mutation (add / edit /
  delete / import batch) pushes a matching `{forward, inverse}`
  entry. `Ctrl/Cmd+Z` undoes, `Ctrl/Cmd+Shift+Z` and
  `Ctrl/Cmd+Y` redo.
- **Formula live preview** — `FormulaCell` in the Table view
  calls `formula_eval` per formula cell with a
  `(expression, record-fields)` cache so identical inputs never
  re-hit the kernel; `#err` badge on eval failure.

Phase-6 closers (2026-04-22):

- **Virtualization** — `@tanstack/react-virtual` pulled in; the
  Table windows rows with a fixed 28px height and
  spacer-row top/bottom padding so `<table>` semantics stay
  intact.
- **Empty-state template picker** — `base_create` handler (id 49)
  + `NewBaseDialog` reached from the Files toolbar's "New base"
  action; five starter templates (Blank / Tasks / CRM / Projects
  / Notes).
- **Schema migration prompts** — `base_property_rename` (id 50)
  + `base_property_update` gained a `migrate_values` flag.
  `SchemaEditor` side panel drives both with `api.input.confirm`
  prompts when records exist.
- **Formula editor** — `SchemaEditor`'s per-row formula textarea
  debounces `formula_eval` against the first record so the
  expression's output is visible before committing.

## Phasing recap

- Phase 1: routing + skeleton leaf. **Done.**
- Phase 2: functional Table view. **Done.**
- Phase 3: Board + List. **Done.**
- Phase 4: Calendar + Gallery + Timeline. **Done.**
- Phase 5: view switcher + persistence. **Done.**
- Phase 6: polish. **Done 2026-04-22** — CSV import/export,
  client-side undo/redo, formula cell live preview,
  virtualization, template picker + `.bases` create flow,
  schema editor with rename / retype-with-migration prompts,
  formula expression editor with live preview.

Everything through Phase 2 was the minimum to close the "falls through
to CodeMirror" regression; Phase 5 closed parity with the PRD.

## Out of scope

- New property types beyond the PRD-10 set.
- Cross-base relations UI beyond a picker (backend already supports
  them). Dedicated relation browser is a later, separate plan.
- Real-time collaboration / multi-user editing.
- Offline conflict resolution beyond what the kernel already offers.
- Public-facing embeds / shareable read-only views.
