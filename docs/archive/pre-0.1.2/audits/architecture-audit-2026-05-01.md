# Microkernel Architecture Audit Report

**System:** Nexus Forge
**Archetype:** Nexus Forge (Rust / Tauri multi-crate workspace, WASM-sandboxed plugins, capability SDK)
**Inputs:** Code + Docs (cross-referenced)
**Date:** 2026-05-01
**Branch:** `claude/audit-nexus-architecture-ugQM2`

---

## Executive Summary

Nexus is a **structurally healthy** microkernel: the four invariants stated in
`docs/architecture/invariants.md` are not just aspirational — they are codified
as compiled tests, capability checks at the IPC dispatch site, and a WASM
sandbox with per-plugin `wasmtime::Store` isolation. The kernel surface is
genuinely minimal, dependency inversion is enforced by
`crates/nexus-bootstrap/tests/dep_invariants.rs`, and error/test discipline is
strong (typed `thiserror` enums, zero production `unwrap()` outside
documented poison paths, eight per-subsystem IPC integration tests).

The single **WARN** is on IPC boundary strictness: the dispatcher signature is
`serde_json::Value → Value`, with typed argument structs deserialized
*inside* each handler. No struct uses `#[serde(deny_unknown_fields)]`, so
field-name typos and forward-incompatible additions silently round-trip
instead of erroring at the boundary.

**Top recommendation (P0 hardening, not a structural defect):** add
`#[serde(deny_unknown_fields)]` to all `*Args` / `*Reply` IPC structs and add
a `cargo test` that fails the build on regression.

## Audit Scorecard

| # | Dimension | Verdict | One-line summary |
|---|-----------|---------|------------------|
| 1 | Core / Plugin Boundary | ✅ PASS | Kernel exposes traits + value types only; subsystems link kernel/plugin-api, never each other. |
| 2 | IPC / Message Contracts | ⚠️ WARN | Typed args/replies + ts-rs/schemars generation + drift gate, but dispatcher uses `Value` and no `deny_unknown_fields`. |
| 3 | Capability-Based Security | ✅ PASS | Default-deny, immutable-after-load, dual-gate (`IpcCall` + per-handler), audit-logged on every denial. |
| 4 | Plugin Sandbox (WASM) | ✅ PASS | Per-plugin `wasmtime::Store`, allowlisted host imports, bounded `unsafe`, serialized buffer ABI. |
| 5 | Extensibility & Lifecycle | ✅ PASS | `CorePlugin` trait with 7 lifecycle hooks; manifest-driven; `api_version` major-version gate. |
| 6 | Dependency Inversion | ✅ PASS | Test-enforced forbidden-dep table; KV trait/impl split; no `lazy_static!` / `static mut` in core. |
| 7 | Async / Concurrency | ✅ PASS | Single-runtime `tokio`; no sync mutex held across `.await`; broadcast bus handles lagged subscribers. |
| 8 | Test Coverage & Errors | ✅ PASS | `thiserror` enums with domain-specific variants; 8 per-subsystem IPC tests; negative-path coverage. |

---

## Detailed Findings

### 1. Core / Plugin Boundary Enforcement — ✅ PASS

**Findings:**

- Kernel public surface is intentionally narrow: `crates/nexus-kernel/src/lib.rs:32-51`
  re-exports `PluginContext`, `Kernel`, `CapabilitySet`, `NexusEvent`,
  `IpcDispatcher`, `PluginInfo`, `PluginStatus`, `TrustLevel`. Internal
  trait objects (`KernelPluginContext`, `EventBus`, `KvStore`) are exported
  for *bootstrap* injection, not for direct subsystem use.
- `crates/nexus-kernel/Cargo.toml` has zero dependencies on subsystem crates
  (`nexus-storage`, `nexus-ai`, `nexus-editor`, etc.). Confirmed by the
  forbidden-dep table at `crates/nexus-bootstrap/tests/dep_invariants.rs:17-41`,
  which also forbids `nexus-kernel → rusqlite` and `nexus-kernel → nexus-kv`.
- Subsystems hold only `&dyn PluginContext` (trait object, defined at
  `crates/nexus-kernel/src/context.rs`), so they cannot downcast to bypass
  capability checks in `KernelPluginContext`.
- The 22 Tauri commands in `shell/src-tauri/src/lib.rs` (`#[tauri::command]`
  attributes at lines 62, 160, 213, 329, 350, 386 …; registered via
  `generate_handler!` at `shell/src-tauri/src/lib.rs:503`) are scoped to
  host-intrinsic concerns (kernel lifecycle, plugin scanning, persistence,
  popout windows). No bespoke per-feature command handlers — feature flow
  goes through `kernel_invoke` → `ipc_call`.

**Recommendations:** None. The boundary is real and well-policed.

---

### 2. IPC / Message-Passing Contracts — ⚠️ WARN

**Findings:**

- Per-subsystem typed args/replies exist and use serde:
  `crates/nexus-storage/src/ipc.rs` defines `StorageSearchArgs`,
  `StorageReadFileArgs`, `StorageWriteFileArgs`, `StorageListDirArgs`
  (≈lines 45–262), each with `#[derive(Serialize, Deserialize)]`.
  Equivalent patterns in `nexus-ai`, `nexus-editor`, `nexus-database`.
- Cross-language contract generation is real: `scripts/check_ipc_drift.sh`
  regenerates `packages/nexus-extension-api/src/generated/ipc/*.ts` (via
  ts-rs) and `crates/nexus-bootstrap/schemas/ipc/*.json` (via schemars) and
  fails CI on `git diff` non-empty.
- IPC integration tests exist for each subsystem:
  `crates/nexus-bootstrap/tests/{agent,database,editor,forge,skills,theme,
  workflow,community_to_core}_ipc.rs` and `ipc_schema_emit.rs` — 8+ files
  proving round-trip fidelity through the actual dispatcher.
- Capability gate is layered correctly:
  `crates/nexus-kernel/src/context_impl.rs:301-335` checks
  `Capability::IpcCall` first, then walks `dispatcher.required_caller_caps()`
  per handler. Caller cannot launder caps through a more-privileged callee.

**Gaps (driving the WARN verdict):**

- The `IpcDispatcher` trait (`crates/nexus-kernel/src/ipc.rs`,
  `crates/nexus-plugin-api/src/ipc.rs`) operates on
  `serde_json::Value → Value`. Typed structs are deserialized inside each
  handler body, not at the dispatch surface. The plugin-author API is
  therefore effectively untyped at the boundary — a contract bug shows up
  as a runtime deserialize error in the handler, not a compile error.
- **Zero** uses of `#[serde(deny_unknown_fields)]` anywhere in `crates/`
  (`grep -rn 'deny_unknown_fields' crates/` returns nothing). Misspelled
  field names from a TS plugin will silently deserialize to defaults; an
  IPC v1 → v2 evolution that drops a field will silently accept legacy
  payloads with the now-unknown field still attached.
- No explicit numeric IPC schema version on per-handler types; the
  `api_version` field in plugin manifests gates plugin compatibility but
  individual IPC handler shapes evolve without a version field.

**Recommendations:**

1. Add `#[serde(deny_unknown_fields)]` to all `*Args` and `*Reply` IPC
   structs in `nexus-storage`, `nexus-ai`, `nexus-editor`, `nexus-database`,
   `nexus-agent`, `nexus-workflow`, `nexus-skills`, `nexus-theme`. Land
   alongside a workspace-wide grep test in
   `crates/nexus-bootstrap/tests/ipc_schema_emit.rs` that asserts every
   generated JSON schema has `additionalProperties: false`.
2. Consider a typed-handler wrapper that takes `Args: DeserializeOwned` and
   `Reply: Serialize` in its function signature, with the `Value` boundary
   pushed down into a single generic adapter — this preserves the uniform
   plugin ABI while restoring compile-time typing for handler authors.
3. If IPC contracts will evolve (likely), add an explicit
   `#[serde(default)] pub _schema_version: u8` on each `*Args` type and
   gate behavior on it; alternatively encode the version in the
   `command` string (e.g., `storage.read.v1`).

---

### 3. Capability-Based Security Model — ✅ PASS

**Findings:**

- Closed enum at `crates/nexus-plugin-api/src/capability.rs:13-42` defines
  the 14-variant `Capability` taxonomy with bidirectional `as_str()` /
  `from_str()`. ADR 0002 codifies the hierarchy and risk tiers
  (`fs.read.external`, `net.http`, `process.spawn`, `ipc.call` flagged
  HIGH).
- Enforcement at the dispatcher: `crates/nexus-kernel/src/context_impl.rs:92-102`
  (`require_capability`) and `:301-335` (`ipc_call`) check the granted set
  before any kernel-mediated operation.
- Default-deny, immutable-after-load: `KernelPluginContext` constructs its
  `CapabilitySet` once from the manifest (via
  `crates/nexus-plugins/src/loader.rs`), and the granted set is **not**
  exposed mutably to plugin code — there is no API for a plugin to escalate
  itself.
- Audit logging on every denial path:
  `crates/nexus-kernel/src/audit.rs:25-33` (`log_capability_denied`),
  `:14-22` (`log_capability_granted`), and
  `:56-64` (`log_path_traversal_denied`). All emit structured tracing
  events with `audit = true` for downstream collectors.
- Path-confinement applied uniformly:
  `crates/nexus-kernel/src/context_impl.rs:120-152` rejects path traversal
  attempts and logs them.
- Host-side mutation gate (`set_plugin_granted_capabilities` in
  `shell/src-tauri/src/lib.rs`) validates every capability string against
  `Capability::from_str()` before persisting to `granted_caps.json` —
  arbitrary strings cannot be smuggled into the granted set.

**Recommendations:** Document explicitly in
`shell/src-tauri/src/lib.rs` that the renderer-side consent UI is the
trust boundary for `set_plugin_granted_capabilities`; today the
prerequisite is implicit. (One-line module comment is sufficient.)

---

### 4. Plugin Sandbox Integrity (WASM) — ✅ PASS

**Findings:**

- Per-plugin isolation: `crates/nexus-plugins/src/sandbox.rs:9` uses
  `wasmtime`. `WasmSandbox::new()` constructs an `Engine`, `Module`, and
  `Store<PluginData>` *per plugin instance* — no shared store. This is
  the critical invariant for memory isolation, and it holds.
- Host imports are explicitly allowlisted in
  `crates/nexus-plugins/src/host_fns.rs:56-67` via `register_host_fns`:
  `host::log`, `host::kv_get/set`, `host::emit_event`,
  `host::read_file/write_file`, `host::invoke_command`,
  `host::get_settings`, `host::notify`. Nothing is added to the linker
  outside this function.
- Each host function gates on `PluginData.capabilities: CapabilitySet`
  before executing; denial routes through `audit::log_capability_denied()`
  and returns `HOST_CAPABILITY_DENIED`.
- `unsafe` is bounded and audited: `host_fns.rs:74-79` wraps
  `wasmtime::Memory::data()` reads in length-checked
  `std::slice::from_raw_parts`. No raw pointer or function-reference
  passing across the boundary; the ABI is `(ptr, len)` pairs into
  bounded buffer copies (`read_wasm_bytes`, `read_wasm_str`).
- Resource limits via `StoreLimitsBuilder` (sandbox.rs:62-63) cap
  per-plugin memory, preventing runaway allocation by a misbehaving guest.
- Trust boundary at compile time: `crates/nexus-plugins/src/loader.rs:542`
  rejects `trust_level = "core"` from a manifest path; `:649`
  (`register_core`) rejects anything else. Native vs WASM is dispatched
  by trust level, not by special-casing in handlers (per ADR 0016).
- Browser-side sandboxing for UI plugins (per ADR 0015): handled in the
  shell's `shell/src/host/sandbox/` TypeScript layer (out of scope of
  the Rust audit, but the iframe runs `sandbox="allow-scripts"` with no
  `allow-same-origin`).

**Recommendations:** Add an integration test under
`crates/nexus-plugins/tests/` that loads a fixture WASM plugin without
`Capability::FsRead` and asserts that `host::read_file` returns
`HOST_CAPABILITY_DENIED` and emits an audit event. Today the gating logic
is verified by hand reading host_fns.rs; an end-to-end test would lock
it down.

---

### 5. Extensibility & Plugin Lifecycle — ✅ PASS

**Findings:**

- `CorePlugin` trait at `crates/nexus-plugins/src/loader.rs:93-159`
  exposes seven lifecycle hooks: `on_init`, `on_start`, `on_stop`,
  `on_enable`, `on_disable`, `on_settings_changed`, plus `dispatch` and
  `dispatch_async`. State transitions (Init → Running → Stopped) are
  driven through these hooks, not by ad-hoc construction.
- Manifest format defined at `crates/nexus-plugins/src/manifest.rs:42-91`
  (`PluginManifest`). Required fields include `api_version` (line 57),
  `capabilities` (line 64), `registrations` (UI commands / panels /
  settings tabs / CLI subcommands, line 80), `lifecycle` (line 82),
  and semver-locked `dependencies` (line 90). TOML format.
- ABI compatibility check: `crates/nexus-plugins/src/loader.rs:1571`
  (`check_api_version`) compares manifest `api_version` major against
  `PLUGIN_API_VERSION` constant in
  `crates/nexus-plugin-api/src/lib.rs:54`. Mismatches are rejected
  before `on_init`.
- Discovery path differs by tier:
  - Core plugins are registered explicitly by `nexus-bootstrap` in
    deterministic order — see `crates/nexus-bootstrap/src/lib.rs` and
    the dedicated `crates/nexus-bootstrap/tests/registration_order.rs`
    test.
  - Community plugins are scanned lazily by the shell from
    `~/.nexus-shell/plugins/` via `scan_plugin_directory` at
    `shell/src-tauri/src/lib.rs:62`.
- Adding a new plugin does **not** require editing `nexus-kernel` —
  community plugins drop into the plugins directory; new core plugins
  add a registration line in bootstrap.

**Recommendations:** None.

---

### 6. Dependency Inversion & Coupling — ✅ PASS

**Findings:**

- Compile-time dependency graph is acyclic and one-directional:
  `nexus-types` (leaf) ← `nexus-plugin-api` ← `nexus-kernel` ←
  `nexus-plugins` ← `nexus-bootstrap` ← invokers
  (`nexus-cli`, `nexus-tui`, `nexus-mcp`). Subsystems
  (`nexus-storage`, `nexus-ai`, `nexus-editor`, …) depend on
  `nexus-kernel` + `nexus-plugins` but not on each other.
- Trait/impl split is correct: `KvStore` trait at
  `crates/nexus-kernel/src/kv_store.rs:24`, sole impl `SqliteKvStore` in
  `crates/nexus-kv/src/lib.rs:22`, injected via `Kernel::new` at
  `crates/nexus-kernel/src/kernel.rs:50`. `nexus-kernel` is therefore
  storage-backend-agnostic.
- Forbidden-dep test (`crates/nexus-bootstrap/tests/dep_invariants.rs:17-41`)
  enforces 11 pairs, including the closure of `cfg`-conditional dep
  blocks (`:81-95`) — caught issue #83 before it landed.
- No global mutable state in the kernel: `grep -rn 'static mut\|lazy_static!\|once_cell::sync::Lazy' crates/nexus-kernel/src/`
  returns nothing. State lives on struct instances (`Arc<Mutex<…>>` or
  injected handles).
- `parse_manifest` accepts a `&str`, returns a typed struct — no I/O is
  buried in the parse layer. The bootstrap layer owns I/O; subsystems
  own logic.

**Recommendations:** None.

---

### 7. Async Patterns & Concurrency Safety — ✅ PASS

**Findings:**

- Single async runtime: `grep -rn 'async-std' crates/ shell/src-tauri/`
  returns nothing. Workspace `Cargo.toml:42-52` pins `tokio = "1.51"`
  with a deliberate feature set (`rt-multi-thread`, `macros`, `fs`,
  `io-util`, `net`, `time`, `sync`, `process`, `signal`).
- No sync mutex held across `.await`. The single
  `std::sync::Mutex` in kernel core is `crates/nexus-kernel/src/kv_store.rs:13`,
  guarding an in-memory `HashMap`; the lock is acquired and released
  inside synchronous helpers, then `async fn` wrappers call those
  helpers without an intervening await
  (`crates/nexus-kernel/src/context_impl.rs:256-275`).
- Mutex-poison handling is explicit — `kv_store.rs:80-81` unwraps with a
  documented "lock poisoned" comment rather than swallowing.
- IPC dispatch is non-blocking and structured:
  `crates/nexus-kernel/src/context_impl.rs:344-353` prefers
  `dispatch_async`, falling back to `tokio::task::spawn_blocking` at
  `:356-360` for legacy sync handlers, with the join handle awaited.
  No fire-and-forget.
- Event bus uses `tokio::broadcast`
  (`crates/nexus-kernel/src/event_bus.rs:5,53`); slow subscribers are
  signalled via `RecvError::Lagged(n)` (`:182-194`) rather than
  silently dropped.
- `block_on` usages all sit on the sync↔async bridge:
  - Bootstrap helpers (`crates/nexus-bootstrap/src/{database,storage,terminal}.rs`)
    are explicitly documented as sync→async bridges.
  - CLI command modules use a dedicated runtime per subcommand
    (`tokio::runtime::Builder::new_current_thread`).
  - Shutdown paths in `crates/nexus-mcp/src/core_plugin.rs:130` and
    `crates/nexus-ai/src/indexing_daemon.rs:384` are inside
    `std::thread::spawn` blocks with their own runtimes, with hard
    shutdown deadlines (e.g., MCP's 5-second poll loop at
    `core_plugin.rs:135-139`) — pattern is correct, not a violation.
- No `tokio::spawn(...)` without join handles in kernel source.

**Recommendations:** None — the design correctly chooses sync `main()`
+ runtime-built-from-bootstrap over `#[tokio::main]`, allowing the same
runtime to be shared across CLI/TUI/MCP frontends.

---

### 8. Test Coverage & Error Handling — ✅ PASS

**Findings:**

- Typed errors via `thiserror`:
  - `crates/nexus-kernel/src/error.rs:16-45` defines a closed `Error`
    enum with domain-specific variants (`PluginError`,
    `CapabilityError`, `IpcError`, `BusError`, `KvError`,
    `ConfigError`).
  - Sub-variants carry context: `PluginError::LoadFailed { plugin_id,
    reason }`, `IpcError::Timeout { plugin_id, command, timeout_ms }`,
    `BusError::TypeIdNamespaceMismatch { plugin_id, type_id }`,
    `PluginError::DependencyCycle { plugins }`. Zero `Error(String)`
    catch-alls.
  - Wire-compatible mirror in `crates/nexus-plugin-api/src/error.rs:6-80`
    plus `IpcErrorEnvelope` (`:110-223`) for cross-process
    propagation.
- Production-code `unwrap`/`panic!` usage is disciplined:
  - In `crates/nexus-kernel/src/`, all non-test `unwrap()` calls trace to
    `audit.rs:86,118` (mutex-poison rationale documented inline) and
    `kv_store.rs:80-81`. All other unwraps live under `#[cfg(test)]`.
  - `crates/nexus-storage/src/atomic.rs:187-263` and
    `crates/nexus-storage/src/index.rs:781-922` unwraps are inside
    `#[cfg(test)]` modules.
  - `panic!` in production is limited to init-time invariants
    (`crates/nexus-kernel/src/event_bus.rs:49-51` asserts
    `EventBus` capacity > 0).
- Negative-path tests:
  - Capability denial: `crates/nexus-kernel/src/context_impl.rs:427-432`
    asserts `kv_get`/`kv_set` denied when caps absent.
  - Path traversal: `:509-540` verifies the audit event is emitted.
  - Error-envelope round-trip: `crates/nexus-plugin-api/src/error.rs:309-442`
    covers `Timeout`, `PluginCrashed`, `CapabilityDenied`,
    `SerializationFailed`, etc.
- IPC integration tests at
  `crates/nexus-bootstrap/tests/{agent,database,editor,forge,skills,theme,
  workflow,community_to_core}_ipc.rs` exercise the kernel ↔ plugin
  dispatch path end-to-end with real plugin instances (no mocks at the
  IPC boundary).
- Cross-process / shell-side test coverage in
  `shell/tests/*.test.ts` (workflow-store, saved-commands, bases-store,
  theme-store, sandbox-orchestrator, popout-shell, markdown-doc-xss,
  backlinks-filter) using `node:test`.

**Recommendations:**

1. The fixture story is implicit (each test calls `tempfile::tempdir()`).
   Consider a `crates/nexus-bootstrap/tests/common/` helper module that
   builds a "minimal forge" with deterministic test markdown — would
   shorten setup boilerplate in the per-subsystem IPC tests.

---

## Cross-Cutting Observations

1. **Tests are the architecture's enforcement layer, not just verification.**
   `dep_invariants.rs` (Dim 1, 6), `registration_order.rs` (Dim 5),
   `plugin_contract_purity.rs` (Dim 1), `process_spawn_gate.rs` (Dim 3),
   `ipc_schema_emit.rs` (Dim 2), and the eight per-subsystem IPC tests
   (Dim 2, 8) collectively make it *physically impossible* to merge a
   PR that violates the four invariants. This is rare and load-bearing —
   protect it.
2. **The IPC `Value`-shaped boundary is an intentional trade-off, not an
   oversight.** It enables a uniform dispatcher across native + WASM +
   community plugins. The recommendation in Dim 2 is therefore *additive
   hardening* (`deny_unknown_fields` + a wrapper for typed handlers),
   not a redesign — the architecture is consistent with the constraint.
3. **Capability-gated audit logging closes the loop.** Every denial in
   Dim 3 routes through structured tracing with `audit = true`, making
   the security posture observable in production rather than implicit.
   This is the correct way to make capability checks a first-class
   architectural feature instead of a checkbox.
4. **Frontends and bridges are scrupulously thin.** The `block_on` usages
   in CLI/MCP/AI shutdown paths all sit in dedicated thread+runtime
   bridges; none pollute async contexts. This is the right pattern for
   embedding tokio runtimes inside larger sync programs (Tauri is sync at
   the top level), and Nexus applies it consistently.

## Prioritized Action Items

| Priority | Item | Dimension(s) |
|----------|------|--------------|
| P0 | Add `#[serde(deny_unknown_fields)]` to all `*Args` / `*Reply` IPC structs across subsystem crates; assert `additionalProperties: false` in the schema-emit test. | 2 |
| P1 | Add an integration test in `crates/nexus-plugins/tests/` proving that a WASM plugin without `Capability::FsRead` cannot call `host::read_file` (currently relies on host_fns code review). | 4 |
| P1 | Decide on an explicit IPC schema-versioning convention (per-handler version field or `command` suffix `.v1`) and document it before the first community-plugin release. | 2 |
| P2 | Document the renderer-side consent flow as a load-bearing prerequisite of `set_plugin_granted_capabilities` in a comment at the Tauri command site. | 3 |
| P2 | Extract a shared "minimal forge" test fixture helper to reduce setup duplication in the eight `*_ipc.rs` integration tests. | 8 |

## Appendix: Evidence References

- `docs/architecture/invariants.md` — the four invariants and their enforcement mechanisms.
- `docs/adr/0002-hierarchical-capability-strings.md` — capability taxonomy.
- `docs/adr/0004-crate-boundaries-and-ownership.md` — boundary ownership.
- `docs/adr/0005-single-dispatch-handler-ids.md` — IPC handler-ID convention.
- `docs/adr/0011-adopt-plugin-first-shell.md` — single shell target rationale.
- `docs/adr/0015-iframe-sandbox-plugin-runtime.md` — UI sandbox.
- `docs/adr/0016-microkernel-native-vs-wasm-plugin-split.md` — native/WASM split.
- `Cargo.toml:42-52` — workspace tokio configuration.
- `crates/nexus-kernel/src/lib.rs:32-51` — kernel public surface.
- `crates/nexus-kernel/src/context_impl.rs:92-102, 120-152, 256-275, 301-335, 344-360` — capability + IPC + KV impl.
- `crates/nexus-kernel/src/audit.rs:14-22, 25-33, 56-64` — audit logging.
- `crates/nexus-kernel/src/event_bus.rs:5, 18, 49-51, 53, 100, 127, 147, 154-159, 182-194` — broadcast bus.
- `crates/nexus-kernel/src/kv_store.rs:13, 24, 80-81` — KV trait + poison handling.
- `crates/nexus-kernel/src/error.rs:16-45` — kernel error enum.
- `crates/nexus-plugin-api/src/capability.rs:13-42` — Capability enum.
- `crates/nexus-plugin-api/src/error.rs:6-80, 110-223, 309-442` — plugin-side errors + envelope.
- `crates/nexus-plugin-api/src/lib.rs:54` — `PLUGIN_API_VERSION` constant.
- `crates/nexus-plugins/src/loader.rs:93-159, 542, 649, 1571` — `CorePlugin` trait, trust gating, ABI check.
- `crates/nexus-plugins/src/manifest.rs:42-91` — manifest schema.
- `crates/nexus-plugins/src/sandbox.rs:9, 31, 62-63` — wasmtime per-plugin Store + StoreLimits.
- `crates/nexus-plugins/src/host_fns.rs:31-39, 56-67, 71-86` — host imports + capability gating + bounded unsafe.
- `crates/nexus-bootstrap/tests/dep_invariants.rs:17-41, 81-95` — forbidden-dep table + cfg-traversal.
- `crates/nexus-bootstrap/tests/{agent,database,editor,forge,skills,theme,workflow,community_to_core}_ipc.rs` — per-subsystem IPC integration.
- `crates/nexus-bootstrap/tests/{ipc_schema_emit,registration_order,plugin_contract_purity,process_spawn_gate}.rs` — invariant tests.
- `crates/nexus-storage/src/ipc.rs:45-262` — typed storage IPC args.
- `crates/nexus-mcp/src/core_plugin.rs:115-139` — bridge-pattern shutdown.
- `crates/nexus-ai/src/indexing_daemon.rs:375-394` — daemon thread + runtime.
- `shell/src-tauri/src/lib.rs:62, 160, 213, 329, 350, 386, 503` — Tauri command surface.
- `scripts/check_ipc_drift.sh` — IPC drift gate.
