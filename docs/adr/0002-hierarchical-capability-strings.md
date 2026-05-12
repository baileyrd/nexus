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

## Capability inventory (as of 2026-04-16)

| String | Variant | Risk |
|--------|---------|------|
| `fs.read` | `FsRead` | Low |
| `fs.write` | `FsWrite` | Low |
| `fs.read.external` | `FsReadExternal` | **High** |
| `fs.write.external` | `FsWriteExternal` | **High** |
| `net.http` | `NetHttp` | **High** |
| `net.http.localhost` | `NetHttpLocalhost` | Medium |
| `process.spawn` | `ProcessSpawn` | **High** |
| `kv.read` | `KvRead` | Low |
| `kv.write` | `KvWrite` | Low |
| `ipc.call` | `IpcCall` | **High** |
| `db.query` | `DbQuery` | Low |
| `db.write` | `DbWrite` | Low |
| `events.publish` | `EventsPublish` | Medium — a plugin can publish arbitrary events to the kernel bus, visible to all subscribers |
| `ui.notify` | `UiNotify` | Low — shows toasts; no destructive effect, but spam potential warrants a gate |

`host::get_settings` (own-plugin settings reads) is intentionally ungated: a plugin reading its own validated settings is first-party and carries no cross-plugin risk.

## Addendum 2026-05-12 — capability inventory has grown

The inventory above is the table as of 2026-04-16 (14 entries). [ADR
0022] added eight `ai.*` capabilities (Phase 1: six; Phase 2: two), so
the canonical surface is now 22 entries. Per the immutable-body
convention, the original table is preserved; the authoritative inventory
lives at [ADR 0022 §Inventory note (2026-05-12)](0022-per-handler-ai-capabilities.md#inventory-note-2026-05-12),
mirrored from `crates/nexus-plugin-api/src/capability.rs::Capability::ALL`.

[ADR 0022]: 0022-per-handler-ai-capabilities.md
