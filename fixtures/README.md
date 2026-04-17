# Fixtures — a fully-capable demo forge

Seed data that turns an empty directory into a realistic, hands-on
Nexus forge. Exercises storage (markdown + canvas + frontmatter +
wikilinks + tags), database views (Table / Kanban / Calendar /
Gallery over three `.bases` directories), editable base records,
graph links, and templates — every subsystem that has a UI today.

## One-command seed

```bash
bash scripts/seed_fixtures.sh /path/to/new/forge --init
```

The `--init` flag runs `nexus-cli forge init` first, so the target
directory doesn't need to be pre-prepared. Re-running is idempotent;
pass `--overwrite` to replace existing entries.

Then open it:

```bash
# Desktop app
NEXUS_FORGE_DIR=/path/to/new/forge cargo tauri dev

# TUI
cargo run -p nexus-tui -- /path/to/new/forge
```

## What ships in the forge

```
<forge>/
├── .forge/                           ← created by `forge init`
│   ├── index.sqlite3                 ← files / blocks / links / tags
│   ├── kv.sqlite3                    ← kernel KV store
│   └── plugins/                      ← empty — community plugins go here
├── README.md                         ← welcome note, home base
├── Getting Started.md                ← two-minute tour + keybindings
├── Workspace.canvas                  ← 13-node whiteboard linking the
│                                       content below
├── notes/
│   ├── 2026-04-15 Daily.md           ← daily note with tasks + war story
│   └── 2026-04-17 Daily.md           ← daily note referencing today
├── projects/
│   └── Nexus/
│       ├── Overview.md               ← project dossier with PRD status
│       ├── PRD Tracker.md            ← narrative vs. live Tasks.bases
│       └── Architecture Notes.md     ← data-flow diagrams
├── areas/                            ← evergreen topic notes
│   ├── Microkernel Patterns.md
│   └── Editor Shell Architecture.md
├── people/                           ← one note per contact
│   ├── Maya Patel.md
│   └── Jordan Rivers.md
├── templates/                        ← skeletons to copy when needed
│   ├── Daily Note.md
│   └── Meeting Notes.md
└── fixtures/
    └── bases/
        ├── Tasks.bases/              ← 14 records, 5 views
        ├── Books.bases/              ← 10 records, 5 views
        └── Contacts.bases/           ← 10 records, 6 views
```

## What each fixture exercises

### Markdown content (`README.md`, `notes/`, `projects/`, `areas/`, `people/`)

- **YAML frontmatter** — every note ships with `tags`, sometimes
  `aliases`, `date`, `stakeholders`, `mood`.
- **Wikilinks** — every note links to ≥2 others so the graph view has
  dense edges. Display text overrides
  (`[[fixtures/bases/Tasks.bases|Tasks]]`) are covered too.
- **Inline tags** — `#microkernel`, `#daily`, `#PRD-10` scattered
  throughout for Tantivy to index.
- **Task items** — checked and unchecked `- [ ]` / `- [x]` lines so the
  task query returns something.
- **Callouts** — `> [!note]`, `> [!tip]`, `> [!important]` blocks.
- **Embedded references** — notes link into the `.bases` databases
  so clicking through the graph exercises the whole view engine.

### `Workspace.canvas`

13 nodes across 6 types (file / text / link / group / database /
terminal) and 11 edges. Opening it:

- Shows file nodes loading previews from the forge.
- Shows a `database` node pointing at `Tasks.bases`.
- Shows a `terminal` node whose `command` field runs
  `cargo test -p nexus-terminal --lib`.
- Demonstrates group boundaries (coloured rectangles).

### The three `.bases` databases

| Fixture | Records | Views | Exercises |
|---------|--------:|------:|-----------|
| `Tasks.bases` | 14 | 5 | Kanban by status · Calendar by due (with null bucket) · `neq` filter · multi-level sort |
| `Books.bases` | 10 | 5 | Gallery-first · null-ratings (nulls-last) · `gte` numeric filter · filtered gallery |
| `Contacts.bases` | 10 | 6 | `in` filter · asc-sort on dates · 6 different views over one dataset |

Every record has an `id` plus a realistic spread of field shapes —
text, `select`, number, ISO-date, `multi-select`, long-text notes, and
scattered `null`s so you can watch null-sort semantics behave.

## Smoke test after seeding

1. **Open `README.md`.** It's the forge's landing page. Ctrl-click any
   wikilink to verify they resolve.
2. **Click `fixtures/bases/Tasks.bases`** in the file tree. The
   editable base surface opens. Inline-edit a cell, press Enter, watch
   the save indicator. Reopen the tab — edit persists.
3. **Open `Workspace.canvas`.** Nodes render with their previews;
   edges connect labelled pairs.
4. **Press `Ctrl+Shift+T`** to open a terminal tab. Run `ls` against
   the forge — directory listing matches what you see in the tree.
5. **Press `Ctrl+F`** and search `microkernel`. Results hit both
   `areas/Microkernel Patterns.md` and `projects/Nexus/*.md`.

## Safety net

Every fixture parses through a unit test —
`nexus_types::bases::tests::committed_fixtures_round_trip_through_load_base`
loads each `.bases` fixture on `cargo test -p nexus-types` and
validates every record against its schema. If a future `ViewType`
rename or schema tightening would desync the fixtures, the build
fails before the regression ships.

## Editing the fixtures

The files on disk are the source of truth. Any edit you make through
the UI writes back through `save_forge_base` → `nexus_types::bases::save_base`
— the same function the unit tests use. Round-trip safe.

When tweaking fixtures for new demos, keep the three invariants that
make them good for smoke tests:

1. **Density.** Every note wikilinks ≥2 others. The graph view only
   looks alive with real edges.
2. **Null coverage.** At least one record per `.bases` is missing at
   least one field, so null-handling behaviours (nulls-last sort,
   `(none)` kanban bucket) are visible.
3. **Cross-references.** `people/` notes reference `Contacts.bases`
   record ids; `projects/Nexus/PRD Tracker.md` references
   `Tasks.bases`. This proves the file/database boundaries aren't
   silos.
