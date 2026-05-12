# Documentation Archive

> Historical documentation: completed implementation plans, point-in-time
> audits, pre-migration designs, and reference material for code that no
> longer exists. Reorganized 2026-04-26.

Nothing here is deleted. Anything moved into this folder has an
`> **Archived <date>** — <reason>` line at the top of the file
explaining why it's no longer in the active set.

## When to look here

- You need the *historical* context for a decision (the design that
  led to the current code, or the migration that produced it).
- A commit message, ADR, or older doc references a plan or audit by
  filename and you need to read it.
- You're tracing why a feature works the way it does and the current
  architecture doc points back to a superseded plan.

For anything load-bearing for current development, **read the docs in
the parent directory first** (`docs/ARCHITECTURE.md`,
`docs/leaf-architecture.md`, `docs/legacy-shell-retirement.md`,
`docs/PRDs/`, `docs/adr/`).

## Layout

```
docs/archive/
├── README.md                                  ← this file
├── planning/                                  ← phase plans + audits
│   ├── README.md
│   ├── PHASE-1..5-IMPLEMENTATION-PLAN.md
│   ├── INTEGRATION-REVIEW.md
│   ├── MICROKERNEL-AUDIT.md
│   ├── UI-AUDIT.md
│   ├── SHELL-COMPARISON.md
│   ├── PARITY-CHECKLIST.md
│   └── TERMINAL-BROADCASTER-REFACTOR.md
├── superpowers/                               ← pre-impl specs + plans
│   ├── README.md
│   ├── specs/
│   └── plans/
│
│   ── Top-level archived plans/audits ──
├── FORGE-UI-PLAN.md                           pre-shell-migration UI plan
├── CommandBook-ReverseEngineering-Report.md   external-app static teardown
├── shell-kernel-bridge-plan.md                Phase 0–4 bridge plan, shipped
├── leaf-migration-plan.md                     Leaf+ViewRegistry intro, shipped
├── editor-transaction-wiring-plan.md          editor→kernel routing, shipped
├── editor-phase-status.md                     point-in-time audit
├── editor-shell-auditor.md                    auditor agent prompt
├── bases-shell-plan.md                        Bases UI plan, shipped
├── canvas-shell-plan.md                       Canvas UI plan, shipped
├── global-graph-view-plan.md                  global graph plan, shipped
├── tab-context-menu-plan.md                   tab "⋯" menu plan, shipped
├── wi01-chatpanel-reference.md                legacy ChatPanel reference
├── wi07-agent-status.md                       agent plugin audit
├── wi10-bases-status.md                       Bases plugin audit
├── wi11-canvas-status.md                      Canvas plugin audit
└── wi30-sandbox-design.md                     iframe sandbox design, shipped
```

## Related

- [`../audits/`](../audits/) — point-in-time audit snapshots (kept distinct from archive: snapshots, not completed plans).
- [`../roadmap/`](../roadmap/) — in-flight planning docs (move into archive once shipped).
- [`../research/`](../research/) — comparative research and exploratory assessments.
| `docs/adr/*` | All architecture decision records |
| `docs/README.md` | Curated audience-oriented entry point (added 2026-04-30) |
| `docs/plugin-authors/` | Plugin-author landing + quickstart (added 2026-04-30) |
| `docs/users/` | End-user docs hub: cli / tui / mcp (added 2026-04-30) |
| `docs/roadmap/README.md` | Roadmap index for in-flight planning docs (added 2026-04-30) |

## Shell-specific archive

`shell/docs/archive/` mirrors this pattern for docs scoped to the
shell crate. See [`shell/docs/archive/`](../../shell/docs/archive/)
for the inventory.
