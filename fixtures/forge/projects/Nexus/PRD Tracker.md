---
project: Nexus
tags: [tracking, roadmap]
---

# PRD tracker

Live tracker of which PRDs have shipped vs. not. Source of truth is
[[fixtures/bases/Tasks.bases]]; this note is the narrative summary.

## Shipping-grade (✅)

- **PRD-01 Kernel & Event System** — event bus, lifecycle, capability
  system.
- **PRD-02 Security** — WASM sandbox, capability gating, audit log,
  install-time consent.
- **PRD-03 Storage** — forge layout + SQLite + Tantivy + graph +
  watcher + CRDT hooks.
- **PRD-04 Plugin System** — manifest, WASM, hot-reload, activation
  events.
- **PRD-06 File Formats** — markdown / MDX / canvas / bases / config.
- **PRD-07 Theming & UI** — 497-token CSS registry, contribution
  registry, workspace layout.

## Substantially complete (🟢)

- **PRD-05 CLI** — 12 command groups; agent/workflow CLIs blocked on
  their subsystems.
- **PRD-09 Terminal & Process Manager** — full library (239 tests),
  `com.nexus.terminal` core plugin, both editor-shell surfaces (TUI
  ratatui pane + Tauri React panel) render live PTY output.
- **PRD-11 Git** — 1.1k-line `GitEngine` over `git2`; worker-thread
  wrapper for UI still needed.
- **PRD-17 Cross-Platform** — Tauri desktop shipping.

## Partial (🟡)

- **PRD-08 Editor** — block-tree core shipped; PRD §4 amended to
  CM6-owns-text.
- **PRD-10 Database Engine** — `.bases` parse + validate + formula
  IPC + view engine (all four types) + editable React surface; UI
  polish (property-type editors) in progress.
- **PRD-12 AI** — provider trait + chunker shipped; chat UI,
  streaming, and agent tools not yet.
- **PRD-14 MCP** — `McpClient` + `mcp.toml` parser; Host orchestrator
  pending.

## Spec-only (⚪)

- **PRD-13 Skills**
- **PRD-15 Agent System**
- **PRD-16 Workflow System**

## Next slices (from the live board)

See [[fixtures/bases/Tasks.bases]] sorted by `priority` desc:

- Editable base records (in progress — this note is evidence it's
  basically done)
- AI chat UI with streaming (next big slice)
- Property-type editors (blocked on BL-002)
- MCP Host lifecycle orchestrator

## See also

- [[projects/Nexus/Overview]]
- [[areas/Microkernel Patterns]]
