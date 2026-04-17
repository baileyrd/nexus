---
aliases: [Quickstart, Tour]
tags: [onboarding]
created: 2026-04-17
---

# Getting started with Nexus

Two-minute tour. Start here if you haven't used Nexus before.

> [!note] Editor shell
> The chrome around the edges — sidebar, tabs, status bar, command
> palette — is the thin editor shell. Every panel, view, and tool
> surface you see is a plugin registered into extension points.
> This forge demonstrates the full set.

## Panels

| Key | Panel |
|-----|-------|
| `Ctrl+B` | Toggle the file tree |
| `Ctrl+K` | Command palette |
| `Ctrl+Shift+T` | Open a terminal tab |
| `Ctrl+F` | Full-text search |
| `Ctrl+P` | Go to file |

## Core workflow

1. **Open a note.** Click `notes/2026-04-17 Daily.md` in the tree —
   or press `Ctrl+P` and type `daily`.
2. **Follow a wikilink.** The daily note links to
   [[projects/Nexus/Overview]]. Cmd/Ctrl-click the link to open.
3. **Open a database.** In the tree, click a `.bases` directory
   (e.g. [[fixtures/bases/Tasks.bases|Tasks]]). The editable surface
   appears; switch views via the tab strip.
4. **Run a command.** `Ctrl+K` → `Terminal: Open`. Type anything;
   `Ctrl+D` closes the session.
5. **Search everything.** `Ctrl+F` across the whole forge. Results
   respect frontmatter tags, inline tags like #onboarding, and
   wikilink text.

## The graph

`Ctrl+K` → `Graph: Open` (when wired). Every wikilink in this forge
is a real edge — the seed data creates a graph of about 20 nodes so
the view has something to show.

## Making it yours

- Delete this file. Nothing breaks; the rest of the forge stands on
  its own. (Running the seed script again puts it back.)
- Reorganise the `notes/`, `projects/`, `areas/`, `people/` split
  freely. Nexus doesn't care about folder names.
- Edit `schema.json` inside a `.bases` directory to add a field.
  New cells show up as `—` (null) in the editable table on next
  open.

## See also

- [[areas/Microkernel Patterns]]
- [[areas/Editor Shell Architecture]]
- [[projects/Nexus/Overview]]
