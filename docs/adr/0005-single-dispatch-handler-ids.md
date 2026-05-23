# ADR 0005: Plugin Calling Convention — Single Dispatch with Handler IDs

**Date:** 2026-04-11
**Status:** Accepted

## Context

WASM plugins register CLI subcommands, IPC commands, event subscribers, etc.
We need a wire-level protocol for the kernel to invoke plugin handlers.

## Decision

Each plugin exports exactly one function: `nexus_dispatch(handler_id: u32,
args_ptr: u32, args_len: u32) -> u64`. The manifest assigns stable handler
IDs to each registration. The plugin SDK (PRD 04a templates) generates the
dispatch function from `#[handler(id = N)]` attributes. JSON via `serde_json`
is the wire format; shared types live in `nexus-types`.

Handler ID namespacing: `0x01_xx_xx_xx` = CLI, `0x02_xx_xx_xx` = IPC, etc.

## Alternatives considered

- Named exports per handler: verbose, no runtime handler add, worse for
  hot-reload stability.
- WIT component model: modern but overkill for Rust-only plugin authorship
  in a personal tool. Revisit if cross-language plugins become a goal.

## Consequences

- Plugin SDK has a tiny surface: one function, one macro.
- Handler IDs are stable across hot-reloads even if Rust function names
  change internally.
- Debugging is less friendly (stack traces show `nexus_dispatch` not the
  real handler) but the cost is small.
