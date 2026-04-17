---
project: Nexus
status: active
tags: [project, meta]
stakeholders: [bailey]
---

# Nexus — project overview

A Rust-based, AI-native developer knowledge environment. Obsidian
for the shape, VS Code for the plug-in model, a microkernel
underneath everything.

## Pillars

1. **Microkernel.** The kernel knows about events, capabilities, and
   plugin lifecycle — nothing else. Every feature (storage, editor,
   terminal, AI, …) lives in a plugin that registers into extension
   points. See [[areas/Microkernel Patterns]].
2. **Editor shell.** The application chrome is a thin, plugin-
   powered surface — sidebar, tabs, status bar, command palette.
   See [[areas/Editor Shell Architecture]].
3. **Forge as the unit of state.** All content — notes, bases,
   canvases, config — lives on disk inside a forge directory. No
   hidden app-level state.

## Current status

Cross-ref [[projects/Nexus/PRD Tracker]] for per-PRD detail.

```
✅ Kernel + event system        PRD-01
✅ Security model               PRD-02
✅ Storage engine               PRD-03
✅ Plugin system                PRD-04
🟢 CLI                          PRD-05
✅ File formats                 PRD-06
✅ Theming + UI                 PRD-07
🟡 Editor engine                PRD-08
🟢 Terminal + process manager   PRD-09
🟡 Database engine              PRD-10
🟢 Git                          PRD-11
🟡 AI                           PRD-12
⚪ Skills                       PRD-13
🟡 MCP integration              PRD-14
⚪ Agents                       PRD-15
⚪ Workflows                    PRD-16
🟢 Cross-platform               PRD-17
```

## Running tasks

Live task board at [[fixtures/bases/Tasks.bases]]. Open it in any of
five views — kanban by status, calendar by due date, top-rated
gallery, and a simple filtered table of the open tasks.

## Architectural constraints

> [!important] Invariant #3
> Invokers (CLI, TUI, Tauri) must reach subsystem features via
> `ipc_call("com.nexus.<subsystem>", …)`, never by linking the
> library directly. The terminal + database integrations this
> forge ships both follow this rule.

## Further reading

- [[projects/Nexus/Architecture Notes]]
- [[areas/Microkernel Patterns]]
- [[areas/Editor Shell Architecture]]
- [[notes/2026-04-17 Daily]]
