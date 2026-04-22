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

#### Phase 1 — view registration + routing

Budget: 2–3 hours.

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

Budget: 1 day.

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

Budget: 1 day total.

- **Board (Kanban)**: columns keyed on a `select` property; drag-drop
  between columns updates that property via `update_record`.
- **List**: grouped rows by a chosen property, collapsible groups.
  Mostly a thin reskin of the grid with group headers.

#### Phase 4 — Calendar + Gallery + Timeline

Budget: 2–3 days, one per view.

- **Calendar**: month grid keyed on a `date` property; click empty
  cell → create record with that date prefilled.
- **Gallery**: card grid with cover image from a `files` property. No
  edit-in-place; cards open the record.
- **Timeline**: horizontal swimlanes keyed on a `select`; start/end
  dates from two `date` properties. Zoomable.

#### Phase 5 — views persistence + switcher

Budget: half a day.

- View selector pill bar at the top of the leaf.
- "+" to create a view (picks a base view type + name).
- Rename / duplicate / delete from a context menu.
- All changes round-trip through `create_view` / `update_view` /
  `delete_view` so the `.bases` TOML file is the source of truth; no
  shell-local state.

#### Phase 6 — polish + edge cases

- **Formula engine UX**: inline formula editor with autocomplete and
  validation feedback. Hook `formula_eval` for live preview.
- **CSV import/export buttons**: wire the existing `csv_import` /
  `csv_export` handlers to the Table view toolbar.
- **Undo/redo**: leverage the kernel's edit history if present;
  otherwise a shallow client-side stack scoped to the current session.
- **Schema migration**: change-type prompts warning + preview rows.
- **Empty-state UX**: first-run template picker (Tasks / CRM /
  Projects / Notes) that seeds property types and a starter view.

## Phasing recap

- Phase 1: routing + skeleton leaf — behaves like a proper document tab.
- Phase 2: functional Table view — day-one usable for most notes apps
  use cases.
- Phase 3: Board + List — parity with Obsidian Bases.
- Phase 4: Calendar + Gallery + Timeline — feature-complete.
- Phase 5: view switcher + persistence — round-trip to `.bases` file.
- Phase 6: polish.

Everything through Phase 2 is the minimum to close the "falls through
to CodeMirror" regression; Phase 5 closes parity with the PRD.

## Out of scope

- New property types beyond the PRD-10 set.
- Cross-base relations UI beyond a picker (backend already supports
  them). Dedicated relation browser is a later, separate plan.
- Real-time collaboration / multi-user editing.
- Offline conflict resolution beyond what the kernel already offers.
- Public-facing embeds / shareable read-only views.
