# Nexus Documentation

> Reorganized 2026-05-12 — root cleared of loose docs; every file
> lives under the directory that matches its kind.

Pick the path that matches what you're trying to do.

## I'm contributing to Nexus core

You're modifying the kernel, a service crate, the bootstrap, IPC handlers,
or the Tauri bridge.

| Read | What it gives you |
|---|---|
| [`../CONTRIBUTING.md`](../CONTRIBUTING.md) | Policies, guardrails, the `dep_invariants` test |
| [`architecture/C4.md`](architecture/C4.md) | C4 model: System Context → Containers → Components → Code |
| [`architecture/invariants.md`](architecture/invariants.md) | The four rules and how each is enforced |
| [`architecture/leaf-architecture.md`](architecture/leaf-architecture.md) | Shell pane/leaf chrome-vs-content separation |
| [`architecture/editor-transaction-architecture.md`](architecture/editor-transaction-architecture.md) | Edit-flow model from shell → kernel → storage |
| [`architecture/ipc-schemas.md`](architecture/ipc-schemas.md) | IPC drift-check policy; generated dirs are the listing |
| [`architecture/legacy-shell-retirement.md`](architecture/legacy-shell-retirement.md) | Why `app/` and `crates/nexus-app` are gone (v0.4.0) |
| [`adr/README.md`](adr/README.md) | ADR conventions; jump from there to the numbered ADRs |
| [`PRDs/IMPLEMENTATION_STATUS.md`](PRDs/IMPLEMENTATION_STATUS.md) | What's shipped, in-progress, blocked — the live state doc |
| [`PRDs/BACKLOG.md`](PRDs/BACKLOG.md) | Live work-item index referenced by other planning docs |

## I'm writing a plugin

You're building a community WASM/JS plugin or a shell-side TS plugin.

| Read | What it gives you |
|---|---|
| [`developer/README.md`](developer/README.md) | **Start here.** Topic-decomposed developer hub: plugins, editor, UI, themes, core plugins, reference. |
| [`developer/getting-started.md`](developer/getting-started.md) | Ten-minute "hello world" walkthrough. |
| [`shell/writing-a-plugin.md`](shell/writing-a-plugin.md) | In-depth shell plugin reference (manifest, sandbox, capabilities, slot system, worked example). |
| [`shell/plugin-api.md`](shell/plugin-api.md) | The `@nexus/extension-api` import surface. |
| [`adr/0002-hierarchical-capability-strings.md`](adr/0002-hierarchical-capability-strings.md) | Capability taxonomy |
| [`adr/0015-iframe-sandbox-plugin-runtime.md`](adr/0015-iframe-sandbox-plugin-runtime.md) | JS/TS sandbox model |
| [`adr/0016-microkernel-native-vs-wasm-plugin-split.md`](adr/0016-microkernel-native-vs-wasm-plugin-split.md) | Native vs WASM/JS choice |
| [`PRDs/templates/community-plugin/README.md`](PRDs/templates/community-plugin/README.md) | Plugin scaffolding template |
| [`PRDs/templates/core-plugin/README.md`](PRDs/templates/core-plugin/README.md) | Core plugin template |

## I'm using Nexus to manage notes

You want to install Nexus, point it at a forge of markdown, and use it.

| Read | What it gives you |
|---|---|
| [`../README.md`](../README.md) | Install, build, CLI / TUI / shell / MCP quickstart |
| [`help/README.md`](help/README.md) | **Help docs** — task-oriented guides modelled on Obsidian's help (start here) |
| [`users/README.md`](users/README.md) | End-user documentation hub (reference) |
| [`users/cli.md`](users/cli.md) | Full CLI command reference |
| [`users/tui.md`](users/tui.md) | TUI key bindings and behaviour |
| [`users/mcp.md`](users/mcp.md) | MCP server, the `nexus_*` tools, Claude Code / Cursor integration |

## I'm an AI agent picking up context

You're Claude Code, Cursor, or another agent reading the repo for the first time.

| Read | What it gives you |
|---|---|
| [`../CLAUDE.md`](../CLAUDE.md) | Agent-tuned summary: commands, architecture, where things live, the four invariants. **Start here.** |
| [`PRDs/IMPLEMENTATION_STATUS.md`](PRDs/IMPLEMENTATION_STATUS.md) | What is shipped vs in-progress right now |
| [`architecture/invariants.md`](architecture/invariants.md) | The four rules to obey when changing anything structural |
| [`adr/`](adr/) | Why the code looks the way it does |
| [`archive/README.md`](archive/README.md) | When a commit message references a doc you can't find |

## In-flight planning, research, audits

| Directory | What it holds |
|---|---|
| [`roadmap/`](roadmap/README.md) | In-flight planning docs — OPEN-ITEMS, REQUIRED-FOR-FORMAL-RELEASE, exploratory AI plans, notion-block UX plan |
| [`research/`](research/README.md) | Comparative research and ecosystem assessments — Obsidian parity, Tolaria comparison, AnythingLLM/AffinE/CommandBook, GitNexus |
| [`audits/`](audits/README.md) | Point-in-time audit snapshots — subsystem assessments (2026-05-06), AI interaction surface, shell UI, architecture, docs |

## Shell-specific docs

Shell-internal reference under [`shell/`](shell/README.md): plugin
architecture, the slot/registry system, the extension host, and
Obsidian-internal references for parity work
([`shell/obsidian/`](shell/obsidian/)). Shell-specific archive lives
at [`shell/archive/`](shell/archive/README.md).

## References

- [`references/`](references/) — UX references for parity work (Obsidian settings modal, etc.).

## Archive

[`archive/README.md`](archive/README.md) — historical docs (completed
plans, superseded designs). Each carries a `> **Archived <date>** —
<reason>` header.

## Layout

```
docs/
├── README.md                   ← this file (curated entry point)
│
├── architecture/               C4, invariants, leaf, editor-transaction, ipc-schemas, legacy-shell-retirement
├── adr/                        architecture decision records (0001–0025)
├── PRDs/                       17 numbered PRDs, IMPLEMENTATION_STATUS, BACKLOG, templates
│
├── developer/                  plugin authors hub (topic-decomposed)
├── help/                       task-oriented user help
├── users/                      end-user reference (CLI, TUI, MCP)
├── shell/                      shell-internal reference (plugin-api, registry, slot system, ...)
│
├── roadmap/                    in-flight planning
├── research/                   comparative research
├── audits/                     point-in-time audit snapshots
├── references/                 UX reference captures
└── archive/                    historical / superseded
```

## Conventions

- **ADRs are immutable.** Once accepted, an ADR's content is not edited; if a decision is later revised, write a new ADR that supersedes it.
- **PRDs are authoritative product spec.** When behavior diverges from a PRD, fix one — not both silently.
- **Plans go to archive when shipped.** A plan in `roadmap/` should describe work still active or upcoming; once shipped, move it under `archive/` with an archive note. Architecture-level details that outlast the plan should land in `architecture/` separately.
- **Audits stay snapshots.** A point-in-time audit doesn't age into "current architecture"; it stays under `audits/` and a fresh audit is filed if needed.
