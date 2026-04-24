# IPC Schemas — Shared Command Taxonomy (Phase 4 WI-36)

**Status:** Pilot (5 handlers). v1.1 migrates the remaining 47+.
**Source of truth:** Rust types in `crates/nexus-storage/src/ipc.rs` and
`crates/nexus-ai/src/ipc.rs`.

## Why

Before WI-36, each frontend described the same IPC call slightly differently:

- **CLI** (`crates/nexus-cli`) passed untyped `serde_json::Value`.
- **MCP** (`crates/nexus-mcp/src/server.rs`) had hand-authored
  `#[derive(JsonSchema)]` request types per tool.
- **Shell plugins** (`shell/src/plugins/**`) hand-decoded returns behind
  `api.kernel.invoke<unknown>(…)`.
- **Kernel handlers** decoded args inline from `serde_json::Value`.

Drift between the four surfaces was silent until runtime. WI-36 collapses
them onto one Rust type per handler, generated into both TypeScript and
JSON Schema so every frontend reads the same contract.

## The 5 pilot handlers

| Command | Arg type | Return type |
|---|---|---|
| `com.nexus.storage::search`     | `StorageSearchArgs`    | `StorageSearchResult` (+ `StorageSearchHit`) |
| `com.nexus.storage::read_file`  | `StorageReadFileArgs`  | `StorageReadFileResult` |
| `com.nexus.storage::write_file` | `StorageWriteFileArgs` | `StorageWriteFileResult` |
| `com.nexus.storage::list_dir`   | `StorageListDirArgs`   | `StorageListDirResult` (+ `StorageListDirEntry`) |
| `com.nexus.ai::stream_ask`      | `AiStreamAskArgs`      | `AiStreamAskResult` (+ `AiStreamAskMessage`, `AiStreamAskRole`, `AiStreamAskSource`) |

These five were chosen because all three frontends (CLI, MCP, shell) call
at least four of them, and `stream_ask` is the RAG workhorse behind the
chat UI (Phase 2 WI-01).

## How the generator works

Each pilot type carries two feature-gated derives:

```rust
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS, schemars::JsonSchema))]
#[cfg_attr(feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"))]
pub struct StorageSearchArgs { … }
```

Two outputs, one source:

- **TypeScript** — `ts-rs` writes `StorageSearchArgs.ts` into
  `packages/nexus-extension-api/src/generated/ipc/` when its auto-emitted
  `export_bindings_storagesearchargs` test runs.
- **JSON Schema** — `schemars` generates via
  `crates/nexus-bootstrap/tests/ipc_schema_emit.rs`, which writes each
  schema to `crates/nexus-bootstrap/schemas/ipc/<plugin>_<command>_<role>.json`.

Both trees are **committed to the repo** (same convention as Phase 1's
`generated/` TS output). CI regenerates and diffs — the drift-check fails
on any change.

## Regenerating

```bash
# Regenerate everything and diff against HEAD in one shot:
./scripts/check_ipc_drift.sh
```

Internally this runs:

```bash
# Phase 1 WI-20 plugin-api bindings:
cargo test -p nexus-plugin-api --features ts-export

# Phase 4 WI-36 pilot IPC TS bindings:
cargo test -p nexus-storage --features ts-export --tests
cargo test -p nexus-ai      --features ts-export --tests

# Phase 4 WI-36 pilot IPC JSON Schemas:
cargo test -p nexus-bootstrap --test ipc_schema_emit --features ts-export
```

Default `cargo build --workspace` and `cargo test --workspace` do **not**
pull `ts-rs` or `schemars` — both deps are optional behind the
`ts-export` feature on their respective crates.

## CI drift-check

`.github/workflows/ipc-drift-check.yml` runs `scripts/check_ipc_drift.sh`
on every PR touching the pilot surfaces. The script ends with
`git diff --exit-code` over:

- `packages/nexus-extension-api/src/generated/`
- `crates/nexus-bootstrap/schemas/ipc/`

If you edit a pilot type without running the regen, CI fails with a
pointer back to this doc.

## Adding a new handler to the pilot set

v1.1 will migrate the remaining ~47 handlers. The per-handler recipe:

1. In the handler's owning crate (e.g. `nexus-editor`), add `ts-rs` +
   `schemars` as **optional** deps under a new `ts-export` feature.
   Mirror `crates/nexus-storage/Cargo.toml`.
2. Add a new `ipc` module (or extend an existing one) with hand-authored
   arg + return mirror types carrying the feature-gated derives. Mirror
   types stay faithful to whatever the current dispatch decodes — the
   JSON on the wire doesn't change.
3. Append the types to `crates/nexus-bootstrap/tests/ipc_schema_emit.rs`
   so `cargo test -p nexus-bootstrap --test ipc_schema_emit --features
   ts-export` emits their JSON Schema.
4. Append an entry to `packages/nexus-extension-api/src/generated/ipc/index.ts`
   so the package barrel re-exports the new TypeScript types.
5. Extend `scripts/check_ipc_drift.sh` if the new crate's test suite isn't
   already covered (it iterates by crate).
6. Run `./scripts/check_ipc_drift.sh`, commit the regenerated files.

No changes required to dispatch code: the mirror types are separate
from the existing inline decode path, so adding one is always safe. When
a handler is refactored to call `parse_args::<FooArgs>(…)` directly, the
mirror becomes the real type — that's out of scope for the pilot and
tracked as v1.1.

## Risk & mitigation

- **ts-rs fails on unusual fields** (e.g. `serde_json::Value`, trait
  objects, lifetimes). Falling back to a hand-authored mirror is the
  pattern — it's exactly what the pilot does today. The relevant §3.1
  risk row in `PHASE-4-IMPLEMENTATION-PLAN.md` documents the decision.
- **Schema collisions with MCP.** `nexus-mcp` keeps its own
  `schemars::JsonSchema` derives on hand-authored request DTOs.
  Separate crates, separate derives — unify only if a real collision
  surfaces.
- **Scope creep.** The pilot is 5 handlers. Migrating more is v1.1.
