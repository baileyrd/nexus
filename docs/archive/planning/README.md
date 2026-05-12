# Planning Archive

> **Archived 2026-04-26** — Folder relocated from `docs/planning/` to
> `docs/archive/planning/` as part of the docs-cleanup pass.
> Contents unchanged; refs updated.

These are historical planning and audit artifacts — phase
implementation plans that have been substantially completed, and
point-in-time audits of the shell / microkernel / legacy tri-pane.

They are preserved because commit messages, tests, and other docs
still reference them by work-item id (e.g. "WI-30 sandbox per
PHASE-3-IMPLEMENTATION-PLAN.md §5"), and because the raw findings
in the audits are useful history even when the specific remediation
has shipped.

**Current architecture docs live in `docs/architecture/`**
(`C4.md`, `invariants.md`, `leaf-architecture.md`,
`legacy-shell-retirement.md`) and in `docs/adr/` (architecture
decision records). Go there first for anything load-bearing.

Contents:

- `PHASE-1..5-IMPLEMENTATION-PLAN.md` — per-phase work-item plans
- `INTEGRATION-REVIEW.md` — 2026-04 integration audit that motivated
  the plugin-first shell adoption (see ADR 0011)
- `UI-AUDIT.md` / `MICROKERNEL-AUDIT.md` — 10-dimension security +
  architecture walkthroughs that fed Phase 3 and the PRD backlog
- `SHELL-COMPARISON.md` — per-command legacy-vs-plugin-first map
  captured before the legacy shell was deleted
- `PARITY-CHECKLIST.md` — parity tracking for the same migration
