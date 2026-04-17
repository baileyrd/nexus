---
project: Nexus
tags: [architecture, reference]
---

# Architecture notes

Quick-reference for the cross-cutting decisions. Long-form lives in
`docs/ARCHITECTURE.md`; this is the TL;DR you want open in a
second pane while debugging.

## The three-layer rule

Every subsystem is built in three layers, with strict boundaries:

1. **Pure library** (`nexus-<name>`). No kernel, no IPC, no `tokio`,
   no Tauri. Unit-tested exhaustively. Examples:
   `nexus-terminal`, `nexus-database`, `nexus-git`.
2. **Core plugin** (`com.nexus.<name>` inside the library crate).
   Wraps the library behind a `CorePlugin: Send + Sync` and exposes
   handlers over `PluginContext::ipc_call`. Registered by
   `nexus-bootstrap`.
3. **Invoker adapters** (Tauri commands in `nexus-app`, CLI
   subcommands in `nexus-cli`, TUI panels in `nexus-tui`). Thin.
   Each one is a JSON marshalling layer that forwards to
   `ipc_call("com.nexus.<name>", …)`.

## Invariants

> [!important] #3 — no library linkage across invokers
> Invokers reach subsystem features through `ipc_call` exclusively.
> Keeps hot-reload, WASM plugin reuse, and the microkernel boundary
> all working.

> [!important] #7 — the kernel is the bus
> Plugins talk to each other only through the kernel event bus or
> `ipc_call`. Direct cross-plugin Rust calls are forbidden.

## Data flow — opening a `.bases` dir

```
User clicks Tasks.bases in file tree
  → openContentTab("base-file:fixtures/bases/Tasks.bases", …)
  → PaneView sees base-file: prefix → renders <BaseFileView />
  → BaseFileView mounts → invoke("load_forge_base", { relpath })
  → Tauri bridge reads forge_root, calls
    nexus_types::bases::load_base(forge_root / relpath)
  → returns Base { schema, records, views, … }
  → React renders editable grid + <BaseViewPanel />
  → BaseViewPanel calls invoke("db_apply_view", …)
  → Tauri bridge → ctx.ipc_call("com.nexus.database", "apply_view", …)
  → DatabaseCorePlugin::dispatch → nexus_database::views::apply_view
  → AppliedView flows back through the same path to the renderer
```

No step in that chain links `nexus-database` or `nexus-types` into
`nexus-app` beyond the thin adapter. The whole round-trip goes
through kernel IPC.

## Threading

- Most core plugins' `dispatch` is sync. `Mutex<InnerEngine>` is
  the standard guard when an engine needs interior state (see
  `TerminalCorePlugin`).
- Async IPC exists for engines that do HTTP or nested `ipc_call`s
  (AI providers, MCP).
- Blocking reads (PTY) live on dedicated OS threads with `mpsc`
  channels back to the main thread. See the `ceb2dc9` commit
  message for why.

## See also

- [[areas/Microkernel Patterns]]
- [[areas/Editor Shell Architecture]]
- [[projects/Nexus/Overview]]
