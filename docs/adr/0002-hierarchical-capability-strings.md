# ADR 0002: Hierarchical Dot-Namespaced Capability Strings

**Date:** 2026-04-11
**Status:** Accepted

## Context

PRD 01 defined capabilities as a Rust enum; plugin manifests reference them
as strings. We need a canonical mapping that avoids the typo-bypass bug class
and scales cleanly to M2–M5 additions.

## Decision

Capabilities are hierarchical dot-namespaced strings (`"fs.read"`,
`"net.http.localhost"`). The `Capability` enum is the single source of truth,
with `as_str()` / `from_str()` providing bidirectional conversion. Manifest
parsing validates strings against the enum at parse time. No wildcards.

## Alternatives considered

- Flat strings (`"FsRead"`, `"Network"`): loses risk-gradient info.
- Enum-as-canonical in manifests: ugly TOML, hostile to non-Rust tooling.
- Two-tier split (`"filesystem"` + `"read"`): more typing, no benefit.

## Consequences

- Adding a capability requires editing one const table.
- Typos in manifests fail at parse time, pointing to the offending line.
- M2–M5 additions slot cleanly under existing namespaces (`ai.*`, `db.*`).
- Risk-level metadata lives separately in `nexus-security`, not the enum.
