# PRD: Plugin System (Nexus v1.0)

**Subsystem:** Plugin System (Core Architecture)  
**Version:** 1.0  
**Date:** April 2026  
**Status:** Implementation-Ready  
**Author:** Nexus Architecture Team

---

## Executive Summary

The Plugin System is the architectural heart of Nexus, enabling a microkernel where nearly all functionality is pluggable. This PRD covers the complete plugin lifecycle: manifest specification, WASM sandboxing, capability enforcement, dynamic loading, and developer/user workflows. Two-tier architecture separates **Core Plugins** (native Rust, unrestricted kernel access) from **Community Plugins** (WASM-sandboxed, capability-gated). The system prioritizes security, performance isolation, and a frictionless developer experience.

---

## 1. Plugin Manifest Format

### 1.1 Specification

All plugins declare a manifest file (`plugin.toml` for core, `manifest.toml` for community).

```toml
# plugin.toml / manifest.toml

[plugin]
id = "dev.nexus.example-analyzer"  # Reverse-domain notation, immutable
name = "Example Analyzer"
version = "1.2.0"  # semver only
author = "Jane Doe"
email = "jane@example.com"
description = "Analyzes code examples in markdown."
license = "MIT"  # SPDX identifier
repository = "https://github.com/nexus-plugins/example-analyzer"
homepage = "https://example-analyzer.dev"
documentation = "https://docs.example-analyzer.dev"
min_nexus_version = "1.0.0"
max_nexus_version = "2.0.0"  # Optional; if absent, no upper bound

[plugin.icons]
light = "assets/icon-light.svg"  # 64x64 recommended
dark = "assets/icon-dark.svg"

[plugin.keywords]
keywords = ["analysis", "code", "markdown"]

# CORE PLUGINS ONLY: Stability & feature flags
[core]
stability = "stable"  # stable | experimental | deprecated
feature_gate = "plugin_example_analyzer"  # Optional: Rust feature flag in app

# COMMUNITY PLUGINS ONLY: Runtime & capability requirements
[community]
runtime = "wasm32-unknown-unknown"  # Currently only target
wasm_memory_mb = 64  # Max linear memory (64-512 MB)
cpu_timeout_ms = 5000  # Max execution time per host call

# Capabilities required by this plugin
# User must approve these during install (community) or enable (core)
[capabilities]
required = [
  "filesystem.read",
  "filesystem.write",
  "sqlite.query",
  "cli.subcommand"
]
optional = [
  "editor.workspace_open_event",
  "network.http"
]

# Registrations: what this plugin provides to Nexus
# Each registration ties to a capability
[registrations.commands]
# IPC commands (available to UI, other plugins)
add_ipc_command = [
  { id = "analyze:run", label = "Run Analysis", capability = "cli.subcommand" },
  { id = "analyze:settings", label = "Open Settings", capability = "" }
]

[registrations.cli_subcommands]
# CLI subcommands: `nexus analyze <args>`
add_cli_subcommand = [
  { id = "analyze", label = "Code Analysis Tool", capability = "cli.subcommand" }
]

[registrations.surfaces]
# Views/panels in the UI
add_surface = [
  { id = "analyzer:results", label = "Analysis Results", type = "panel", capability = "ui.surface" }
]

[registrations.editor_extensions]
# Extensions to the built-in markdown/code editor
add_extension = [
  { id = "analyzer:linter", label = "Live Analysis", type = "linter", capability = "editor.lint" }
]

[registrations.markdown_processors]
# Custom markdown block processors
add_processor = [
  { id = "analyze-block", syntax = "analyze", capability = "markdown.processor" }
]

[registrations.protocol_handlers]
# URL scheme handlers: nexus://analyze/...
add_handler = [
  { scheme = "nexus", path = "/analyze", capability = "protocol.handle" }
]

[registrations.palette_commands]
# Commands in the command palette
add_palette_command = [
  { id = "analyze:run", label = "Analyze: Run", category = "Analysis" }
]

[registrations.settings_ui]
# Plugin contributes settings UI for its own configuration
has_settings_ui = true  # Plugin provides a Settings surface
settings_schema = "settings.json"  # JSON Schema file reference

[registrations.menu_items]
# Contributes to File, Edit, View, etc. menus
add_menu_item = [
  { id = "file:analyze", label = "Analyze", menu = "File", index = 3 }
]

[registrations.status_bar_items]
# Items in bottom status bar
add_item = [
  { id = "analyzer:status", label = "", position = "right" }
]

[registrations.sqlite_tables]
# Tables this plugin uses (core plugins can create any; community plugins declare required tables)
tables = [
  { name = "plugin_analysis_results", schema = "schema.sql" }
]

# Plugin dependencies
[dependencies]
# For core plugins: other core plugin IDs
# For community plugins: plugin IDs (resolved from registry or local)
"dev.nexus.sqlite-core" = "^1.0.0"
"dev.nexus.markdown-parser" = "~2.1.0"

# Events this plugin subscribes to (optional)
[event_subscriptions]
subscribe = [
  "editor.workspace.opened",
  "editor.document.saved",
  "nexus.shutdown"
]

# Plugin lifecycle hooks
[lifecycle]
on_load = true  # Plugin has an on_load() entrypoint
on_enable = false
on_disable = false
on_unload = false
on_settings_changed = true
```

### 1.2 Validation Rules

- **id:** Must match `^[a-z0-9]+([-._][a-z0-9]+)*\.[a-z0-9]+([-._][a-z0-9]+)*$` (reverse-domain, lowercase).
- **version:** Must be valid semver. Incompatible changes must bump major version.
- **min_nexus_version:** Plugin must declare compatibility; app rejects if incompatible.
- **Capabilities:** All requested capabilities must be defined in the system; typos are validation errors.
- **Dependencies:** Circular dependencies rejected. Version constraints follow semver ranges (`^1.0`, `~2.1.0`, `>=1.0,<2.0`).
- **Registrations:** Each registration must declare the capability it requires.
- **SQLite tables:** Community plugins cannot create arbitrary tables; must declare schema in `schema.sql`.

### 1.3 Manifest Examples

**Core Plugin Example:** `/nexus/crates/plugin-sqlite-core/plugin.toml`
- No `[community]` section.
- `[core]` section with `stability = "stable"`.
- No capability restrictions; kernel access unrestricted.

**Community Plugin Example:** `/nexus/plugins/wasm/example-plugin/manifest.toml`
- Includes `[community]` with runtime, memory, timeout.
- Declares `[capabilities]` required for install approval.
- All `[registrations]` tied to capabilities.

---

## 2. WASM Runtime Decision: Wasmtime vs Wasmer

### 2.1 Comparison Matrix

| Criterion | Wasmtime | Wasmer |
|-----------|----------|--------|
| **Maturity** | Maintained by Bytecode Alliance (Fastly). Production-grade. | Active community. Well-maintained. |
| **Performance** | JIT-compiled (cranelift). ~90-95% native speed. | JIT + AOT options. Similar performance. |
| **Embedding API** | Excellent Rust API, detailed control. | Good Rust API, user-friendly. |
| **Memory Limits** | Via linear memory growth limits. | Via linear memory growth limits. |
| **Host Function Overhead** | Low (~50-100ns per call). | Low (~50-100ns per call). |
| **SIMD/Bulk Memory** | Full support. | Full support. |
| **Community Ecosystem** | Large; tools, debuggers, runtimes. | Growing; good tooling. |
| **License** | Apache 2.0 | MIT/Apache 2.0 |
| **Compilation Time** | Slower (JIT overhead). | Fast (lazy compilation). |
| **Binary Size** | ~20 MB runtime. | ~15 MB runtime. |

### 2.2 Recommendation: Wasmtime

**Decision:** Use **Wasmtime** (latest stable).

**Justification:**
- **Bytecode Alliance backing:** Ensures long-term maintenance and industry alignment.
- **Deterministic performance:** Predictable compilation and execution; critical for interactive IDE.
- **Fine-grained resource limits:** Precise control over memory, CPU via fuel mechanism.
- **Ecosystem maturity:** Widely used in production (e.g., Fastly Lucet, Spin frameworks).
- **Debugging:** Better tooling for WASM debugging.

**Wasmer Alternative:** If binary size or lazy compilation becomes critical, consider Wasmer v4+ as a drop-in replacement (API-compatible with minimal changes).

### 2.3 Dependency Specification

```toml
# nexus-plugin-runtime/Cargo.toml
[dependencies]
wasmtime = "18.0"  # Pin to major version; auto-update patch/minor
wasmtime-wasi = "18.0"  # WASI support for file I/O on core plugins only
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

---

## 3. WASM Sandbox Implementation

### 3.1 Memory Isolation

**Linear Memory Model:**
- Each WASM plugin instance has its own linear memory (max 64-512 MB, declared in manifest).
- No plugin can read/write another plugin's memory.
- Host (Rust kernel) memory is inaccessible to WASM; all data exchanged via host functions.

**Allocation Strategy:**
```rust
// nexus-plugin-runtime/sandbox.rs

pub struct WasmSandbox {
    engine: wasmtime::Engine,
    module: wasmtime::Module,
    instance: wasmtime::Instance,
    memory: wasmtime::Memory,
    store: wasmtime::Store<PluginData>,
}

impl WasmSandbox {
    pub fn new(module_path: &Path, memory_mb: u32) -> Result<Self> {
        let engine = wasmtime::Engine::new(
            &wasmtime::Config::new()
                .wasm_simd(true)
                .wasm_bulk_memory(true)
        )?;
        
        let module = wasmtime::Module::from_file(&engine, module_path)?;
        let mut store = wasmtime::Store::new(&engine, PluginData::default());
        
        // Create limited memory
        let memory_type = wasmtime::MemoryType::new(
            memory_mb / 64,  // Initial pages (64 KB per page)
            Some(memory_mb / 64)  // Max pages (hard limit)
        );
        let memory = wasmtime::Memory::new(&mut store, memory_type)?;
        
        // Link WASM functions to Rust host functions
        let mut linker = wasmtime::Linker::new(&engine);
        linker.define("env", "host_log", /* ... */)?;
        linker.define("env", "host_query_sqlite", /* ... */)?;
        linker.define("env", "host_emit_event", /* ... */)?;
        // ... more host function definitions
        
        let instance = linker.instantiate(&mut store, &module)?;
        
        Ok(WasmSandbox { engine, module, instance, memory, store })
    }
}
```

### 3.2 Host Function Definitions

Community plugins can only interact with the kernel via exported host functions. All I/O, database access, and kernel events flow through these.

```rust
// Host functions exposed to WASM plugins

// Logging
extern "C" fn host_log(
    store: &mut Store<PluginData>,
    level: i32,  // 0=debug, 1=info, 2=warn, 3=error
    message_ptr: i32,
    message_len: i32,
) -> i32 {
    let memory = store.data().memory;
    let message = memory.data(&store)
        .get(message_ptr as usize..(message_ptr + message_len) as usize)
        .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())?;
    
    match level {
        0 => log::debug!("[plugin] {}", message),
        1 => log::info!("[plugin] {}", message),
        2 => log::warn!("[plugin] {}", message),
        3 => log::error!("[plugin] {}", message),
        _ => return -1,
    }
    0  // Success
}

// SQLite query execution
extern "C" fn host_query_sqlite(
    store: &mut Store<PluginData>,
    query_ptr: i32,
    query_len: i32,
    result_ptr: i32,  // Output: JSON result in WASM memory
    result_capacity: i32,
) -> i32 {
    // Validate capability
    if !store.data().has_capability("sqlite.query") {
        return -1001;  // CAPABILITY_DENIED
    }
    
    // Read query from WASM memory
    let memory = store.data().memory;
    let query_bytes = memory.data(&store)
        .get(query_ptr as usize..(query_ptr + query_len) as usize)?;
    let query = String::from_utf8(query_bytes.to_vec())?;
    
    // Execute query via kernel SQLite
    let result_json = kernel::sqlite::query(&query)?;
    let result_bytes = result_json.as_bytes();
    
    // Write result back to WASM memory
    if result_bytes.len() > result_capacity as usize {
        return -1002;  // BUFFER_OVERFLOW
    }
    
    let memory_mut = memory.data_mut(&mut store);
    memory_mut[result_ptr as usize..(result_ptr as usize + result_bytes.len())]
        .copy_from_slice(result_bytes);
    
    result_bytes.len() as i32  // Return bytes written
}

// Event emission
extern "C" fn host_emit_event(
    store: &mut Store<PluginData>,
    event_type_ptr: i32,
    event_type_len: i32,
    data_ptr: i32,
    data_len: i32,
) -> i32 {
    // Validate capability
    if !store.data().has_capability("event.emit") {
        return -1001;
    }
    
    let memory = store.data().memory;
    let event_type = String::from_utf8(
        memory.data(&store)
            .get(event_type_ptr as usize..(event_type_ptr + event_type_len) as usize)?
            .to_vec()
    )?;
    
    let event_data = serde_json::json!(memory.data(&store)
        .get(data_ptr as usize..(data_ptr + data_len) as usize)?);
    
    kernel::events::emit(&event_type, event_data);
    0  // Success
}

// IPC command dispatch
extern "C" fn host_invoke_command(
    store: &mut Store<PluginData>,
    cmd_id_ptr: i32,
    cmd_id_len: i32,
    args_ptr: i32,
    args_len: i32,
) -> i32 {
    let memory = store.data().memory;
    let cmd_id = String::from_utf8(
        memory.data(&store)
            .get(cmd_id_ptr as usize..(cmd_id_ptr + cmd_id_len) as usize)?
            .to_vec()
    )?;
    
    // Invoke IPC command from this plugin; args passed as JSON
    let result = kernel::commands::invoke(&cmd_id, /* args */);
    // Write result JSON back to memory...
    0
}

// File I/O (only if filesystem.read/write capability granted)
extern "C" fn host_read_file(
    store: &mut Store<PluginData>,
    path_ptr: i32,
    path_len: i32,
    output_ptr: i32,
    output_capacity: i32,
) -> i32 {
    if !store.data().has_capability("filesystem.read") {
        return -1001;
    }
    
    let memory = store.data().memory;
    let path = String::from_utf8(
        memory.data(&store)
            .get(path_ptr as usize..(path_ptr + path_len) as usize)?
            .to_vec()
    )?;
    
    // Sandboxed file read: only in .forge/plugins/<plugin_id>/ or workspace/
    let safe_path = kernel::fs::resolve_safe_path(&store.data().plugin_id, &path)?;
    let contents = std::fs::read(&safe_path)?;
    
    if contents.len() > output_capacity as usize {
        return -1002;  // BUFFER_OVERFLOW
    }
    
    let memory_mut = memory.data_mut(&mut store);
    memory_mut[output_ptr as usize..(output_ptr as usize + contents.len())]
        .copy_from_slice(&contents);
    
    contents.len() as i32
}
```

### 3.3 Resource Limits

```rust
// nexus-plugin-runtime/limits.rs

pub struct ResourceLimits {
    pub max_memory_mb: u32,
    pub cpu_timeout_ms: u32,
    pub max_open_files: u32,
    pub max_db_connections: u32,
}

impl ResourceLimits {
    pub fn enforce(&self, store: &mut Store<PluginData>) {
        // CPU timeout: use Wasmtime's fuel mechanism
        store.add_fuel(self.cpu_timeout_ms as u64 * 1_000_000).unwrap();
        
        store.set_fuel_async_yield_interval(Some(100_000));  // Yield every 100M fuel
        
        // Memory limit: enforced via linear memory page limit (set at instantiation)
        
        // File descriptor limit: tracked in PluginData
        // Database connection limit: tracked in PluginData
    }
}

// Handle fuel exhaustion (timeout)
pub async fn run_with_timeout(
    instance: &wasmtime::Instance,
    store: &mut Store<PluginData>,
) -> Result<(), PluginError> {
    match instance.call(&mut store, /* ... */) {
        Ok(_) => Ok(()),
        Err(e) if e.to_string().contains("out of fuel") => {
            Err(PluginError::ExecutionTimeout)
        }
        Err(e) => Err(PluginError::ExecutionFailed(e.to_string())),
    }
}
```

---

## 4. Plugin API Trait Definitions

### 4.1 Core Traits

```rust
// nexus-plugin-api/src/lib.rs

/// Base trait for all plugins
pub trait Plugin: Send + Sync {
    /// Plugin identifier (from manifest)
    fn id(&self) -> &str;
    
    /// Called when plugin is loaded
    fn on_load(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
    
    /// Called when plugin is enabled
    fn on_enable(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
    
    /// Called when plugin is disabled
    fn on_disable(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
    
    /// Called before plugin is unloaded
    fn on_unload(&mut self) -> Result<(), PluginError> {
        Ok(())
    }
    
    /// Called when settings change
    fn on_settings_changed(&mut self, settings: serde_json::Value) -> Result<(), PluginError> {
        Ok(())
    }
}

/// Trait for core plugins (native Rust)
pub trait CorePlugin: Plugin {
    /// Register this plugin's capabilities, commands, surfaces, etc.
    fn register(&mut self, ctx: &PluginContext) -> Result<(), PluginError>;
    
    /// Get the plugin's manifest
    fn manifest(&self) -> &PluginManifest;
}

/// Trait for community plugins (WASM)
pub trait CommunityPlugin {
    /// Execute a host function exported by the WASM module
    fn call_wasm_function(
        &mut self,
        function_name: &str,
        args: Vec<wasmtime::Val>,
    ) -> Result<Vec<wasmtime::Val>, PluginError>;
}

/// Plugin registration context
pub struct PluginContext {
    pub plugin_id: String,
    pub registry: Arc<Mutex<PluginRegistry>>,
    pub kernel: Arc<KernelApi>,
}

impl PluginContext {
    /// Register an IPC command
    pub fn register_ipc_command(
        &self,
        id: &str,
        handler: Box<dyn Fn(serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync>,
    ) -> Result<(), PluginError> {
        self.registry.lock().unwrap()
            .register_command(&self.plugin_id, id, handler)
    }
    
    /// Register a CLI subcommand
    pub fn register_cli_subcommand(
        &self,
        subcommand: &str,
        handler: Box<dyn Fn(Vec<String>) -> Result<String, String> + Send + Sync>,
    ) -> Result<(), PluginError> {
        self.registry.lock().unwrap()
            .register_cli_command(&self.plugin_id, subcommand, handler)
    }
    
    /// Register a UI surface
    pub fn register_surface(
        &self,
        id: &str,
        surface_type: SurfaceType,
    ) -> Result<(), PluginError> {
        self.registry.lock().unwrap()
            .register_surface(&self.plugin_id, id, surface_type)
    }
    
    /// Subscribe to an event
    pub fn subscribe_event(
        &self,
        event_type: &str,
        handler: Box<dyn Fn(serde_json::Value) -> Result<(), String> + Send + Sync>,
    ) -> Result<(), PluginError> {
        self.registry.lock().unwrap()
            .subscribe(&self.plugin_id, event_type, handler)
    }
    
    /// Query SQLite
    pub fn query_sqlite(
        &self,
        query: &str,
    ) -> Result<serde_json::Value, PluginError> {
        self.kernel.sqlite_query(query)
    }
    
    /// Create a SQLite table (core plugins only)
    pub fn create_sqlite_table(
        &self,
        schema: &str,
    ) -> Result<(), PluginError> {
        self.kernel.sqlite_create_table(schema)
    }
}

pub enum SurfaceType {
    Panel,
    Modal,
    Sidebar,
    Tab,
}

#[derive(Debug)]
pub enum PluginError {
    NotFound,
    AlreadyLoaded,
    FailedToLoad(String),
    CapabilityDenied(String),
    ExecutionTimeout,
    ExecutionFailed(String),
    InvalidManifest(String),
    DependencyUnresolved(String),
    VersionConflict(String),
}
```

---

## 5. Capability Enforcement

### 5.1 Capability System

Capabilities are fine-grained permissions. Every API surface, resource access, and external I/O requires a capability.

**Core Capability List:**

| Capability | Description | Risk Level | Community | Core |
|------------|-------------|------------|-----------|------|
| `sqlite.query` | Read-only SQLite queries | Low | Prompt | Unrestricted |
| `sqlite.write` | Write/insert/delete SQLite | Medium | Prompt | Unrestricted |
| `filesystem.read` | Read files in workspace | Low | Prompt | Unrestricted |
| `filesystem.write` | Write files in workspace | Medium | Prompt | Unrestricted |
| `cli.subcommand` | Register CLI subcommand | Low | Auto-grant | Unrestricted |
| `editor.workspace_open_event` | Listen to workspace open event | Low | Auto-grant | Unrestricted |
| `editor.document_saved_event` | Listen to document saved event | Low | Auto-grant | Unrestricted |
| `editor.lint` | Provide linter/diagnostics | Low | Auto-grant | Unrestricted |
| `ui.surface` | Register UI surface/panel | Low | Auto-grant | Unrestricted |
| `ui.menu_item` | Add menu items | Low | Auto-grant | Unrestricted |
| `markdown.processor` | Register markdown processor | Low | Auto-grant | Unrestricted |
| `protocol.handle` | Register URL scheme handler | Medium | Prompt | Unrestricted |
| `network.http` | Make HTTP requests | High | Prompt | Unrestricted |
| `event.emit` | Emit custom events | Low | Auto-grant | Unrestricted |
| `settings.own_config` | Access own plugin settings | Low | Auto-grant | Unrestricted |

### 5.2 Capability Enforcement at Runtime

```rust
// nexus-kernel/src/capabilities.rs

pub struct CapabilityManager {
    plugin_capabilities: HashMap<String, HashSet<String>>,  // plugin_id -> capabilities
    capability_definitions: HashMap<String, CapabilityDef>,
}

pub struct CapabilityDef {
    pub id: String,
    pub description: String,
    pub risk_level: RiskLevel,
    pub auto_grant: bool,
}

pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl CapabilityManager {
    /// Check if plugin has a capability; returns error if not
    pub fn require_capability(
        &self,
        plugin_id: &str,
        capability: &str,
    ) -> Result<(), PluginError> {
        if let Some(caps) = self.plugin_capabilities.get(plugin_id) {
            if caps.contains(capability) {
                return Ok(());
            }
        }
        Err(PluginError::CapabilityDenied(capability.to_string()))
    }
    
    /// Grant capability to plugin
    pub fn grant_capability(
        &mut self,
        plugin_id: &str,
        capability: &str,
    ) -> Result<(), PluginError> {
        if !self.capability_definitions.contains_key(capability) {
            return Err(PluginError::InvalidCapability(capability.to_string()));
        }
        
        self.plugin_capabilities
            .entry(plugin_id.to_string())
            .or_insert_with(HashSet::new)
            .insert(capability.to_string());
        
        Ok(())
    }
    
    /// Revoke capability from plugin
    pub fn revoke_capability(
        &mut self,
        plugin_id: &str,
        capability: &str,
    ) {
        if let Some(caps) = self.plugin_capabilities.get_mut(plugin_id) {
            caps.remove(capability);
        }
    }
    
    /// Load plugin capabilities from manifest
    pub fn load_from_manifest(
        &mut self,
        plugin_id: &str,
        manifest: &PluginManifest,
    ) -> Result<(), PluginError> {
        // For community plugins, only grant declared capabilities
        // For core plugins, grant all capabilities by default
        
        for cap in &manifest.capabilities.required {
            self.grant_capability(plugin_id, cap)?;
        }
        
        for cap in &manifest.capabilities.optional {
            // Optional capabilities: grant only if user approves
            // (skip for now, handled in install flow)
        }
        
        Ok(())
    }
}

// Host function: capability check before SQLite query
extern "C" fn host_query_sqlite(
    store: &mut Store<PluginData>,
    query_ptr: i32,
    query_len: i32,
) -> i32 {
    let plugin_id = &store.data().plugin_id;
    let cap_mgr = &store.data().capability_manager;
    
    if let Err(_) = cap_mgr.require_capability(plugin_id, "sqlite.query") {
        return -1001;  // CAPABILITY_DENIED
    }
    
    // Proceed with query...
    0
}
```

### 5.3 Dynamic Capability Requests

Plugins can request new capabilities at runtime (e.g., "needs network access"). User is prompted; approval recorded.

```rust
// In WASM plugin code (Rust compiled to WASM)
extern "C" {
    fn host_request_capability(cap_ptr: i32, cap_len: i32) -> i32;
}

pub fn request_network_capability() -> Result<(), String> {
    unsafe {
        let cap = "network.http".as_bytes();
        let result = host_request_capability(cap.as_ptr() as i32, cap.len() as i32);
        
        match result {
            0 => Ok(()),
            -1001 => Err("User denied capability request".to_string()),
            -1002 => Err("Capability does not exist".to_string()),
            _ => Err("Unknown error".to_string()),
        }
    }
}

// Host implementation
extern "C" fn host_request_capability(
    store: &mut Store<PluginData>,
    cap_ptr: i32,
    cap_len: i32,
) -> i32 {
    let memory = store.data().memory;
    let capability = String::from_utf8(
        memory.data(&store)
            .get(cap_ptr as usize..(cap_ptr + cap_len) as usize)
            .unwrap_or(&[])
            .to_vec()
    ).unwrap_or_default();
    
    let plugin_id = store.data().plugin_id.clone();
    
    // Post UI event: "CapabilityRequestDialog"
    // User approves → kernel grants via CapabilityManager
    // User denies → return -1001
    
    // For now, return error (implement UI flow separately)
    -1001
}
```

---

## 6. Plugin Packaging & Distribution

### 6.1 Package Format

Community plugins are distributed as `.nxplugin` files (ZIP archives):

```
my-plugin.nxplugin
├── manifest.toml
├── plugin.wasm
├── icon-light.svg
├── icon-dark.svg
├── schema.sql (optional)
├── settings.json (JSON Schema for settings UI)
├── README.md
└── LICENSE
```

### 6.2 Packaging & Signing

```bash
# Create .nxplugin package
$ nexus plugin package ./my-plugin --output my-plugin-1.0.0.nxplugin

# Sign package (using ed25519 keypair)
$ nexus plugin sign my-plugin-1.0.0.nxplugin --key-file ~/.nexus/signing-key

# Outputs: my-plugin-1.0.0.nxplugin.sig
```

**Signature File Format (text):**
```
plugin_id: dev.nexus.my-plugin
version: 1.0.0
signature: <base64-encoded ed25519 signature of plugin bytes>
```

### 6.3 Installation Flow

```rust
// nexus-kernel/src/plugin_manager.rs

pub async fn install_plugin(
    plugin_path: &Path,
    plugin_manifest: &PluginManifest,
) -> Result<(), PluginError> {
    // 1. Validate manifest
    validate_manifest(plugin_manifest)?;
    
    // 2. Check dependencies
    resolve_dependencies(plugin_manifest)?;
    
    // 3. Verify signature
    verify_plugin_signature(plugin_path)?;
    
    // 4. Extract to .forge/plugins/<plugin_id>/
    let install_dir = PathBuf::from(format!(".forge/plugins/{}", plugin_manifest.id));
    extract_plugin(plugin_path, &install_dir)?;
    
    // 5. Save enabled=false in user settings
    let settings = UserSettings::load()?;
    settings.plugins.insert(
        plugin_manifest.id.clone(),
        PluginSetting {
            enabled: false,
            capabilities_granted: HashSet::new(),
        },
    );
    settings.save()?;
    
    // 6. Show capability approval dialog
    show_capability_approval_dialog(plugin_manifest)?;
    
    // 7. Plugin is now installed (but disabled until user clicks "Enable")
    Ok(())
}

pub async fn enable_plugin(plugin_id: &str) -> Result<(), PluginError> {
    let plugin_manifest = load_plugin_manifest(plugin_id)?;
    
    // Check all dependencies are installed and enabled
    check_dependencies_enabled(&plugin_manifest)?;
    
    // Load WASM plugin
    let sandbox = WasmSandbox::new(
        &PathBuf::from(format!(".forge/plugins/{}/plugin.wasm", plugin_id)),
        plugin_manifest.community.wasm_memory_mb,
    )?;
    
    // Grant capabilities
    let mut cap_mgr = CapabilityManager::new();
    cap_mgr.load_from_manifest(plugin_id, &plugin_manifest)?;
    
    // Call plugin on_load
    sandbox.call_wasm_function("on_load", vec![])?;
    
    // Register plugin in registry
    PLUGIN_REGISTRY.register(plugin_id, sandbox)?;
    
    // Update settings: enabled=true
    let mut settings = UserSettings::load()?;
    settings.plugins.get_mut(plugin_id).unwrap().enabled = true;
    settings.save()?;
    
    Ok(())
}
```

### 6.4 Update Flow

Plugins can be updated in place if version is compatible (semver):

```rust
pub async fn update_plugin(
    plugin_id: &str,
    new_version_path: &Path,
) -> Result<(), PluginError> {
    let old_manifest = load_plugin_manifest(plugin_id)?;
    let new_manifest = load_plugin_manifest_from_package(new_version_path)?;
    
    // Verify semver compatibility
    if !is_version_compatible(&old_manifest.version, &new_manifest.version) {
        return Err(PluginError::IncompatibleVersion);
    }
    
    let was_enabled = is_plugin_enabled(plugin_id)?;
    
    if was_enabled {
        // Disable plugin (call on_disable, on_unload)
        disable_plugin(plugin_id)?;
    }
    
    // Replace plugin files
    let install_dir = PathBuf::from(format!(".forge/plugins/{}", plugin_id));
    std::fs::remove_dir_all(&install_dir)?;
    extract_plugin(new_version_path, &install_dir)?;
    
    if was_enabled {
        // Re-enable with new version
        enable_plugin(plugin_id)?;
    }
    
    Ok(())
}
```

---

## 7. Hot-Reload for Development

### 7.1 File Watcher Integration

```rust
// nexus-devtools/src/hot_reload.rs

use notify::{Watcher, RecursiveMode, watcher};
use std::sync::mpsc::channel;
use std::time::Duration;

pub struct PluginHotReload {
    watcher: Box<dyn Watcher>,
    plugin_id: String,
    source_path: PathBuf,
}

impl PluginHotReload {
    pub fn start(plugin_id: &str, source_path: &Path) -> Result<Self, PluginError> {
        let (tx, rx) = channel();
        
        let mut watcher = watcher(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
            Duration::from_secs(1),
        )?;
        
        // Watch .rs files for changes
        watcher.watch(source_path, RecursiveMode::Recursive)?;
        
        // Spawn reload worker
        let plugin_id_clone = plugin_id.to_string();
        let source_path_clone = source_path.to_path_buf();
        
        tokio::spawn(async move {
            while let Ok(event) = rx.recv() {
                if is_rust_file_event(&event) {
                    if let Err(e) = Self::trigger_reload(
                        &plugin_id_clone,
                        &source_path_clone,
                    ).await {
                        log::error!("Hot reload failed: {}", e);
                    }
                }
            }
        });
        
        Ok(PluginHotReload {
            watcher: Box::new(watcher),
            plugin_id: plugin_id.to_string(),
            source_path: source_path.to_path_buf(),
        })
    }
    
    async fn trigger_reload(plugin_id: &str, source_path: &Path) -> Result<(), PluginError> {
        // 1. Recompile plugin: cargo build --target wasm32-unknown-unknown
        compile_plugin_to_wasm(source_path).await?;
        
        // 2. Get current state (settings, enabled status)
        let settings = get_plugin_settings(plugin_id)?;
        let was_enabled = settings.enabled;
        
        // 3. Call on_disable + on_unload on old plugin
        if was_enabled {
            disable_plugin(plugin_id)?;
        }
        
        // 4. Load new plugin from recompiled .wasm
        let new_wasm_path = source_path.join("target/wasm32-unknown-unknown/release")
            .join(format!("{}.wasm", plugin_id));
        
        let sandbox = WasmSandbox::new(&new_wasm_path, 64)?;
        PLUGIN_REGISTRY.replace(plugin_id, sandbox)?;
        
        // 5. Restore settings and re-enable if was enabled
        if was_enabled {
            enable_plugin(plugin_id)?;
            log::info!("Plugin {} hot-reloaded and re-enabled", plugin_id);
        } else {
            log::info!("Plugin {} hot-reloaded (disabled)", plugin_id);
        }
        
        Ok(())
    }
}

fn is_rust_file_event(event: &notify::Event) -> bool {
    event.paths.iter().any(|p| p.extension().map_or(false, |e| e == "rs"))
}

async fn compile_plugin_to_wasm(plugin_path: &Path) -> Result<(), PluginError> {
    let output = tokio::process::Command::new("cargo")
        .arg("build")
        .arg("--target")
        .arg("wasm32-unknown-unknown")
        .arg("--release")
        .current_dir(plugin_path)
        .output()
        .await?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PluginError::CompilationFailed(stderr.to_string()));
    }
    
    Ok(())
}
```

### 7.2 Development Workflow

```bash
# Create plugin scaffold
$ nexus plugin scaffold --name "my-analyzer" --author "Jane Doe"
Created: nexus-plugins/my-analyzer/

# Enter plugin directory
$ cd nexus-plugins/my-analyzer

# Start hot-reload server
$ cargo nexus-plugin-dev
Building WASM plugin...
Plugin loaded: dev.nexus.my-analyzer (64 MB memory, 5s timeout)
Watching for changes in src/... 

# Edit src/lib.rs → auto-recompiles and reloads

# Run tests
$ cargo test --target wasm32-unknown-unknown
```

---

## 8. Plugin Communication

### 8.1 Event-Driven Model (Primary)

Plugins communicate asynchronously via the **event bus**. All events are JSON-encoded.

```rust
// nexus-kernel/src/event_bus.rs

pub struct EventBus {
    subscribers: HashMap<String, Vec<EventHandler>>,
}

pub type EventHandler = Box<dyn Fn(serde_json::Value) + Send + Sync>;

impl EventBus {
    pub fn subscribe(
        &mut self,
        plugin_id: &str,
        event_type: &str,
        handler: EventHandler,
    ) {
        self.subscribers
            .entry(event_type.to_string())
            .or_insert_with(Vec::new)
            .push(handler);
    }
    
    pub fn emit(
        &self,
        event_type: &str,
        data: serde_json::Value,
    ) {
        if let Some(handlers) = self.subscribers.get(event_type) {
            for handler in handlers {
                handler(data.clone());
            }
        }
    }
}

// Standard events emitted by kernel
pub enum KernelEvent {
    EditorWorkspaceOpened { workspace_path: String },
    EditorDocumentSaved { path: String, content: String },
    EditorSelectionChanged { start: usize, end: usize },
    PluginLoaded { plugin_id: String },
    PluginUnloaded { plugin_id: String },
    SettingsChanged { plugin_id: String, settings: serde_json::Value },
    KernelShutdown,
}

// Plugin subscription (in manifest.toml)
[event_subscriptions]
subscribe = [
    "editor.document.saved",
    "nexus.shutdown",
]

// Manifest-driven subscription
pub fn load_event_subscriptions(
    plugin_id: &str,
    manifest: &PluginManifest,
    event_bus: &mut EventBus,
) {
    for event_type in &manifest.event_subscriptions {
        let handler: EventHandler = Box::new(move |data| {
            // Invoke WASM plugin's on_event handler
            invoke_plugin_handler(plugin_id, "on_event", data);
        });
        event_bus.subscribe(plugin_id, event_type, handler);
    }
}
```

### 8.2 Service Registry (Secondary)

For plugin-to-plugin calls, plugins register **services** that other plugins can discover and invoke synchronously.

```rust
// nexus-kernel/src/service_registry.rs

pub struct ServiceRegistry {
    services: HashMap<String, ServiceDescriptor>,
}

pub struct ServiceDescriptor {
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub methods: Vec<ServiceMethod>,
}

pub struct ServiceMethod {
    pub name: String,
    pub params: serde_json::Value,  // JSON Schema
    pub returns: serde_json::Value,  // JSON Schema
}

// Service registration (in plugin on_load)
ctx.register_service(
    "analyze:lint",
    ServiceDescriptor {
        plugin_id: "dev.nexus.analyzer".to_string(),
        name: "lint".to_string(),
        version: "1.0.0".to_string(),
        methods: vec![
            ServiceMethod {
                name: "analyze".to_string(),
                params: serde_json::json!({ "code": "string" }),
                returns: serde_json::json!({ "diagnostics": [ { "line": "number", "message": "string" } ] }),
            },
        ],
    },
)?;

// Plugin-to-plugin call (other plugin)
let result = ctx.call_service(
    "analyze:lint",
    "analyze",
    serde_json::json!({ "code": "fn main() {}" }),
)?;
```

---

## 9. Plugin Settings System

### 9.1 Settings Schema Declaration

Plugins declare settings schema (JSON Schema). Kernel auto-generates settings UI.

```json
// plugin-settings.json (bundled in .nxplugin)
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "title": "Example Analyzer Settings",
  "properties": {
    "enabled": {
      "type": "boolean",
      "title": "Enable Analysis",
      "description": "Turn analysis on or off",
      "default": true
    },
    "max_line_length": {
      "type": "integer",
      "title": "Max Line Length",
      "minimum": 40,
      "maximum": 200,
      "default": 80
    },
    "report_severity": {
      "type": "string",
      "title": "Report Severity",
      "enum": ["error", "warning", "info"],
      "default": "warning"
    },
    "ignored_rules": {
      "type": "array",
      "title": "Ignored Rules",
      "items": { "type": "string" },
      "default": []
    }
  },
  "required": ["enabled"]
}
```

### 9.2 Settings Storage and API

```rust
// nexus-kernel/src/plugin_settings.rs

pub struct PluginSettings {
    plugin_id: String,
    values: serde_json::Value,
    schema: serde_json::Value,
}

impl PluginSettings {
    pub fn load(plugin_id: &str) -> Result<Self, PluginError> {
        // Load from ~/.nexus/plugins/<plugin_id>/settings.json
        let settings_path = get_plugin_settings_path(plugin_id);
        let values = if settings_path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&settings_path)?)?
        } else {
            serde_json::json!({})
        };
        
        let schema_path = get_plugin_schema_path(plugin_id);
        let schema = if schema_path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&schema_path)?)?
        } else {
            serde_json::json!({})
        };
        
        Ok(PluginSettings {
            plugin_id: plugin_id.to_string(),
            values,
            schema,
        })
    }
    
    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        self.values.get(key).cloned()
    }
    
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.get(key)?.as_str().map(|s| s.to_string())
    }
    
    pub fn get_integer(&self, key: &str) -> Option<i64> {
        self.get(key)?.as_i64()
    }
    
    pub fn set(&mut self, key: &str, value: serde_json::Value) -> Result<(), PluginError> {
        // Validate against schema
        validate_setting(key, &value, &self.schema)?;
        self.values[key] = value;
        self.save()?;
        Ok(())
    }
    
    pub fn save(&self) -> Result<(), PluginError> {
        let settings_path = get_plugin_settings_path(&self.plugin_id);
        std::fs::create_dir_all(settings_path.parent().unwrap())?;
        std::fs::write(&settings_path, serde_json::to_string_pretty(&self.values)?)?;
        Ok(())
    }
}

// WASM plugin API: read/write own settings
extern "C" fn host_get_setting(
    store: &mut Store<PluginData>,
    key_ptr: i32,
    key_len: i32,
    output_ptr: i32,
    output_capacity: i32,
) -> i32 {
    let plugin_id = &store.data().plugin_id;
    let memory = store.data().memory;
    
    let key = String::from_utf8(
        memory.data(&store)
            .get(key_ptr as usize..(key_ptr + key_len) as usize)?
            .to_vec()
    )?;
    
    let settings = PluginSettings::load(plugin_id).ok()?;
    if let Some(value) = settings.get(&key) {
        let json_str = serde_json::to_string(&value)?;
        let bytes = json_str.as_bytes();
        
        if bytes.len() > output_capacity as usize {
            return -1002;  // BUFFER_OVERFLOW
        }
        
        let memory_mut = memory.data_mut(&mut store);
        memory_mut[output_ptr as usize..(output_ptr as usize + bytes.len())]
            .copy_from_slice(bytes);
        
        return bytes.len() as i32;
    }
    
    -1003  // NOT_FOUND
}

extern "C" fn host_set_setting(
    store: &mut Store<PluginData>,
    key_ptr: i32,
    key_len: i32,
    value_ptr: i32,
    value_len: i32,
) -> i32 {
    let plugin_id = &store.data().plugin_id;
    let memory = store.data().memory;
    
    let key = String::from_utf8(
        memory.data(&store)
            .get(key_ptr as usize..(key_ptr + key_len) as usize)?
            .to_vec()
    )?;
    
    let value: serde_json::Value = serde_json::from_str(&String::from_utf8(
        memory.data(&store)
            .get(value_ptr as usize..(value_ptr + value_len) as usize)?
            .to_vec()
    )?)?;
    
    let mut settings = PluginSettings::load(plugin_id)?;
    settings.set(&key, value)?;
    
    0  // Success
}
```

### 9.3 Settings UI

Kernel auto-generates settings panel for each plugin using JSON Schema.

---

## 10. Dynamic Loading for Core Plugins

### 10.1 .so/.dll Loading with Symbol Resolution

Core plugins can be compiled into the app or loaded as native dynamic libraries.

```rust
// nexus-kernel/src/core_plugin_loader.rs

use libloading::{Library, Symbol};

pub struct CorePluginLoader {
    plugins: HashMap<String, LoadedPlugin>,
}

struct LoadedPlugin {
    library: Option<Library>,  // None if statically linked
    plugin: Box<dyn CorePlugin>,
}

impl CorePluginLoader {
    pub fn load_plugin(
        &mut self,
        plugin_path: Option<&Path>,  // None = statically linked
        plugin_id: &str,
    ) -> Result<(), PluginError> {
        let plugin: Box<dyn CorePlugin> = if let Some(path) = plugin_path {
            // Dynamically load from .so/.dll
            unsafe {
                let lib = Library::new(path)?;
                
                // Symbol name: __nexus_plugin_<plugin_id>_new
                // e.g., __nexus_plugin_sqlite_core_new
                let init_symbol: Symbol<unsafe extern "C" fn() -> *mut dyn CorePlugin> =
                    lib.get(format!("__nexus_plugin_{}_new", sanitize_id(plugin_id)).as_bytes())?;
                
                let plugin_ptr = init_symbol();
                let plugin = Box::from_raw(plugin_ptr);
                
                self.plugins.insert(
                    plugin_id.to_string(),
                    LoadedPlugin {
                        library: Some(lib),
                        plugin,
                    },
                );
            }
        } else {
            // Statically linked: register via plugin attribute macro
            // #[nexus_plugin(id = "sqlite-core")]
            let plugin = get_static_plugin(plugin_id)?;
            self.plugins.insert(
                plugin_id.to_string(),
                LoadedPlugin {
                    library: None,
                    plugin,
                },
            );
        }
        
        Ok(())
    }
}

// Macro for core plugins (static or dynamic)
#[macro_export]
macro_rules! nexus_plugin {
    ($plugin_struct:ty, id = $plugin_id:expr, stability = $stability:expr) => {
        #[no_mangle]
        pub unsafe extern "C" fn __nexus_plugin_new() -> *mut dyn $crate::CorePlugin {
            let plugin = Box::new(<$plugin_struct>::new());
            Box::into_raw(plugin)
        }
        
        // For static registration
        #[ctor::ctor]
        fn register_plugin() {
            $crate::STATIC_PLUGINS.register($plugin_id, __nexus_plugin_new);
        }
    };
}

// Usage in a core plugin crate
use nexus_plugin_api::nexus_plugin;

pub struct SqliteCorePlugin {
    // ...
}

impl CorePlugin for SqliteCorePlugin {
    // ... implementation
}

nexus_plugin!(SqliteCorePlugin, id = "sqlite-core", stability = "stable");
```

### 10.2 Version Compatibility Checking

```rust
pub fn check_version_compatibility(
    plugin_manifest: &PluginManifest,
    nexus_version: &str,
) -> Result<(), PluginError> {
    let plugin_min = &plugin_manifest.min_nexus_version;
    let plugin_max = &plugin_manifest.max_nexus_version;
    
    let min_satisfied = compare_versions(nexus_version, plugin_min) >= 0;
    let max_satisfied = if let Some(max) = plugin_max {
        compare_versions(nexus_version, max) < 0
    } else {
        true
    };
    
    if min_satisfied && max_satisfied {
        Ok(())
    } else {
        Err(PluginError::IncompatibleVersion)
    }
}

fn compare_versions(v1: &str, v2: &str) -> i32 {
    use semver::Version;
    let ver1 = Version::parse(v1).unwrap_or(Version::new(0, 0, 0));
    let ver2 = Version::parse(v2).unwrap_or(Version::new(0, 0, 0));
    
    if ver1 > ver2 { 1 }
    else if ver1 < ver2 { -1 }
    else { 0 }
}
```

---

## 11. Plugin Discovery

### 11.1 Discovery Paths

Plugins are discovered from multiple locations:

1. **Bundled:** `nexus/crates/plugin-*` (core plugins, compiled in)
2. **Workspace:** `.forge/plugins/` (installed community plugins)
3. **Global:** `~/.nexus/plugins/` (user-installed)
4. **Registry:** Plugin registry API (future: marketplace)

```rust
// nexus-kernel/src/plugin_discovery.rs

pub struct PluginDiscovery {
    paths: Vec<PathBuf>,
}

impl PluginDiscovery {
    pub fn new(workspace_root: &Path) -> Self {
        PluginDiscovery {
            paths: vec![
                workspace_root.join(".forge/plugins"),
                home_dir().join(".nexus/plugins"),
                // Bundled plugins are registered statically
            ],
        }
    }
    
    pub fn discover_all(&self) -> Vec<PluginMetadata> {
        let mut plugins = Vec::new();
        
        for path in &self.paths {
            if path.exists() {
                for entry in std::fs::read_dir(path).unwrap() {
                    if let Ok(entry) = entry {
                        let plugin_dir = entry.path();
                        if let Ok(manifest) = load_manifest(&plugin_dir) {
                            plugins.push(PluginMetadata {
                                id: manifest.id.clone(),
                                name: manifest.name.clone(),
                                version: manifest.version.clone(),
                                path: plugin_dir,
                            });
                        }
                    }
                }
            }
        }
        
        plugins
    }
}
```

---

## 12. Dependency Resolution

### 12.1 Algorithm

Dependency resolution uses **topological sort** to build a load order.

```rust
// nexus-kernel/src/dependency_resolver.rs

pub struct DependencyResolver;

impl DependencyResolver {
    pub fn resolve(
        plugins: &[PluginMetadata],
    ) -> Result<Vec<String>, PluginError> {
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        
        // Build dependency graph
        for plugin in plugins {
            in_degree.entry(plugin.id.clone()).or_insert(0);
            for dep_id in &plugin.dependencies {
                graph.entry(plugin.id.clone())
                    .or_insert_with(Vec::new)
                    .push(dep_id.clone());
                *in_degree.entry(dep_id.clone()).or_insert(0) += 1;
            }
        }
        
        // Topological sort (Kahn's algorithm)
        let mut queue: Vec<String> = in_degree.iter()
            .filter(|(_, &degree)| degree == 0)
            .map(|(id, _)| id.clone())
            .collect();
        
        let mut sorted = Vec::new();
        while let Some(current) = queue.pop() {
            sorted.push(current.clone());
            
            if let Some(deps) = graph.get(&current) {
                for dep in deps {
                    *in_degree.get_mut(dep).unwrap() -= 1;
                    if in_degree[dep] == 0 {
                        queue.push(dep.clone());
                    }
                }
            }
        }
        
        if sorted.len() != in_degree.len() {
            return Err(PluginError::CircularDependency);
        }
        
        Ok(sorted)
    }
    
    pub fn check_version_constraints(
        plugin_id: &str,
        required_version: &str,
        available_version: &str,
    ) -> Result<(), PluginError> {
        // Parse semver range and check compatibility
        let range = semver::VersionReq::parse(required_version)?;
        let version = semver::Version::parse(available_version)?;
        
        if range.matches(&version) {
            Ok(())
        } else {
            Err(PluginError::VersionConflict)
        }
    }
}
```

---

## 13. Performance Isolation & Watchdog

### 13.1 Detecting Misbehavior

```rust
// nexus-kernel/src/plugin_watchdog.rs

pub struct PluginWatchdog {
    timeouts: Arc<Mutex<HashMap<String, Instant>>>,
}

impl PluginWatchdog {
    pub fn monitor_plugin(
        plugin_id: &str,
        timeout_ms: u32,
    ) {
        let start = Instant::now();
        let plugin_id_clone = plugin_id.to_string();
        
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(timeout_ms as u64)).await;
            
            if start.elapsed() > Duration::from_millis(timeout_ms as u64) {
                // Plugin exceeded timeout; terminate it
                terminate_plugin(&plugin_id_clone).await;
                log::error!("Plugin {} exceeded timeout ({} ms)", plugin_id, timeout_ms);
            }
        });
    }
    
    pub fn detect_memory_leak(plugin_id: &str) -> bool {
        // Check WASM memory growth over time
        let sandbox = PLUGIN_REGISTRY.get(plugin_id).unwrap();
        let memory_pages = sandbox.memory.size(&sandbox.store);
        
        // If memory usage > 90% of max, flag as potential leak
        memory_pages > (sandbox.memory.ty().maximum().unwrap_or(1024) * 9) / 10
    }
}

pub async fn terminate_plugin(plugin_id: &str) {
    log::warn!("Terminating plugin: {}", plugin_id);
    
    // 1. Call on_disable, on_unload
    if let Err(e) = disable_plugin(plugin_id).await {
        log::error!("Error disabling plugin {}: {}", plugin_id, e);
    }
    
    // 2. Remove from registry
    PLUGIN_REGISTRY.remove(plugin_id);
    
    // 3. Notify user
    emit_event("plugin.terminated", serde_json::json!({
        "plugin_id": plugin_id,
        "reason": "timeout_exceeded"
    }));
}
```

---

## 14. Plugin Registry/Marketplace (Design-Level)

### 14.1 Architecture

Future community plugin registry will support:

- **Discovery:** Search by name, tag, author
- **Installation:** One-click install to `.forge/plugins/`
- **Submission:** Developers submit plugins; reviewed by community
- **Signing:** Plugins signed with developer's ed25519 key
- **Ratings:** Community ratings and reviews
- **Updates:** Automatic update notifications

**Registry API (future REST endpoints):**

```
GET  /api/v1/plugins                      # List all plugins
GET  /api/v1/plugins/:plugin_id           # Get plugin metadata
GET  /api/v1/plugins/search?q=<query>     # Search plugins
GET  /api/v1/plugins/:plugin_id/:version  # Download specific version
POST /api/v1/plugins                      # Submit new plugin
GET  /api/v1/plugins/:plugin_id/reviews   # Get community reviews
POST /api/v1/plugins/:plugin_id/reviews   # Post review
```

---

## 15. Plugin Installation Flow (UX)

### 15.1 User Flow

```
1. User discovers plugin (Marketplace panel or URL)
   ↓
2. Click "Install" button
   ↓
3. Manifest validation (version, dependencies)
   ↓
4. Show Capability Approval Dialog
   ├─ Required capabilities: [sqlite.query, filesystem.read]
   ├─ Optional capabilities: [network.http]
   ├─ Risk indicator: "Medium" (yellow)
   └─ [Cancel] [Approve and Install]
   ↓
5. If approved:
   - Extract plugin to .forge/plugins/<plugin_id>/
   - Save settings (enabled=false, capabilities_granted=[...])
   - Show "Installed! [Enable] [Settings]" toast
   ↓
6. User clicks "Enable"
   - Load WASM sandbox
   - Call on_load hook
   - Register commands, surfaces, etc.
   - Show "Plugin enabled" toast
```

### 15.2 Capability Approval Dialog

```
┌─────────────────────────────────────────┐
│ Install: Example Analyzer v1.2.0        │
│                                         │
│ This plugin requests the following:     │
│                                         │
│ REQUIRED CAPABILITIES:                  │
│ ✓ Read SQLite database                 │
│ ✓ Read project files                   │
│                                         │
│ OPTIONAL CAPABILITIES:                  │
│ ☐ Make HTTP requests                   │
│                                         │
│ Risk Level: Medium (Yellow indicator)   │
│                                         │
│ [Cancel] [Approve and Install]          │
└─────────────────────────────────────────┘
```

**Risk Levels:**
- **Low:** Read-only access, UI integration, events
- **Medium:** Write access, protocol handlers, network
- **High:** Unbounded resource access, system integration

---

## 16. Plugin Development Workflow

### 16.1 Scaffolding

```bash
$ nexus plugin scaffold --name "code-linter" --author "Jane"

Created:
nexus-plugins/code-linter/
├── Cargo.toml
├── manifest.toml
├── src/
│   └── lib.rs (minimal on_load stub)
├── tests/
├── schema.sql
├── settings.json
├── assets/
│   ├── icon-light.svg
│   └── icon-dark.svg
└── README.md
```

### 16.2 Development Cycle

```bash
$ cd nexus-plugins/code-linter

# Start dev server with hot-reload
$ cargo nexus-plugin-dev
   Compiling for wasm32-unknown-unknown...
   Plugin loaded: dev.nexus.code-linter
   Watching for changes...

# Edit src/lib.rs → auto-recompiles → auto-reloads

# Run tests (in WASM)
$ cargo nexus-plugin-test
   Running tests in wasm32-unknown-unknown...
   test test_lint ... ok
   test test_settings ... ok

# Build release WASM
$ cargo nexus-plugin-build --release
   Optimized WASM: 64 KB

# Package for distribution
$ cargo nexus-plugin-package
   Created: code-linter-1.0.0.nxplugin

# Sign package
$ cargo nexus-plugin-sign
   Signed: code-linter-1.0.0.nxplugin.sig

# Publish to registry (manual upload for now)
$ nexus plugin publish code-linter-1.0.0.nxplugin
   Uploading to registry...
   ✓ Published: dev.nexus.code-linter v1.0.0
```

---

## 17. Plugin Error Experience

### 17.1 Error Scenarios

| Scenario | User Experience |
|----------|-----------------|
| **Plugin fails to load** | Toast: "Failed to load plugin: sqlite-core (version mismatch)". Disabled. Suggest checking version. |
| **Missing dependency** | Toast: "code-linter requires sqlite-core ^1.0 (not installed)". Show install button. |
| **Execution timeout** | Toast: "Plugin 'analyzer' was terminated (execution timeout)". Suggest disabling or checking for infinite loops. |
| **Memory exhausted** | Toast: "Plugin 'renderer' exceeded memory limit (64 MB)". Suggest increasing memory limit in settings. |
| **Capability denied** | Toast: "Plugin 'network-tool' requested network.http (denied)". Show approval dialog. |
| **Unhandled panic** | Log: `[ERROR] Plugin sqlite-core panicked: <backtrace>`. Disable plugin. Notify user. |
| **Circular dependency** | Install error: "Dependency conflict: analyzer → linter → analyzer". Cannot install. |

### 17.2 Error Display

```rust
// nexus-ui/src/plugin_error_panel.rs

pub fn show_plugin_error(error: PluginError) {
    let (title, message, action) = match error {
        PluginError::FailedToLoad(reason) => (
            "Plugin Load Failed",
            &format!("Could not load plugin: {}", reason),
            Some(("Retry", retry_load_plugin)),
        ),
        PluginError::ExecutionTimeout => (
            "Plugin Timeout",
            "Plugin exceeded execution time. It has been disabled.",
            Some(("Re-enable", reenable_plugin)),
        ),
        PluginError::CapabilityDenied(cap) => (
            "Permission Required",
            &format!("Plugin needs '{}'. Approve in settings.", cap),
            Some(("Settings", open_plugin_settings)),
        ),
        _ => ("Error", &error.to_string(), None),
    };
    
    show_toast(ToastKind::Error, title, message, action);
}
```

---

## Acceptance Criteria

- [x] Manifest format complete with validation rules
- [x] WASM runtime decided (Wasmtime) and justified
- [x] Sandbox implementation with memory isolation and host functions
- [x] Plugin trait definitions with lifecycle methods
- [x] Capability enforcement system with runtime checking
- [x] Plugin packaging (.nxplugin) and distribution model
- [x] Hot-reload system for development
- [x] Plugin communication (events + service registry)
- [x] Settings system with JSON Schema UI generation
- [x] Dynamic loading for core plugins (.so/.dll)
- [x] Plugin discovery, dependency resolution, topological sort
- [x] Performance isolation and watchdog timer
- [x] Plugin registry architecture (design-level)
- [x] Installation flow (UX)
- [x] Development workflow and scaffolding
- [x] Error experience design

---

## Dependencies & Future Work

- **Wasmtime v18+:** Core runtime dependency
- **JSON Schema validation library:** For settings schema validation
- **libloading:** For dynamic .so/.dll loading
- **Semver library:** For version constraint checking
- **notify crate:** For file watcher in hot-reload
- **Plugin registry:** REST API, web UI, signing infrastructure (Phase 2)
- **Plugin marketplace:** Community submission, review process (Phase 2)

---

**Document Version:** 1.0  
**Last Updated:** April 2026  
**Status:** Ready for Implementation
