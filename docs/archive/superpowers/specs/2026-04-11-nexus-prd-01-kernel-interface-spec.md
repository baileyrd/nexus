# Nexus PRD 01 — Kernel & Event System Interface Spec

**Version:** 0.1
**Date:** 2026-04-11
**Status:** Approved (brainstorming session output)
**Scope:** Public contract for the `nexus-kernel` crate. Defines the exact Rust types, traits, and function signatures that downstream PRDs depend on. This doc is the contract agents implement against when building `nexus-kernel` for M1.

**Parent docs:**
- [`2026-04-11-nexus-roadmap-design.md`](./2026-04-11-nexus-roadmap-design.md) — strategic v0.1 roadmap
- [`2026-04-11-nexus-m1-foundation-spec.md`](./2026-04-11-nexus-m1-foundation-spec.md) — M1 implementation spec (authoritative for anything not explicitly settled here)

---

## 1. Frame

### What this contract covers

The public API surface of `nexus-kernel`, the leaf crate of the M1 dependency DAG. Everything in this doc is what downstream crates (`nexus-security`, `nexus-storage`, `nexus-plugins`, `nexus-cli`) can rely on. Anything not listed here is private to the crate and subject to change without notice.

### What this contract does NOT cover

- Implementation details: SQLite schema, internal mutexes, private module structure
- Tech stack choices already locked by the M1 spec §3 (tokio, tracing, rusqlite, etc.)
- The `Capability` enum shape (M1 spec §4 — hierarchical dot-strings, single source of truth)
- The `NexusEvent` enum variants (M1 spec §5 — closed enum + `Custom` variant with anti-spoofing)
- Crate boundaries and ownership (M1 spec §2 + §6)
- Anything from PRDs 02–05 (those will get their own interface specs)

### Authority

If this doc conflicts with PRD 01, **this doc wins**. PRD 01 was written before the brainstorming sessions and contains several shapes that are now overridden — see §2 (Amendments). PRD 01 itself needs a cleanup pass to mark the overridden sections; that's tracked in §13 as a follow-up.

If this doc conflicts with the M1 spec, **the M1 spec wins** — this doc is meant to refine, not contradict. If a real conflict is discovered, treat it as a bug in this doc and fix it here.

---

## 2. Amendments to PRD 01

These are decisions the M1 spec already made that PRD 01 does not reflect. They are **fait accompli** — listed here so agents reading PRD 01 during implementation are not confused.

| # | PRD 01 says | This contract says | Rationale |
|---|---|---|---|
| **A1** | `Capability` is a Rust enum with variants like `FileRead`, `FileWrite`, `EditorAccess`, `NetworkHttp`, `Custom(String)` | `Capability` uses hierarchical dot-strings: `"fs.read"`, `"fs.write"`, `"net.http"`; the enum is generated from a single source-of-truth table with `as_str()` / `from_str()` | M1 spec §4 — namespacing encodes risk gradient, extensible, matches WASM manifest model |
| **A2** | Plugins register IPC commands at runtime via `ctx.register_ipc_command(...)` with a trait-object handler | IPC commands are declared in the manifest's `[[registrations.ipc_command]]` with a `handler_id`; routing goes through the plugin's `nexus_dispatch` function | M1 spec §6.4 — manifest-based matches WASM single-dispatch convention, validates upfront, no runtime surprises |
| **A3** | `PluginLifecycle` has 5 hooks: `on_load`, `on_init`, `on_start`, `on_stop`, `on_shutdown` | `PluginLifecycle` has 3 hooks: `on_init`, `on_start`, `on_stop` | M1 spec §6.2 — WASM plugins can't do useful work during load/shutdown; kernel owns those transitions. `on_load` / `on_shutdown` deferred to v0.2. |
| **A4** | Plugin state machine has 7 states: Discovered → Loaded → Initialized → Started → Stopped → Unloaded → Error | Plugin state machine has 5 states: Loaded → Initialized → Running → Stopped, with Crashed as an error-path sink | Simpler, matches the 3-hook lifecycle |
| **A5** | `PluginContext` includes `watch_directory()`, `query_plugin()`, `index_query()`, `index_insert()`, `register_ipc_command()` | `PluginContext` is limited to the surface defined in §5.2 below. Extended methods deferred to v0.2+ | M1 spec §6.3 |
| **A6** | `NexusEvent` has 40+ variants across all subsystems (editor, terminal, AI, etc.) | `NexusEvent` in M1 has only M1-relevant variants (file, plugin lifecycle, capability, indexing, custom). M2–M5 add their variants when they reach their phases | M1 spec §5 — phased event taxonomy |
| **A7** | Event metadata carries `event_id`, `timestamp`, `source_plugin_id`, `correlation_id`, `span_id`, `severity` | Event metadata carries `event_id`, `timestamp`, `source_plugin_id`, `span_id`. `correlation_id` and `severity` deferred to v0.2 (no driving use case in M1) | Brainstorm Q1b — `span_id` is cheap given commitment to `tracing`; the other two need a driving use case |
| **A8** | Runtime IPC registration, Event history / replay, `EventHistory` trait | Not in M1 contract | Deferred; `EventHistory` is kernel-internal only for debug commands, not exposed to plugins |

---

## 3. Public Types

### 3.1 `NexusEvent`

Closed enum + single `Custom` variant for plugin events. M1 variants only; M2–M5 will add variants as they reach their phases.

```rust
// nexus-kernel/src/event.rs
use std::path::PathBuf;
use std::sync::Arc;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NexusEvent {
    // M1: storage events
    FileCreated  { path: PathBuf, content_hash: String },
    FileModified { path: PathBuf, content_hash: String },
    FileDeleted  { path: PathBuf },
    FileRenamed  { from: PathBuf, to: PathBuf, content_hash: String },

    // M1: plugin lifecycle events
    PluginLoaded  { plugin_id: String, version: String },
    PluginStarted { plugin_id: String },
    PluginStopped { plugin_id: String, reason: StopReason },
    PluginCrashed { plugin_id: String, error: String },

    // M1: capability lifecycle events
    CapabilityGranted { plugin_id: String, capability: Capability },
    CapabilityDenied  { plugin_id: String, capability: Capability },

    // M1: indexing events
    IndexingStarted   { total_files: usize },
    IndexingProgress  { files_processed: usize, total_files: usize },
    IndexingCompleted { duration_ms: u64 },

    // M1: plugin-emitted custom events (anti-spoofing enforced at publish time)
    Custom {
        type_id: String,          // must start with emitting_plugin's id (reverse-DNS)
        emitting_plugin: String,  // set by kernel, not by plugin
        payload: serde_json::Value,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    UserRequested,
    HotReload,
    Shutdown,
    CrashRecovery,
}
```

**Design notes:**
- `#[serde(tag = "type")]` emits `{"type": "FileCreated", ...}` for wire format, matching the walking-skeleton smoke test output in M1 spec §11.3.
- All payloads are `Clone`; the bus fans out `Arc<NexusEvent>` to subscribers so cloning is cheap.
- Adding a new variant to `NexusEvent` (M2+) is an explicit PR to `nexus-kernel`. Downstream subscribers will get compile errors from non-exhaustive matches — this is the intended forcing function for cross-phase coordination.

### 3.2 `EventMetadata`

Attached by the kernel to every published event. Not constructed by plugins.

```rust
// nexus-kernel/src/event.rs
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// Metadata attached to every event. Populated by the kernel's PluginContext impl
/// when a plugin calls `ctx.publish(event)` — plugins cannot construct this directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    pub event_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub source_plugin_id: String,
    pub span_id: Option<String>,   // from current tracing::Span, None if no span active
}

/// An event as it flows through the bus: payload + metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishedEvent {
    pub metadata: EventMetadata,
    pub event: NexusEvent,
}
```

**The bus transports `Arc<PublishedEvent>`.** Subscribers receive metadata alongside the event.

### 3.3 `EventFilter`

```rust
// nexus-kernel/src/event.rs
#[derive(Debug, Clone)]
pub enum EventFilter {
    /// Match all events. Intended for debug/tracing use only — produces high traffic.
    All,
    /// Match a single NexusEvent variant by its name (e.g., "FileCreated", "PluginStarted").
    /// Invalid variant names are caught at subscribe time with EventBus::SubscribeError::UnknownVariant.
    Variant(&'static str),
    /// Match NexusEvent::Custom events whose type_id starts with the given prefix.
    CustomPrefix(String),
    /// Match exactly one NexusEvent::Custom type_id.
    CustomExact(String),
}
```

### 3.4 `Capability`

Single source of truth for capability names. M1 set only; future phases add variants here.

```rust
// nexus-kernel/src/capability.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    FsRead,            // "fs.read"
    FsWrite,           // "fs.write"
    FsReadExternal,    // "fs.read.external"   — HIGH
    FsWriteExternal,   // "fs.write.external"  — HIGH

    NetHttp,           // "net.http"           — HIGH
    NetHttpLocalhost,  // "net.http.localhost" — MEDIUM

    ProcessSpawn,      // "process.spawn"      — HIGH

    KvRead,            // "kv.read"
    KvWrite,           // "kv.write"

    IpcCall,           // "ipc.call"           — HIGH

    DbQuery,           // "db.query"           — MEDIUM
    DbWrite,           // "db.write"           — MEDIUM
}

impl Capability {
    /// Canonical string representation. Used in manifest parsing and tracing logs.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Capability::FsRead           => "fs.read",
            Capability::FsWrite          => "fs.write",
            Capability::FsReadExternal   => "fs.read.external",
            Capability::FsWriteExternal  => "fs.write.external",
            Capability::NetHttp          => "net.http",
            Capability::NetHttpLocalhost => "net.http.localhost",
            Capability::ProcessSpawn     => "process.spawn",
            Capability::KvRead           => "kv.read",
            Capability::KvWrite          => "kv.write",
            Capability::IpcCall          => "ipc.call",
            Capability::DbQuery          => "db.query",
            Capability::DbWrite          => "db.write",
        }
    }

    /// Parse from a manifest string. Returns CapabilityError::UnknownString on unknown input.
    pub fn from_str(s: &str) -> Result<Self, CapabilityError> {
        match s {
            "fs.read"            => Ok(Capability::FsRead),
            "fs.write"           => Ok(Capability::FsWrite),
            "fs.read.external"   => Ok(Capability::FsReadExternal),
            "fs.write.external"  => Ok(Capability::FsWriteExternal),
            "net.http"           => Ok(Capability::NetHttp),
            "net.http.localhost" => Ok(Capability::NetHttpLocalhost),
            "process.spawn"      => Ok(Capability::ProcessSpawn),
            "kv.read"            => Ok(Capability::KvRead),
            "kv.write"           => Ok(Capability::KvWrite),
            "ipc.call"           => Ok(Capability::IpcCall),
            "db.query"           => Ok(Capability::DbQuery),
            "db.write"           => Ok(Capability::DbWrite),
            other                => Err(CapabilityError::UnknownString(other.to_string())),
        }
    }

    /// All variants, for exhaustive iteration (e.g., manifest validation reports).
    pub const ALL: &'static [Capability] = &[
        Capability::FsRead,
        Capability::FsWrite,
        Capability::FsReadExternal,
        Capability::FsWriteExternal,
        Capability::NetHttp,
        Capability::NetHttpLocalhost,
        Capability::ProcessSpawn,
        Capability::KvRead,
        Capability::KvWrite,
        Capability::IpcCall,
        Capability::DbQuery,
        Capability::DbWrite,
    ];
}
```

**Risk metadata lives in `nexus-security`**, not here (per M1 spec §4 and §6).

### 3.5 `CapabilitySet`

A plugin's granted capabilities at runtime. Immutable once constructed (capabilities are granted once, at plugin load time, per M1).

```rust
// nexus-kernel/src/capability.rs
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct CapabilitySet {
    set: HashSet<Capability>,
}

impl CapabilitySet {
    pub fn empty() -> Self { Self { set: HashSet::new() } }
    pub fn from_iter(iter: impl IntoIterator<Item = Capability>) -> Self;
    pub fn contains(&self, cap: Capability) -> bool;
    pub fn iter(&self) -> impl Iterator<Item = &Capability>;
}
```

Modification is not part of the public API — `CapabilitySet` is constructed once when the plugin is loaded (by `nexus-plugins` from the parsed manifest) and stored on the plugin's `PluginContext` impl.

### 3.6 `KernelConfig`

```rust
// nexus-kernel/src/config.rs
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct KernelConfig {
    /// Root directory of the forge (workspace).
    pub forge_root: PathBuf,

    /// Event bus ring buffer capacity. Bounded; slow subscribers get Lagged(n)
    /// once they fall more than (capacity) events behind.
    pub event_bus_capacity: usize,   // default 2048

    /// Directories to search for plugin manifests.
    /// Default: [<forge_root>/.nexus/plugins]
    pub plugin_search_paths: Vec<PathBuf>,

    /// Enable hot-reload of plugins when their WASM files change on disk.
    pub hot_reload_enabled: bool,    // default true
}

impl KernelConfig {
    /// Load from <forge_root>/.nexus/config.toml, falling back to defaults for
    /// any missing fields. Returns ConfigError if the file exists but is malformed.
    pub fn load(forge_root: &std::path::Path) -> Result<Self, ConfigError>;

    /// Programmatic construction for tests. Uses defaults for everything except forge_root.
    pub fn for_testing(forge_root: PathBuf) -> Self;
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            forge_root: PathBuf::from("."),
            event_bus_capacity: 2048,
            plugin_search_paths: vec![],
            hot_reload_enabled: true,
        }
    }
}
```

### 3.7 `LogLevel`

Used by `PluginContext::log()`. Matches `tracing::Level` but doesn't leak the `tracing` dependency into plugin API.

```rust
// nexus-kernel/src/log.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}
```

---

## 4. Error Types

Nested per-subsystem sub-enums under a top-level `Error` enum. All public APIs return `Result<T, Error>`; narrow APIs can return narrow sub-enums directly.

Backtraces enabled on `#[source]` fields via `#[backtrace]`. Display format: plain English, lowercase first letter, no trailing period.

### 4.1 Top-level `Error`

```rust
// nexus-kernel/src/error.rs

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Plugin(#[from] PluginError),

    #[error(transparent)]
    Capability(#[from] CapabilityError),

    #[error(transparent)]
    Ipc(#[from] IpcError),

    #[error(transparent)]
    Bus(#[from] BusError),

    #[error(transparent)]
    Kv(#[from] KvError),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
```

### 4.2 `PluginError`

```rust
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("plugin '{plugin_id}' failed to load: {reason}")]
    LoadFailed { plugin_id: String, reason: String },

    #[error("plugin '{plugin_id}' failed to initialize: {reason}")]
    InitFailed { plugin_id: String, reason: String },

    #[error("plugin '{plugin_id}' failed to start: {reason}")]
    StartFailed { plugin_id: String, reason: String },

    #[error("plugin '{plugin_id}' failed to stop: {reason}")]
    StopFailed { plugin_id: String, reason: String },

    #[error("plugin '{plugin_id}' crashed: {reason}")]
    Crashed { plugin_id: String, reason: String },

    #[error("plugin '{plugin_id}' panicked during {phase}")]
    Panicked { plugin_id: String, phase: &'static str },

    #[error("dependency cycle among plugins: {plugins:?}")]
    DependencyCycle { plugins: Vec<String> },

    #[error("plugin '{plugin_id}' missing required dependency '{missing}'")]
    MissingDependency { plugin_id: String, missing: String },

    #[error("plugin '{plugin_id}' dependency '{missing}' version mismatch: required {required}, found {found}")]
    DependencyVersionMismatch { plugin_id: String, missing: String, required: String, found: String },

    #[error("duplicate plugin id '{plugin_id}'")]
    DuplicatePluginId { plugin_id: String },

    #[error("plugin '{plugin_id}' not found")]
    NotFound { plugin_id: String },
}
```

### 4.3 `CapabilityError`

```rust
#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
    #[error("capability '{cap:?}' denied to plugin '{plugin_id}'")]
    Denied { plugin_id: String, cap: Capability },

    #[error("unknown capability string '{0}'")]
    UnknownString(String),
}
```

### 4.4 `IpcError`

```rust
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("target plugin '{plugin_id}' not found")]
    PluginNotFound { plugin_id: String },

    #[error("command '{command}' not found on plugin '{plugin_id}'")]
    CommandNotFound { plugin_id: String, command: String },

    #[error("IPC call to '{plugin_id}'.'{command}' timed out after {timeout_ms}ms")]
    Timeout { plugin_id: String, command: String, timeout_ms: u64 },

    #[error("plugin '{plugin_id}' crashed during IPC call to '{command}'")]
    PluginCrashedDuringCall { plugin_id: String, command: String },

    #[error("IPC argument serialization failed: {reason}")]
    SerializationFailed { reason: String },

    #[error("IPC return value deserialization failed: {reason}")]
    DeserializationFailed { reason: String },
}
```

### 4.5 `BusError`

```rust
#[derive(Debug, thiserror::Error)]
pub enum BusError {
    #[error("event bus is closed")]
    Closed,

    #[error("custom event rejected: type_id '{type_id}' does not start with emitting plugin id '{plugin_id}'")]
    TypeIdNamespaceMismatch { plugin_id: String, type_id: String },

    #[error("plugins cannot publish kernel events; only NexusEvent::Custom is allowed from plugins")]
    PluginPublishingKernelEvent,
}

#[derive(Debug, thiserror::Error)]
pub enum RecvError {
    #[error("subscriber lagged by {0} events (events lost)")]
    Lagged(u64),

    #[error("event bus is closed")]
    Closed,
}
```

### 4.6 `KvError`

```rust
#[derive(Debug, thiserror::Error)]
pub enum KvError {
    #[error("key '{key}' not found")]
    NotFound { key: String },

    #[error("SQLite error in KV store: {0}")]
    Sqlite(#[from] rusqlite::Error),
}
```

### 4.7 `ConfigError`

```rust
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config file not found at '{path}'")]
    NotFound { path: PathBuf },

    #[error("invalid config at '{path}': {reason}")]
    Invalid { path: PathBuf, reason: String },

    #[error("TOML parse error in '{path}': {source}")]
    TomlParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}
```

---

## 5. Public Traits

### 5.1 `PluginLifecycle`

Three-hook contract. Plugins implement this to participate in the kernel's lifecycle.

```rust
// nexus-kernel/src/plugin.rs
use async_trait::async_trait;

#[async_trait]
pub trait PluginLifecycle: Send + Sync {
    /// Called after the plugin is loaded and its manifest parsed, before it starts
    /// receiving events. Use to initialize state, optionally restore from KV.
    async fn on_init(&mut self, ctx: &dyn PluginContext) -> Result<()>;

    /// Called after on_init succeeds. Plugin is now "running" — subscribed events
    /// will be delivered, IPC calls can be received. This is where the plugin's
    /// main work happens (e.g., spawning background tasks, registering handlers).
    async fn on_start(&mut self, ctx: &dyn PluginContext) -> Result<()>;

    /// Called when the kernel is shutting down the plugin (normal shutdown, hot-reload,
    /// or user-requested stop). The plugin should persist any state it wants to survive.
    /// After on_stop returns, the plugin will be dropped.
    async fn on_stop(&mut self, ctx: &dyn PluginContext) -> Result<()>;
}
```

**State machine:**
```
(loaded from disk)
    ↓
Loaded ──on_init──> Initialized ──on_start──> Running
                                                  │
                                                  │ (kernel initiates stop)
                                                  ↓
                                              Stopping ──on_stop──> Stopped
                                                                      ↓
                                                                   (dropped)

Any failure in on_init / on_start / on_stop → state becomes Crashed.
Crashed plugins emit PluginCrashed event and are not retried.
```

### 5.2 `PluginContext`

The full public surface a plugin sees. The trait is what plugins hold; the impl lives in `nexus-kernel::context_impl` and is where capability enforcement happens.

```rust
// nexus-kernel/src/context.rs
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::sync::Arc;

#[async_trait]
pub trait PluginContext: Send + Sync {
    // ---- Identity ----

    /// The plugin's id (reverse-DNS, e.g., "com.example.weather").
    fn plugin_id(&self) -> &str;

    /// The plugin's version string from the manifest.
    fn plugin_version(&self) -> &str;

    /// Check whether this plugin currently holds a capability.
    fn has_capability(&self, cap: Capability) -> bool;

    // ---- File system (gated by fs.* capabilities) ----

    /// Read a file. Path is resolved relative to forge_root for fs.read;
    /// can be any absolute path for fs.read.external.
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>>;

    /// Write a file. Same path semantics as read_file.
    async fn write_file(&self, path: &Path, contents: &[u8]) -> Result<()>;

    /// Delete a file.
    async fn delete_file(&self, path: &Path) -> Result<()>;

    /// List files in a directory (non-recursive). Gated by fs.read.
    async fn list_files(&self, dir: &Path) -> Result<Vec<PathBuf>>;

    // ---- KV store (gated by kv.read / kv.write, per-plugin namespaced) ----

    /// Get a value. Key is plugin-local; the kernel internally namespaces it.
    async fn kv_get(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Set a value. Returns error if kv.write not granted.
    async fn kv_set(&self, key: &str, value: &[u8]) -> Result<()>;

    /// Delete a key. Returns Ok even if key doesn't exist.
    async fn kv_delete(&self, key: &str) -> Result<()>;

    // ---- Events ----

    /// Publish a NexusEvent::Custom. Plugins can only publish Custom events;
    /// publishing a kernel-owned variant returns BusError::PluginPublishingKernelEvent.
    /// The type_id must start with the plugin's id (reverse-DNS namespace);
    /// otherwise returns BusError::TypeIdNamespaceMismatch.
    /// The kernel populates metadata (event_id, timestamp, source_plugin_id, span_id).
    fn publish(&self, type_id: &str, payload: serde_json::Value) -> Result<()>;

    /// Subscribe to events matching the given filter. The subscription is dropped
    /// automatically when it goes out of scope.
    fn subscribe(&self, filter: EventFilter) -> EventSubscription;

    // ---- IPC (gated by ipc.call) ----

    /// Call an IPC command on another plugin. Timeout is required.
    /// Returns IpcError::Timeout if the call takes longer than the given duration.
    /// Returns IpcError::PluginNotFound / CommandNotFound for routing failures.
    async fn ipc_call(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: serde_json::Value,
        timeout: Duration,
    ) -> std::result::Result<serde_json::Value, IpcError>;

    // ---- Logging ----

    /// Emit a log message at the given level. Plumbed through `tracing`
    /// with structured fields including plugin_id.
    fn log(&self, level: LogLevel, message: &str);
}
```

**Capability enforcement pattern** (illustrative impl, not part of the contract — plugins don't see this):

```rust
// nexus-kernel/src/context_impl.rs
impl PluginContext for KernelPluginContext {
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        if !self.capabilities.contains(Capability::FsRead) {
            let cap = Capability::FsRead;
            self.security.audit_denied(&self.plugin_id, cap);
            self.bus.publish_kernel(NexusEvent::CapabilityDenied {
                plugin_id: self.plugin_id.clone(),
                capability: cap,
            })?;
            return Err(CapabilityError::Denied {
                plugin_id: self.plugin_id.clone(),
                cap,
            }.into());
        }
        self.security.audit_used(&self.plugin_id, Capability::FsRead);
        self.storage.read_file(path).await.map_err(Into::into)
    }
    // ...
}
```

The plugin holds only `&dyn PluginContext`; there is no second code path that reads files on the plugin's behalf. Capability checks cannot be bypassed.

---

## 6. Public Structs

### 6.1 `Kernel`

```rust
// nexus-kernel/src/kernel.rs
use std::sync::Arc;

pub struct Kernel {
    // private fields — implementation detail
}

impl Kernel {
    /// Synchronous constructor. Builds the Kernel struct, loads config,
    /// but does NOT start background tasks, discover plugins, or emit events.
    /// Fast and total (only fails on config errors).
    pub fn new(config: KernelConfig) -> Result<Self>;

    /// Start the kernel: spawns the event bus, discovers plugins from disk,
    /// initializes them in topological order, emits PluginLoaded/Started events.
    /// Asynchronous — can fail with PluginError if plugins fail to initialize.
    pub async fn start(&self) -> Result<()>;

    /// Graceful shutdown: stops all plugins in reverse topological order,
    /// drains the event bus, flushes the audit log, closes DB connections.
    /// Idempotent — safe to call twice.
    pub async fn shutdown(&self) -> Result<()>;

    /// Get a handle to the event bus. Used by nexus-cli to install event taps
    /// (e.g., for `nexus logs tail`) without going through a plugin.
    pub fn event_bus(&self) -> Arc<EventBus>;

    /// Access the plugin registry for introspection (e.g., `nexus plugin list`).
    pub fn plugins(&self) -> &PluginRegistry;
}
```

**Expected usage pattern from `nexus-cli`:**

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let forge_root = find_forge_root()?;
    let config = KernelConfig::load(&forge_root)?;
    let kernel = Kernel::new(config)?;
    kernel.start().await?;

    // dispatch subcommand, publish events, query kernel ...

    kernel.shutdown().await?;
    Ok(())
}
```

### 6.2 `EventBus`

```rust
// nexus-kernel/src/event_bus.rs
use std::sync::Arc;

pub struct EventBus {
    // private fields — tokio::sync::broadcast internals
}

impl EventBus {
    /// Publish a kernel-owned event. Only the kernel itself calls this;
    /// plugins publish via PluginContext::publish() which goes through a
    /// different code path that enforces Custom-only semantics.
    pub(crate) fn publish_kernel(&self, event: NexusEvent) -> Result<()>;

    /// Subscribe to events matching the filter. Returns an EventSubscription
    /// handle that auto-unsubscribes on drop.
    pub fn subscribe(&self, filter: EventFilter) -> EventSubscription;
}
```

**Note:** `publish_kernel` is `pub(crate)`, not public. Kernel code publishes events via this method; plugins cannot reach it (they only hold `&dyn PluginContext`).

### 6.3 `EventSubscription`

```rust
// nexus-kernel/src/event_bus.rs
pub struct EventSubscription {
    // private fields — wraps a tokio broadcast::Receiver + filter
}

impl EventSubscription {
    /// Receive the next event matching the filter. Skips non-matching events
    /// internally so the caller only sees matches.
    ///
    /// Errors:
    /// - Err(RecvError::Lagged(n)) — subscriber fell behind; n events lost;
    ///   recoverable (call recv again)
    /// - Err(RecvError::Closed) — bus is shut down; subscription is dead
    pub async fn recv(&mut self) -> std::result::Result<Arc<PublishedEvent>, RecvError>;

    /// Try to receive without blocking. Returns Ok(None) if no events available.
    pub fn try_recv(&mut self) -> std::result::Result<Option<Arc<PublishedEvent>>, RecvError>;
}

// Dropping EventSubscription auto-unsubscribes (broadcast::Receiver drop semantics).
```

### 6.4 `PluginRegistry`

Read-only view of loaded plugins, exposed to `nexus-cli` for introspection commands.

```rust
// nexus-kernel/src/plugin_registry.rs
pub struct PluginRegistry {
    // private fields
}

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub trust_level: TrustLevel,
    pub status: PluginStatus,
    pub capabilities: CapabilitySet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustLevel {
    Core,
    Community,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginStatus {
    Loaded,
    Initialized,
    Running,
    Stopped,
    Crashed,
}

impl PluginRegistry {
    /// List all currently loaded plugins, in topological order.
    pub fn list(&self) -> Vec<PluginInfo>;

    /// Get info for a specific plugin.
    pub fn get(&self, plugin_id: &str) -> Option<PluginInfo>;

    /// Count plugins by status.
    pub fn count_by_status(&self) -> std::collections::HashMap<PluginStatus, usize>;
}
```

---

## 7. KV Store Semantics

- **API:** three methods only — `kv_get`, `kv_set`, `kv_delete` (see §5.2).
- **Values:** opaque `Vec<u8>`. Serialization is the plugin's concern.
- **Keys:** plugin-local strings from the plugin's perspective. The kernel namespaces internally so plugins' keys never collide.
- **Namespacing (implementation detail):** each plugin gets its own SQLite table or key prefix — plugins cannot see another plugin's keys. This is not observable through the API.
- **Concurrent access:** `kv_set` from a plugin is atomic (single-row update). There is no compare-and-swap, no atomic read-modify-write. Plugins with concurrent write needs must serialize in their own code.
- **Size limits:** no enforced size limit in M1. Plugins are trusted to not store unreasonable values. If a real need surfaces, add a `KvError::ValueTooLarge` variant and a `max_value_bytes` field to `KernelConfig`.
- **Persistence:** KV writes are flushed to SQLite synchronously before `kv_set` returns. Crash-safe.
- **No iteration:** listed as a post-M1 follow-up. Plugins that need to iterate maintain an index under a well-known key (e.g., `__index__`).

---

## 8. IPC Call Semantics

- **Routing:** by `(target_plugin_id, command_id)`. The command_id must be registered in the target plugin's manifest.
- **Timeout:** required parameter on `ipc_call`. The caller must choose a duration appropriate to the operation. No global default.
- **Timeout enforcement:** internally via `tokio::time::timeout`. On timeout, `IpcError::Timeout` is returned; the target plugin is not forcibly killed.
- **Argument serialization:** `serde_json::Value` on the wire. Shared types live in `nexus-types`. Serialization failures return `IpcError::SerializationFailed`.
- **Plugin crashes during IPC:** if the target plugin panics / traps mid-call, the kernel catches the trap and returns `IpcError::PluginCrashedDuringCall`. The kernel also emits a `PluginCrashed` event and transitions the target plugin to `Crashed` status.
- **Self-calls:** a plugin calling itself via IPC is allowed — it goes through the same dispatch path.
- **No queuing across reloads:** if the target plugin is hot-reloading at the time of call, the call returns `IpcError::PluginNotFound` (the plugin is temporarily unavailable during the swap). The caller can retry with backoff.

---

## 9. Manifest Dependency Format

Declared in the plugin's `manifest.toml`:

```toml
[plugin]
id = "com.example.weather-dashboard"
name = "Weather Dashboard"
version = "0.1.0"

[plugin.dependencies]
required = [
    "com.example.weather@>=0.1.0",
    "com.example.http-client@^1.0.0",
]
optional = [
    "com.example.notifications@*",
]
```

- **`required`:** dependencies that must be loaded before this plugin; loading fails with `PluginError::MissingDependency` or `DependencyVersionMismatch` if unmet.
- **`optional`:** dependencies that are tried but not required. Loading continues without them.
- **Version constraints:** Cargo semver syntax (`^`, `~`, `>=`, `<=`, `*`, exact version).
- **Topological sort** uses `required` only for ordering. `optional` deps are best-effort and don't affect load order.
- **Plugin availability at runtime:** a plugin can query `ctx.plugin_available("com.example.notifications")` (not in M1 contract — added to `PluginContext` in v0.2 if needed). For M1, optional deps are effectively "load the dep if it's around; the dependent plugin is responsible for not breaking if it isn't."

---

## 10. Topological Sort and Cycle Behavior

- **Algorithm:** Kahn's algorithm, per PRD 01 §4.3.
- **Cycle detection:** if a cycle is found, all plugins **in the cycle** fail to load with `PluginError::DependencyCycle { plugins: [...] }`.
- **Cycle transitive impact:** plugins whose required deps are in a cycle (but who are not themselves in a cycle) fail to load with `PluginError::MissingDependency`.
- **Independent plugins continue to load.** A cycle between plugins A and B does not prevent plugin D (independent) from loading.
- **Each failed plugin emits `NexusEvent::PluginCrashed`** with `reason` describing the failure cause. The kernel logs each at `error` level.
- **No automatic resolution:** the kernel does not attempt to break cycles or retry failed plugins. The user must fix the cycle in manifests and reload.
- **Failure during topological sort is not fatal to kernel startup.** `Kernel::start()` returns `Ok(())` if at least the non-cyclic plugins loaded successfully; the cyclic ones surface as `PluginCrashed` events.

---

## 11. What the Contract Does NOT Include (Non-Public)

Listed so agents implementing `nexus-kernel` know what they have freedom to change:

- **Internal module structure** of `nexus-kernel` (e.g., `src/event_bus/ring.rs` vs `src/event_bus.rs` — doesn't matter).
- **KV store SQLite schema** — one table per plugin, one table with a plugin_id column, JSON blob column, etc.
- **Plugin discovery algorithm** on disk (glob pattern, manifest walker, etc.).
- **Hot-reload detection mechanism** (notify watcher on plugin directory vs. explicit API call).
- **Wasmtime instance lifecycle** (per-plugin instance, reuse, etc.) — that's `nexus-plugins`' concern.
- **Exact tracing span names and structured field names** beyond what's in §10.1 of the M1 spec.
- **Thread model** for the event bus (single task, multi-task, priority lanes — all private).

Anything not listed in this doc is implementation detail and can be changed without a contract amendment.

---

## 12. Acceptance Criteria for This Contract

The `nexus-kernel` crate is considered "interface complete" when:

1. **Every public type, trait, struct, and function in sections 3–6 is defined and compiles.** Method bodies may be `todo!()` placeholders initially; interface completeness is about shape, not implementation.
2. **Doc comments** are present on every public item, describing purpose, panics (if any), and error cases. Run `cargo doc --no-deps` and verify no missing-docs warnings.
3. **All error variants in §4 exist.** Tests may not exercise all of them, but they must be constructible from code that would legitimately produce them.
4. **`cargo check --all-targets`** passes with no warnings.
5. **A minimal "smoke" test** exists: `Kernel::new(config).and_then(|k| k.start())` can be called without panicking, even if no plugins are present (empty plugin directory). `kernel.shutdown()` also returns successfully.

**These are interface acceptance criteria, not full M1 acceptance criteria** — the M1 spec §11 has the cross-PRD integration tests that verify the contract actually works end-to-end. This doc's acceptance is narrower: does the public surface exist and compile?

---

## 13. Open Follow-ups

These are known items that surface from the PRD 01 work but are not blockers for M1:

- **PRD 01 cleanup pass.** The raw `PRDs/01-kernel-event-system.md` still contains the pre-amendment shapes (enum capabilities, 5-hook lifecycle, 7-state machine, runtime IPC registration, 40+ event variants). A ~30-minute pass at the end of PRD 01 implementation work should add "**Amended by PRD 01 interface spec 2026-04-11**" notes in each affected section so future readers aren't confused. Logged as separate follow-up.
- **`correlation_id` / `severity` in event metadata.** Deferred — add in v0.2 when a driving use case exists (likely M4's AI engine correlating async events across tool calls).
- **`kv_list_prefix` and iteration API.** Deferred — add in v0.2 if M1 dogfood experience makes index-maintenance painful.
- **Runtime IPC registration** (`ctx.register_ipc_command(...)`). Deferred — v0.2 if M4/M5 agents need to register handlers dynamically.
- **`on_load` / `on_shutdown` lifecycle hooks.** Deferred — v0.2 if a real use case surfaces.
- **Extended `PluginContext` methods** (`watch_directory`, `query_plugin`, `index_query`, `index_insert`). Deferred — added when the PRDs that need them (M2+, M3+) reach their phases.
- **Plugin availability query** (`ctx.plugin_available(...)`). Deferred — needed for optional-dep runtime checks, but M1 plugins can work around it.
- **Event history / replay** (`EventHistory` trait). Deferred — kernel-internal for debug commands only in M1.
- **Capability string validation at manifest parse time.** Implemented as part of `nexus-plugins` manifest parser; this contract provides `Capability::from_str()` which that parser calls.

---

## 14. Next Step

With this contract approved, the next step is to invoke the `superpowers:writing-plans` skill to produce the implementation plan for PRD 01. The plan will:

1. Start with ADRs (the 10 from M1 spec §10.4 — particularly the ones affecting `nexus-kernel`: workspace layout, capability strings, event taxonomy, error shape).
2. Build up `nexus-types` crate first (since it's the leaf of the leaf) with all the shared types that `nexus-kernel` will re-export or consume.
3. Build `nexus-kernel` module by module in TDD order: types → errors → event bus → capability set → plugin trait → kernel struct → registry → context impl.
4. End with the smoke test in §12 passing.
5. Leave `todo!()` stubs where a method's behavior depends on `nexus-security`, `nexus-storage`, or `nexus-plugins` — those fill in when their respective PRDs land.

---

**End of PRD 01 interface spec. Approval gate: user reviews and signs off, then we proceed to writing-plans.**
