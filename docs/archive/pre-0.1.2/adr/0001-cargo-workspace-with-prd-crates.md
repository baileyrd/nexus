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

## Addendum 2026-05-12 — workspace grew to 28 crates

The "Decision" section above lists six crates as the original cut. The
workspace now ships **28**, all still under the one-crate-per-subsystem
spirit. The decision holds — boundaries are still enforced at the
compiler level (`crates/nexus-bootstrap/tests/dep_invariants.rs`); only
the inventory drifted. Authoritative sources of truth are the workspace
`Cargo.toml` `[workspace] members =` array and the Level-2 container
listing in [`../architecture/C4.md`](../architecture/C4.md). Categorised
view as of 2026-05-12:

- **Leaf primitives.** `nexus-types`, `nexus-plugin-api` — no nexus
  deps; everything else may depend on these.
- **Kernel + lifecycle.** `nexus-kernel`, `nexus-plugins`,
  `nexus-bootstrap`.
- **Security.** `nexus-security`.
- **Storage + index plane.** `nexus-storage`, `nexus-kv`,
  `nexus-formats`, `nexus-database`.
- **Editor / content surfaces.** `nexus-editor`, `nexus-templates`,
  `nexus-comments`, `nexus-crdt`, `nexus-skills`, `nexus-linkpreview`,
  `nexus-theme`.
- **AI + agent stack.** `nexus-ai`, `nexus-agent`, `nexus-workflow`.
- **External-system bridges.** `nexus-terminal`, `nexus-git`,
  `nexus-lsp`, `nexus-mcp`.
- **Frontend binaries.** `nexus-cli`, `nexus-tui`.
- **Quality + diagnostics.** `nexus-panic-log`, `nexus-fuzz`.

Crates added since the original decision (`nexus-bootstrap`,
`nexus-plugin-api`, `nexus-kv`, `nexus-formats`, `nexus-database`,
`nexus-editor`, `nexus-templates`, `nexus-comments`, `nexus-crdt`,
`nexus-skills`, `nexus-linkpreview`, `nexus-theme`, `nexus-ai`,
`nexus-agent`, `nexus-workflow`, `nexus-terminal`, `nexus-git`,
`nexus-lsp`, `nexus-mcp`, `nexus-tui`, `nexus-panic-log`, `nexus-fuzz`)
were each introduced as their subsystem materialised — see the
relevant PRD or ADR for the per-crate rationale, and the git log on
the top-level `Cargo.toml` for when each `members =` entry landed.
The original-decision sextet (`nexus-types`, `nexus-kernel`,
`nexus-security`, `nexus-storage`, `nexus-plugins`, `nexus-cli`) ships
unchanged.

[ADR 0004] cross-references this addendum from its own appendix.

[ADR 0004]: 0004-crate-boundaries-and-ownership.md
