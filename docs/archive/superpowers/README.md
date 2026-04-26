# Superpowers Archive

> **Archived 2026-04-26** — Folder relocated from `docs/superpowers/`
> to `docs/archive/superpowers/` as part of the docs-cleanup pass.
> Contents are pre-implementation specs and plans dated 2026-04-11
> through 2026-04-17, produced through the superpowers workflow
> (brainstorming → spec → plan → execute). All material here predates
> the Phase 4 plugin-first-shell migration and most items have shipped
> against the current `shell/` + `crates/`.

## What's here

Two parallel directories, paired by date and topic:

- `specs/` — design specs (the "what / why" output of brainstorming).
- `plans/` — implementation plans (the "how / order" with checkboxes
  for task tracking).

Each topic appears twice: a `*-design.md` spec under `specs/` and a
matching `*.md` (or `*-plan.md`) plan under `plans/`. The split
follows the superpowers workflow.

## Why preserved

- Commit messages and other docs reference these files by date-stamped
  filenames as the source of truth for design decisions made during
  M1 / M2 work.
- The specs document the requirements at the time each PRD was
  brainstormed; useful when re-evaluating tradeoffs in future work.

## Pointers to current docs

If you're looking for current architecture or product requirements,
go to:

- [`docs/PRDs/`](../../PRDs/) — current product requirements (PRDs 01–17).
- [`docs/PRDs/IMPLEMENTATION_STATUS.md`](../../PRDs/IMPLEMENTATION_STATUS.md) —
  rolling tracker of PRD status.
- [`docs/ARCHITECTURE.md`](../../ARCHITECTURE.md) — current C4-level
  architecture.
- [`docs/adr/`](../../adr/) — architecture decision records.

## Inventory

### specs/

- `2026-04-11-nexus-m1-foundation-spec.md` — M1 foundation spec.
- `2026-04-11-nexus-prd-01-kernel-interface-spec.md` — Kernel + event-bus interface.
- `2026-04-11-nexus-roadmap-design.md` — Top-level v0.1 roadmap.
- `2026-04-12-nexus-prd-02-security-design.md` — Security model.
- `2026-04-12-nexus-prd-03-storage-design.md` — Storage engine.
- `2026-04-12-nexus-prd-04-plugins-design.md` — Plugin system.
- `2026-04-12-nexus-prd-04a-templates-design.md` — Plugin templates.
- `2026-04-12-nexus-prd-05-cli-design.md` — CLI surface.
- `2026-04-12-nexus-tui-design.md` — TUI design.
- `2026-04-13-ai-engine-design.md` — AI engine.
- `2026-04-13-daily-notes-typed-properties-design.md` — Daily notes + typed properties.
- `2026-04-13-knowledge-graph-design.md` — In-memory knowledge graph.
- `2026-04-13-mcp-server-design.md` — MCP server.
- `2026-04-13-phase5-polish-design.md` — Phase 5 polish.
- `2026-04-13-prd-06-markdown-enhancements-design.md` — Markdown extensions.
- `2026-04-13-search-scoping-design.md` — Search operators.
- `2026-04-17-split-size-persistence-design.md` — Split-size persistence.

### plans/

Mirror of the specs above, one per topic.
