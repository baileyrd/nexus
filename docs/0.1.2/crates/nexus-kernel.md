# nexus-kernel

> Kind: lib · IPC plugin id: — (the kernel registers no IPC handlers; it routes) · CorePlugin: no · Has settings: `KernelConfig` · As of: 2026-05-25

## Overview

`nexus-kernel` is the microkernel core of Nexus. It owns four things and nothing else: the **event bus**, the **capability gate**, the **plugin-facing context surface** (`PluginContext` and its concrete `KernelPluginContext` implementation, including the IPC dispatch path), and an in-process **metrics / audit** emission layer. Everything domain-specific — storage, AI, git, editor, terminal — lives in subsystem crates that depend on the kernel; the kernel never depends on a subsystem. The crate description sums it up: "event bus, plugin lifecycle, capability system."

The crate is built to honour the **microkernel isolation** invariant directly. Its only `nexus-*` dependencies are the two leaf crates `nexus-plugin-api` and `nexus-types`. The dependency-direction rule is machine-enforced from outside — `crates/nexus-bootstrap/tests/dep_invariants.rs` forbids `nexus-kernel → rusqlite` and `nexus-kernel → nexus-kv`, encoding the rule that the kernel defines the `KvStore` *trait* but never links a concrete backend; bootstrap injects one via `Kernel::new`. The same backend-agnostic posture applies to the audit store (`AuditStore` trait here, SQLite impl in `nexus-bootstrap`) and to metrics (registry here, installed globally at boot).

A subtlety worth stating up front: many of this crate's public modules are **re-export shims**. The stable plugin-ABI types — `Capability`/`CapabilitySet`, `NexusEvent`/`EventFilter`/`PublishedEvent`, `IpcDispatcher`/`IpcFuture`, `LogLevel`, `PluginInfo`/`PluginStatus`/`TrustLevel`, and the `IpcError`/`BusError`/`CapabilityError` enums — are *defined* in `nexus-plugin-api` (the "F-2.1.1 extraction") and merely re-exported through `nexus-kernel` so that internal `crate::*` imports and downstream `nexus_kernel::*` imports keep resolving. The genuinely kernel-internal types are `EventBus`, `EventSubscription`, `KernelPluginContext`, the `PluginContext` trait (and its six narrower supertraits), `KvStore`/`InMemoryKvStore`, `Kernel`, `KernelConfig`/`WasmCapsCeiling`, the `audit`/`audit_store`/`metrics`/`cancel` modules, and the kernel-local `Error`/`PluginError`/`KvError`/`RecvError`/`ConfigError` enums.

The **IPC-over-direct-calls** invariant is realised through `KernelPluginContext::ipc_call`. A plugin (or a frontend) holds only a `&dyn PluginContext` (or one of the narrower supertrait objects), so the only way it can reach another plugin's command is `ipc_call(target, command, args, timeout)`. That single method is where caller-side capability enforcement, args-aware additional-cap checks, the `internal = true` (core-trust-only) gate, cooperative cancellation, sync/async dispatch selection, timeout, panic isolation, metrics, and per-failure audit logging all converge. The kernel itself registers **no IPC handlers**; it routes to a dispatcher (`Arc<dyn IpcDispatcher>`) supplied by `nexus-plugins`.

The **capabilities-gate-everything** and **file-as-truth** invariants are visible in `context_impl.rs`: every `FileSystem`/`KvAccess`/`Ipc` method calls `require_capability(...)` before touching anything, and the filesystem methods additionally confine every path to the canonical forge root (with TOCTOU-safe writes via `ForgePathValidator`). The kernel does **not** own plugin lifecycle despite its description — that lives in `nexus-plugins` (`PluginLoader`/`PluginManager`). `Kernel::start`/`shutdown` are thin logging/sentinel hooks retained for PRD-01 compatibility (see the OI-13 note that removed the always-empty `Kernel::plugins()` accessor).

## Position in the dependency graph

- **Direct nexus-* dependencies:** `nexus-plugin-api` and `nexus-types` only — both leaf crates. This is the microkernel isolation invariant, enforced negatively by `dep_invariants.rs` (`nexus-kernel` must not depend on `rusqlite` or `nexus-kv`).
- **Notable external dependencies (+ why):**
  - `tokio` — the event bus is a `tokio::sync::broadcast` channel; the IPC dispatch path uses `tokio::time::timeout`, `tokio::select!`, `spawn_blocking`, and `task_local!`.
  - `tokio-util` — `CancellationToken` for cooperative IPC cancellation (re-exported from `cancel` so callers needn't take a direct dep).
  - `serde` / `serde_json` — `serde_json::Value` is the IPC payload and event-payload carrier; audit/metrics snapshots derive `Serialize`.
  - `toml` — `KernelConfig::load` parses `<forge_root>/.nexus/config.toml`.
  - `uuid` — `EventMetadata::event_id` (fresh v4 per publish).
  - `chrono` — `EventMetadata::timestamp` and `AuditEntry` time fields.
  - `thiserror` — derives the kernel-local error enums.
  - `async-trait` — `FileSystem`, `KvAccess`, `Ipc` are async traits; `KernelPluginContext` implements them via `#[async_trait]`.
  - `tracing` — all audit events and operational logging are structured `tracing` events.
  - dev-dependencies: `tempfile` (forge-root fixtures), `tracing-subscriber` (the `audit::test_support` capture layer), `criterion` (event-bus benches).
- **Crates that depend on this one:** nearly the whole service/frontend layer. Direct dependents include `nexus-plugins`, `nexus-kv`, `nexus-bootstrap`, `nexus-storage`, `nexus-ai`, `nexus-ai-runtime`, `nexus-security`, `nexus-git`, `nexus-editor`, `nexus-terminal`, `nexus-lsp`, `nexus-dap`, `nexus-acp`, `nexus-mcp`, `nexus-agent`, `nexus-skills`, `nexus-workflow`, `nexus-notifications`, `nexus-audio`, `nexus-collab`, `nexus-crdt`, `nexus-theme`, `nexus-remote`, `nexus-cli`, `nexus-tui`, and `nexus-fuzz`.

## Public API surface

### Crate root (`lib.rs`)

Crate-wide lints: `#![deny(missing_docs)]`, `#![warn(clippy::pedantic)]`, `#![allow(clippy::module_name_repetitions)]`. The root re-exports both the stable plugin-api types (so they appear in the `nexus_kernel::*` namespace) and the kernel-internal types. Public modules: `audit`, `audit_store`, `cancel`, `metrics` are `pub mod`; `capability`, `config`, `context`, `context_impl`, `error`, `event`, `event_bus`, `ipc`, `kernel`, `kv_store`, `log`, `plugin` are private modules whose contents are selectively re-exported.

### `kernel` module — `struct Kernel`

The entry point, but deliberately thin. Holds `config: KernelConfig`, `event_bus: Arc<EventBus>`, `kv_store: Arc<dyn KvStore>`, and a `shutdown_flag: Arc<AtomicBool>`.

- `Kernel::new(config, kv_store) -> Result<Self>` — synchronous; builds in-memory state, starts no background tasks, discovers no plugins. `kv_store` is injected (pick `SqliteKvStore` or `InMemoryKvStore` from `nexus-kv`). Constructs the `EventBus` with `config.event_bus_capacity`. Infallible today (the `Result` is forward-compat).
- `event_bus() -> Arc<EventBus>` — clone of the bus handle (used by `nexus-cli` to install event taps without a plugin).
- `kv_store() -> Arc<dyn KvStore>` — clone of the KV handle (used by `nexus-plugins` to inject storage into sandboxes).
- `config() -> &KernelConfig`.
- `async start() -> Result<()>` — emits a "kernel online" trace marker and returns. Idempotent. Plugin lifecycle is explicitly *not* owned here.
- `async shutdown() -> Result<()>` — flips `shutdown_flag` via `swap(true, SeqCst)`; idempotent (a second call is a logged no-op). Plugins are drained by `PluginManager::shutdown`, not here.

### `event_bus` module

- **`struct EventBus`** — wraps `broadcast::Sender<Arc<PublishedEvent>>`.
  - `new(capacity) -> Self` — **panics if `capacity == 0`** (issue #81: `tokio::broadcast::channel(0)` panics; failing at the construction site names the bus and the constraint).
  - `publish_plugin(source_plugin_id, type_id, payload) -> Result<()>` — plugin-tier publish. Only emits `NexusEvent::Custom`; enforces that `type_id` is in `source_plugin_id`'s namespace (or is a kernel-owned shared topic). The kernel populates `emitting_plugin` from the caller — a plugin cannot override attribution.
  - `publish_core(plugin_id, event) -> Result<()>` — core-tier publish; a `trust_level = core` plugin may emit any first-class `NexusEvent` variant. Currently infallible.
  - `publish_kernel(event) -> Result<()>` — `pub(crate)`; kernel-owned events (`source_plugin_id = "kernel"`), not reachable from plugins. Wired by `nexus-plugins` (PRD 04).
  - `subscribe(filter) -> EventSubscription`.
  - `subscriber_count() -> usize`.
- **`fn type_id_in_namespace(type_id, plugin_id) -> bool`** (`#[doc(hidden)]`, `pub`) — pure anti-spoofing predicate: true iff `type_id == plugin_id` or `type_id` is `<plugin_id>.<non-empty-suffix>`. A plain `starts_with` is unsafe here (`com.fo` would match `com.foo.event`); the `.`-separator anchors the boundary (issue #79). Exposed `pub` so the `nexus-fuzz` crate can drive it directly.
- **`fn is_kernel_owned_shared_topic(type_id) -> bool`** (`#[doc(hidden)]`) — true for `KERNEL_OWNED_SHARED_TOPICS`, currently only `nexus_types::activity::ACTIVITY_APPENDED_TOPIC`. These are cross-plugin fan-out channels any plugin may publish to; attribution is still recorded so a shared topic is not an anonymity escape hatch.
- **`struct EventSubscription`** — holds a `broadcast::Receiver` + an `EventFilter`. `async recv() -> Result<Arc<PublishedEvent>, RecvError>` loops past non-matching events; `try_recv() -> Result<Option<Arc<PublishedEvent>>, RecvError>` is the non-blocking form. Dropping the subscription auto-unsubscribes.

### `context` module — the plugin-facing API

`PluginContext` is split into **six narrower supertraits** for interface-segregation; a blanket impl auto-derives `PluginContext` for any type implementing all six.

- **`trait Identity: Send + Sync`** — `plugin_id()`, `plugin_version()`, `has_capability(cap) -> bool`.
- **`trait FileSystem` (`#[async_trait]`)** — `read_file`, `write_file`, `delete_file`, `list_files`. Each gated by `fs.read`/`fs.write`.
- **`trait KvAccess` (`#[async_trait]`)** — `kv_get`, `kv_set`, `kv_delete`. Keys are plugin-local; the kernel namespaces internally. Gated by `kv.read`/`kv.write`.
- **`trait Events`** — `publish(type_id, payload) -> Result<()>`, `subscribe(filter) -> EventSubscription`.
- **`trait Ipc` (`#[async_trait]`)** — `ipc_call(target_plugin_id, command_id, args, timeout) -> Result<serde_json::Value, IpcError>`. `timeout` is required.
- **`trait Log`** — `log(level, message)`, plumbed through `tracing` with a `plugin_id` field.
- **`trait PluginContext: Identity + FileSystem + KvAccess + Events + Ipc + Log`** — the umbrella; `impl<T> PluginContext for T where T: …` auto-derives it. Handlers should depend on the narrowest supertrait they need.

### `context_impl` module — `struct KernelPluginContext`

The concrete `PluginContext` implementation handed to each plugin. `Clone` (Arc-backed). Fields: `plugin_id`, `plugin_version`, `capabilities: Arc<RwLock<CapabilitySet>>` (live/mutable — see BL-096 below), `kv: Arc<dyn KvStore>`, `event_bus: Arc<EventBus>`, `forge_root_canonical`, `path_validator: ForgePathValidator`, `ipc_dispatcher: Option<Arc<dyn IpcDispatcher>>`, `caller_trust_level: TrustLevel`.

- `new(plugin_id, plugin_version, capabilities, kv, event_bus, forge_root, ipc_dispatcher) -> Result<Self>` — canonicalizes the forge root once via the validator (failure → `Error::Io(InvalidInput)`). `caller_trust_level` defaults to `Community` (the restrictive value).
- `with_trust_level(level) -> Self` — builder hook; bootstrap upgrades core-plugin contexts to `TrustLevel::Core` so they can reach `internal = true` handlers.
- `caps_handle() -> Arc<RwLock<CapabilitySet>>` — the loader stashes this so `PluginLoader::revoke_capability` can mutate the cap set in place, with no plugin restart (BL-096).
- `capabilities_snapshot() -> CapabilitySet` — for `PluginInfo` reporting/tests.
- The six supertrait impls plus private helpers `caps_contains`, `require_capability`, `confine_path`, `ipc_call_inner`.
- `pub fn in_flight_sync_dispatches() -> usize` (module-level) — advisory count of sync IPC dispatches currently held on the blocking pool.

### `kv_store` module

- **`trait KvStore: Send + Sync + Debug`** — `get/set/delete(namespace, key)` returning `Result<…, KvError>`. Each namespace is isolated. The real durable backend (`SqliteKvStore`) lives in `nexus-kv`; bootstrap injects it.
- **`struct InMemoryKvStore`** — `Mutex<HashMap<(String, String), Vec<u8>>>` fake for tests. `new()`.
- `impl KvError { pub fn backend(msg) -> Self }` — convenience constructor for `KvError::BackendError`.

### `config` module

`KernelConfig` and `WasmCapsCeiling` — see [Settings / Config](#settings--config).

### `error` module

`Result<T>`; `enum Error` (transparently wraps `Plugin`, `Capability`, `Ipc`, `Bus`, `Kv`, `Config`, and `Io`). Kernel-local error enums: `PluginError` (LoadFailed, InitFailed, StartFailed, StopFailed, Crashed, Panicked, DependencyCycle, MissingDependency, DependencyVersionMismatch, DuplicatePluginId, NotFound), `KvError` (NotFound, BackendError), `RecvError` (Lagged(u64), Closed), `ConfigError` (NotFound, Invalid, TomlParse). The plugin-ABI errors `IpcError`/`BusError`/`CapabilityError` are re-imported from `nexus-plugin-api`.

### Re-export shim modules

`capability`, `event`, `ipc`, `log`, `plugin` are one-line `pub use` shims onto `nexus-plugin-api`. Their definitions are documented in `docs/0.1.2/crates/nexus-plugin-api.md`; the kernel re-exports `Capability`, `CapabilityParseError`, `CapabilitySet`; `EventFilter`, `EventMetadata`, `NexusEvent`, `PublishedEvent`, `StopReason`; `IpcDispatcher`, `IpcFuture`; `LogLevel`; `PluginInfo`, `PluginStatus`, `TrustLevel`; plus `BusError`, `CapabilityError`, `IpcError`, `IpcErrorEnvelope`, `IpcErrorKind`, `PLUGIN_API_VERSION`.

## IPC handlers

**The kernel registers no IPC handlers — it is the router, not a service.** It holds an `Option<Arc<dyn IpcDispatcher>>` (the dispatch table is owned by `nexus-plugins`) and exposes the single client-side entry point `KernelPluginContext::ipc_call`. The `IpcDispatcher` trait (defined in `nexus-plugin-api`, re-exported here) is the lookup/dispatch contract:

- `dispatch(target, command, &args) -> Result<Value, IpcError>` — sync path.
- `dispatch_async(target, command, args) -> Option<IpcFuture>` — returns `Some` when the target has an async handler, `None` to fall back to sync.
- `required_caller_caps(target, command) -> Vec<Capability>` — extra caps beyond the unconditional `ipc.call` check (issue #77, so an effect can't be laundered through `IpcCall` alone).
- `required_caller_caps_for_args(target, command, &args) -> Vec<Capability>` — args-aware tightening (ADR 0022 Phase 2; defaults to the args-less form).
- `is_handler_internal_only(target, command) -> bool` — P1-02 "in-tree only" gate; defaults to `false`.

`ipc_call` (public) wraps `ipc_call_inner` with a metrics timer (BL-093) and per-failure audit logging (audit gap D3). `ipc_call_inner`'s flow:

1. Require `Capability::IpcCall` (audit + `IpcError::CapabilityDenied` on failure).
2. Resolve the dispatcher or return `IpcError::DispatcherUnavailable` (the `None` case — e.g. unit tests).
3. For each cap in `required_caller_caps_for_args(...)`, require it (audit + deny on failure).
4. If `is_handler_internal_only(...)` and `caller_trust_level != Core`, audit + deny (P1-02).
5. Bind a `CancellationToken` for this dispatch: if already inside a dispatch (the task-local `IPC_CANCEL` is set), derive a **child token** so cancels propagate down nested `ipc_call` chains; otherwise create a fresh root token.
6. **Async path** (`dispatch_async` returned `Some`): scope the future under the token via `cancel::scope_async`, then `tokio::select!` (`biased`) the token's `cancelled()` against `tokio::time::timeout(timeout, …)`. Cancel → `IpcError::Cancelled`; elapsed → `IpcError::Timeout`.
7. **Sync path** (`None`): `spawn_blocking_sync_dispatch` runs `dispatch` under `cancel::scope_sync`; the same `biased` select races cancel vs `timeout(timeout, join)`. A `JoinError` from the blocking task maps to `IpcError::PluginCrashedDuringCall { reason: "" }` (empty reason = true panic).

**Cooperative cancellation** (`cancel` module, "Track A"): Rust has no safe thread interrupt, so cancellation is cooperative. The kernel sets a `tokio::task_local!` `IPC_CANCEL: CancellationToken` for the dispatch duration; handlers opt in via the re-exported `nexus_kernel::ipc_cancel_token() -> Option<CancellationToken>` and `select!` against `token.cancelled()` (or poll `is_cancelled()` at yield points in sync handlers). `CancellationToken` is re-exported from the `cancel` module so callers (e.g. the Tauri shell's per-window cancel map) needn't take a direct `tokio-util` dependency. Outside a dispatch, `ipc_cancel_token()` returns `None`. Important honest caveat documented in the source: even on the wait-side cancel, the kernel returns `Cancelled` and abandons the handler future / blocking-pool slot, but **cannot forcibly abort** an in-progress future or `spawn_blocking` body — handlers holding expensive resources must opt in to actually release them. `scope_async`/`scope_sync` are the kernel-only install side; plugins only read.

## Capabilities

The capability enum/set are defined in `nexus-plugin-api`; this crate is where enforcement *happens*. Enforcement is structural: a plugin holds only a `&dyn PluginContext`, and every gated method on `KernelPluginContext` calls `require_capability(cap)` (which does `caps_contains` then, on miss, `audit::log_capability_denied` + returns `CapabilityError::Denied`) before any work. The cap set lives behind `Arc<RwLock<CapabilitySet>>` so a runtime revocation (BL-096) is observed by every subsequent check without a restart.

Gate map (from `context_impl.rs`):
- `read_file`, `list_files` → `Capability::FsRead`
- `write_file`, `delete_file` → `Capability::FsWrite`
- `kv_get` → `Capability::KvRead`; `kv_set`, `kv_delete` → `Capability::KvWrite`
- `ipc_call` → unconditional `Capability::IpcCall`, plus any `required_caller_caps_for_args`, plus the `internal = true` core-trust gate
- `publish` → namespace check at the context boundary (mirrors the bus check); not a `require_capability` call in the code read (note: there is an `EventsPublish` cap defined in plugin-api, but `Events::publish` enforces namespace, not that cap, in this impl).

**Path confinement** (file-as-truth boundary): `confine_path` resolves relative paths against the canonical forge root, canonicalizes, and prefix-checks; anything outside emits `audit::log_path_traversal_denied` and returns `Error::Io(PermissionDenied)`. There is **no auto-promotion** from `FsRead` to `FsReadExternal` for absolute paths (OI-12 / MK F-6.3.1) — an absolute outside-forge path fails loud and typed, not silently. Writes use `ForgePathValidator::validate_for_write`, which canonicalizes the deepest existing ancestor (resolving symlinks) before the prefix check, closing the canonicalize-then-open TOCTOU race (MK F-5.3.2; covered by `write_file_rejects_symlinked_parent`).

Capability variants and HIGH-risk classification (the full enum, 33 variants) are documented in `docs/0.1.2/crates/nexus-plugin-api.md` and `docs/0.1.2/capabilities.md`. The kernel references them by value (`Capability::IpcCall`, `Capability::FsRead`, etc.) but does not define them.

## Settings / Config

`KernelConfig` (`config` module; `Debug + Clone`). Loaded from `<forge_root>/.nexus/config.toml` via `KernelConfig::load(&forge_root)`; a missing file returns defaults (not an error). All TOML fields are optional (`RawConfig` / `RawWasmCaps` use `Option<…>`). `for_testing(forge_root)` uses defaults for everything but the root.

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `forge_root` | `PathBuf` | `"."` | Workspace root. Set from the `forge_root` arg, not the TOML body. |
| `event_bus_capacity` | `usize` | `2048` | Broadcast ring-buffer size. `load` **rejects 0** (`ConfigError::Invalid`); `EventBus::new(0)` would panic. (Note: `metrics.rs` doc text mentions a "default 1024" for the gauge cap — stale; the real default is 2048.) |
| `plugin_search_paths` | `Vec<PathBuf>` | `[]` | Dirs to scan for manifests. Doc comment claims a default of `[<forge_root>/.nexus/plugins]`, but the code default is an empty vec. |
| `hot_reload_enabled` | `bool` | `true` | Hot-reload plugins on WASM file change. |
| `lifecycle_timeout_secs` | `u64` | `30` | Per-hook deadline for `on_init`/`on_start` (BL-095); `0` disables the watchdog. Consumed by the loader, not the kernel struct. |
| `tls_pinning_enabled` | `bool` | `false` | BL-102; pins TLS to AI endpoints in `nexus_security::tls_pins::HOST_PINS`. Default off because the shipped pin table is empty. |
| `require_signatures` | `bool` | `false` | BL-099; require a verified `[signature]` block on community plugins. A declared-but-malformed signature still fails loud even when off. |
| `wasm_caps` | `WasmCapsCeiling` | see below | P1-09 system-wide ceiling clamped against each per-plugin `WasmConfig` at load. |

`WasmCapsCeiling` (`Debug + Clone + Copy`): `max_memory_mb: u32 = 128`, `max_fuel: u64 = 100_000_000`, `max_execution_ms: u64 = 30_000`. `ConfigError` variants: `NotFound`, `Invalid { path, reason }`, `TomlParse { path, source }`. Note that `forge_root` is `<forge_root>/.nexus/config.toml`, not the `.forge/` directory the broader settings docs describe for other configs — the kernel config is the one that lives under `.nexus/`.

## Events

The bus is a bounded `tokio::sync::broadcast` channel transporting `Arc<PublishedEvent>` (so each subscriber shares one allocation). Semantics:

- **Topic model:** `NexusEvent` is a closed enum of kernel-owned variants — `PluginLoaded`, `PluginStarted`, `PluginStopped { reason: StopReason }`, `PluginCrashed`, `CapabilityGranted`, `CapabilityDenied` — plus a single open `Custom { type_id, emitting_plugin, payload }` variant for plugin signals. Domain events (file changes, git commits) are `Custom` events with reverse-DNS `type_id`s (e.g. `com.nexus.storage.*`), not new enum variants.
- **Filtering** (`EventFilter`): `All`, `Variant(name)` (matches a kernel variant by name string), `CustomPrefix(prefix)`, `CustomExact(type_id)`. Non-matching events are skipped inside `recv`/`try_recv`, not delivered-and-discarded by the subscriber.
- **Delivery:** fan-out broadcast. A slow subscriber that falls more than `event_bus_capacity` events behind gets `RecvError::Lagged(n)` and can then recover and keep receiving from what remains in the buffer. A dropped bus yields `RecvError::Closed`. `broadcast::Sender::send` returning `Err` (zero subscribers) is treated as normal and ignored — hence the publish methods are effectively infallible aside from the namespace check.
- **Metadata** (`EventMetadata`, kernel-populated, plugin cannot forge): fresh `event_id` (UUID v4) per publish, UTC `timestamp`, `source_plugin_id` (set from the caller / `"kernel"`), and `span_id` (the current `tracing` span's numeric id via `Id::into_u64().to_string()` — issue #81 fixed it from leaking the `Debug` repr `"Id(1)"`).
- **Anti-spoofing:** plugin publishes must namespace-match (`type_id_in_namespace`) or hit a kernel-owned shared topic; the context boundary (`Events::publish`) fast-fails with the same `BusError::TypeIdNamespaceMismatch` the bus would.
- **Metrics tie-in:** every publish records `event_bus_published_total{plugin_id}` and re-samples the `event_bus_queue_depth` gauge (`sender.len()`).

## Internals & notable implementation details

- **Sync-dispatch blocking-pool observability.** Sync IPC handlers run on tokio's blocking pool via `spawn_blocking_sync_dispatch`, which maintains a process-global `AtomicUsize` `IN_FLIGHT_SYNC_DISPATCHES`. A `Drop` guard inside the spawned closure decrements even on handler panic. When depth crosses `KERNEL_BLOCKING_POOL_WARN_DEPTH` (from `nexus_types::constants`) a one-shot `audit=true` warn fires; a `HIGH_WATER_WARNED: AtomicBool` latch with hysteresis (resets below half the threshold) prevents per-call spam. The pool is bounded by the host runtime's `max_blocking_threads`, which frontends size from `KERNEL_BLOCKING_POOL_SIZE`. Reads use `Ordering::Relaxed` (advisory).
- **`biased` select toward cancel.** Both dispatch arms put the cancel branch first with `biased` so a same-tick cancel beats a stale ready result from the future arm.
- **Metrics (`metrics` module, BL-093).** A self-contained in-process registry — deliberately *not* the `metrics` crate, and no Prometheus endpoint (deferred). `KernelMetrics` holds `CounterMap`s and `HistogramMap`s (`Mutex<HashMap<String, …>>`) plus a single-slot `Gauge` (`AtomicU64`). Histograms use fixed exponential ns buckets (1µs…10s, +∞) and interpolate p50/p95/p99 from cumulative bucket counts. Cardinality is capped at `MAX_KEYS_PER_METRIC = 4096` per map; overflow increments a `metrics_dropped_total` sentinel rather than growing unbounded. Recorded metrics: `ipc_calls_total{plugin::command::status}`, `ipc_call_duration` histogram, `event_bus_published_total`, `capability_checks_total{plugin::cap::result}`, `plugin_lifecycle_duration` histogram, and the `event_bus_queue_depth` gauge. `CallStatus`: `Ok`, `CapabilityDenied`, `NotFound`, `Timeout`, `Cancelled`, `Error`. Installed globally via `metrics::install(Arc<KernelMetrics>)` (`OnceLock`); hot-path call sites branch on `metrics::global()` being `Some` and skip recording otherwise (so unit tests that don't boot a kernel are unaffected). `time_lifecycle` is a timing convenience for the loader's watchdog (BL-095).
- **Audit (`audit` + `audit_store`, BL-094).** `audit.rs` emits structured `tracing` events (all carry `audit = true`) for capability grant/revoke/deny, plugin lifecycle, credential access, MCP tool/resource calls, and path-traversal denial — and *also* appends to a pluggable store. `audit_store.rs` defines the `AuditStore` trait (`append`/`query`/`clear`) plus `AuditEntry` and `AuditQuery` (serde) and a global `OnceLock` accessor (`install`/`append`/`query`/`clear`). Emission is **infallible from the caller's perspective**: the backend swallows DB errors and warns — "audit pipelines must never break the operation they record." The SQLite impl lives in `nexus-bootstrap` (microkernel invariant), and is installed at boot. Output destination of the `tracing` events is configured by the binary crate (`tracing-subscriber` + `tracing-appender`).
- **Thread-safety / locking.** The cap set uses `RwLock` (read for gates, write for revocation). KV (in-memory) and metrics maps use `Mutex` and recover from poisoning via `into_inner()` (metrics) or surface a `KvError::backend("lock poisoned")` (KV). The event bus, KV store, and dispatcher are all `Arc`-shared. No `unsafe` in the crate.
- **`KernelPluginContext` cloning.** Cheap — all fields are `Arc`/`String`/`PathBuf`/`Copy`. The loader clones it per plugin sandbox.
- **Why `start`/`shutdown` are thin.** Plugin lifecycle is owned by `nexus-plugins` (`PluginManager::load_all` / `::shutdown`, reverse-registration drain). The kernel kept these as logging/sentinel hooks for PRD-01 compat after OI-13 removed the always-empty plugin registry.

## Tests

- `crates/nexus-kernel/tests/smoke_kernel.rs` — PRD-01 §12 acceptance: new→start→shutdown, event-bus round-trip (subscription + `try_recv` on an empty plugin set), config-from-disk, idempotent shutdown, and a compile-only `smoke_all_public_types_importable` that names every public type the interface spec promises (regression guard against contract drift).
- `crates/nexus-kernel/tests/metrics_smoke.rs` — installs the global registry, drives every recording surface, asserts the snapshot reflects each event (BL-093).
- `crates/nexus-kernel/benches/event_bus.rs` (criterion, BL-092) — publish throughput with 0/1/10 subscribers and filter match/no-match; baselines only, no SLO assertions.
- **Unit tests (in-module):**
  - `event_bus.rs` — publish/recv, variant + custom-prefix filtering, lagged/closed recovery, fresh-UUID-per-publish, `new(0)` panic (#81), the issue-#79 namespace-spoofing battery (`type_id_in_namespace_unit_cases`, substring-prefix rejection, dotted-suffix/bare-id allowance), and kernel-owned-shared-topic publish with preserved attribution.
  - `context_impl.rs` — identity, cap-gated KV/FS, namespace-spoof rejection at the context boundary, `read/write` roundtrip, path traversal blocked, **OI-07** coverage that denials route through `audit::log_capability_denied` (asserting `audit=true result=denied` reaches the tracing channel), **OI-12** typed-traversal-error assertions for absolute outside-forge read/write, the `write_file_rejects_symlinked_parent` MK F-5.3.2 regression, the sync-dispatch in-flight counter, and the Track-A `ipc_call_returns_cancelled_when_parent_token_fires` test (proves a parent cancel short-circuits a 10-s sleeping async handler in <1 s).
  - `config.rs` — defaults, override load, malformed-TOML parse error, zero-capacity rejection.
  - `error.rs` — `Display`/wrapping for each error enum.
  - `cancel.rs` — `ipc_cancel_token` is `None` outside a scope, returns the scoped token inside, and clears after the scope exits.
  - `audit.rs` / `audit_store.rs` / `metrics.rs` / `kv_store.rs` / `kernel.rs` — per-helper emission assertions, fake `AuditStore` round-trip + filtering, histogram percentile attribution + gauge last-write-wins semantics, KV namespace isolation, and kernel construct/start/shutdown idempotence.
- **External enforcement (not in this crate):** `crates/nexus-bootstrap/tests/dep_invariants.rs` asserts `nexus-kernel` does not directly depend on `rusqlite` or `nexus-kv` — the backend-agnostic / microkernel-isolation guard.
