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
