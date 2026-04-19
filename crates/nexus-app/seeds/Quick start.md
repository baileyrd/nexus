# Quick start

A five-minute tour of the surfaces you'll use most.

## Writing

Forge's editor is live markdown. Typing `# Heading` gives you a
heading without a round-trip into "preview mode" — the document
always renders in place.

Wikilinks use `[[Note title]]` syntax. They resolve against every
note in the forge, so [[Welcome]] just works even though it's a
sibling, and [[Architecture]] resolves across folders.

Inline `code spans` and fenced code blocks both render with the
JetBrains Mono stack:

```ts
import { forge } from "nexus";

await forge.open("~/notes");
```

## Tables

| Feature       | Status        | Notes                                |
| ------------- | ------------- | ------------------------------------ |
| Wikilinks     | Live          | Resolve cross-folder                 |
| Tables        | Live          | GFM pipe tables                      |
| Backlinks     | Live          | Index refresh < 100 ms on a 10k doc  |
| Canvas        | Scaffolded    | See [[Architecture]]                 |

## Callouts

> [!note]
> Callouts are standard markdown blockquotes with a type hint on
> the first line. Forge themes style them with an ember left-rail
> so important passages don't get lost in the body.

## Tasks

Tasks are regular list items with a checkbox. They're collected
by the tasks panel and queryable via the command palette.

- [x] Install Nexus
- [x] Open this forge
- [ ] Write your first note
- [ ] Invite someone to collaborate

## What's next

- [[Tasks]] — a running list for tiny things you don't want to lose.
- [[Reading list]] — an example database-ish page with status chips.
- [[Architecture]] — how Forge is put together under the hood.
