# nexus-plugin-api

> Kind: lib · IPC plugin id: — · CorePlugin: no · Has settings: no · As of: 2026-05-25

## Overview

`nexus-plugin-api` is the **stable plugin contract** for Nexus. It contains only the types and traits that cross the kernel/plugin boundary and must remain stable across kernel refactors: the capability enum and set, the plugin-observable error surface, the kernel event types, the IPC dispatch abstraction, plugin identity/status types, the log level, and the `PLUGIN_API_VERSION` ABI constant. Its crate-level docs are explicit that plugin authors should depend on **this crate only**, never on `nexus-kernel` or `nexus-plugins` directly.

The crate is one of the two leaf crates that the microkernel isolation invariant is built around. Per the architecture, `nexus-kernel` depends only on `nexus-types` and `nexus-plugin-api`, both leaves. This crate has **no `nexus-*` dependencies** at all, and a dedicated test (`tests/kernel_free_guard.rs`) enforces that no kernel-internal crate ever leaks into its dependency graph or its source. That guard is what keeps every kernel refactor from silently becoming a plugin-ABI break — the original motivation for extracting this crate (the "F-2.1.1 extraction").

The crate primarily upholds the **capabilities-gate-everything** invariant (the `Capability` enum here is "the single source of truth for the plugin permission system") and the **IPC-over-direct-calls** invariant (the `IpcDispatcher` trait and `IpcFuture` type define the abstraction that all frontends and plugins route through). It does not touch file-as-truth directly, but it carries the event types (`NexusEvent`, `PublishedEvent`) that storage and other services use to broadcast domain changes.

Notably, this crate does **not** define `CorePlugin`, `PluginContext`, `EventBus`, or `KvStore` — those are kernel-/plugins-internal and live elsewhere (the lib docs enumerate this exclusion list explicitly). What lives here is the *contract*: the discriminated types and trait signatures that both sides agree on. Where the task template asks for the `CorePlugin` trait and `InvokerContext`/`ipc_call` surface, the honest answer is that **those are not defined in this crate** — see the IPC handlers and Public API sections below for what is.

## Position in the dependency graph

- **Direct nexus-* dependencies:** None. This is a leaf crate by design and by enforced test.
- **Notable external dependencies (+ why):**
  - `serde` / `serde_json` — every boundary type is `Serialize`/`Deserialize`; `serde_json::Value` is the IPC payload and event-payload carrier.
  - `thiserror` — derives `Display`/`Error` for the three error enums (`IpcError`, `BusError`, `CapabilityError`) and `CapabilityParseError`.
  - `uuid` — `EventMetadata::event_id`.
  - `chrono` — `EventMetadata::timestamp` (`DateTime<Utc>`).
  - `async-trait` — listed as a dependency for async trait support across the boundary. (Note: the current `IpcDispatcher` trait uses a hand-rolled boxed-future return type, `IpcFuture`, rather than `#[async_trait]`; the dependency is present in `Cargo.toml` but not applied to a trait in the source read.)
  - `ts-rs` — **optional**, gated behind the `ts-export` feature; emits TypeScript bindings into `packages/nexus-extension-api/src/generated/` so the shell's `@nexus/extension-api` contract stays in sync with the Rust types.
  - dev-dependencies: `serde_json`, `toml` (the kernel-free guard parses `Cargo.toml`).
- **Crates that depend on this one:** `nexus-kernel`, `nexus-plugins`, `nexus-notifications`, `nexus-ai-runtime`, `nexus-remote`, and `nexus-fuzz` declare it directly. In practice the entire plugin/service layer reaches these types transitively through the kernel. This is a foundational, widely-consumed leaf.

## Features

- **`ts-export`** (off by default): pulls in `ts-rs` (with `chrono-impl` + `uuid-impl`) and activates the `#[ts(export, export_to = "…/packages/nexus-extension-api/src/generated/")]` attributes on the boundary types. Off by default so normal Rust builds don't pull `ts-rs`. Regenerate bindings with `cargo test -p nexus-plugin-api --features ts-export`. This is the Rust half of the IPC-drift check (`scripts/check_ipc_drift.sh`).

## Public API surface

### Crate root (`lib.rs`)

- `pub const PLUGIN_API_VERSION: u32 = 1` — the host's current plugin API **major** version. Manifests must declare `api_version = "1"` (or `"1.<minor>"`); the loader rejects plugins whose major version differs. Incremented only on breaking ABI changes; backwards-compatible minor extensions bump the documented spec minor without changing this constant.
- Re-exports the public surface from each module (capability, error, event, ipc, log, plugin).
- Crate-wide lints: `#![deny(missing_docs)]`, `#![warn(clippy::pedantic)]`, `#![allow(clippy::module_name_repetitions)]`.

### `capability` module

- **`enum Capability`** (`Copy`, `Hash`, serde) — the canonical in-memory permission. 33 variants (asserted in tests). Each carries a doc comment describing risk and the ADR/backlog item that introduced it. Methods:
  - `const ALL: &'static [Capability]` — all variants in declaration order.
  - `const fn is_high_risk(self) -> bool` — HIGH-risk caps require explicit, persisted user approval. The HIGH-risk set: `FsReadExternal`, `FsWriteExternal`, `NetHttp`, `ProcessSpawn`, `IpcCall`, `AiConfigWrite`, `AudioRecord`, `ProtocolHostContribute`, `SecurityWrite`, `SecurityAuditWrite`, `NetworkBind`.
  - `const fn as_str(self) -> &'static str` — canonical dot-namespaced string (e.g. `fs.read`).
  - `fn from_str(s: &str) -> Result<Self, CapabilityParseError>` — parse from a manifest string (inherent, not the `FromStr` trait).
  - `impl Display` — delegates to `as_str`.
- **`enum CapabilityParseError`** — `UnknownString(String)` for unrecognized capability names.
- **`struct CapabilitySet`** — immutable-ish set of granted capabilities. Despite the doc comment claiming a bitmask, the actual backing store is a `std::collections::HashSet<Capability>`. Methods: `empty`, `contains`, `insert`, `remove`, `iter`, `len`, `is_empty`; plus `impl FromIterator<Capability>` and `Default`.

### `error` module

- **`enum IpcError`** (`Clone`, `thiserror`) — stable IPC failure surface: `PluginNotFound`, `CommandNotFound`, `Timeout { timeout_ms }`, `PluginCrashedDuringCall { reason }`, `SerializationFailed`, `DeserializationFailed`, `CapabilityDenied`, `DispatcherUnavailable`, `ReentrantCall`, `Cancelled`. The `Cancelled` variant's doc explains it can be either returned by a cancellation-aware handler or synthesised by the kernel when the cancellation token fires before the handler future resolves.
- **`enum IpcErrorKind`** (`Copy`, snake_case serde) — coarse, JSON-stable classification: `Timeout`, `PluginCrashed`, `CapabilityDenied`, `DispatchFailed`, `Serialization`, `Cancelled`, `Unknown`. `Unknown` is reserved so old shells still surface future `IpcError` additions.
- **`struct IpcErrorEnvelope`** (snake_case serde) — wire-stable error returned as the `Err` payload of `kernel_invoke`. Fields: `kind`, `plugin_id`, `command`, `message`, `retryable`. Constructors:
  - `from_ipc_error(&IpcError) -> Self` — maps each variant to a `kind` + `retryable`. Only `Timeout` and `Cancelled` are marked `retryable: true`.
  - `from_ipc_error_in_context(&IpcError, fallback_plugin_id, fallback_command)` — fills empty `plugin_id`/`command` fields from caller-supplied fallbacks (used at the `kernel_invoke` boundary for variants like `SerializationFailed` that carry no routing context).
- **`enum BusError`** (`thiserror`) — `Closed` (reserved; the current `tokio::broadcast` bus treats no-subscribers as non-error — see issue #81), `TypeIdNamespaceMismatch { plugin_id, type_id }` (anti-spoofing), `PluginPublishingKernelEvent`.
- **`enum CapabilityError`** (`thiserror`) — `Denied { plugin_id, cap }`, `UnknownString(String)`.

### `event` module

- **`enum NexusEvent`** (serde, `#[serde(tag = "type")]`) — closed set of kernel-owned events plus one open `Custom` variant. Kernel-owned: `PluginLoaded { plugin_id, version }`, `PluginStarted`, `PluginStopped { reason }`, `PluginCrashed { error }`, `CapabilityGranted { capability }`, `CapabilityDenied { capability }`. The `Custom { type_id, emitting_plugin, payload }` variant carries an arbitrary `serde_json::Value` payload (exported to TS as `unknown`); `emitting_plugin` is set by the kernel, not the plugin.
- **`enum StopReason`** (`Copy`, serde) — `UserRequested`, `HotReload`, `Shutdown`, `CrashRecovery`; attached to `PluginStopped`.
- **`enum EventFilter`** (`Clone`) — subscription filter: `All`, `Variant(String)`, `CustomPrefix(String)`, `CustomExact(String)`.
- **`struct EventMetadata`** — `event_id: Uuid`, `timestamp: DateTime<Utc>`, `source_plugin_id: String`, `span_id: Option<String>`. Populated by the kernel at publish time; plugins cannot construct it meaningfully.
- **`struct PublishedEvent`** — `{ metadata, event }`; the bus transports `Arc<PublishedEvent>` to avoid per-subscriber clones.

### `ipc` module

- **`type IpcFuture`** = `Pin<Box<dyn Future<Output = Result<serde_json::Value, IpcError>> + Send>>` — boxed `'static` `Send` future returned by an async IPC handler.
- **`trait IpcDispatcher: Send + Sync`** — dispatches an IPC command to a loaded plugin's handler. The caller's capability check is performed by the kernel context *before* delegating here; the dispatcher only resolves the target and invokes the handler. Methods:
  - `dispatch(target_plugin_id, command_id, args: &Value) -> Result<Value, IpcError>` — required, synchronous.
  - `dispatch_async(target_plugin_id, command_id, args: Value) -> Option<IpcFuture>` — default returns `None` (sync-only); when `Some`, the target has an async handler.
  - `required_caller_caps(target, command) -> Vec<Capability>` — caps the caller must hold *in addition to* the unconditional `IpcCall` check (issue #77). Default empty.
  - `required_caller_caps_for_args(target, command, args) -> Vec<Capability>` — args-aware extension (ADR 0022 Phase 2); default delegates to `required_caller_caps`. `KernelPluginContext::ipc_call` consults this before dispatch.
  - `is_handler_internal_only(target, command) -> bool` — P1-02 in-tree-only gate; default `false`. When `true`, only a `TrustLevel::Core` context may call the handler, independent of accumulated caps (seed caller: `com.nexus.ai::resolve_credentials`).

### `log` module

- **`enum LogLevel`** (`Copy`) — `Trace`, `Debug`, `Info`, `Warn`, `Error`. Deliberately independent of `tracing::Level` so the tracing crate doesn't leak into the stable surface.

### `plugin` module

- **`enum TrustLevel`** (`Copy`, serde) — `Core` (any capability allowed) and `Community` (HIGH-risk caps need install-time approval).
- **`enum PluginStatus`** (`Copy`, `Hash`, serde) — `Loaded`, `Initialized`, `Running`, `Stopped`, `Crashed`.
- **`struct PluginInfo`** — public view of a loaded plugin: `id`, `name`, `version`, `trust_level`, `status`, `capabilities: CapabilitySet`.

## IPC handlers

None — this crate **defines the IPC contract types** (`IpcDispatcher`, `IpcFuture`, `IpcError`, `IpcErrorEnvelope`/`IpcErrorKind`) but registers **no handlers** and has no plugin id. It is a contract-only library with no runtime, no `CorePlugin` impl, and no bootstrap registration. Handler registration and the concrete `ipc_call` / `kernel_invoke` plumbing live in `nexus-kernel` and the service crates that implement `IpcDispatcher`.

## Capabilities

This crate is the **canonical source of truth** for the capability permission system. The `Capability` enum (33 variants) and its `as_str`/`from_str` string mapping define every capability string in the system. The complete list with canonical strings:

`fs.read`, `fs.write`, `fs.read.external`*, `fs.write.external`*, `net.http`*, `net.http.localhost`, `process.spawn`*, `kv.read`, `kv.write`, `ipc.call`*, `db.query`, `db.write`, `events.publish`, `ui.notify`, `ai.chat`, `ai.index`, `ai.session.read`, `ai.session.write`, `ai.config.write`*, `ai.activity.write`, `ai.tools.write`, `ai.tools.mcp`, `audio.record`*, `audio.synthesize`, `ai.runtime.submit`, `ai.runtime.control`, `ai.runtime.observe`, `notifications.inbox.read`, `notifications.inbox.write`, `protocol.host.contribute`*, `security.write`*, `security.audit.write`*, `network.bind`*.

(`*` = classified HIGH risk by `is_high_risk`, requiring an explicit persisted user grant.)

Capability *checks* are not performed here — this crate only models the data. The enforcement happens in the kernel context: `IpcCall` is checked unconditionally before any IPC dispatch, and the dispatcher's `required_caller_caps_for_args` plus `is_handler_internal_only` express per-command tightening. `CapabilityError::Denied { plugin_id, cap }` is the boundary error returned when a grant is missing, and the `CapabilityGranted` / `CapabilityDenied` `NexusEvent` variants record the decisions on the bus.

Many capabilities carry ADR/backlog provenance in their doc comments: `ai.*` from ADR 0022 (Phases 1 and 2), `audio.*` from BL-117, `ai.runtime.*` from BL-134 / ADR 0028, `notifications.inbox.*` from BL-136 / ADR 0029, `protocol.host.contribute` per ADR 0027, and `security.write` / `security.audit.write` / `network.bind` from the P1-01 / P1-07 follow-ups.

## Settings / Config

None. This crate defines no `Config` struct, reads no `.forge/` TOML, and has no settings surface. Its only build-time knob is the `ts-export` Cargo feature (see Features above).

## Events

The event *types* are defined here (the kernel and plugins do the actual publishing). The closed kernel-owned `NexusEvent` set covers plugin lifecycle (`PluginLoaded`, `PluginStarted`, `PluginStopped`, `PluginCrashed`) and capability lifecycle (`CapabilityGranted`, `CapabilityDenied`). All domain events (file changes, git commits, etc.) flow through the single open `Custom { type_id, emitting_plugin, payload }` variant using reverse-DNS namespaced `type_id`s (e.g. `com.nexus.storage.*`).

Anti-spoofing is part of the contract: `BusError::TypeIdNamespaceMismatch` is returned if a plugin publishes a `Custom` event whose `type_id` doesn't start with its own id, and `BusError::PluginPublishingKernelEvent` rejects any attempt by a plugin to emit a kernel-owned variant. `emitting_plugin` and the entire `EventMetadata` (`event_id`, `timestamp`, `source_plugin_id`, `span_id`) are kernel-populated. Subscriptions filter via `EventFilter` (`All` / `Variant` / `CustomPrefix` / `CustomExact`); non-matching events are silently skipped in `recv()`. Events ride the bus as `Arc<PublishedEvent>`.

## Internals & notable implementation details

- **Trait contracts, not impls.** Every public item is a data type or a trait signature. There is no async-runtime code, no I/O, and no concrete plugin. `IpcDispatcher` uses a manual `Pin<Box<dyn Future ... + Send>>` (`IpcFuture`) for async dispatch rather than `#[async_trait]`, which keeps the return type explicit and object-safe. `async-trait` is listed as a dependency but is not applied to any trait in the source as read — worth confirming if a future change relies on it.
- **Serialization stability.** `IpcErrorEnvelope` and `IpcErrorKind` use `#[serde(rename_all = "snake_case")]` and exist precisely because the rich `IpcError` enum doesn't survive a clean JSON round-trip; the envelope flattens it to a stable `kind` + `retryable` so frontends branch without string-sniffing `Display` output. `NexusEvent` uses `#[serde(tag = "type")]` (internally tagged).
- **TS binding parity.** Nearly every boundary type carries `#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]` plus an `export_to` into `packages/nexus-extension-api/src/generated/`. The `Custom` event payload is exported to TS as `unknown`. This is the mechanism keeping the Rust contract and the TypeScript `@nexus/extension-api` contract in lockstep via `scripts/check_ipc_drift.sh`.
- **Version negotiation.** `PLUGIN_API_VERSION = 1` is the only ABI version primitive here; the actual manifest matching (rejecting mismatched majors) lives in the loader, not this crate.
- **A doc/implementation discrepancy to flag:** `CapabilitySet`'s doc comment says "Internally a bitmask over the `Capability` discriminant for O(1) contains," but the field is actually `std::collections::HashSet<Capability>`. Lookups are still O(1) amortized, so behavior matches the intent, but the comment is inaccurate about the representation.

## Tests

- **Inline unit tests** (in each module's `#[cfg(test)] mod tests`):
  - `capability.rs` — round-trips every `Capability::ALL` variant through `as_str`/`from_str`; rejects unknown strings; `CapabilitySet` contains/empty behavior; asserts `Capability::ALL.len() == 33` (with a comment breaking down the count by ADR/backlog item); spot-checks `is_high_risk` (`AiConfigWrite` true, `AiChat` false).
  - `error.rs` — the largest test module: `IpcError` display formatting, `Clone`, `BusError` display, and exhaustive `IpcErrorEnvelope::from_ipc_error` mapping for every variant (kind + retryable + plugin/command fields), the `_in_context` fallback fill/no-overwrite behavior, and snake_case serialization of both the envelope and each `IpcErrorKind` variant.
  - `event.rs` — `StopReason` distinctness and variant-name serialization; `EventFilter` clone and `CustomPrefix` payload; `NexusEvent::PluginLoaded` tagged serialization.
  - `log.rs` — `LogLevel` variant distinctness and `Copy`.
  - `plugin.rs` — `PluginInfo` construction with all fields; `TrustLevel` distinctness; `PluginStatus` `Copy`/`Eq`.
- **`tests/kernel_free_guard.rs`** — the architecturally important integration test, two cases:
  1. `cargo_toml_has_no_kernel_internal_dependencies` — parses this crate's `Cargo.toml` (`[dependencies]`, `[build-dependencies]`, and per-target tables) and asserts none of a `FORBIDDEN` allowlist (`nexus-kernel`, `nexus-plugins`, `nexus-app`, `nexus-bootstrap`, `nexus-cli`, `nexus-tui`, `nexus-storage`, `nexus-security`, `nexus-kv`, `nexus-database`, `nexus-ai`, `nexus-mcp`, `nexus-git`, `nexus-editor`, `nexus-terminal`, `nexus-agent`, `nexus-skills`, `nexus-workflow`, `nexus-formats`, `nexus-theme`) appears.
  2. `source_files_do_not_reference_kernel_internal_crates` — walks `src/`, skips comment lines, and fails if any source line mentions a forbidden crate's snake_case name. This enforces the microkernel-isolation invariant at the contract layer. (Note: the `FORBIDDEN` list is hand-maintained and does not include every newer service crate such as `nexus-notifications`, `nexus-ai-runtime`, `nexus-remote`, `nexus-collab`, etc. — a dependency on one of those would not currently be caught by this guard.)
