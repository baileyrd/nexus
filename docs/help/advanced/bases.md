# Bases (databases and views)

A **base** turns a folder of notes (or any structured data) into a
database with multiple views: table, Kanban, calendar. Think
Notion-style databases on top of plain markdown.

## Create a base

A base is a `.bases` TOML file plus an external JSON record store:

```toml
# tasks.bases
name = "Tasks"
records = "tasks.records.json"

[[fields]]
id = "title"
type = "string"
required = true

[[fields]]
id = "status"
type = "select"
options = ["todo", "in-progress", "done"]

[[fields]]
id = "due"
type = "date"

[[fields]]
id = "owner"
type = "string"

[[views]]
id = "by-status"
type = "kanban"
group_by = "status"

[[views]]
id = "due-soon"
type = "table"
sort = ["due"]
filter = "status != 'done'"

[[views]]
id = "calendar"
type = "calendar"
date_field = "due"
```

Open the `.bases` file in the shell to see the views.

## View types

- **Table** — spreadsheet-style grid with sortable columns and filter
  rows.
- **Kanban** — cards grouped by a chosen field. Drag a card between
  columns to mutate the field on disk.
- **Calendar** — events placed on a date field. Drag to reschedule.

## Records

Records live in a sibling JSON file (or a markdown frontmatter source,
configured per base). One record per object:

```json
[
  { "id": "1", "title": "Ship help docs", "status": "in-progress", "due": "2026-05-10", "owner": "alex" },
  { "id": "2", "title": "Write canvas docs", "status": "done", "due": "2026-05-03", "owner": "alex" }
]
```

## Edits

Editing a cell in any view writes back to the records file. Drag-to-
group on Kanban mutates the grouping field. Calendar drag mutates the
date field. All edits are atomic (write to `.forge/temp/` then
rename).

## CLI

```bash
nexus bases query tasks.bases --view due-soon
nexus bases validate tasks.bases       # schema check
```

## Markdown-source bases

Set `records = "@frontmatter"` to source records from the YAML
frontmatter of every note in a folder:

```toml
records = "@frontmatter"
folder = "tasks/"
```

Each note in `tasks/` becomes one record; its frontmatter fills the
columns; editing a cell writes back to that note's frontmatter.

## Limitations

- Database-view **blocks inside notes** (`[[{db:query}]]` inline) are
  on the backlog. Today, views render only when you open the `.bases`
  file directly.
- Formulas and computed columns are minimal — basic arithmetic and
  string concat only.
- No relations between bases yet.

## When to use a base vs. a folder of notes

- A folder of notes wins when each item has free-form structure
  (essays, journal entries, meeting notes).
- A base wins when items have the **same shape** and you want to
  filter, group, or schedule across them (tasks, contacts, books,
  experiments).
