# ADR 0001: Cargo Workspace with One Crate Per PRD

**Date:** 2026-04-11
**Status:** Accepted

## Context

Nexus is spec'd as 17 PRDs covering distinct subsystems. We need a physical
project structure that lets AI agents work in parallel on isolated pieces
without merge conflicts, and that enforces inter-PRD contracts at compile time.

## Decision

Single Cargo workspace with one crate per PRD:
- `nexus-types` (leaf — shared primitives)
- `nexus-kernel` (PRD 01)
- `nexus-security` (PRD 02)
- `nexus-storage` (PRD 03)
- `nexus-plugins` (PRD 04 + 04a)
- `nexus-cli` (PRD 05)

Each crate has its own `Cargo.toml` referencing `workspace.dependencies`.

## Alternatives considered

- Single-crate monolith: fails for parallel agent work (every agent touches
  the same `Cargo.toml`); no boundary enforcement.
- Multi-repo: too much overhead for solo+agents (submodule churn, version
  pinning, cross-repo PRs).
- Phase-sized crates: fewer crates, but less clean boundaries and harder
  agent delegation.

## Consequences

- Inter-PRD contracts enforced at the Rust compiler level — `nexus-plugins`
  can't reach into `nexus-kernel` internals, only `pub` interfaces.
- Small cost: ~6 `Cargo.toml` files to maintain instead of 1.
- Cross-crate refactors are slightly more friction than in-crate refactors.
