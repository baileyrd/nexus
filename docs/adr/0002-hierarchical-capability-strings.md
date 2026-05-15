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

## Addendum 2026-05-15 — BL-138 default-deny registration

The historical "if a handler is not listed in `add_cap_requirement(…)`,
its only caller requirement is `ipc.call`" rule was a silent-omission
hazard: a new handler that *should* have required an extra cap shipped
unrestricted unless the author remembered to extend the matrix. BL-138
replaces the hand-maintained call wall in `nexus-bootstrap/src/lib.rs`
with a TOML matrix at `crates/nexus-bootstrap/cap_matrix.toml`, applied
at bootstrap by [`nexus_bootstrap::cap_matrix::apply`].

Every in-tree IPC handler ships as one row in the matrix, declared as
either:

- `caps = [...]` — caller must hold each listed capability (still on
  top of the unconditional `ipc.call` check). Equivalent to the
  pre-BL-138 `add_cap_requirement(…)` entry.
- `unrestricted = "<why>"` — handler is intentionally available to any
  caller with `ipc.call`; the string is the one-line rationale
  (read-only probe, version negotiation, etc.). Replaces the
  pre-BL-138 implicit default.

A `cap_matrix_complete` integration test under
`crates/nexus-bootstrap/tests/` walks every live `(plugin, command)`
pair and fails CI if any is missing from the matrix. The completeness
sweep is `#[ignore]`d during BL-138 Phase 1 (which ships the
infrastructure plus the 17 historical entries); the per-service-plugin
follow-ups that classify the remaining ~150+ handlers as
`unrestricted = "<why>"` un-ignore it.

Args-aware capability requirements (ADR 0022 Phase 2's
`stream_chat` / `propose_tool_calls` `tools=…` lookup) cannot live in
TOML, so each closure is registered under a stable name in
`crates/nexus-bootstrap/src/cap_policies.rs` and the matrix row
references it via `policy = "<name>"`.

`register_handler_caps` / `register_handler_unrestricted` on
[`SharedPluginLoader`] are the canonical Rust-side registration API
the matrix loader uses; they should not be called directly from any
new bootstrap code. New entries go in the TOML file.
