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

## What's *not* archived

| Doc | Why it's still current |
|---|---|
| `docs/ARCHITECTURE.md`, `docs/architecture/C4.md` | Current architecture overview |
| `docs/legacy-shell-retirement.md` | Current record of the migration |
| `docs/leaf-architecture.md` | Current pane/leaf architecture reference |
| `docs/editor-transaction-architecture.md` | Current edit-flow reference |
| `docs/ipc-schemas.md` | Current IPC schema policy |
| `docs/nexus-cli.md` | Current CLI surface doc |
| `docs/notion-block-ux-plan.md` | In-flight plan; phases not all shipped |
| `docs/OPEN-ITEMS.md` | Current post-migration carryover gaps |
| `docs/REQUIRED-FOR-FORMAL-RELEASE.md` | Current "deferred to public release" tracker |
| `docs/AI-INTEGRATION-DIRECTIONS.md`, `AI-MEMORY-LAYER-PLAN.md`, `AI-AMBIENT-COPILOT-PLAN.md` | Active AI-roadmap planning docs |
| `docs/writing-your-first-plugin.md` | Plugin author tutorial |
| `docs/references/` | Active UX references (Obsidian settings modal, etc.) |
| `docs/PRDs/*` | All current product requirements |
| `docs/adr/*` | All architecture decision records |

## Shell-specific archive

`shell/docs/archive/` mirrors this pattern for docs scoped to the
shell crate. See [`shell/docs/archive/`](../../shell/docs/archive/)
for the inventory.
