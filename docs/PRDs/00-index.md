# Nexus PRD Index

**Version 1.0 | April 2026 | Detailed Subsystem Specifications**

This directory contains standalone, implementation-ready PRDs for every Nexus subsystem. Each PRD expands on the high-level [Nexus PRD v0.1](../Nexus-PRD-v0.1.md) with full design, implementation, and UX detail.

**Files are numbered in build order** — the sequence you should implement them.

> **Where are we right now?** See [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md) for a per-PRD status table, gaps, and evidence. Pending work lives in [BACKLOG.md](BACKLOG.md); delivered items are archived in [backlog/](backlog/).

---

## Phase 1 — Foundation

> *Must exist before anything else can run. The CLI is here because it's the headless entry point to the kernel — how you test everything without a UI.*

| # | PRD | Scope |
|---|-----|-------|
| 01 | [Kernel & Event System](01-kernel-event-system.md) | Microkernel, event bus, plugin lifecycle, capability system |
| 02 | [Security Model](02-security-model.md) | Threat model, WASM sandbox, capabilities, credentials, encryption, audit |
| 03 | [Storage Engine](03-storage-engine.md) | File-as-truth, SQLite index, file watcher, CRDT sync, Tantivy search |
| 04 | [Plugin System](04-plugin-system.md) | Core/community plugins, WASM sandbox (wasmtime), packaging, hot-reload |
| 04a | [Plugin Templates](04a-plugin-templates.md) | cargo-generate templates, manifest schemas, lifecycle stubs, event wiring |
| 05 | [CLI](05-cli.md) | `nexus` commands, output formatting, shell completions, headless mode, watch mode |

## Phase 2 — Core Surfaces

> *The first things users see and interact with. Requires Phase 1 for storage, plugin registration, and rendering.*

| # | PRD | Scope |
|---|-----|-------|
| 06 | [File Formats](06-file-formats.md) | Markdown, MDX, Canvas, Bases, forge config — full format specifications |
| 07 | [Theming & UI](07-theming-ui.md) | CSS variables, themes, workspace layout, Zustand, IPC, platform chrome, accessibility |
| 08 | [Editor Engine](08-editor-engine.md) | Block tree, rich text, CodeMirror 6, MDX, slash commands, undo/redo |

## Phase 3 — Developer Power Features

> *Independent core plugins that can be developed in parallel. Each registers through Phase 1 infrastructure.*

| # | PRD | Scope |
|---|-----|-------|
| 09 | [Terminal & Process Manager](09-terminal-process-manager.md) | PTY, sessions, process lifecycle, signals, output buffers, AI integration |
| 10 | [Database Engine](10-database-engine.md) | .bases format, property types, views, formulas, relations, rollups |
| 11 | [Git Integration](11-git-integration.md) | git2 crate, status/diff/blame, staging, commits, merge/rebase, auto-sync |

## Phase 4 — Intelligence Layer

> *AI capabilities that reach across all subsystems built in Phases 1–3.*

| # | PRD | Scope |
|---|-----|-------|
| 12 | [AI Engine](12-ai-engine.md) | Provider traits, context assembly, streaming, tool calling, embeddings, inline assist |
| 13 | [Skills](13-skills.md) | .skill.md format, activation, composition, built-in skills, authoring SDK |
| 14 | [MCP Integration](14-mcp-integration.md) | MCP host + server, tool/resource definitions, auth, dynamic registration |

## Phase 5 — Autonomy & Automation

> *Orchestration layers that compose everything built above.*

| # | PRD | Scope |
|---|-----|-------|
| 15 | [Agent System](15-agent-system.md) | Agent framework, planning, tool access, memory, built-in archetypes, collaboration |
| 16 | [Workflow System](16-workflow-system.md) | Triggers, conditions, actions, variables, control flow, AI steps, templates |

## Phase 6 — Scale

> *Additional platforms after desktop is solid.*

| # | PRD | Scope |
|---|-----|-------|
| 17 | [Desktop Strategy](17-cross-platform-strategy.md) | Tauri 2.x desktop shell on Linux / macOS / Windows. Web + mobile sections preserved as deferred design rationale (DG-38, 2026-05-12). |

---

## Key Decisions Made in These PRDs

Several open questions from the v0.1 PRD are resolved in these detailed specifications:

- **WASM Runtime:** Wasmtime (see PRD 04)
- **.bases File Format:** TOML schema + JSON records hybrid (see PRD 10, 06)
- **Skills Format:** First-class `.skill.md` files with YAML frontmatter (see PRD 13)
- **Formula Language:** Notion-compatible subset (see PRD 10)
- **Markdown Parser:** comrak (GFM support) (see PRD 03, 06)
- **Vector Storage:** sqlite-vec extension for embeddings (see PRD 12)
- **Workflow System:** New subsystem added — event-driven automation pipelines (see PRD 16)

## Dependency Graph

```
Phase 1: Kernel (01) → Security (02) → Storage (03) → Plugins (04) → CLI (05)
              │
Phase 2:      ├── File Formats (06) → Theming/UI (07) → Editor (08)
              │
Phase 3:      ├── Terminal (09) ──┐
              ├── Database (10) ──┼── (parallel, independent)
              ├── Git (11) ───────┘
              │
Phase 4:      ├── AI Engine (12) → Skills (13) → MCP (14)
              │
Phase 5:      ├── Agents (15) → Workflows (16)
              │
Phase 6:      └── Cross-Platform (17)
```

## How to Read These PRDs

Each PRD follows a consistent structure:

1. **Executive Summary** — What this subsystem does and why it matters
2. **Implementation-Ready Sections** — Rust types, trait definitions, algorithms, data models, schemas
3. **Design-Level Sections** — Architecture decisions, performance targets, testing strategy
4. **UX Sections** — User flows, UI descriptions, interaction patterns
5. **Acceptance Criteria** — Testable criteria for completion
6. **Dependencies** — What this subsystem needs and what needs it
