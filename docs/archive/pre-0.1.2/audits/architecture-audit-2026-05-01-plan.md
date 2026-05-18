# Audit 2026-05-01 — Implementation Plan

**Source:** `docs/architecture/audit-2026-05-01.md`
**Branch:** `claude/audit-nexus-architecture-ugQM2`
**Scope:** Five action items across IPC strictness, WASM sandbox testing,
versioning convention, capability documentation, and test fixtures.

This plan addresses the P0/P1/P2 items from the audit. The P0 work is the
critical path — it closes the only WARN dimension. P1 work is defense in
depth. P2 work is hygiene.

## Sequencing at a glance

```
P0-1 (deny_unknown_fields) ──┬─► P0-2 (schema-emit test asserts the rule)
                             │
                             └─► [unblocks] P1-2 (versioning ADR + scaffolding)
                                            │
                                            └─► [enabled by] P0-1's strict mode

P1-1 (WASM denial test)        — independent, parallelizable
P2-1 (consent flow comment)    — independent, trivial
P2-2 (fixture helper)          — independent, refactor only
```

Suggested calendar: P0 in week 1 (lands together), P1 + P2 in week 2.

## Discovered scope adjustment

While planning, surfaced an additional gap: only `nexus-ai` and
`nexus-storage` currently emit JSON schemas to
`crates/nexus-bootstrap/schemas/ipc/`. The other subsystems
(`nexus-editor`, `nexus-database`, `nexus-agent`, `nexus-workflow`,
`nexus-skills`, `nexus-theme`, `nexus-comments`, `nexus-git`,
`nexus-terminal`, `nexus-mcp`, `nexus-linkpreview`) define IPC types but
aren't wired into the generator. The schema-emit test therefore can only
police the two crates that opt in. This reframes the plan:

- **Track A (the original P0)** — add `deny_unknown_fields` to
  every IPC arg/reply struct workspace-wide. This is purely a Rust-side
  hardening; it doesn't depend on schema generation.
- **Track B (added scope)** — bring the other subsystems into the schema
  generator so the schema-emit test polices them too. This is a separate,
  longer effort that follows after Track A.

The plan below treats Track A as P0 and Track B as a new P1 (P1-3).

---

## P0-1 — Add `deny_unknown_fields` to all IPC arg/reply structs

**Goal:** Eliminate silent acceptance of unknown fields on IPC payloads.
After this lands, a misspelled or stale field name from any caller (Rust,
TS shell, WASM plugin, MCP client) fails deserialization at the boundary
with a clear error instead of round-tripping as a no-op.

**Files in scope:**

Already centralized in an `ipc.rs` module (audit by `ls crates/*/src/ipc.rs`):

- `crates/nexus-ai/src/ipc.rs`
- `crates/nexus-storage/src/ipc.rs`

Other crates with IPC types not yet centralized — locate via
`grep -rn 'ts_rs::TS\|JsonSchema' crates/nexus-{editor,database,agent,workflow,skills,theme,comments,git,terminal,mcp,linkpreview,kv}/src/`:

- `nexus-editor`, `nexus-database`, `nexus-agent`, `nexus-workflow`,
  `nexus-skills`, `nexus-theme`, `nexus-comments`, `nexus-git`,
  `nexus-terminal`, `nexus-mcp`, `nexus-linkpreview`

**Implementation:**

1. Audit step (one shell command):
   ```
   grep -rn '#\[derive(.*Deserialize.*)\]' crates/ \
     | grep -v 'tests/\|src/error\.rs\|kernel/src/config\.rs'
   ```
   Filter manually to types that flow through `ipc_call`. Build a
   checklist.
2. For each struct, add `#[serde(deny_unknown_fields)]` between the
   derive macro and the struct declaration. Example:
   ```rust
   #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
   #[serde(deny_unknown_fields)]
   #[ts(export)]
   pub struct StorageReadFileArgs {
       pub path: String,
       #[serde(default)]
       pub follow_symlinks: Option<bool>,
   }
   ```
   Note: any optional field must already have `#[serde(default)]` and/or
   be `Option<T>`; `deny_unknown_fields` only catches *unknown* fields,
   not missing ones.
3. Internal helper enums tagged with `#[serde(tag = "kind")]` cannot use
   `deny_unknown_fields` directly on the enum (serde limitation). For
   those, add the attribute to each variant's struct payload instead.
   Inspect output of `cargo build` for compile errors and adjust.
4. Run `cargo test --workspace`. Triage failures:
   - Tests that built JSON inline with stale or misspelled field names
     will now fail correctly — fix the test.
   - Tests that intentionally exercise the "extra field" path (none
     expected, but verify) need `#[serde(rename = "_")]` or a documented
     escape hatch.
5. Run `scripts/check_ipc_drift.sh`. Verify regenerated JSON schemas
   gain `"additionalProperties": false`. Commit the regenerated
   bindings alongside the source change to keep the gate green.

**Validation:**

- `cargo test --workspace` green.
- `scripts/check_ipc_drift.sh` exits 0 with no diff after the
  regeneration commit.
- Manually craft a failing payload (extra field) against one handler
  via an integration test in
  `crates/nexus-bootstrap/tests/ipc_strictness.rs` to lock in the
  behavior. One assertion is enough.

**Risk & rollback:**

- Risk: a community plugin shipping a JSON payload with extra fields
  that previously round-tripped will now error. Mitigation: the
  community-plugin surface is small today (per CLAUDE.md, the shell
  starts empty), and any such payload was a latent bug. Document the
  change in `DEPRECATED.md`.
- Rollback: revert the source-side commits; the regenerated schemas
  follow automatically on the next drift check.

**Effort:** ~1 day of focused work for the well-known ipc.rs files,
plus ~1 day of triage across the remaining 11 crates depending on how
their IPC types are organized today.

---

## P0-2 — Schema-emit test asserts `additionalProperties: false`

**Goal:** Lock in P0-1 with a compiled test so the rule cannot regress.
Without this gate, a future struct could be added without
`deny_unknown_fields` and slip past code review.

**Files in scope:**

- `crates/nexus-bootstrap/tests/ipc_schema_emit.rs` (extend
  existing test, do not create a new file).

**Implementation:**

1. Walk `crates/nexus-bootstrap/schemas/ipc/*.json`.
2. For each schema, parse with `serde_json::from_str::<Value>`.
3. Recurse object types (top-level + any nested `definitions` /
   `$defs` / `properties.<x>.type == object`). For each, assert
   `additionalProperties == false`.
4. Collect violations into a single panic message listing every
   offending schema + path, so a single test run surfaces all gaps.

**Sketch:**
```rust
fn assert_strict_objects(value: &Value, path: &str, violations: &mut Vec<String>) {
    if value.get("type").and_then(Value::as_str) == Some("object") {
        match value.get("additionalProperties") {
            Some(Value::Bool(false)) => {}
            _ => violations.push(format!("{path}: additionalProperties is not false")),
        }
    }
    // recurse into properties, definitions, $defs, items, anyOf, oneOf...
}
```

**Validation:**

- Test fails on `main` *before* P0-1 commits land (or with a synthetic
  schema lacking `additionalProperties: false`).
- Test passes after P0-1 lands.

**Effort:** ~2 hours.

---

## P1-1 — WASM capability-denial integration test

**Goal:** Lock in the host_fns capability-gating behavior end-to-end.
Today the gate is verified by hand reading
`crates/nexus-plugins/src/host_fns.rs:31-39`; an integration test
prevents accidental regression (e.g., a future refactor that forgets
to call `deny_capability` in a new host function).

**Files in scope:**

- New `crates/nexus-plugins/tests/wasm_capability_denial.rs`.
- New `crates/nexus-plugins/tests/fixtures/denial_probe.wat` (text
  format, not binary — see "fixture choice" below).

**Fixture choice:**

WebAssembly Text Format (`.wat`) compiled at test time via the `wat`
crate is preferable to a vendored `.wasm` binary:
- No `wasm32-unknown-unknown` toolchain dependency in CI.
- Diff-friendly in code review.
- Tiny footprint (~1KB text vs ~10KB binary).

The fixture imports `host::read_file` and exports a `probe()` function
that calls it with a fixed path and returns the host's status code.

**Implementation:**

1. Add `wat = "1"` as a `[dev-dependencies]` to
   `crates/nexus-plugins/Cargo.toml`.
2. Write `denial_probe.wat`:
   ```wat
   (module
     (import "host" "read_file"
       (func $read_file (param i32 i32) (result i32)))
     (memory (export "memory") 1)
     (data (i32.const 0) "test.md")
     (func (export "probe") (result i32)
       (call $read_file (i32.const 0) (i32.const 7))))
   ```
3. In the test:
   - Build a `WasmSandbox` with `CapabilitySet::empty()`.
   - Compile the fixture via `wat::parse_file` and instantiate.
   - Call `probe`. Assert returned i32 is `HOST_CAPABILITY_DENIED`
     (constant from `host_fns.rs` — re-export for test access if
     not already public).
   - Use the in-memory audit-log capture
     (`crates/nexus-kernel/src/audit.rs:75` test helper) to assert
     a `capability.denied` event was emitted with
     `cap = "fs.read"`.
4. Add a *positive* counterpart in the same file: same fixture, but
   `CapabilitySet` containing `Capability::FsRead`, and assert
   `read_file` returns success (mock the FS read or point at a
   tempfile).

**Validation:**

- Test passes on `main`.
- Mutating `host_fns.rs` to skip the `deny_capability` call (locally,
  do not commit) makes the test fail with a clear message.

**Risk:** none — additive test only.

**Effort:** ~half a day (most of the time is the .wat fixture and
verifying the audit-log capture works under integration tests).

---

## P1-2 — IPC schema versioning convention

**Goal:** Establish the rule for evolving IPC handler shapes before
the first community-plugin release ships and locks contract evolution
into informal back-compat. After P0-1's strict deserialization, the
need for an explicit migration story sharpens — adding a field to a
`*Args` struct now breaks every old caller, not just silently changes
behavior.

**Decision (recommended):** version in the **command name**, not in the
payload. Example:

| Today | After |
|-------|-------|
| `storage.read_file` | `storage.read_file.v1` |
| `ai.ask` | `ai.ask.v1` |
| (new shape) | `storage.read_file.v2` |

Rationale:
- Dispatcher already routes on command string; versioning at that layer
  keeps the typed-args structs single-purpose (one struct per version,
  no internal `match version` branches).
- Migration is explicit: when v2 ships, v1 stays in the dispatch table
  with an alias and a deprecation timer; v3 retires v1.
- Tools (drift check, schema-emit test) need no changes — each version
  gets its own `*Args` type and its own schema file.

**Alternative considered:** `_schema_version: u8` field inside each
`*Args`. Rejected — complicates the struct (every handler must branch
on version) and conflicts with `deny_unknown_fields` (can't add the
field retroactively without breaking old callers).

**Files in scope:**

- New `docs/adr/0021-ipc-handler-versioning.md`.
- `crates/nexus-kernel/src/ipc.rs` — small change to support handler
  aliasing (one command string maps to multiple aliases).
- New `crates/nexus-bootstrap/tests/ipc_versioning.rs` — asserts that
  for any handler `cmd.vN` with `N > 1`, either `cmd.v(N-1)` is also
  registered (deprecation window) or there's a documented removal
  marker.
- One representative handler retrofit (e.g., `storage.read_file`):
  register both `storage.read_file` (legacy) and
  `storage.read_file.v1` as aliases of the same handler.

**Implementation:**

1. Write the ADR. Cover: convention, aliasing semantics, deprecation
   window (suggest "two minor versions"), removal procedure, examples.
2. Extend the IPC dispatcher with `register_handler_alias(canonical,
   alias)` — internally just a second entry in the dispatch table
   pointing at the same handler instance.
3. Update one subsystem (`nexus-storage` is a good candidate) to
   register every handler under both bare and `.v1` names. This proves
   the aliasing works without a behavioral change.
4. Add the versioning test.

**Validation:**

- All existing IPC integration tests still pass — alias is transparent.
- Calling `storage.read_file.v1` from
  `crates/nexus-bootstrap/tests/forge_ipc.rs` (or a new test) returns
  the same result as `storage.read_file`.

**Effort:** ~1 day for ADR + scaffolding + retrofit of one subsystem;
the bulk-rename of every existing handler can follow as a separate PR.

---

## P1-3 — Bring remaining subsystems into the schema generator

**Goal:** Make P0-2's gate police the whole workspace, not just
`nexus-ai` + `nexus-storage`. Today, even with `deny_unknown_fields`
on every struct, the schema-emit test only validates two crates
because the other subsystems aren't wired into the generator.

**Files in scope:**

- The schema-emit machinery — likely
  `crates/nexus-bootstrap/build.rs` or
  `crates/nexus-bootstrap/tests/ipc_schema_emit.rs` (whichever drives
  schemars). Locate by reading
  `scripts/check_ipc_drift.sh` first.
- `crates/nexus-{editor,database,agent,workflow,skills,theme,comments,git,terminal,mcp,linkpreview}/src/` — IPC type modules.

**Implementation (per crate):**

1. Add `schemars` and `ts-rs` to `[dependencies]` (gated on a `schema`
   feature if needed to keep release builds lean).
2. Add `#[derive(JsonSchema, TS)]` and `#[ts(export)]` to every
   IPC arg/reply struct.
3. Wire the crate into the schema-emit driver so its types land in
   `crates/nexus-bootstrap/schemas/ipc/com_nexus_<crate>__*.json`
   and `packages/nexus-extension-api/src/generated/ipc/<Crate>*.ts`.
4. Run `scripts/check_ipc_drift.sh`; commit generated outputs.

**Validation:**

- `scripts/check_ipc_drift.sh` exits 0.
- `crates/nexus-bootstrap/schemas/ipc/` gains files for every
  subsystem.
- P0-2's schema-emit test now polices the whole surface.

**Risk:** Each crate's IPC type module may be organized differently
(some may have args inline in the handler module rather than a
dedicated `ipc.rs`). Refactoring those into a shared module is part of
the work.

**Effort:** ~half a day per crate × 11 crates = ~6 days. Can be split
across multiple PRs (one per subsystem).

---

## P2-1 — Document Tauri consent flow prerequisite

**Goal:** Make the trust boundary on `set_plugin_granted_capabilities`
explicit in code. Today the renderer-side consent UI is the only thing
preventing arbitrary capability grants from any frontend code path
that reaches the Tauri command — that prerequisite is implicit.

**Files in scope:**

- `shell/src-tauri/src/lib.rs` — add a doc comment immediately above
  the `#[tauri::command] async fn set_plugin_granted_capabilities`
  signature.

**Implementation:**

Doc comment text (one-line WHY, not WHAT):
```rust
/// SECURITY: this command mutates the persisted capability grant.
/// The renderer must obtain explicit user consent via the consent UI
/// before invoking it. The host validates the capability strings against
/// `Capability::from_str` but performs no additional gate — the consent
/// flow is the trust boundary.
```

**Validation:** review only.

**Effort:** 5 minutes.

---

## P2-2 — Shared "minimal forge" test fixture helper

**Goal:** Reduce setup boilerplate across the eight `*_ipc.rs`
integration tests in `crates/nexus-bootstrap/tests/`. Today each test
hand-rolls `tempfile::tempdir()`, calls `build_cli_runtime`, and seeds
markdown by hand — duplicate code that drifts.

**Files in scope:**

- New `crates/nexus-bootstrap/tests/common/mod.rs` — declared from
  each test file via `#[path = "common/mod.rs"] mod common;`. (This is
  the standard Rust-integration-test pattern for shared modules.)
- One pilot refactor: e.g., `crates/nexus-bootstrap/tests/forge_ipc.rs`
  switches to use the helper. Other tests follow in subsequent PRs.

**API sketch:**
```rust
pub struct MinimalForge {
    pub tempdir: TempDir,
    pub runtime: Runtime,
}

impl MinimalForge {
    pub async fn new() -> Self { ... }
    pub async fn with_markdown(files: &[(&str, &str)]) -> Self { ... }
    pub async fn ipc_call<R: DeserializeOwned>(
        &self, plugin_id: &str, command: &str, args: Value,
    ) -> Result<R> { ... }
}
```

**Validation:**

- Pilot test still passes after refactor.
- Diff shows ~30 fewer lines in the pilot file.

**Risk:** none — refactor only.

**Effort:** ~half a day for helper + pilot; remaining seven tests
follow over time.

---

## Cross-cutting considerations

**Drift gate dependency:** Every change that touches an IPC type
must run `scripts/check_ipc_drift.sh` and commit the regenerated
bindings. The plan above respects this — each item lists drift-check
as part of its validation.

**Backwards compatibility:** P0-1 is the only item with breakage risk.
Mitigation strategy:
- Land P0-1 + P0-2 together as one logical change in one PR.
- Regenerate schemas in the same commit so the drift check stays
  green.
- Add an entry to `DEPRECATED.md` explaining that unknown-field
  payloads are now rejected.

**Test infrastructure:** P1-1 introduces `wat` as a dev-dependency.
This is the only new toolchain dependency in the plan; it's pure-Rust
with no system-level installer requirements.

**ADR sequencing:** P1-2's ADR (0021) should land before any new
handler is added to the dispatcher in v2 form. The ADR doesn't block
P0 work; do P0 first, then ADR, then aliasing rollout.

**Out of scope (deliberately):**

- Replacing the `Value → Value` dispatcher signature with a typed
  generic adapter. The audit's Dim 2 finding suggested this as a
  longer-term option; it's not in this plan because (a) it touches
  every plugin author's surface API, (b) the `deny_unknown_fields`
  fix addresses the same underlying symptom (silent contract drift)
  with much less churn. Revisit if community-plugin authoring
  becomes painful enough to justify it.
- Per-handler authorization beyond capabilities (e.g., user-level
  ACLs, per-document gating). Not flagged in the audit; not in scope
  here.

## Tracking checklist

```
[ ] P0-1   deny_unknown_fields applied workspace-wide + drift refresh
[ ] P0-2   ipc_schema_emit.rs asserts additionalProperties: false
[ ] P1-1   wasm_capability_denial.rs integration test added
[ ] P1-2   ADR 0021 + handler aliasing scaffold + nexus-storage retrofit
[ ] P1-3   Remaining 11 subsystems wired into schema generator (split PRs)
[ ] P2-1   set_plugin_granted_capabilities security doc comment
[ ] P2-2   Shared test fixture + one pilot refactor
```

Single-PR-per-item is the recommended cadence except P0-1 + P0-2
which should land together so the gate is enforced from day one.
