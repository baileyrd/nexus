# Wikilinks and outgoing links

A **wikilink** is a `[[double-bracketed]]` reference to another note.
They're how Nexus turns a folder of files into a connected workspace.

## Syntax

| Form | Resolves to |
|---|---|
| `[[Foo]]` | The first file in the forge whose name matches `Foo.md` (case-insensitive) |
| `[[folder/Foo]]` | `folder/Foo.md` specifically |
| `[[Foo|alias]]` | `Foo.md`, displayed as "alias" |
| `[[Foo#Heading]]` | `Foo.md` scrolled to the heading |
| `[[Foo#^block-id]]` | `Foo.md` scrolled to the block |
| `![[Foo]]` | Embed (see [Embeds](../editing/embeds-and-mdx.md)) |

## Auto-complete

Type `[[` in the editor and a fuzzy picker shows every note in the
forge. Arrow keys to select, `Enter` to insert. The picker shows the
note title and folder so you can disambiguate.

## Resolution

When you write `[[Foo]]`, Nexus looks for a file whose stem matches
`Foo`. Resolution rules:

1. Exact filename match (case-insensitive).
2. Path match if you wrote a path (`Notes/Foo`).
3. Otherwise: **unresolved**. The link is rendered with a different
   color and clicking it offers to create the file.

The cascade is intentionally simple — no fuzzy matching, no aliases
file. (A 3-tier strict → fuzzy → unresolved cascade is on the backlog
as BL-004.)

## Creating from a wikilink

Click an unresolved `[[New Note]]` in the editor. Nexus prompts you for
the folder, then creates `New Note.md` and opens it. The original link
becomes resolved automatically.

## Outgoing links panel

The right-panel **Outgoing links** tab lists every wikilink in the
current note, with the target's preview. Click to navigate.

CLI:

```bash
nexus content links README.md
```

## Renaming targets

Rename `Foo.md` → `Bar.md` and existing `[[Foo]]` references will go
unresolved. Auto-update of inbound wikilinks on rename is on the
roadmap; for now use search-and-replace across the workspace.

## Performance

Wikilink resolution is indexed: O(1) lookup by filename. Even forges
with tens of thousands of notes resolve links instantly.
