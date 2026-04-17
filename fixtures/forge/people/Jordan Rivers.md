---
tags: [person, contact]
company: Research Collective
role: Researcher
base: fixtures/bases/Contacts.bases
---

# Jordan Rivers

Qualitative researcher at Research Collective. Wants to pilot
Nexus for their interview coding archive — see
[[fixtures/bases/Contacts.bases|record c-002]].

## What they do

Runs a team of three doing discovery interviews across
enterprise B2B. Typical workflow today: Otter transcripts in
Google Docs, thematic coding in spreadsheets, a pile of tags
they re-derive every quarter.

## Where Nexus fits

- Transcripts as markdown notes with frontmatter tags.
- Codes as `.bases` records with `multi-select` tag fields —
  exactly the shape [[fixtures/bases/Books.bases]] already
  demonstrates.
- A kanban view per project grouping interviews by status
  (raw / coded / synthesised).

## Open questions before pilot

- [ ] Does the storage engine handle ~2k markdown notes without
      Tantivy pagination tweaks?
- [ ] Is a relation (BaseRelation) enough for their
      interview → participant cross-ref, or do we need rollups
      first?
- [ ] How do they share views with a teammate who doesn't have
      Nexus installed? Export-to-CSV exists; export-to-PDF is
      not on the roadmap.

## Links

- [[people/Maya Patel]] (mutual intro)
- [[fixtures/bases/Contacts.bases]]
