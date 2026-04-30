# Nexus Documentation

> Repository-level documentation index. Reorganized 2026-04-30 (audience-oriented).

Pick the path that matches what you're trying to do.

## I'm contributing to Nexus core

You're modifying the kernel, a service crate, the bootstrap, IPC handlers,
or the Tauri bridge.

| Read | What it gives you |
|---|---|
| [`../CONTRIBUTING.md`](../CONTRIBUTING.md) | Policies, guardrails, the `dep_invariants` test |
| [`architecture/C4.md`](architecture/C4.md) | C4 model: System Context → Containers → Components → Code |
| [`architecture/invariants.md`](architecture/invariants.md) | The four rules and how each is enforced |
| [`adr/README.md`](adr/README.md) | ADR conventions; jump from there to the 20 numbered ADRs |
| [`PRDs/IMPLEMENTATION_STATUS.md`](PRDs/IMPLEMENTATION_STATUS.md) | What's shipped, in-progress, blocked — the live state doc |
| [`PRDs/BACKLOG.md`](PRDs/BACKLOG.md) | Live work-item index referenced by other planning docs |
| [`ipc-schemas.md`](ipc-schemas.md) | IPC drift-check policy; generated dirs are the listing |
| [`leaf-architecture.md`](leaf-architecture.md) | Shell pane/leaf chrome-vs-content separation |
| [`editor-transaction-architecture.md`](editor-transaction-architecture.md) | Edit-flow model from shell → kernel → storage |
| [`legacy-shell-retirement.md`](legacy-shell-retirement.md) | Why `app/` and `crates/nexus-app` are gone (v0.4.0) |

## I'm writing a plugin

You're building a community WASM/JS plugin or a shell-side TS plugin.

| Read | What it gives you |
|---|---|
| [`plugin-authors/README.md`](plugin-authors/README.md) | Curated journey for plugin authors |
| [`plugin-authors/quickstart.md`](plugin-authors/quickstart.md) | Scaffold and run your first plugin |
| [`../shell/docs/writing-a-plugin.md`](../shell/docs/writing-a-plugin.md) | In-depth shell plugin reference |
| [`../shell/docs/plugin-api.md`](../shell/docs/plugin-api.md) | The `@nexus/extension-api` surface |
| [`adr/0002-hierarchical-capability-strings.md`](adr/0002-hierarchical-capability-strings.md) | Capability taxonomy |
| [`adr/0015-iframe-sandbox-plugin-runtime.md`](adr/0015-iframe-sandbox-plugin-runtime.md) | JS/TS sandbox model |
| [`adr/0016-microkernel-native-vs-wasm-plugin-split.md`](adr/0016-microkernel-native-vs-wasm-plugin-split.md) | Native vs WASM/JS choice |
| [`templates/community-plugin/README.md`](templates/community-plugin/README.md) | Plugin scaffolding template |
| [`templates/core-plugin/README.md`](templates/core-plugin/README.md) | Core plugin template |

## I'm using Nexus to manage notes

You want to install Nexus, point it at a forge of markdown, and use it.

| Read | What it gives you |
|---|---|
| [`../README.md`](../README.md) | Install, build, CLI / TUI / shell / MCP quickstart |
| [`users/README.md`](users/README.md) | End-user documentation hub |
| [`users/cli.md`](users/cli.md) | Full CLI command reference |
| [`users/tui.md`](users/tui.md) | TUI key bindings and behaviour |
| [`users/mcp.md`](users/mcp.md) | MCP server, the 15 `nexus_*` tools, Claude Code / Cursor integration |

## I'm an AI agent picking up context

You're Claude Code, Cursor, or another agent reading the repo for the first time.

| Read | What it gives you |
|---|---|
| [`../CLAUDE.md`](../CLAUDE.md) | Agent-tuned summary: commands, architecture, where things live, the four invariants. **Start here.** |
| [`PRDs/IMPLEMENTATION_STATUS.md`](PRDs/IMPLEMENTATION_STATUS.md) | What is shipped vs in-progress right now |
| [`architecture/invariants.md`](architecture/invariants.md) | The four rules to obey when changing anything structural |
| [`adr/`](adr/) | Why the code looks the way it does |
| [`archive/README.md`](archive/README.md) | When a commit message references a doc you can't find |

## In-flight planning

The roadmap hub at [`roadmap/README.md`](roadmap/README.md) catalogs
in-flight planning docs. Direct pointers to the active set:

| Read | What it covers |
|---|---|
| [`OPEN-ITEMS.md`](OPEN-ITEMS.md) | Post-migration carryover gaps |
| [`REQUIRED-FOR-FORMAL-RELEASE.md`](REQUIRED-FOR-FORMAL-RELEASE.md) | WI-41/42/44/46 deferred from personal-tool scope |
| [`AI-INTEGRATION-DIRECTIONS.md`](AI-INTEGRATION-DIRECTIONS.md) | Exploratory AI-roadmap design |
| [`AI-MEMORY-LAYER-PLAN.md`](AI-MEMORY-LAYER-PLAN.md) | Personal-memory-layer thinking |
| [`AI-AMBIENT-COPILOT-PLAN.md`](AI-AMBIENT-COPILOT-PLAN.md) | Ambient copilot UX plan |
| [`notion-block-ux-plan.md`](notion-block-ux-plan.md) | Block UX plan in flight |
| [`PRDs/Nexus_Growth_Plan.md`](PRDs/Nexus_Growth_Plan.md) | Long-term growth thinking (currently filed under PRDs/) |

> Mechanical `git mv` of these files into `roadmap/` is queued for a
> follow-up so git history follows the moves. The roadmap hub above
> documents the planned final layout.

## Reference

- [`references/`](references/) — UX references for parity work (Obsidian settings modal, etc.).
- [`../shell/docs/obsidian/`](../shell/docs/obsidian/) — Obsidian internals reverse-engineered for design parity. Reference only — not authoritative for Nexus.

## Archive

- [`archive/README.md`](archive/README.md) — historical docs (completed plans, superseded designs, point-in-time audits). Each carries a `> **Archived <date>** — <reason>` header.

## Layout

```
docs/
├── README.md                              ← this file (curated entry point)
├── ARCHITECTURE.md                        redirect → architecture/C4.md
│
│   ── Architecture (current) ──
├── architecture/
│   ├── C4.md                              canonical C4 model
│   └── invariants.md                      the four rules
├── leaf-architecture.md                   shell pane/leaf model
├── editor-transaction-architecture.md     edit-flow model
├── ipc-schemas.md                         IPC drift-check policy
├── legacy-shell-retirement.md             v0.4.0 migration record
│
│   ── Product (current) ──
├── PRDs/                                  PRDs 01-17, IMPLEMENTATION_STATUS, BACKLOG
├── adr/                                   architecture decision records (0001-0020)
│
│   ── Audiences ──
├── plugin-authors/                        plugin author journey (quickstart + index)
├── users/                                 end-user docs (CLI, TUI, MCP)
├── roadmap/                               in-flight planning index (catalogs the docs below)
│
│   ── In-flight planning (canonical files; mechanical mv into roadmap/ queued) ──
├── OPEN-ITEMS.md
├── REQUIRED-FOR-FORMAL-RELEASE.md
├── AI-INTEGRATION-DIRECTIONS.md
├── AI-MEMORY-LAYER-PLAN.md
├── AI-AMBIENT-COPILOT-PLAN.md
├── notion-block-ux-plan.md
│
│   ── Reference ──
├── references/                            UX reference captures (Obsidian settings modal, etc.)
│
│   ── Archive ──
└── archive/                               historical / superseded — see archive/README.md
    ├── planning/                          phase plans + audits
    ├── superpowers/                       pre-impl specs
    └── *.md                               individual completed plans + audits
```

The audience landings (`plugin-authors/`, `users/`, `roadmap/`) are the
new navigation layer added by the docs-reorg. The current canonical
locations of long-lived planning docs (`OPEN-ITEMS.md`, etc.) are kept
in place so that this PR doesn't move file content — git history
preservation is handled by a follow-up `git mv` step. Cross-links from
this README and from `roadmap/README.md` already use the correct final
paths for everything that has been physically reorganised
(architecture, audience directories, plugin-author quickstart) and the
canonical paths for everything still queued.

## Conventions

- **ADRs are immutable.** Once accepted, an ADR's content is not edited; if a decision is later revised, write a new ADR that supersedes it.
- **PRDs are authoritative product spec.** When behavior diverges from a PRD, fix one — not both silently.
- **Plans go to archive when shipped.** A plan in `roadmap/` should describe work still active or upcoming; once shipped, move it under `archive/` with an archive note. Architecture-level details that outlast the plan should land in `architecture/` separately.
- **Audits stay snapshots.** A point-in-time audit doesn't age into "current architecture"; it stays as a snapshot under `archive/` and a fresh audit is filed if needed.

## Shell-specific docs

The shell crate has its own docs tree at [`../shell/docs/`](../shell/docs/) covering plugin architecture, the slot/registry system, and the extension host. Shell-specific archive lives at [`../shell/docs/archive/`](../shell/docs/archive/).
