---
aliases: [Home, Welcome]
tags: [meta, onboarding]
created: 2026-04-17
---

# Welcome to your demo forge

This forge is the result of running `scripts/seed_fixtures.sh` — a
realistic Nexus forge with every subsystem exercised. Open any file
from the left-hand tree to explore.

## The lay of the land

- **[[Getting Started]]** — a two-minute tour of the UI with
  keyboard shortcuts. Start here.
- **[[notes/2026-04-17 Daily]]** — a daily note template with tasks,
  wikilinks, and tags. Daily notes live under `notes/`.
- **[[projects/Nexus/Overview]]** — a project dossier backed by the
  [[fixtures/bases/Tasks.bases|Tasks database]].
- **[[areas/Microkernel Patterns]]** — an evergreen note on the
  architecture the rest of this forge demonstrates.
- **[[people/Maya Patel]]** — one of several contact notes cross-
  referenced to the [[fixtures/bases/Contacts.bases|Contacts]]
  database.
- **`Workspace.canvas`** — a whiteboard showing how notes, bases,
  and terminals relate. Drag nodes around.

## What to try

- [ ] Press **Ctrl+K** to open the command palette; run
      `Terminal: Open`.
- [ ] Click `fixtures/bases/Tasks.bases` in the tree — inline-edit a
      priority cell, watch it autosave.
- [ ] Use **Ctrl+F** to full-text search this forge (try
      `microkernel`).
- [ ] Open `Workspace.canvas` and explore the graph.
- [ ] Create a new daily note — copy `templates/Daily Note.md` into
      `notes/` and rename it to today's date.

## How the content is organized

```
README.md               ← you are here
Getting Started.md
Workspace.canvas
notes/                  ← dated daily notes
projects/               ← ongoing work with sub-folders per project
areas/                  ← evergreen, topic-based notes
people/                 ← one file per contact, mirrors Contacts.bases
templates/              ← skeletons you copy into place
fixtures/bases/         ← three demo databases (Tasks, Books, Contacts)
```

This layout is just the default the seed script ships — every folder
and file is yours to reorganize or delete.
