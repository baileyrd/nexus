# ADR 0021: IPC Handler Versioning Convention

**Date:** 2026-05-01
**Status:** Accepted
**Audit reference:** [audit-2026-05-01](../audits/architecture-audit-2026-05-01.md) §
Dim 2 (P1-2 follow-up)

## Context

The audit's Dim 2 finding flagged that IPC handler shapes evolve without an
explicit migration story:

- Handlers are addressed by string (`com.nexus.storage::read_file`) and the
  dispatcher does a flat lookup against the manifest's `ipc_commands` table.
- Adding a new field to a `*Args` struct used to round-trip silently.
- After PR #116 landed `#[serde(deny_unknown_fields)]` workspace-wide, the
  same shape change now breaks every old caller hard. The strict gate
  sharpens the need for an explicit versioning convention before
  community plugins ship and informally lock contracts in.

Two reasonable conventions:

- **Option A — version in the command name.** Today's `storage.read_file`
  becomes `storage.read_file.v1`; the next shape ships as
  `storage.read_file.v2`.
- **Option B — version in the payload.** Each `*Args` carries
  `_schema_version: u8` (or similar); the handler branches on it.

## Decision

Adopt **Option A**: version in the command name.

Rationale:

- The dispatcher already routes on a string. Versioning at that layer keeps
  each `*Args` struct single-purpose — no internal `match version` branches.
  This composes cleanly with `#[serde(deny_unknown_fields)]` (P0-1): adding
  a `_schema_version` field would have to be optional with a default, which
  weakens the strict gate.
- Migration is explicit. When `v2` ships, `v1` stays in the dispatch table
  with an alias and a deprecation timer; a later release retires `v1`.
- Tooling needs no changes. Each version gets its own `*Args` type, its
  own JSON schema file under `crates/nexus-bootstrap/schemas/ipc/`, and
  its own ts-rs binding. The schema-emit gate (P0-2) polices each version
  independently.

### Naming

```
<plugin-id>::<command>            # bare alias for the current version
<plugin-id>::<command>.v<N>       # explicit version
```

For the current state of the world, the bare alias points at `v1`. Every
core handler should be reachable under both `<command>` and `<command>.v1`
so callers can opt in to explicit versioning at their own pace.

When `v2` ships:

1. Register `<command>.v2` pointing at the new handler.
2. Repoint the bare alias `<command>` from `v1`'s handler to `v2`'s.
3. Keep `<command>.v1` registered for the deprecation window.
4. Document the change in `DEPRECATED.md`.

Bare-alias semantics: `<command>` always tracks the **current** version,
never a deprecated one. Callers that want shape stability across releases
must pin to an explicit `.v<N>`.

### Deprecation window

Two minor releases minimum between announcement and removal of an old
version. Example timeline if `v2` lands in `0.7`:

| Release | `<command>` resolves to | `.v1` registered | `.v2` registered |
|---------|-------------------------|------------------|------------------|
| `0.6`   | `v1`                    | yes              | no               |
| `0.7`   | `v2`                    | yes (deprecated) | yes              |
| `0.8`   | `v2`                    | yes (deprecated) | yes              |
| `0.9`   | `v2`                    | **no**           | yes              |

Plugins that pinned to `.v1` get one full minor release of warning before
removal.

### Removal

Removing `<command>.v<N>` is a `[[ipc_command]]` entry deletion in
`crates/nexus-bootstrap/src/lib.rs`. No code change beyond the registration
table. The schema-emit test (P0-2) will surface any plugin still trying
to call the removed name with a clear `CommandNotFound` at runtime.

## Mechanism

The dispatch table is a list of `(command_name, handler_id)` pairs.
Aliasing is achieved by registering multiple names against the same
`handler_id` — no dispatcher code changes are needed.

Bootstrap exposes a helper:

```rust
fn with_v1_aliases(ipc_commands: &[(&str, u32)]) -> Vec<(String, u32)>
```

For each `(name, handler_id)` it emits both `(name, handler_id)` and
`(name.v1, handler_id)`, doubling the registration table. Subsystems
opt in by wrapping their `core_manifest_with_ipc(...)` argument list in
`with_v1_aliases(&[...])`.

When a subsystem ships `v2`, it switches from the helper to a hand-written
list that carries all three names (bare/`.v1`/`.v2`) explicitly so the
deprecation timeline is visible at the registration site.

## Pilot

`com.nexus.storage` opts in as the pilot in this ADR's implementing PR.
All ~50 storage handlers get `.v1` aliases. Existing IPC integration tests
under `crates/nexus-bootstrap/tests/` continue to pass — the alias is
transparent to callers using the bare name.

## Other subsystems

Roll out to the remaining subsystems opportunistically as part of the
P1-3 work (issue #113 — wiring the rest of the workspace into the schema
generator). No need to retrofit every subsystem in one PR.

## Forward-deprecation guard

A test in `crates/nexus-bootstrap/tests/ipc_versioning.rs` walks every
registered command and asserts: for any `cmd.v<N>` with `N > 1`, either
`cmd.v(N-1)` is also registered (deprecation window in effect) or there
is a documented removal marker. With no `v2` handlers today the test
passes vacuously and acts as a forward guard for the convention.

## Out of scope

- **Per-version capability requirements.** A future `cmd.v2` that needs
  a stronger capability than `cmd.v1` is encouraged to declare that via
  `add_cap_requirement` (issue #77 mechanism) rather than at the schema
  level. Not addressed here.
- **Renames.** This ADR covers shape evolution of an existing command,
  not renaming. To rename `cmd_a` to `cmd_b`, register `cmd_b` and
  `cmd_b.v1` per this convention, and treat `cmd_a` as a separate
  alias to `cmd_b`'s handler with its own deprecation timer.
- **Breaking the bare alias.** Operators who wish to enforce explicit
  versioning across the whole workspace can stop registering bare aliases
  in their fork. The convention does not mandate that every subsystem
  ship a bare alias, only that if one is shipped it tracks the current
  version.

## Consequences

- Every IPC handler in opt-in subsystems has two registration entries.
  Storage's table doubles from ~50 to ~100 entries. Acceptable; the
  cost is one helper call.
- A `cmd.v<N>` removal is a one-line PR. The deprecation-window test
  prevents accidental removal during the announcement window.
- Caller code that references the bare name silently follows the current
  version. This is the documented behavior; callers wanting stability
  across releases must pin.
