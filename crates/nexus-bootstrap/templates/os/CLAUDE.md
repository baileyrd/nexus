# Forge OS — operator manual

This forge follows the **Agentic OS** layout (BL-054). The shape on disk
is part of the contract: the AI navigates by these conventions, and the
shell's `nexus.osArchitecture` panel cross-references them when it ships.

## Memory map

```
raw/        Append-only. Research, transcripts, scratch notes, captures.
            Never edit a file here in place — write a new one and link.
wiki/       Synthesized articles, one concept per file. The dense layer
            the AI cites from. Cross-link freely with [[wikilinks]].
output/     Final deliverables. Read-only after publish; treat as a
            release surface, not a draft surface.
projects/   Active project memory. One folder per project, each with:
              decisions.md   ADR-style append-only log
              state.md       what's in flight, blockers, owners
              learnings.md   what worked / didn't — fuel for future work
ops/        SOPs, runbooks, troubleshooting playbooks.
personal/   Non-work notes. Excluded from work-context retrieval.
archive/    Frozen past projects. Searchable but not re-indexed in
            day-to-day retrieval rotations.
```

## Memory write rules

- **Research arrives in `raw/`.** Source URL, capture date, raw extract.
  No synthesis at write time.
- **Synthesis lives in `wiki/`.** When a topic accretes enough material
  in `raw/`, promote a concept article into `wiki/`. Link the wiki
  article back to its sources.
- **Project decisions append to `projects/<name>/decisions.md`.** New
  ADRs go below older ones — never rewrite a decision, supersede it.
- **State drift goes to `projects/<name>/state.md`.** Updated freely;
  reflects the current reality, not the historical record.
- **Learnings consolidate to `projects/<name>/learnings.md`.** Written
  at project close or at retrospective checkpoints.
- **Outputs publish to `output/`.** Once a deliverable lands here, treat
  it as immutable; iterate by versioning the filename, not editing.

## Architecture

`architecture.md` (sibling to this file) is the canonical
domain → task → skill registry. It's empty until the OS Setup skill runs
the architecture elicitation interview (BL-054 Phase 5). The
`nexus.osArchitecture` panel renders it once it exists; until then,
treat it as the placeholder it is.

## Skills

Skill definitions live under `.forge/skills/`. The `com.nexus.skills`
service auto-scans them on forge open. The "Run" affordance for invoking
a skill from the panel is BL-054 Phase 3 — until that ships, run skills
through the agent surface (`com.nexus.agent::run`) with the skill body
included as `system_prompt_extra`.

## Conventions for AI agents

When writing into this forge:

1. Determine which root the content belongs in (`raw/` / `wiki/` /
   `projects/<name>/` / `output/` / `ops/`) using the rules above. If
   unsure, ask the user — wrong placement compounds.
2. Use `[[wikilinks]]` for cross-references inside the forge so the
   knowledge graph stays connected.
3. Cite sources by filename when synthesising into `wiki/` — every claim
   in the wiki should be traceable back to a `raw/` capture.
4. Keep frontmatter minimal. The fields the shell understands today are
   `title`, `tags`, `category`, `updated`. Extra keys round-trip but are
   not surfaced.
