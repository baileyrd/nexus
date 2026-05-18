# IPC Schemas — Shared Command Taxonomy

**Status:** Active. 131 JSON schemas + 166 TS types committed (counts from the generated directories; authoritative listing is the directories themselves, not this header).
**Authoritative listing:** the generated directories, not this doc.

- TypeScript types: `packages/nexus-extension-api/src/generated/ipc/`
- JSON schemas: `crates/nexus-bootstrap/schemas/ipc/`

This file describes the *policy* (what the generator does, when to run it,
what fails CI). It does **not** enumerate handlers — that listing lives in
the generated trees above and would drift if duplicated here.

## Why

Before WI-36, each frontend described the same IPC call slightly differently:
the CLI passed untyped `serde_json::Value`; the MCP server hand-authored
`#[derive(JsonSchema)]` request types per tool; shell plugins hand-decoded
returns behind `api.kernel.invoke<unknown>(…)`; kernel handlers decoded args
inline from `serde_json::Value`. Drift between the four surfaces was silent
until runtime.

WI-36 collapsed each handler onto one Rust type generated into both
TypeScript and JSON Schema, so every frontend reads the same contract.

## How the generator works

Each handler's arg + return types carry two feature-gated derives:

```rust
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS, schemars::JsonSchema))]
#[cfg_attr(feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"))]
pub struct StorageSearchArgs { … }
```

Two outputs from one source:

- **TypeScript** — `ts-rs` writes `<TypeName>.ts` into
  `packages/nexus-extension-api/src/generated/ipc/` when its auto-emitted
  `export_bindings_*` test runs.
- **JSON Schema** — `schemars` generates via
  `crates/nexus-bootstrap/tests/ipc_schema_emit.rs`, which writes each
  schema to
  `crates/nexus-bootstrap/schemas/ipc/<plugin>_<command>_<role>.json`.

Both trees are committed to the repo. CI regenerates and diffs — the
drift-check fails on any change.

## Regenerating

```bash
./scripts/check_ipc_drift.sh
```

Internally this runs the per-crate `ts-export` test suites and the
`ipc_schema_emit` test in `nexus-bootstrap`. Default
`cargo build --workspace` and `cargo test --workspace` do **not** pull
`ts-rs` or `schemars` — both deps are optional behind the `ts-export`
feature on their respective crates.

## CI drift-check

`.github/workflows/ipc-drift-check.yml` runs `scripts/check_ipc_drift.sh`
on every PR. The script ends with `git diff --exit-code` over:

- `packages/nexus-extension-api/src/generated/`
- `crates/nexus-bootstrap/schemas/ipc/`

If you edit an IPC-typed struct without running the regen, CI fails with
a pointer back to this doc.

## Adding a handler

Recipe per handler:

1. In the handler's owning crate (e.g. `nexus-editor`), add `ts-rs` +
   `schemars` as **optional** deps under a `ts-export` feature. Mirror
   `crates/nexus-storage/Cargo.toml`.
2. Add an `ipc` module with arg + return mirror types carrying the
   feature-gated derives. Mirror types stay faithful to whatever the
   dispatch decodes — the JSON on the wire does not change.
3. Append the types to `crates/nexus-bootstrap/tests/ipc_schema_emit.rs`
   so the JSON schema is emitted.
4. Append an entry to
   `packages/nexus-extension-api/src/generated/ipc/index.ts` so the
   package barrel re-exports the new TypeScript types.
5. Extend `scripts/check_ipc_drift.sh` if the new crate's test suite isn't
   already covered (it iterates by crate).
6. Run `./scripts/check_ipc_drift.sh`, commit the regenerated files.

No changes required to dispatch code: mirror types are separate from any
inline decode path, so adding one is always safe.

## Known caveats

- **`ts-rs` fails on unusual fields** (`serde_json::Value`, trait objects,
  lifetimes). Hand-authored mirror is the standard fallback — that's
  what the original WI-36 pilot did and the pattern persists.
- **Schema collisions with MCP.** `nexus-mcp` keeps its own
  `schemars::JsonSchema` derives on hand-authored request DTOs (the
  `#[tool(...)]` macro path). Separate crates, separate derives. Unify
  only if a real collision surfaces.
- **`nexus-mcp` and the IPC trees are independent.** The 15 `nexus_*` MCP
  tools are not the same surface as the IPC handlers tracked here. MCP
  tools route *to* IPC handlers internally, but the MCP tool schema set
  is its own contract for external AI clients.

## Where to look

| You want to... | Look at |
|---|---|
| List every IPC type | `packages/nexus-extension-api/src/generated/ipc/index.ts` |
| List every IPC schema | `crates/nexus-bootstrap/schemas/ipc/` |
| See a handler's args/return shape | The same crate's `src/ipc.rs` (or `src/ipc/*.rs`) |
| Understand why this exists | This doc |
| Run the drift check | `./scripts/check_ipc_drift.sh` |
