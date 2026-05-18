# ADR 0006: KV-Backed, Plugin-Managed Hot-Reload State

**Date:** 2026-04-11
**Status:** Accepted

## Context

PRD 04 keeps hot-reload in M1. When a plugin's WASM file changes, the kernel
swaps in the new version. What happens to the old instance's in-memory state?

## Decision

Plugins own their persistence. Plugins that need state across reloads
explicitly call `ctx.kv_set("state", bytes)` in `on_stop` and
`ctx.kv_get("state")` in `on_init`. No special kernel mechanism beyond the
existing KV API. PRD 04a templates include a commented-out example showing
the pattern; plugins opt in.

## Alternatives considered

- Kernel-mediated checkpoint hooks (`on_checkpoint` / `on_restore`): forces
  every plugin to implement lifecycle methods it doesn't care about.
- Live migration (snapshot WASM linear memory): brittle, requires identical
  memory layouts between old and new modules, almost always broken after
  recompilation.

## Consequences

- Zero new kernel surface beyond KV API already present.
- Crash safety is automatic: state is written before the old instance dies.
- Each plugin reimplements the same serialize/deserialize boilerplate;
  acceptable for a personal tool.
