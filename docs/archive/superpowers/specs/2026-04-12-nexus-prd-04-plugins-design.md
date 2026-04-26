# Nexus PRD 04 — Plugin System (M1-Slimmed) Design Spec

**Version:** 1.0
**Date:** 2026-04-12
**Status:** Approved (brainstorming session output)
**Scope:** M1-scoped design for `nexus-plugins` — manifest parsing, wasmtime WASM sandbox, host functions (logging, events, KV), plugin loader with lifecycle management, JSON Schema settings validation, and hot-reload. Templates (PRD 04a) are a separate spec.

**Parent docs:**
- [`PRDs/04-plugin-system.md`](../../../PRDs/04-plugin-system.md) — full PRD (this spec implements the M1 slice)
- [`PRDs/04a-plugin-templates.md`](../../../PRDs/04a-plugin-templates.md) — plugin templates (separate spec)
- [`2026-04-11-nexus-m1-foundation-spec.md`](2026-04-11-nexus-m1-foundation-spec.md) — M1 spec §6 (Plugin Architecture), §9 (Security)
- [`2026-04-11-nexus-roadmap-design.md`](2026-04-11-nexus-roadmap-design.md) — roadmap §3 (PRD 04 cuts)

---

## 1. Architecture Overview

Single `nexus-plugins` workspace member crate, following the one-crate-per-PRD pattern. All subsystems are internal modules behind a `PluginManager` facade struct.

Dependencies:
- `nexus-kernel` — for `PluginLifecycle`, `PluginContext`, `PluginInfo`, `PluginStatus`, `TrustLevel`, `Capability`, `CapabilitySet`, `EventBus`, `EventFilter`, `EventSubscription`, `NexusEvent`
- `nexus-security` — for `risk_level()`, audit functions
- `nexus-types` — shared types

The crate introduces `wasmtime` as the WASM runtime (per M1 spec §4 tech stack decision). Host functions bridge WASM plugins to kernel services.

---

## 2. Crate Structure

```
crates/nexus-plugins/
├── Cargo.toml
└── src/
    ├── lib.rs              # public re-exports, PluginManager facade
    ├── error.rs            # PluginError enum
    ├── manifest.rs         # manifest parsing + validation
    ├── sandbox.rs          # WasmSandbox: wasmtime engine, module, store
    ├── host_fns.rs         # host functions exposed to WASM (log, events, KV)
    ├── loader.rs           # PluginLoader: scan dirs, load, instantiate, lifecycle
    ├── settings.rs         # JSON Schema validation, per-plugin settings storage
    └── hot_reload.rs       # file watcher on plugin dirs, reload lifecycle
```

---

## 3. Dependencies

New workspace dependencies (added to root `Cargo.toml`):

| Crate | Version | Purpose |
|---|---|---|
| `wasmtime` | 18.0+ | WASM runtime (cranelift backend) |
| `jsonschema` | 0.17+ | JSON Schema v7 validation for plugin settings |
| `semver` | 1.0+ | Semver parsing and validation for plugin versions |

`nexus-plugins` depends on `nexus-kernel` and `nexus-security`. It does **not** depend on `nexus-storage` — plugins access storage through `PluginContext` host functions, not by importing the storage crate directly.

---

## 4. PluginError Enum

```rust
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("manifest not found: {0}")]
    ManifestNotFound(String),

    #[error("manifest invalid at {path}: {reason}")]
    ManifestInvalid { path: String, reason: String },

    #[error("manifest validation failed for {plugin_id}: {reason}")]
    ManifestValidation { plugin_id: String, reason: String },

    #[error("WASM load failed for {plugin_id}: {reason}")]
    WasmLoadFailed { plugin_id: String, reason: String },

    #[error("execution timeout for {plugin_id}")]
    ExecutionTimeout { plugin_id: String },

    #[error("execution failed for {plugin_id}: {reason}")]
    ExecutionFailed { plugin_id: String, reason: String },

    #[error("lifecycle error for {plugin_id} in {hook}: {reason}")]
    LifecycleError { plugin_id: String, hook: String, reason: String },

    #[error("capability denied for {plugin_id}: {capability}")]
    CapabilityDenied { plugin_id: String, capability: String },

    #[error("plugin not found: {0}")]
    PluginNotFound(String),

    #[error("duplicate plugin: {0}")]
    DuplicatePlugin(String),

    #[error("duplicate CLI subcommand '{subcommand}' from {plugin_id}")]
    DuplicateCliSubcommand { plugin_id: String, subcommand: String },

    #[error("settings invalid for {plugin_id}: {reason}")]
    SettingsInvalid { plugin_id: String, reason: String },

    #[error("reload failed for {plugin_id}: {reason}")]
    ReloadFailed { plugin_id: String, reason: String },

    #[error("plugin reloading: {0}")]
    PluginReloading(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
```

---

## 5. Manifest Parsing & Validation

### 5.1 Manifest format

Each plugin lives in its own directory under `.forge/plugins/<plugin-id>/` with a `manifest.toml`:

```toml
[plugin]
id = "com.example.weather"
name = "Weather"
version = "0.1.0"
trust_level = "community"        # "core" or "community"
api_version = "1"

[capabilities]
required = ["fs.read", "kv.read", "kv.write"]
optional = ["net.http"]

[wasm]
module = "weather.wasm"          # relative to plugin dir
memory_mb = 16                   # 1-256
fuel = 10000000                  # wasmtime fuel; 0 = unlimited (core only)

[settings]
schema = "settings.json"         # optional; JSON Schema v7 file in plugin dir

[[registrations.cli_subcommand]]
id = "weather.forecast"
handler_id = 1
description = "Get weather forecast"

[[registrations.ipc_command]]
id = "weather.get-current"
handler_id = 100

[[registrations.event_subscriber]]
id = "weather.on-file-created"
filter = "FileCreated"           # matches EventFilter::Variant
handler_id = 200

[lifecycle]
on_init = true
on_start = true
on_stop = true
```

### 5.2 Parsed types

```rust
#[derive(Debug, Clone)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub trust_level: TrustLevel,
    pub api_version: String,
    pub capabilities: ManifestCapabilities,
    pub wasm: WasmConfig,
    pub settings: Option<SettingsConfig>,
    pub registrations: Registrations,
    pub lifecycle: LifecycleConfig,
}

#[derive(Debug, Clone)]
pub struct ManifestCapabilities {
    pub required: Vec<String>,
    pub optional: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WasmConfig {
    pub module: String,
    pub memory_mb: u32,
    pub fuel: u64,
}

#[derive(Debug, Clone)]
pub struct SettingsConfig {
    pub schema: String,
}

#[derive(Debug, Clone, Default)]
pub struct Registrations {
    pub cli_subcommands: Vec<CliSubcommandReg>,
    pub ipc_commands: Vec<IpcCommandReg>,
    pub event_subscribers: Vec<EventSubscriberReg>,
}

#[derive(Debug, Clone)]
pub struct CliSubcommandReg {
    pub id: String,
    pub handler_id: u32,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct IpcCommandReg {
    pub id: String,
    pub handler_id: u32,
}

#[derive(Debug, Clone)]
pub struct EventSubscriberReg {
    pub id: String,
    pub filter: String,
    pub handler_id: u32,
}

#[derive(Debug, Clone)]
pub struct LifecycleConfig {
    pub on_init: bool,
    pub on_start: bool,
    pub on_stop: bool,
}
```

### 5.3 Validation rules

Enforced by `validate(manifest, plugin_dir) -> Result<(), PluginError>`:

1. Plugin ID matches regex `^[a-z0-9]+([-._][a-z0-9]+)*\.[a-z0-9]+([-._][a-z0-9]+)*$`
2. Version is valid semver (via `semver::Version::parse`)
3. All capability strings in `required` and `optional` exist in the `Capability` enum (via `Capability::from_str`)
4. All `handler_id` values are unique across all registrations within the manifest
5. `wasm.memory_mb` is in `[1, 256]`
6. `wasm.fuel` > 0 unless `trust_level = "core"` (core may use 0 for unlimited)
7. `wasm.module` file exists in the plugin directory
8. If `settings.schema` is set, the referenced file must exist and parse as valid JSON

---

## 6. WASM Sandbox

### 6.1 WasmSandbox struct

```rust
pub struct WasmSandbox {
    engine: wasmtime::Engine,
    store: wasmtime::Store<PluginData>,
    instance: wasmtime::Instance,
}
```

### 6.2 PluginData (store context)

Per-plugin state accessible from host functions via the wasmtime `Store`:

```rust
pub struct PluginData {
    pub plugin_id: String,
    pub capabilities: CapabilitySet,
    pub memory: Option<wasmtime::Memory>,
    pub event_bus: Arc<EventBus>,
    pub kv_store: Arc<dyn KvStore>,
}
```

### 6.3 Sandbox creation

```rust
impl WasmSandbox {
    pub fn new(
        wasm_bytes: &[u8],
        config: &WasmConfig,
        plugin_data: PluginData,
    ) -> Result<Self, PluginError>;
}
```

Steps:
1. Create `wasmtime::Engine` with config: `wasm_simd(true)`, `wasm_bulk_memory(true)`, `consume_fuel(true)` if fuel > 0
2. Compile `wasmtime::Module` from bytes
3. Create `wasmtime::Store` with `PluginData`
4. Add fuel to store: `store.set_fuel(config.fuel)` if fuel > 0
5. Create `wasmtime::Linker`, define host functions (log, events, KV) via `host_fns` module
6. Set memory limits via `MemoryType::new(initial_pages, Some(max_pages))` where max pages = `memory_mb * 16` (64KB per page)
7. Instantiate module via linker

### 6.4 Calling convention

The plugin exports `nexus_dispatch(handler_id: u32, args_ptr: u32, args_len: u32) -> u64`.

```rust
impl WasmSandbox {
    /// Call a handler in the WASM plugin.
    pub fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError>;
}
```

Steps:
1. Serialize args to JSON bytes
2. Call plugin's `nexus_alloc(len)` export to allocate space in WASM linear memory
3. Copy JSON bytes into WASM memory at the allocated pointer
4. Call `nexus_dispatch(handler_id, ptr, len)`
5. Unpack return value: `result_ptr = (ret >> 32)`, `result_len = (ret & 0xFFFF_FFFF)`
6. Read result JSON from WASM memory
7. Fuel exhaustion maps to `PluginError::ExecutionTimeout`

### 6.5 Lifecycle methods

```rust
impl WasmSandbox {
    pub fn call_on_init(&mut self) -> Result<(), PluginError>;
    pub fn call_on_start(&mut self) -> Result<(), PluginError>;
    pub fn call_on_stop(&mut self) -> Result<(), PluginError>;
}
```

Well-known handler IDs for lifecycle: 0 = init, 1 = start, 2 = stop. Dispatched via the same `dispatch` mechanism with empty args `{}`.

---

## 7. Host Functions

M1 minimal set: logging, events, and KV. All linked into the wasmtime `Linker`.

### 7.1 host_log

```
fn host_log(level: i32, message_ptr: i32, message_len: i32) -> i32
```

- Read message string from WASM linear memory
- Map level: 0=debug, 1=info, 2=warn, 3=error
- Emit via `tracing` with `plugin_id` structured field
- Returns 0 on success, -1 on invalid level
- No capability gating — logging is always allowed

### 7.2 host_publish_event

```
fn host_publish_event(type_id_ptr: i32, type_id_len: i32, payload_ptr: i32, payload_len: i32) -> i32
```

- Read `type_id` and `payload` JSON from WASM memory
- Validate `type_id` starts with the plugin's ID (namespace enforcement)
- Publish `NexusEvent::Custom` via the event bus
- Returns 0 on success, -1001 on namespace violation
- No explicit capability gate — namespace enforcement is the safety mechanism

### 7.3 host_kv_get

```
fn host_kv_get(key_ptr: i32, key_len: i32, result_ptr: i32, result_capacity: i32) -> i32
```

- Check `kv.read` capability → -1001 if denied
- Read key from WASM memory, look up in KV store (namespaced by plugin ID)
- Write value bytes into WASM memory at `result_ptr`
- Returns bytes written on success, -1 if key not found, -1002 if buffer overflow

### 7.4 host_kv_set

```
fn host_kv_set(key_ptr: i32, key_len: i32, value_ptr: i32, value_len: i32) -> i32
```

- Check `kv.write` capability → -1001 if denied
- Read key and value from WASM memory, store in KV (namespaced by plugin ID)
- Returns 0 on success

### 7.5 host_kv_delete

```
fn host_kv_delete(key_ptr: i32, key_len: i32) -> i32
```

- Check `kv.write` capability → -1001 if denied
- Delete key from KV store
- Returns 0 on success

### 7.6 Error codes

| Code | Meaning |
|---|---|
| 0 | Success |
| -1 | General error (invalid arguments, unknown level) |
| -1001 | Capability denied |
| -1002 | Buffer overflow (result too large for provided buffer) |

---

## 8. Plugin Loader

### 8.1 PluginLoader struct

```rust
pub struct PluginLoader {
    plugins_dir: PathBuf,
    loaded: HashMap<String, LoadedPlugin>,
}

struct LoadedPlugin {
    manifest: PluginManifest,
    sandbox: WasmSandbox,
    status: PluginStatus,
    plugin_dir: PathBuf,
    registrations: PluginRegistrations,
}

struct PluginRegistrations {
    cli_subcommands: Vec<String>,
    ipc_commands: Vec<String>,
    event_subscriptions: Vec<String>,
}
```

### 8.2 Plugin directory layout

```
<forge>/.forge/plugins/
├── com.example.weather/
│   ├── manifest.toml
│   ├── weather.wasm
│   └── settings.json
├── com.example.analyzer/
│   ├── manifest.toml
│   └── analyzer.wasm
```

### 8.3 Discovery

`PluginLoader::scan()` walks `.forge/plugins/`, finds subdirectories containing `manifest.toml`.

### 8.4 Load sequence

`PluginLoader::load(plugin_dir)`:

1. Parse `manifest.toml` → `PluginManifest`
2. Validate manifest (ID, capabilities, handler IDs, memory, WASM file exists)
3. Check for duplicate plugin ID → `PluginError::DuplicatePlugin`
4. If settings schema declared, load and register with `SettingsManager`
5. Read WASM bytes from `manifest.wasm.module` path
6. Build `PluginData` with capabilities (core gets all, community gets declared set)
7. Create `WasmSandbox` from WASM bytes + config
8. Call `sandbox.call_on_init()` if `lifecycle.on_init` is true
9. Call `sandbox.call_on_start()` if `lifecycle.on_start` is true
10. Register CLI subcommands, IPC commands, event subscribers (reject duplicates)
11. Set status to `Running`

### 8.5 Unload sequence

`PluginLoader::unload(plugin_id)`:

1. Call `sandbox.call_on_stop()` if `lifecycle.on_stop` is true (5-second timeout)
2. Drop the `WasmSandbox` (releases wasmtime resources)
3. Remove from `loaded` map
4. Deregister all CLI subcommands, IPC commands, event subscribers

---

## 9. Settings Infrastructure

### 9.1 SettingsManager

```rust
pub struct SettingsManager {
    schemas: HashMap<String, serde_json::Value>,
}

impl SettingsManager {
    /// Load and store a JSON Schema for a plugin.
    pub fn register_schema(
        &mut self,
        plugin_id: &str,
        schema_json: &str,
    ) -> Result<(), PluginError>;

    /// Validate settings against the registered schema.
    pub fn validate(
        &self,
        plugin_id: &str,
        settings: &serde_json::Value,
    ) -> Result<(), PluginError>;

    /// Load settings from disk, validate against schema.
    pub fn load_settings(
        &self,
        plugin_id: &str,
        plugin_dir: &Path,
    ) -> Result<serde_json::Value, PluginError>;

    /// Save settings to disk after validation.
    pub fn save_settings(
        &self,
        plugin_id: &str,
        plugin_dir: &Path,
        settings: &serde_json::Value,
    ) -> Result<(), PluginError>;
}
```

### 9.2 Settings storage

Plugin settings are stored at `.forge/plugins/<plugin-id>/settings.json`. This is separate from the KV store — settings are structured JSON validated against a schema, while KV is opaque bytes.

If no settings file exists on disk, an empty object `{}` is used. Schemas should define defaults or use all-optional properties so empty settings validate.

Validation uses the `jsonschema` crate (`jsonschema::validator_for(&schema)?.validate(&settings)`).

---

## 10. Hot-Reload

### 10.1 HotReloader struct

```rust
pub struct HotReloader {
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
    rx: mpsc::Receiver<ReloadEvent>,
}

pub struct ReloadEvent {
    pub plugin_id: String,
    pub wasm_path: PathBuf,
}
```

### 10.2 Watch setup

`HotReloader::start(plugins_dir, debounce_ms)`:
- Watches `.forge/plugins/*/` recursively
- Filters for `.wasm` file changes only
- Maps changed paths back to plugin IDs via directory name
- Debounce at 500ms (longer than storage watcher — WASM builds take time)

### 10.3 Reload sequence

When the `PluginManager` receives a `ReloadEvent`:

1. Look up plugin by ID → must be in `Running` status
2. Call `sandbox.call_on_stop()` with 5-second timeout
3. Drop the old `WasmSandbox`
4. Read new WASM bytes from disk
5. Create new `WasmSandbox`
6. Call `sandbox.call_on_init()` → `sandbox.call_on_start()`
7. On success: status stays `Running`, log reload at `info` level
8. On failure: status → `Crashed`, log error, emit `NexusEvent::PluginCrashed`

### 10.4 Message queueing during reload

Between steps 2 and 6, the plugin is temporarily unavailable:
- IPC calls to the plugin are queued in a bounded channel (256 messages)
- Event deliveries to the plugin's subscribers are queued
- If the queue fills, new calls return `PluginError::PluginReloading`
- After reload completes, queued messages are drained and delivered

### 10.5 Configuration

Hot-reload is enabled by default. Disabled via `nexus.toml`:
```toml
[plugins]
hot_reload = false
```

---

## 11. Public API Surface

### 11.1 PluginManager

```rust
pub struct PluginManager { /* private */ }

impl PluginManager {
    pub fn new(
        plugins_dir: &Path,
        event_bus: Arc<EventBus>,
        kv_store: Arc<dyn KvStore>,
        hot_reload: bool,
    ) -> Result<Self, PluginError>;

    pub fn load_all(&mut self) -> Result<Vec<PluginInfo>, PluginError>;
    pub fn load(&mut self, plugin_dir: &Path) -> Result<PluginInfo, PluginError>;
    pub fn unload(&mut self, plugin_id: &str) -> Result<(), PluginError>;

    pub fn list(&self) -> Vec<PluginInfo>;
    pub fn get(&self, plugin_id: &str) -> Option<PluginInfo>;

    pub fn dispatch_cli(
        &mut self, subcommand: &str, args: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError>;

    pub fn dispatch_ipc(
        &mut self, plugin_id: &str, command_id: &str, args: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError>;

    pub fn get_settings(&self, plugin_id: &str) -> Result<serde_json::Value, PluginError>;
    pub fn set_settings(
        &mut self, plugin_id: &str, settings: serde_json::Value,
    ) -> Result<(), PluginError>;

    pub fn poll_reloads(&mut self) -> Result<Vec<String>, PluginError>;
    pub fn shutdown(&mut self) -> Result<(), PluginError>;
}
```

### 11.2 KvStore trait

```rust
pub trait KvStore: Send + Sync {
    fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, PluginError>;
    fn set(&self, namespace: &str, key: &str, value: &[u8]) -> Result<(), PluginError>;
    fn delete(&self, namespace: &str, key: &str) -> Result<(), PluginError>;
}
```

The namespace is the plugin ID. Plugins cannot access each other's KV data. The kernel provides the concrete implementation (backed by the forge's SQLite or a dedicated KV file).

### 11.3 Design decisions

- **Synchronous dispatch, async lifecycle.** The `dispatch` call into WASM is synchronous (wasmtime calls are blocking). Lifecycle hooks (`on_init`, `on_start`, `on_stop`) are also synchronous from the host side — the WASM plugin runs to completion within its fuel budget.
- **PluginManager is not thread-safe.** It holds mutable `WasmSandbox` instances. The CLI (single-threaded) owns the `PluginManager`. If needed later, a `Mutex<PluginManager>` wrapper is straightforward.
- **Capability enforcement in host functions.** Each host function checks capabilities via `PluginData.capabilities.contains()` before performing the operation. This is the enforcement boundary — the plugin physically cannot bypass it.

---

## 12. Key Data Types

```rust
pub struct ReloadEvent {
    pub plugin_id: String,
    pub wasm_path: PathBuf,
}

pub struct PluginManagerConfig {
    pub hot_reload: bool,
    pub debounce_ms: u64,       // default: 500
}

impl Default for PluginManagerConfig {
    fn default() -> Self {
        Self {
            hot_reload: true,
            debounce_ms: 500,
        }
    }
}
```

---

## 13. Deferred from M1

| Item | PRD Section | Rationale | Revisit |
|---|---|---|---|
| Plugin templates (PRD 04a) | 04a entire | Separate spec+plan cycle | PRD 04a |
| File I/O host functions | §3.2 | Needs storage wired through kernel (PRD 05) | PRD 05 |
| SQLite query host functions | §3.2 | Database engine is M3 (PRD 10) | PRD 10 |
| IPC host functions | §3.2 | No consumer in M1 | M2+ |
| Plugin marketplace / registry | §8-10 | Cut per roadmap | v0.2+ |
| Plugin discovery UI | §11 | Cut per roadmap | v0.2+ |
| Plugin code signing | §12 | Cut per roadmap; trust_level is declared, not verified | v0.2 |
| Plugin dependency resolution | §1.1 | Manifest field parsed but not enforced | M2+ |
| `nexus plugin scaffold` CLI | 04a §8 | Part of PRD 04a | PRD 04a |
| Settings UI rendering | §4 | Needs GUI (M2); schema validation and storage exist in M1 | PRD 07/08 |
