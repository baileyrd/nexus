# Nexus Documentation

> Repository-level documentation index. Last reorganized 2026-04-26.

This directory holds the load-bearing docs for Nexus development:
architecture, product requirements, decision records, and active
planning material. Anything that's been superseded, completed, or
made historical by the Phase 4 migration lives under
[`archive/`](archive/) with a one-line note explaining why.

## Start here

| Question | Read |
|---|---|
| What is Nexus and how is it built? | [`ARCHITECTURE.md`](ARCHITECTURE.md), [`architecture/C4.md`](architecture/C4.md) |
| What's the current shell architecture? | [`leaf-architecture.md`](leaf-architecture.md), [`editor-transaction-architecture.md`](editor-transaction-architecture.md) |
| Why does the codebase look the way it does? | [`adr/`](adr/) — architecture decision records |
| What was the recent legacy-shell migration? | [`legacy-shell-retirement.md`](legacy-shell-retirement.md) |
| What capabilities ship and what's deferred? | [`PRDs/IMPLEMENTATION_STATUS.md`](PRDs/IMPLEMENTATION_STATUS.md), [`PRDs/BACKLOG.md`](PRDs/BACKLOG.md) |
| What's the IPC contract? | [`ipc-schemas.md`](ipc-schemas.md) |
| How do I write a plugin? | [`writing-your-first-plugin.md`](writing-your-first-plugin.md) plus [`shell/docs/`](../shell/docs/) |
| What about the CLI? | [`nexus-cli.md`](nexus-cli.md), [`PRDs/05-cli.md`](PRDs/05-cli.md) |

## Layout

```
docs/
├── README.md                              ← this file
│
│   ── Architecture (current) ──
├── ARCHITECTURE.md
├── architecture/C4.md
├── leaf-architecture.md
├── editor-transaction-architecture.md
├── ipc-schemas.md
├── legacy-shell-retirement.md
│
│   ── Product (current) ──
├── PRDs/                                  PRDs 01–17 + status + backlog
├── adr/                                   architecture decision records
│
│   ── Active planning ──
├── notion-block-ux-plan.md                in-flight
├── AI-INTEGRATION-DIRECTIONS.md           AI-roadmap planning
├── AI-MEMORY-LAYER-PLAN.md
├── AI-AMBIENT-COPILOT-PLAN.md
├── OPEN-ITEMS.md                          post-migration carryover
├── REQUIRED-FOR-FORMAL-RELEASE.md         deferred to public release
│
│   ── Reference ──
├── nexus-cli.md
├── writing-your-first-plugin.md
├── references/                            UX reference captures
│
│   ── Archive ──
└── archive/                               historical / superseded — see archive/README.md
    ├── planning/                          phase plans + audits (formerly docs/planning/)
    ├── superpowers/                       pre-impl specs (formerly docs/superpowers/)
    └── *.md                               individual completed plans + audits
```

## Conventions

- **ADRs are immutable**: once accepted, an ADR's content is not edited.
  If a decision is later revised, write a new ADR that supersedes it.
- **PRDs are authoritative product spec**: when behavior diverges from
  a PRD, fix the PRD or the code — not both silently.
- **Plans go to archive when shipped**: an implementation plan in the
  top level of `docs/` should describe work that's still active or
  upcoming. Once shipped, move it under `archive/` with an archive
  note, and keep architecture-level details in a parallel
  `*-architecture.md` doc if they're load-bearing.
- **Audits stay where they were taken**: a point-in-time audit doesn't
  age into "current architecture"; it stays as a snapshot under
  `archive/` and a fresh audit is filed if needed.

## Shell-specific docs

The shell crate has its own docs tree at [`shell/docs/`](../shell/docs/)
covering plugin architecture, the slot/registry system, and the
extension host. Shell-specific archive lives at
[`shell/docs/archive/`](../shell/docs/archive/).
