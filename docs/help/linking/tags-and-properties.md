# Tags and properties

Tags are lightweight, free-form labels. **Properties** (YAML
frontmatter) are typed structured data. They're complementary: use tags
for quick filing, properties for fields you want to query like a
database.

## Tags

Two ways to tag a note:

```markdown
---
tags: [project, draft, q2-2026]
---

Inline #also-works anywhere in the body.
```

Inline tags are the inline `#word` form (no spaces, hyphens and slashes
allowed: `#area/work`, `#status-draft`).

### Tags panel

Right-sidebar **Tags** panel lists every tag in the forge with a count
and a fuzzy filter. Click a tag to see every note that uses it.

### CLI

```bash
nexus tags list
nexus tags list --format json
nexus tags locate project        # files using #project
```

## Properties (YAML frontmatter)

Anything in the YAML block at the top of a note becomes a typed
**property**:

```markdown
---
title: Launch plan
status: in-progress
priority: high
owner: alex
due: 2026-06-01
estimate: 3
tags: [project]
---

# Body of the note...
```

Supported types: string, number, boolean, date (ISO 8601), array.

### Properties panel

Right-sidebar **Properties** panel lets you edit frontmatter visually.
Changes write back to the file as YAML — the markdown stays
human-readable.

### Querying properties

[Bases](../advanced/bases.md) read note properties as columns and let
you filter, group, and view them as tables, Kanban, or calendars.

`nexus content search` supports `tag:` and `prop:` operators (parser
shipped; full filtering is partial — see backlog item BL-003).

## Hierarchical tags

`#area/work` and `#area/personal` are two distinct tags but the panel
will group them under `area/`. There's no special validation — the
slash is just a convention the UI honors.

## When to use which

- **Tag** when the value is a free-form label you might add ad hoc.
- **Property** when you'll filter, sort, or compute on the value
  (`status: in-progress`, `due: 2026-06-01`).

The two coexist. A note can have YAML `tags:` *and* inline tags *and*
typed properties.
