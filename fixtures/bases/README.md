# `.bases` test fixtures

Three realistic `.bases` directories you can drop into a Nexus forge to
exercise the PRD-10 view engine + editable record surface end-to-end.
Each covers a different shape so you can regression-check all four
view types without having to contrive data from scratch.

## The fixtures

| Directory | What it exercises |
|-----------|-------------------|
| `Tasks.bases` | Kanban by `status` · Calendar by `due` · filters with `neq` · multi-level sort · multi-select `tags` |
| `Books.bases` | Gallery-first presentation · null-ratings (nulls-last sort) · `gte` numeric filter (top-rated) · filtered gallery (reading-now) |
| `Contacts.bases` | 6 different views against one dataset · `in` filter (reach-out) · `eq` filter with sentinel-grouped kanban · dormant-first sort (asc on a date column with old values) |

Every record has an `id` plus an interesting spread of field shapes —
text, `select` options, numeric ranges, ISO-date strings, `multi-select`
arrays, long-text notes, and scattered `null`s so you can watch the
null-sort semantics behave correctly.

## How to use them

### Option A — seed an existing forge

```bash
bash scripts/seed_fixtures.sh /path/to/your/forge
```

The script copies all three `.bases` directories under
`<forge>/fixtures/` (creating the parent if needed). Re-running is
idempotent — directories that already exist are skipped unless you
pass `--overwrite`.

### Option B — point the app at the repo

Launch the desktop app with `NEXUS_FORGE_DIR` set to the repo root:

```bash
NEXUS_FORGE_DIR=/mnt/c/Users/baile/dev/Nexus cargo run -p nexus-app
```

Then navigate to `fixtures/bases/` in the file tree. Clicking a
`.bases` directory opens the editable base surface.

## What to try

1. **Open `Tasks.bases`.** Switch between `All tasks`, `By status`
   (kanban), `By due date` (calendar), and `Cards` (gallery). Confirm
   the empty `due` field lands in the "(none)" calendar bucket.
2. **Edit a record.** Click a `priority` cell, change `3` to `5`,
   press Enter. The save indicator should flip to `Saving…` then back
   to `Saved` within ~400ms. Refresh the tab — the change persists.
3. **Add a record.** Click `+ Record`, fill in the cells. Try typing
   `backend, docs` in the `tags` column — the heuristic parser
   converts it to a JSON array on commit.
4. **Toggle a filter by editing the data.** In `Open` view on Tasks,
   mark a `todo` task as `done` — it disappears from the view on the
   next render (the filter lives in `views.toml`, not a local state
   hack).
5. **Delete a record** via the `×` button. The removal autosaves.

## Editing the fixtures themselves

The files on disk are the source of truth. Any edit you make through
the UI writes back through the `save_forge_base` Tauri command, which
calls `nexus_types::bases::save_base` — the same function that
serializes fixtures from the Rust unit tests. Round-trip safe.

If a view stops rendering as expected, check:

- `views.toml` for a typo in `operator` (valid operators are listed in
  [`nexus_database::views::validate_filter_operator`](../../crates/nexus-database/src/views.rs))
- `schema.json` for a `required: true` field that your new record
  omitted (the `validate_record` helper will reject the save)
- `records.json` for JSON that isn't an array at the top level

## Why fixtures, not unit tests

The engine is unit-tested — 18 view tests in `nexus-database`. These
directories cover the *UX loop*: can a user click through the
renderers, inline-edit a cell, and trust the save? Things that
matter at the React + Rust boundary don't surface from pure-logic
tests alone.
