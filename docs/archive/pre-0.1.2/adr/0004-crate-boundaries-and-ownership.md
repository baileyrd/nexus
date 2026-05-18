# ADR 0004: Crate Boundaries and Ownership Map

**Date:** 2026-04-11
**Status:** Accepted

## Context

With per-PRD crates, we need explicit rules for who owns what. The wrong
split causes circular deps, duplicated logic, or capability checks that
live in two places and disagree.

## Decision

- `nexus-kernel`: `Capability` enum, `CapabilitySet`, `PluginLifecycle` trait,
  `PluginContext` trait + runtime impl (with capability enforcement built in),
  `EventBus`, `NexusEvent` enum, KV store API, `Plugin` trait.
- `nexus-security`: threat model, keyring, capability risk-level metadata,
  slimmed audit logger. Depends only on `nexus-kernel`.
- `nexus-storage`: file watcher, SQLite index, Tantivy search. Depends on
  `nexus-kernel` + `nexus-security`.
- `nexus-plugins`: TOML manifest parser, WASM sandbox, plugin loader, hot-
  reload. Depends on `nexus-kernel` + `nexus-security`.
- `nexus-cli`: `nexus` binary. Depends on every other crate.

Strict DAG with `nexus-types` as leaf-of-leaf. No cycles.

## Alternatives considered

- Manifest parser in kernel: rejected — kernel shouldn't know about TOML.
- WASM sandbox in security: rejected — tightly coupled to wasmtime API,
  doesn't fit a "policy" crate.
- Separate `nexus-capabilities` crate: overkill for one enum + risk table.

## Consequences

- Capability checks live in exactly one place (kernel context impl).
- Plugins physically cannot bypass capability checks because they only
  hold `&dyn PluginContext`.
- `nexus-security` is slim — policy + side effects, not a request gate.

## Addendum 2026-05-12 — additional crate boundaries

The "Decision" section names five crates; the workspace now ships **28**.
The same DAG-with-`nexus-types`-at-the-leaf rule still holds — boundaries
are enforced at compile time by `crates/nexus-bootstrap/tests/dep_invariants.rs`.
The full categorised inventory and the rationale per added crate live
in [ADR 0001 §Addendum 2026-05-12](0001-cargo-workspace-with-prd-crates.md#addendum-2026-05-12--workspace-grew-to-28-crates);
this addendum exists only to redirect readers who land here first.

Boundary highlights for the post-2026-05-12 crates not in the original
table:

- `nexus-bootstrap` is the only crate allowed to depend on every
  service plugin — it's the static-wiring orchestrator (per ADR 0011).
- `nexus-plugin-api` is a leaf alongside `nexus-types`; both
  community plugins and core plugins consume it.
- Service crates (`nexus-ai`, `nexus-agent`, `nexus-editor`, …) all
  satisfy "subsystem crates depend on the kernel; the kernel never
  depends on a subsystem" (per `CLAUDE.md`'s microkernel-isolation
  invariant).
- Frontends (`nexus-cli`, `nexus-tui`) reach storage / AI / editor
  only via `context.ipc_call(...)` per ADR 0011's IPC-over-direct-calls
  rule.

[`crates/nexus-bootstrap/tests/dep_invariants.rs`]: ../../crates/nexus-bootstrap/tests/dep_invariants.rs
