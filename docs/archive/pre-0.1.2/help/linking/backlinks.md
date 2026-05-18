# Backlinks

A **backlink** is a wikilink **into** the current note from somewhere
else. If `daily/2026-05-03.md` contains `[[launch]]`, then `launch.md`
has `daily/2026-05-03.md` as a backlink.

## Backlinks panel

Open the right-sidebar **Backlinks** tab. For the current note you'll
see:

- Every other note that links to it.
- The surrounding paragraph (excerpt) for context.
- A **count** in the panel header.

Click any entry to jump to that location in the linking note.

## CLI

```bash
nexus content backlinks launch.md
nexus content backlinks launch.md --format json
```

## Unresolved-link backlinks

Even unresolved links count. If three notes mention `[[Future Idea]]`
but no `Future Idea.md` exists yet, creating that file immediately
shows three backlinks.

## Block-level backlinks

If the linker uses a block reference (`[[launch#^a1b2]]`), the backlink
is attributed to that specific block. The Backlinks panel groups by
block when you scroll to one.

## How they're computed

Backlinks come from the same on-disk graph that powers
`nexus graph neighbors`. The graph is rebuilt incrementally on every
file change (debounced ~300 ms by the file watcher). Deleting a linker
removes its contributions immediately.

## Excluding folders

If you don't want certain folders to count as link sources (e.g.
`Archive/`), exclude them in `.forge/app.toml`:

```toml
[graph]
exclude = ["Archive/", "Templates/"]
```

(Excluded files are still indexed for search; they just don't
contribute edges to the graph.)
