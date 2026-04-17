---
tags: [architecture, evergreen, microkernel]
---

# Microkernel patterns

Evergreen notes on building subsystems in Nexus. Pair with
[[areas/Editor Shell Architecture]] for the UI side.

## The discipline

A microkernel only owns three things: **events**, **capabilities**,
**lifecycle**. Everything else — storage, editor, terminal, AI,
git — is a plugin. The moment a "feature" wants to live in the
kernel, push back.

## Library vs core plugin

When adding a new subsystem, build in this order:

1. **Pure library crate first.** No `tokio`, no `nexus-kernel`, no
   `nexus-plugins`. Exhaustive unit tests. If you can't test it in
   isolation, the seams aren't right yet.
2. **Core plugin second.** A thin `CorePlugin` impl inside the
   library crate wraps the engine behind `ipc_call` handlers. This
   is the single kernel-boundary touchpoint.
3. **Invoker adapters last.** Tauri commands, CLI subcommands, TUI
   panels — each one a JSON marshalling layer over `ipc_call`.

Following this order naturally produces the right seams. Fighting
it creates cross-crate coupling you'll regret.

## Why no cross-plugin Rust calls

Plugins must talk to each other through:

- The kernel event bus (fire-and-forget events)
- `ipc_call` (request/response through the dispatcher)

Why:

- **Hot reload.** Drop a plugin, reload it, nobody else notices.
- **WASM reuse.** The same handler works whether the caller is
  native Rust or WASM.
- **Capability checks.** The kernel sees every call, can audit it,
  can revoke it.

## The reader-thread rule

If your library blocks on external I/O (a PTY, a socket, a
subprocess stdout), put the blocking read on an **OS thread** and
push bytes to a `mpsc` channel. The main thread calls
`recv_timeout` — non-blocking, honours timeouts, no IPC deadlocks.

This is how PRD-09's terminal engine works. The week I spent
debugging a 30-second TUI freeze taught me not to trust
WouldBlock semantics on Linux PTYs.

See [[notes/2026-04-15 Daily]] for the war story.

## The "rename ≠ reshape" check

If a PRD rename (e.g. `BaseView` → `ViewConfig`) should never
break shipped plugins, the reverse-DNS plugin id is the stable
contract. Handler ids are append-only. Argument struct names can
change; the JSON shape on the wire cannot.

## Links

- [[projects/Nexus/Architecture Notes]]
- [[areas/Editor Shell Architecture]]
