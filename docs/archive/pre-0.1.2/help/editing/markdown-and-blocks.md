# Markdown formatting and blocks

Nexus speaks CommonMark + GitHub-flavored markdown, plus a small set of
extensions for wikilinks, embeds, callouts, math, footnotes, and block
references.

## Basic formatting

```markdown
# Heading 1
## Heading 2

**bold**, *italic*, ~~strike~~, `inline code`

> Blockquote line.

- bullet
- bullet
  - nested

1. ordered
2. ordered

- [ ] task
- [x] done

[link text](https://example.com)
![image](path/to/image.png)

---
```

## Code blocks

Triple-backtick fenced with a language tag for syntax highlighting:

````markdown
```rust
fn main() { println!("hi"); }
```
````

## Tables

```markdown
| Col A | Col B |
|-------|-------|
| 1     | 2     |
```

## Math

`$inline$` and `$$display$$` LaTeX math, rendered with KaTeX.

## Footnotes

```markdown
Some claim.[^1]

[^1]: Source.
```

## Callouts

```markdown
> [!note] Title
> Body text.

> [!warning]
> Body.
```

Built-in types: `note`, `tip`, `warning`, `danger`, `info`, `quote`.

## Wikilinks

```markdown
[[Other Note]]                   resolved by filename
[[Other Note|alias]]             with display text
[[folder/Other Note]]            scoped path
```

See [Wikilinks](../linking/wikilinks.md).

## Embeds

`![[…]]` instead of `[[…]]` embeds the target inline:

```markdown
![[image.png]]                   image
![[Other Note]]                  the whole note rendered
![[Other Note#Heading]]          a single section
![[Other Note#^block-id]]        a single block
```

## Block references

Every block in a Nexus note has a stable ID. To reference one:

```markdown
[[Other Note#^a1b2c3]]
```

The block ID is appended as `^a1b2c3` at the end of the block's line on
disk. The shell shows IDs on hover and exposes a **Copy block link**
action in the block-handle menu.

Block links survive content edits to the surrounding text. They break
only when the block itself is deleted.

## Tags

Inline `#tag-name` anywhere in the body, or a list in YAML frontmatter:

```markdown
---
tags: [project, draft]
---
```

## Frontmatter (properties)

YAML at the top of the file becomes structured **properties** that
plugins can read and that the **Properties** panel can edit visually:

```markdown
---
title: Launch plan
status: in-progress
owner: alex
due: 2026-06-01
tags: [project]
---
```

See [Tags and properties](../linking/tags-and-properties.md).

## What renders, what doesn't

Live preview renders all of the above. Source mode shows the raw
markdown unchanged. The on-disk file is **always** the markdown — the
shell never stores rendered HTML.
