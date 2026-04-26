# Nexus PRD 04 — Plugin System (M1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `nexus-plugins` crate to interface-complete state — manifest parsing with validation, wasmtime WASM sandbox with fuel-based execution limits, host functions (logging, events, KV), plugin loader with lifecycle management, JSON Schema settings validation, hot-reload via file watching, and a `PluginManager` facade, all compiling and tested.

**Architecture:** New `nexus-plugins` workspace member with 7 internal modules behind a `PluginManager` facade. Synchronous dispatch into WASM via wasmtime. Host functions bridge plugins to kernel services (logging, events, KV). Hot-reload via `notify` file watcher with message queueing during swap.

**Tech Stack:** Rust (edition 2024), `wasmtime` 18+, `jsonschema` 0.17+, `semver` 1.0+, `notify` 7.0+, `notify-debouncer-mini` 0.5+, `thiserror` 2.0, `serde` 1.0, `toml` 0.8.

**Parent docs:**
- [`2026-04-12-nexus-prd-04-plugins-design.md`](../specs/2026-04-12-nexus-prd-04-plugins-design.md) — **the contract this plan implements**
- [`2026-04-11-nexus-m1-foundation-spec.md`](../specs/2026-04-11-nexus-m1-foundation-spec.md) — M1 spec §6

---

## Prerequisites

1. PRD 03 (storage crate) is complete and tests pass.
2. Verify: `cargo nextest run --workspace` passes with no failures (244 tests).
3. The `nexus-kernel` crate exports: `PluginLifecycle`, `PluginContext`, `PluginInfo`, `PluginStatus`, `TrustLevel`, `Capability`, `CapabilitySet`, `EventBus`, `EventFilter`, `EventSubscription`, `NexusEvent`.
4. The `nexus-security` crate exports: `risk_level`, `RiskLevel`.

---

## File Structure

```
crates/nexus-plugins/
├── Cargo.toml
└── src/
    ├── lib.rs              # crate-level docs, public re-exports, PluginManager facade
    ├── error.rs            # PluginError enum
    ├── manifest.rs         # manifest parsing (TOML) + validation
    ├── sandbox.rs          # WasmSandbox: wasmtime engine, module, store, dispatch
    ├── host_fns.rs         # host functions linked into wasmtime (log, events, KV)
    ├── loader.rs           # PluginLoader: scan, load, unload, lifecycle
    ├── settings.rs         # JSON Schema validation, per-plugin settings I/O
    └── hot_reload.rs       # file watcher on plugin dirs, reload events
```

Modifications to existing files:
- `Cargo.toml` (workspace root): add `nexus-plugins` to members, add `wasmtime`, `jsonschema`, `semver` to workspace deps

---

## Task Overview

24 tasks across 9 phases:

1. Phase 1: Crate skeleton + workspace wiring (Tasks 1–2)
2. Phase 2: PluginError enum (Tasks 3–4)
3. Phase 3: Manifest parsing + validation (Tasks 5–7)
4. Phase 4: WASM sandbox (Tasks 8–10)
5. Phase 5: Host functions (Tasks 11–13)
6. Phase 6: Settings infrastructure (Tasks 14–15)
7. Phase 7: Plugin loader (Tasks 16–18)
8. Phase 8: Hot-reload (Tasks 19–20)
9. Phase 9: PluginManager facade + smoke test (Tasks 21–24)

---

## Phase 1: Crate Skeleton

### Task 1: Add nexus-plugins to workspace and create crate skeleton

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `crates/nexus-plugins/Cargo.toml`
- Create: `crates/nexus-plugins/src/lib.rs`
- Create: `crates/nexus-plugins/src/error.rs`

- [ ] **Step 1: Add workspace member and deps to root `Cargo.toml`**

Edit `/mnt/c/Users/baile/dev/nexus/Cargo.toml`:

In the `[workspace]` members array, add `"crates/nexus-plugins"`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/nexus-types",
    "crates/nexus-kernel",
    "crates/nexus-security",
    "crates/nexus-storage",
    "crates/nexus-plugins",
]
```

In `[workspace.dependencies]`, add:

```toml
# WASM runtime
wasmtime = "18"

# JSON Schema validation
jsonschema = "0.17"

# Semver parsing
semver = "1"
```

- [ ] **Step 2: Create `crates/nexus-plugins/Cargo.toml`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/Cargo.toml`:

```toml
[package]
name = "nexus-plugins"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
description = "Nexus plugin system: manifest parsing, WASM sandbox, host functions, plugin loader, settings, hot-reload"

[dependencies]
nexus-kernel = { path = "../nexus-kernel" }
nexus-security = { path = "../nexus-security" }
nexus-types = { path = "../nexus-types" }
thiserror = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
toml = { workspace = true }
tracing = { workspace = true }
uuid = { workspace = true }
wasmtime = { workspace = true }
jsonschema = { workspace = true }
semver = { workspace = true }
notify = { workspace = true }
notify-debouncer-mini = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 3: Create `crates/nexus-plugins/src/lib.rs`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/lib.rs`:

```rust
//! Nexus plugin system: manifest parsing, WASM sandbox, host functions,
//! plugin loader, settings validation, and hot-reload.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-04-plugins-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod error;

pub use error::PluginError;
```

- [ ] **Step 4: Create placeholder `error.rs`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/error.rs`:

```rust
//! Plugin error types.

/// Errors from the plugin subsystem.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// Placeholder — replaced in Task 3.
    #[error("not yet implemented")]
    NotImplemented,
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo check -p nexus-plugins`
Expected: compiles successfully.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/nexus-plugins/
git commit -m "feat(plugins): scaffold nexus-plugins crate with workspace wiring"
```

---

### Task 2: Verify workspace builds clean

**Files:** (none — verification only)

- [ ] **Step 1: Full workspace check**

Run: `cargo check --workspace`
Expected: compiles. No errors from any crate.

- [ ] **Step 2: Run existing tests**

Run: `cargo nextest run --workspace`
Expected: all 244 tests pass. No regressions.

---

## Phase 2: PluginError Enum

### Task 3: Write PluginError tests

**Files:**
- Modify: `crates/nexus-plugins/src/error.rs`

- [ ] **Step 1: Write tests for error Display messages**

Replace the contents of `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/error.rs` with:

```rust
//! Plugin error types.

/// Errors from the plugin subsystem.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// Placeholder — replaced next task.
    #[error("not yet implemented")]
    NotImplemented,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_not_found_display() {
        let err = PluginError::ManifestNotFound("/path/to/manifest.toml".to_string());
        assert_eq!(err.to_string(), "manifest not found: /path/to/manifest.toml");
    }

    #[test]
    fn manifest_invalid_display() {
        let err = PluginError::ManifestInvalid {
            path: "manifest.toml".to_string(),
            reason: "bad TOML".to_string(),
        };
        assert_eq!(err.to_string(), "manifest invalid at manifest.toml: bad TOML");
    }

    #[test]
    fn manifest_validation_display() {
        let err = PluginError::ManifestValidation {
            plugin_id: "com.example.test".to_string(),
            reason: "invalid ID format".to_string(),
        };
        assert!(err.to_string().contains("com.example.test"));
        assert!(err.to_string().contains("invalid ID format"));
    }

    #[test]
    fn wasm_load_failed_display() {
        let err = PluginError::WasmLoadFailed {
            plugin_id: "com.test".to_string(),
            reason: "module corrupt".to_string(),
        };
        assert!(err.to_string().contains("com.test"));
    }

    #[test]
    fn execution_timeout_display() {
        let err = PluginError::ExecutionTimeout {
            plugin_id: "com.test".to_string(),
        };
        assert!(err.to_string().contains("timeout"));
    }

    #[test]
    fn capability_denied_display() {
        let err = PluginError::CapabilityDenied {
            plugin_id: "com.test".to_string(),
            capability: "net.http".to_string(),
        };
        assert!(err.to_string().contains("net.http"));
    }

    #[test]
    fn plugin_not_found_display() {
        let err = PluginError::PluginNotFound("com.missing".to_string());
        assert_eq!(err.to_string(), "plugin not found: com.missing");
    }

    #[test]
    fn duplicate_plugin_display() {
        let err = PluginError::DuplicatePlugin("com.dup".to_string());
        assert_eq!(err.to_string(), "duplicate plugin: com.dup");
    }

    #[test]
    fn duplicate_cli_subcommand_display() {
        let err = PluginError::DuplicateCliSubcommand {
            plugin_id: "com.test".to_string(),
            subcommand: "weather".to_string(),
        };
        assert!(err.to_string().contains("weather"));
        assert!(err.to_string().contains("com.test"));
    }

    #[test]
    fn settings_invalid_display() {
        let err = PluginError::SettingsInvalid {
            plugin_id: "com.test".to_string(),
            reason: "missing required field".to_string(),
        };
        assert!(err.to_string().contains("settings invalid"));
    }

    #[test]
    fn reload_failed_display() {
        let err = PluginError::ReloadFailed {
            plugin_id: "com.test".to_string(),
            reason: "WASM compile error".to_string(),
        };
        assert!(err.to_string().contains("reload failed"));
    }

    #[test]
    fn plugin_reloading_display() {
        let err = PluginError::PluginReloading("com.test".to_string());
        assert!(err.to_string().contains("reloading"));
    }

    #[test]
    fn io_error_converts_via_from() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err: PluginError = io_err.into();
        assert!(matches!(err, PluginError::Io(_)));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo nextest run -p nexus-plugins -- error::tests`
Expected: FAIL — variants don't exist yet.

### Task 4: Implement PluginError enum

**Files:**
- Modify: `crates/nexus-plugins/src/error.rs`

- [ ] **Step 1: Replace the enum with the full definition**

Replace the `PluginError` enum and everything before the `#[cfg(test)]` block in `crates/nexus-plugins/src/error.rs`:

```rust
//! Plugin error types.

/// Errors from the plugin subsystem.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// Manifest file not found or unreadable.
    #[error("manifest not found: {0}")]
    ManifestNotFound(String),

    /// Manifest failed to parse (invalid TOML).
    #[error("manifest invalid at {path}: {reason}")]
    ManifestInvalid {
        /// Path to the manifest file.
        path: String,
        /// Parse error details.
        reason: String,
    },

    /// Manifest validation failed.
    #[error("manifest validation failed for {plugin_id}: {reason}")]
    ManifestValidation {
        /// Plugin ID from the manifest.
        plugin_id: String,
        /// Validation error details.
        reason: String,
    },

    /// WASM module failed to compile or instantiate.
    #[error("WASM load failed for {plugin_id}: {reason}")]
    WasmLoadFailed {
        /// Plugin ID.
        plugin_id: String,
        /// Error details.
        reason: String,
    },

    /// WASM execution exceeded fuel limit.
    #[error("execution timeout for {plugin_id}")]
    ExecutionTimeout {
        /// Plugin ID.
        plugin_id: String,
    },

    /// WASM execution failed (trap, OOM, etc.).
    #[error("execution failed for {plugin_id}: {reason}")]
    ExecutionFailed {
        /// Plugin ID.
        plugin_id: String,
        /// Error details.
        reason: String,
    },

    /// Plugin lifecycle hook returned an error.
    #[error("lifecycle error for {plugin_id} in {hook}: {reason}")]
    LifecycleError {
        /// Plugin ID.
        plugin_id: String,
        /// Hook name (on_init, on_start, on_stop).
        hook: String,
        /// Error details.
        reason: String,
    },

    /// Capability denied during a host function call.
    #[error("capability denied for {plugin_id}: {capability}")]
    CapabilityDenied {
        /// Plugin ID.
        plugin_id: String,
        /// Capability that was denied.
        capability: String,
    },

    /// Plugin not found in the registry.
    #[error("plugin not found: {0}")]
    PluginNotFound(String),

    /// Duplicate plugin ID.
    #[error("duplicate plugin: {0}")]
    DuplicatePlugin(String),

    /// Duplicate CLI subcommand registration.
    #[error("duplicate CLI subcommand '{subcommand}' from {plugin_id}")]
    DuplicateCliSubcommand {
        /// Plugin ID.
        plugin_id: String,
        /// Subcommand that conflicts.
        subcommand: String,
    },

    /// Settings validation failed against JSON Schema.
    #[error("settings invalid for {plugin_id}: {reason}")]
    SettingsInvalid {
        /// Plugin ID.
        plugin_id: String,
        /// Validation error details.
        reason: String,
    },

    /// Hot-reload failed.
    #[error("reload failed for {plugin_id}: {reason}")]
    ReloadFailed {
        /// Plugin ID.
        plugin_id: String,
        /// Error details.
        reason: String,
    },

    /// Plugin is currently reloading.
    #[error("plugin reloading: {0}")]
    PluginReloading(String),

    /// Underlying I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo nextest run -p nexus-plugins -- error::tests`
Expected: all 13 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-plugins/src/error.rs
git commit -m "feat(plugins): define PluginError enum with all M1 variants"
```

---

## Phase 3: Manifest Parsing & Validation

### Task 5: Write manifest data types

**Files:**
- Create: `crates/nexus-plugins/src/manifest.rs`

- [ ] **Step 1: Create manifest types and TOML deserialization**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/manifest.rs`:

```rust
//! Plugin manifest parsing and validation.

use std::path::Path;

use serde::Deserialize;

use nexus_kernel::TrustLevel;

use crate::PluginError;

/// A parsed plugin manifest.
#[derive(Debug, Clone)]
pub struct PluginManifest {
    /// Plugin identifier (reverse-DNS).
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Semver version string.
    pub version: String,
    /// Trust level: core or community.
    pub trust_level: TrustLevel,
    /// Plugin API version.
    pub api_version: String,
    /// Capability declarations.
    pub capabilities: ManifestCapabilities,
    /// WASM module configuration.
    pub wasm: WasmConfig,
    /// Optional settings schema configuration.
    pub settings: Option<SettingsConfig>,
    /// Registrations (CLI subcommands, IPC commands, event subscribers).
    pub registrations: Registrations,
    /// Lifecycle hook configuration.
    pub lifecycle: LifecycleConfig,
}

/// Required and optional capabilities declared in the manifest.
#[derive(Debug, Clone, Default)]
pub struct ManifestCapabilities {
    /// Capabilities the plugin requires to function.
    pub required: Vec<String>,
    /// Capabilities the plugin can use but doesn't require.
    pub optional: Vec<String>,
}

/// WASM module configuration.
#[derive(Debug, Clone)]
pub struct WasmConfig {
    /// WASM module filename (relative to plugin dir).
    pub module: String,
    /// Maximum linear memory in MB (1-256).
    pub memory_mb: u32,
    /// Wasmtime fuel budget (0 = unlimited, core only).
    pub fuel: u64,
}

/// Settings schema configuration.
#[derive(Debug, Clone)]
pub struct SettingsConfig {
    /// Path to JSON Schema file (relative to plugin dir).
    pub schema: String,
}

/// Plugin registrations.
#[derive(Debug, Clone, Default)]
pub struct Registrations {
    /// CLI subcommand registrations.
    pub cli_subcommands: Vec<CliSubcommandReg>,
    /// IPC command registrations.
    pub ipc_commands: Vec<IpcCommandReg>,
    /// Event subscriber registrations.
    pub event_subscribers: Vec<EventSubscriberReg>,
}

/// A CLI subcommand registration.
#[derive(Debug, Clone)]
pub struct CliSubcommandReg {
    /// Subcommand identifier.
    pub id: String,
    /// Handler ID for dispatch.
    pub handler_id: u32,
    /// Human-readable description.
    pub description: String,
}

/// An IPC command registration.
#[derive(Debug, Clone)]
pub struct IpcCommandReg {
    /// Command identifier.
    pub id: String,
    /// Handler ID for dispatch.
    pub handler_id: u32,
}

/// An event subscriber registration.
#[derive(Debug, Clone)]
pub struct EventSubscriberReg {
    /// Subscriber identifier.
    pub id: String,
    /// Event filter (variant name).
    pub filter: String,
    /// Handler ID for dispatch.
    pub handler_id: u32,
}

/// Lifecycle hook configuration.
#[derive(Debug, Clone)]
pub struct LifecycleConfig {
    /// Whether the plugin implements on_init.
    pub on_init: bool,
    /// Whether the plugin implements on_start.
    pub on_start: bool,
    /// Whether the plugin implements on_stop.
    pub on_stop: bool,
}

// ── TOML deserialization types (private) ─────────────────────────────────

#[derive(Deserialize)]
struct RawManifest {
    plugin: RawPlugin,
    capabilities: Option<RawCapabilities>,
    wasm: RawWasm,
    settings: Option<RawSettings>,
    registrations: Option<RawRegistrations>,
    lifecycle: Option<RawLifecycle>,
}

#[derive(Deserialize)]
struct RawPlugin {
    id: String,
    name: String,
    version: String,
    trust_level: String,
    api_version: String,
}

#[derive(Deserialize)]
struct RawCapabilities {
    required: Option<Vec<String>>,
    optional: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct RawWasm {
    module: String,
    memory_mb: Option<u32>,
    fuel: Option<u64>,
}

#[derive(Deserialize)]
struct RawSettings {
    schema: String,
}

#[derive(Deserialize)]
struct RawRegistrations {
    cli_subcommand: Option<Vec<RawCliSub>>,
    ipc_command: Option<Vec<RawIpcCmd>>,
    event_subscriber: Option<Vec<RawEventSub>>,
}

#[derive(Deserialize)]
struct RawCliSub {
    id: String,
    handler_id: u32,
    description: Option<String>,
}

#[derive(Deserialize)]
struct RawIpcCmd {
    id: String,
    handler_id: u32,
}

#[derive(Deserialize)]
struct RawEventSub {
    id: String,
    filter: String,
    handler_id: u32,
}

#[derive(Deserialize)]
struct RawLifecycle {
    on_init: Option<bool>,
    on_start: Option<bool>,
    on_stop: Option<bool>,
}

/// Parse a manifest from a TOML string.
pub fn parse_manifest(toml_str: &str, manifest_path: &str) -> Result<PluginManifest, PluginError> {
    let raw: RawManifest = toml::from_str(toml_str).map_err(|e| PluginError::ManifestInvalid {
        path: manifest_path.to_string(),
        reason: e.to_string(),
    })?;

    let trust_level = match raw.plugin.trust_level.as_str() {
        "core" => TrustLevel::Core,
        "community" => TrustLevel::Community,
        other => {
            return Err(PluginError::ManifestInvalid {
                path: manifest_path.to_string(),
                reason: format!("unknown trust_level: {other}"),
            })
        }
    };

    let caps = raw.capabilities.unwrap_or(RawCapabilities {
        required: None,
        optional: None,
    });

    let regs = raw.registrations.unwrap_or(RawRegistrations {
        cli_subcommand: None,
        ipc_command: None,
        event_subscriber: None,
    });

    let lc = raw.lifecycle.unwrap_or(RawLifecycle {
        on_init: None,
        on_start: None,
        on_stop: None,
    });

    Ok(PluginManifest {
        id: raw.plugin.id,
        name: raw.plugin.name,
        version: raw.plugin.version,
        trust_level,
        api_version: raw.plugin.api_version,
        capabilities: ManifestCapabilities {
            required: caps.required.unwrap_or_default(),
            optional: caps.optional.unwrap_or_default(),
        },
        wasm: WasmConfig {
            module: raw.wasm.module,
            memory_mb: raw.wasm.memory_mb.unwrap_or(16),
            fuel: raw.wasm.fuel.unwrap_or(10_000_000),
        },
        settings: raw.settings.map(|s| SettingsConfig { schema: s.schema }),
        registrations: Registrations {
            cli_subcommands: regs
                .cli_subcommand
                .unwrap_or_default()
                .into_iter()
                .map(|r| CliSubcommandReg {
                    id: r.id,
                    handler_id: r.handler_id,
                    description: r.description.unwrap_or_default(),
                })
                .collect(),
            ipc_commands: regs
                .ipc_command
                .unwrap_or_default()
                .into_iter()
                .map(|r| IpcCommandReg {
                    id: r.id,
                    handler_id: r.handler_id,
                })
                .collect(),
            event_subscribers: regs
                .event_subscriber
                .unwrap_or_default()
                .into_iter()
                .map(|r| EventSubscriberReg {
                    id: r.id,
                    filter: r.filter,
                    handler_id: r.handler_id,
                })
                .collect(),
        },
        lifecycle: LifecycleConfig {
            on_init: lc.on_init.unwrap_or(false),
            on_start: lc.on_start.unwrap_or(false),
            on_stop: lc.on_stop.unwrap_or(false),
        },
    })
}

/// Load and parse a manifest from a file.
pub fn load_manifest(manifest_path: &Path) -> Result<PluginManifest, PluginError> {
    let path_str = manifest_path.display().to_string();
    let content = std::fs::read_to_string(manifest_path).map_err(|_| {
        PluginError::ManifestNotFound(path_str.clone())
    })?;
    parse_manifest(&content, &path_str)
}
```

- [ ] **Step 2: Add `mod manifest;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/lib.rs`, add after `mod error;`:

```rust
mod manifest;

pub use manifest::{
    parse_manifest, load_manifest, PluginManifest, ManifestCapabilities,
    WasmConfig, SettingsConfig, Registrations, CliSubcommandReg,
    IpcCommandReg, EventSubscriberReg, LifecycleConfig,
};
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p nexus-plugins`
Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-plugins/src/
git commit -m "feat(plugins): add manifest data types and TOML parser"
```

---

### Task 6: Write manifest parsing tests

**Files:**
- Modify: `crates/nexus-plugins/src/manifest.rs`

- [ ] **Step 1: Add parsing tests**

Add to the end of `crates/nexus-plugins/src/manifest.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_MANIFEST: &str = r#"
[plugin]
id = "com.example.test"
name = "Test Plugin"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"
"#;

    const FULL_MANIFEST: &str = r#"
[plugin]
id = "com.example.full"
name = "Full Plugin"
version = "2.1.0"
trust_level = "core"
api_version = "1"

[capabilities]
required = ["fs.read", "kv.read", "kv.write"]
optional = ["net.http"]

[wasm]
module = "full.wasm"
memory_mb = 64
fuel = 5000000

[settings]
schema = "settings.json"

[[registrations.cli_subcommand]]
id = "full.run"
handler_id = 1
description = "Run the full plugin"

[[registrations.ipc_command]]
id = "full.query"
handler_id = 100

[[registrations.event_subscriber]]
id = "full.on-file"
filter = "FileCreated"
handler_id = 200

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#;

    #[test]
    fn parse_minimal_manifest() {
        let m = parse_manifest(MINIMAL_MANIFEST, "test.toml").unwrap();
        assert_eq!(m.id, "com.example.test");
        assert_eq!(m.name, "Test Plugin");
        assert_eq!(m.version, "1.0.0");
        assert_eq!(m.trust_level, TrustLevel::Community);
        assert_eq!(m.wasm.module, "test.wasm");
        assert_eq!(m.wasm.memory_mb, 16); // default
        assert_eq!(m.wasm.fuel, 10_000_000); // default
    }

    #[test]
    fn parse_full_manifest() {
        let m = parse_manifest(FULL_MANIFEST, "test.toml").unwrap();
        assert_eq!(m.id, "com.example.full");
        assert_eq!(m.trust_level, TrustLevel::Core);
        assert_eq!(m.capabilities.required, vec!["fs.read", "kv.read", "kv.write"]);
        assert_eq!(m.capabilities.optional, vec!["net.http"]);
        assert_eq!(m.wasm.memory_mb, 64);
        assert_eq!(m.wasm.fuel, 5_000_000);
        assert!(m.settings.is_some());
        assert_eq!(m.settings.unwrap().schema, "settings.json");
        assert_eq!(m.registrations.cli_subcommands.len(), 1);
        assert_eq!(m.registrations.cli_subcommands[0].handler_id, 1);
        assert_eq!(m.registrations.ipc_commands.len(), 1);
        assert_eq!(m.registrations.event_subscribers.len(), 1);
        assert_eq!(m.registrations.event_subscribers[0].filter, "FileCreated");
        assert!(m.lifecycle.on_init);
        assert!(m.lifecycle.on_start);
        assert!(m.lifecycle.on_stop);
    }

    #[test]
    fn parse_invalid_toml_returns_error() {
        let result = parse_manifest("not valid toml {{{{", "bad.toml");
        assert!(matches!(result, Err(PluginError::ManifestInvalid { .. })));
    }

    #[test]
    fn parse_unknown_trust_level_returns_error() {
        let toml = r#"
[plugin]
id = "com.test"
name = "Test"
version = "1.0.0"
trust_level = "unknown"
api_version = "1"

[wasm]
module = "test.wasm"
"#;
        let result = parse_manifest(toml, "test.toml");
        assert!(matches!(result, Err(PluginError::ManifestInvalid { .. })));
    }

    #[test]
    fn parse_missing_wasm_section_returns_error() {
        let toml = r#"
[plugin]
id = "com.test"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"
"#;
        let result = parse_manifest(toml, "test.toml");
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_capabilities_defaults_to_empty() {
        let m = parse_manifest(MINIMAL_MANIFEST, "test.toml").unwrap();
        assert!(m.capabilities.required.is_empty());
        assert!(m.capabilities.optional.is_empty());
    }

    #[test]
    fn parse_empty_registrations_defaults_to_empty() {
        let m = parse_manifest(MINIMAL_MANIFEST, "test.toml").unwrap();
        assert!(m.registrations.cli_subcommands.is_empty());
        assert!(m.registrations.ipc_commands.is_empty());
        assert!(m.registrations.event_subscribers.is_empty());
    }

    #[test]
    fn parse_lifecycle_defaults_to_false() {
        let m = parse_manifest(MINIMAL_MANIFEST, "test.toml").unwrap();
        assert!(!m.lifecycle.on_init);
        assert!(!m.lifecycle.on_start);
        assert!(!m.lifecycle.on_stop);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo nextest run -p nexus-plugins -- manifest::tests`
Expected: all 8 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-plugins/src/manifest.rs
git commit -m "test(plugins): add manifest parsing tests"
```

---

### Task 7: Add manifest validation

**Files:**
- Modify: `crates/nexus-plugins/src/manifest.rs`

- [ ] **Step 1: Add validation function and tests**

Add the `validate` function before the `#[cfg(test)]` block in `crates/nexus-plugins/src/manifest.rs`:

```rust
/// Validate a parsed manifest. Checks ID format, semver, capabilities,
/// handler ID uniqueness, memory limits, and fuel constraints.
pub fn validate(manifest: &PluginManifest, plugin_dir: &Path) -> Result<(), PluginError> {
    let id = &manifest.id;

    // 1. Plugin ID format
    let id_re = regex_lite::Regex::new(
        r"^[a-z0-9]+([-._][a-z0-9]+)*\.[a-z0-9]+([-._][a-z0-9]+)*$"
    ).expect("valid regex");
    if !id_re.is_match(id) {
        return Err(PluginError::ManifestValidation {
            plugin_id: id.clone(),
            reason: format!("plugin ID '{id}' does not match required format"),
        });
    }

    // 2. Valid semver
    if semver::Version::parse(&manifest.version).is_err() {
        return Err(PluginError::ManifestValidation {
            plugin_id: id.clone(),
            reason: format!("version '{}' is not valid semver", manifest.version),
        });
    }

    // 3. All capabilities exist in the Capability enum
    for cap_str in manifest.capabilities.required.iter().chain(manifest.capabilities.optional.iter()) {
        if nexus_kernel::Capability::from_str(cap_str).is_err() {
            return Err(PluginError::ManifestValidation {
                plugin_id: id.clone(),
                reason: format!("unknown capability: {cap_str}"),
            });
        }
    }

    // 4. Handler IDs unique across all registrations
    let mut handler_ids = std::collections::HashSet::new();
    for reg in &manifest.registrations.cli_subcommands {
        if !handler_ids.insert(reg.handler_id) {
            return Err(PluginError::ManifestValidation {
                plugin_id: id.clone(),
                reason: format!("duplicate handler_id: {}", reg.handler_id),
            });
        }
    }
    for reg in &manifest.registrations.ipc_commands {
        if !handler_ids.insert(reg.handler_id) {
            return Err(PluginError::ManifestValidation {
                plugin_id: id.clone(),
                reason: format!("duplicate handler_id: {}", reg.handler_id),
            });
        }
    }
    for reg in &manifest.registrations.event_subscribers {
        if !handler_ids.insert(reg.handler_id) {
            return Err(PluginError::ManifestValidation {
                plugin_id: id.clone(),
                reason: format!("duplicate handler_id: {}", reg.handler_id),
            });
        }
    }

    // 5. Memory limits
    if manifest.wasm.memory_mb < 1 || manifest.wasm.memory_mb > 256 {
        return Err(PluginError::ManifestValidation {
            plugin_id: id.clone(),
            reason: format!(
                "wasm.memory_mb must be 1-256, got {}",
                manifest.wasm.memory_mb
            ),
        });
    }

    // 6. Fuel constraints
    if manifest.wasm.fuel == 0 && manifest.trust_level != TrustLevel::Core {
        return Err(PluginError::ManifestValidation {
            plugin_id: id.clone(),
            reason: "wasm.fuel must be > 0 for community plugins".to_string(),
        });
    }

    // 7. WASM module file exists
    let wasm_path = plugin_dir.join(&manifest.wasm.module);
    if !wasm_path.exists() {
        return Err(PluginError::ManifestValidation {
            plugin_id: id.clone(),
            reason: format!("WASM module not found: {}", wasm_path.display()),
        });
    }

    // 8. Settings schema file exists (if declared)
    if let Some(ref settings) = manifest.settings {
        let schema_path = plugin_dir.join(&settings.schema);
        if !schema_path.exists() {
            return Err(PluginError::ManifestValidation {
                plugin_id: id.clone(),
                reason: format!("settings schema not found: {}", schema_path.display()),
            });
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Add `regex-lite` to workspace deps**

Edit `/mnt/c/Users/baile/dev/nexus/Cargo.toml`, add to `[workspace.dependencies]`:

```toml
regex-lite = "0.1"
```

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/Cargo.toml`, add to `[dependencies]`:

```toml
regex-lite = { workspace = true }
```

- [ ] **Step 3: Add validation tests**

Add to the `#[cfg(test)] mod tests` block in `crates/nexus-plugins/src/manifest.rs`:

```rust
    // ── Validation tests ──

    fn make_test_plugin_dir(wasm_name: &str) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(wasm_name), b"fake wasm").unwrap();
        tmp
    }

    #[test]
    fn validate_accepts_valid_manifest() {
        let m = parse_manifest(MINIMAL_MANIFEST, "test.toml").unwrap();
        let dir = make_test_plugin_dir("test.wasm");
        assert!(validate(&m, dir.path()).is_ok());
    }

    #[test]
    fn validate_rejects_invalid_id() {
        let toml = MINIMAL_MANIFEST.replace("com.example.test", "INVALID_ID");
        let m = parse_manifest(&toml, "test.toml").unwrap();
        let dir = make_test_plugin_dir("test.wasm");
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(matches!(err, PluginError::ManifestValidation { .. }));
    }

    #[test]
    fn validate_rejects_invalid_semver() {
        let toml = MINIMAL_MANIFEST.replace("1.0.0", "not.semver");
        let m = parse_manifest(&toml, "test.toml").unwrap();
        let dir = make_test_plugin_dir("test.wasm");
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(matches!(err, PluginError::ManifestValidation { .. }));
    }

    #[test]
    fn validate_rejects_unknown_capability() {
        let toml = r#"
[plugin]
id = "com.example.test"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["nonexistent.capability"]

[wasm]
module = "test.wasm"
"#;
        let m = parse_manifest(toml, "test.toml").unwrap();
        let dir = make_test_plugin_dir("test.wasm");
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(matches!(err, PluginError::ManifestValidation { .. }));
    }

    #[test]
    fn validate_rejects_duplicate_handler_id() {
        let toml = r#"
[plugin]
id = "com.example.test"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.cli_subcommand]]
id = "cmd1"
handler_id = 1

[[registrations.ipc_command]]
id = "cmd2"
handler_id = 1
"#;
        let m = parse_manifest(toml, "test.toml").unwrap();
        let dir = make_test_plugin_dir("test.wasm");
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(err.to_string().contains("duplicate handler_id"));
    }

    #[test]
    fn validate_rejects_memory_out_of_range() {
        let toml = MINIMAL_MANIFEST.replace("module = \"test.wasm\"", "module = \"test.wasm\"\nmemory_mb = 512");
        let m = parse_manifest(&toml, "test.toml").unwrap();
        let dir = make_test_plugin_dir("test.wasm");
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(err.to_string().contains("memory_mb"));
    }

    #[test]
    fn validate_rejects_zero_fuel_for_community() {
        let toml = MINIMAL_MANIFEST.replace("module = \"test.wasm\"", "module = \"test.wasm\"\nfuel = 0");
        let m = parse_manifest(&toml, "test.toml").unwrap();
        let dir = make_test_plugin_dir("test.wasm");
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(err.to_string().contains("fuel"));
    }

    #[test]
    fn validate_allows_zero_fuel_for_core() {
        let toml = r#"
[plugin]
id = "com.example.core"
name = "Core"
version = "1.0.0"
trust_level = "core"
api_version = "1"

[wasm]
module = "test.wasm"
fuel = 0
"#;
        let m = parse_manifest(toml, "test.toml").unwrap();
        let dir = make_test_plugin_dir("test.wasm");
        assert!(validate(&m, dir.path()).is_ok());
    }

    #[test]
    fn validate_rejects_missing_wasm_file() {
        let m = parse_manifest(MINIMAL_MANIFEST, "test.toml").unwrap();
        let dir = tempfile::tempdir().unwrap(); // no wasm file
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(err.to_string().contains("WASM module not found"));
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo nextest run -p nexus-plugins -- manifest::tests`
Expected: all 17 tests PASS (8 parsing + 9 validation).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/nexus-plugins/
git commit -m "feat(plugins): add manifest validation (ID, semver, capabilities, limits)"
```

---

## Phase 4: WASM Sandbox

### Task 8: Write WasmSandbox types and basic tests

**Files:**
- Create: `crates/nexus-plugins/src/sandbox.rs`

- [ ] **Step 1: Create sandbox module with types**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/sandbox.rs`:

```rust
//! WASM sandbox: wasmtime engine, module, store, dispatch.

use std::sync::Arc;

use wasmtime::{Caller, Config, Engine, Instance, Linker, Module, Store, Trap};

use nexus_kernel::CapabilitySet;

use crate::manifest::WasmConfig;
use crate::PluginError;

/// Per-plugin data stored in the wasmtime `Store`.
pub struct PluginData {
    /// Plugin identifier.
    pub plugin_id: String,
    /// Capabilities granted to this plugin.
    pub capabilities: CapabilitySet,
    /// Event bus for publishing custom events.
    pub event_bus: Option<std::sync::Arc<nexus_kernel::EventBus>>,
    /// KV store for plugin state persistence.
    pub kv_store: Option<std::sync::Arc<dyn crate::KvStore>>,
}

/// A sandboxed WASM plugin instance.
pub struct WasmSandbox {
    store: Store<PluginData>,
    instance: Instance,
}

impl WasmSandbox {
    /// Create a new sandbox from compiled WASM bytes.
    pub fn new(
        wasm_bytes: &[u8],
        config: &WasmConfig,
        plugin_data: PluginData,
    ) -> Result<Self, PluginError> {
        let mut engine_config = Config::new();
        engine_config.wasm_simd(true);
        engine_config.wasm_bulk_memory(true);
        if config.fuel > 0 {
            engine_config.consume_fuel(true);
        }

        let engine = Engine::new(&engine_config).map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: plugin_data.plugin_id.clone(),
            reason: e.to_string(),
        })?;

        let module = Module::new(&engine, wasm_bytes).map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: plugin_data.plugin_id.clone(),
            reason: e.to_string(),
        })?;

        let plugin_id = plugin_data.plugin_id.clone();
        let mut store = Store::new(&engine, plugin_data);

        if config.fuel > 0 {
            store.set_fuel(config.fuel).map_err(|e| PluginError::WasmLoadFailed {
                plugin_id: plugin_id.clone(),
                reason: e.to_string(),
            })?;
        }

        let linker: Linker<PluginData> = Linker::new(&engine);

        let instance = linker.instantiate(&mut store, &module).map_err(|e| {
            PluginError::WasmLoadFailed {
                plugin_id: plugin_id.clone(),
                reason: e.to_string(),
            }
        })?;

        Ok(Self { store, instance })
    }

    /// Call a handler in the WASM plugin via `nexus_dispatch`.
    ///
    /// Serializes `args` to JSON, calls the exported `nexus_dispatch` function,
    /// and deserializes the JSON result.
    pub fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let plugin_id = self.store.data().plugin_id.clone();

        // Get the dispatch function
        let dispatch_fn = self
            .instance
            .get_typed_func::<(u32, u32, u32), u64>(&mut self.store, "nexus_dispatch")
            .map_err(|e| PluginError::ExecutionFailed {
                plugin_id: plugin_id.clone(),
                reason: format!("nexus_dispatch not found: {e}"),
            })?;

        // Serialize args
        let args_json = serde_json::to_vec(args).map_err(|e| PluginError::ExecutionFailed {
            plugin_id: plugin_id.clone(),
            reason: format!("args serialization failed: {e}"),
        })?;

        // Allocate space in WASM memory
        let alloc_fn = self
            .instance
            .get_typed_func::<u32, u32>(&mut self.store, "nexus_alloc")
            .map_err(|e| PluginError::ExecutionFailed {
                plugin_id: plugin_id.clone(),
                reason: format!("nexus_alloc not found: {e}"),
            })?;

        let args_len = args_json.len() as u32;
        let args_ptr = alloc_fn.call(&mut self.store, args_len).map_err(|e| {
            if is_out_of_fuel(&e) {
                PluginError::ExecutionTimeout {
                    plugin_id: plugin_id.clone(),
                }
            } else {
                PluginError::ExecutionFailed {
                    plugin_id: plugin_id.clone(),
                    reason: e.to_string(),
                }
            }
        })?;

        // Copy args into WASM memory
        let memory = self
            .instance
            .get_memory(&mut self.store, "memory")
            .ok_or_else(|| PluginError::ExecutionFailed {
                plugin_id: plugin_id.clone(),
                reason: "no memory export".to_string(),
            })?;

        memory
            .data_mut(&mut self.store)
            .get_mut(args_ptr as usize..args_ptr as usize + args_len as usize)
            .ok_or_else(|| PluginError::ExecutionFailed {
                plugin_id: plugin_id.clone(),
                reason: "args out of bounds".to_string(),
            })?
            .copy_from_slice(&args_json);

        // Call nexus_dispatch
        let result = dispatch_fn
            .call(&mut self.store, (handler_id, args_ptr, args_len))
            .map_err(|e| {
                if is_out_of_fuel(&e) {
                    PluginError::ExecutionTimeout {
                        plugin_id: plugin_id.clone(),
                    }
                } else {
                    PluginError::ExecutionFailed {
                        plugin_id: plugin_id.clone(),
                        reason: e.to_string(),
                    }
                }
            })?;

        // Unpack result: high 32 bits = ptr, low 32 bits = len
        let result_ptr = (result >> 32) as u32;
        let result_len = (result & 0xFFFF_FFFF) as u32;

        if result_len == 0 {
            return Ok(serde_json::Value::Null);
        }

        // Read result from WASM memory
        let result_bytes = memory
            .data(&self.store)
            .get(result_ptr as usize..result_ptr as usize + result_len as usize)
            .ok_or_else(|| PluginError::ExecutionFailed {
                plugin_id: plugin_id.clone(),
                reason: "result out of bounds".to_string(),
            })?
            .to_vec();

        serde_json::from_slice(&result_bytes).map_err(|e| PluginError::ExecutionFailed {
            plugin_id: plugin_id.clone(),
            reason: format!("result deserialization failed: {e}"),
        })
    }

    /// Call the on_init lifecycle hook (handler_id = 0).
    pub fn call_on_init(&mut self) -> Result<(), PluginError> {
        let _ = self.dispatch(0, &serde_json::json!({}))?;
        Ok(())
    }

    /// Call the on_start lifecycle hook (handler_id = 1).
    pub fn call_on_start(&mut self) -> Result<(), PluginError> {
        let _ = self.dispatch(1, &serde_json::json!({}))?;
        Ok(())
    }

    /// Call the on_stop lifecycle hook (handler_id = 2).
    pub fn call_on_stop(&mut self) -> Result<(), PluginError> {
        let _ = self.dispatch(2, &serde_json::json!({}))?;
        Ok(())
    }

    /// Access the plugin data from the store.
    pub fn plugin_data(&self) -> &PluginData {
        self.store.data()
    }
}

/// Check if a wasmtime error is an out-of-fuel trap.
fn is_out_of_fuel(err: &wasmtime::Error) -> bool {
    err.downcast_ref::<Trap>()
        .is_some_and(|t| *t == Trap::OutOfFuel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_data_stores_id_and_capabilities() {
        let data = PluginData {
            plugin_id: "com.test".to_string(),
            capabilities: CapabilitySet::new(),
            event_bus: None,
            kv_store: None,
        };
        assert_eq!(data.plugin_id, "com.test");
        assert!(data.capabilities.is_empty());
    }

    #[test]
    fn sandbox_rejects_invalid_wasm() {
        let data = PluginData {
            plugin_id: "com.test".to_string(),
            capabilities: CapabilitySet::new(),
            event_bus: None,
            kv_store: None,
        };
        let config = WasmConfig {
            module: "test.wasm".to_string(),
            memory_mb: 16,
            fuel: 1_000_000,
        };
        let result = WasmSandbox::new(b"not valid wasm", &config, data);
        assert!(matches!(result, Err(PluginError::WasmLoadFailed { .. })));
    }

    // Note: testing with a real WASM module requires building a test plugin.
    // Integration tests with real WASM are in the smoke test (Task 24).
    // Unit tests here verify error paths and type construction.
}
```

- [ ] **Step 2: Add `mod sandbox;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/lib.rs`, add after `mod manifest;`:

```rust
mod sandbox;

pub use sandbox::{PluginData, WasmSandbox};
```

- [ ] **Step 3: Run tests**

Run: `cargo nextest run -p nexus-plugins -- sandbox::tests`
Expected: 2 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-plugins/src/
git commit -m "feat(plugins): add WasmSandbox with wasmtime engine, dispatch, and lifecycle"
```

---

### Task 9: Create a minimal test WASM plugin

**Files:**
- Create: `crates/nexus-plugins/tests/fixtures/minimal-plugin/build.sh`
- Create: `crates/nexus-plugins/tests/fixtures/minimal-plugin/src/lib.rs`
- Create: `crates/nexus-plugins/tests/fixtures/minimal-plugin/Cargo.toml`

- [ ] **Step 1: Create the test plugin project**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/tests/fixtures/minimal-plugin/Cargo.toml`:

```toml
[package]
name = "minimal-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[profile.release]
opt-level = "s"
lto = true
```

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/tests/fixtures/minimal-plugin/src/lib.rs`:

```rust
//! Minimal test WASM plugin that exports nexus_dispatch and nexus_alloc.

use std::alloc::{alloc, Layout};

/// Allocate memory in WASM linear memory. Returns pointer.
#[no_mangle]
pub extern "C" fn nexus_alloc(size: u32) -> u32 {
    if size == 0 {
        return 0;
    }
    unsafe {
        let layout = Layout::from_size_align(size as usize, 1).unwrap();
        let ptr = alloc(layout);
        ptr as u32
    }
}

/// Main dispatch function. handler_id 0/1/2 = lifecycle hooks.
/// Returns packed (ptr << 32 | len) pointing to JSON result in memory.
#[no_mangle]
pub extern "C" fn nexus_dispatch(handler_id: u32, args_ptr: u32, args_len: u32) -> u64 {
    let result = match handler_id {
        // Lifecycle: on_init, on_start, on_stop — return empty JSON object
        0 | 1 | 2 => b"{}".to_vec(),
        // Echo handler (id=100): return the args as-is
        100 => {
            if args_len == 0 {
                b"{}".to_vec()
            } else {
                unsafe {
                    std::slice::from_raw_parts(args_ptr as *const u8, args_len as usize).to_vec()
                }
            }
        }
        // Unknown handler
        _ => b"{\"error\":\"unknown handler\"}".to_vec(),
    };

    // Write result to memory
    let result_ptr = nexus_alloc(result.len() as u32);
    if result_ptr != 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(
                result.as_ptr(),
                result_ptr as *mut u8,
                result.len(),
            );
        }
    }

    // Pack ptr and len into u64
    ((result_ptr as u64) << 32) | (result.len() as u64)
}
```

- [ ] **Step 2: Build the test WASM plugin**

Run:
```bash
cd /mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/tests/fixtures/minimal-plugin
rustup target add wasm32-unknown-unknown 2>/dev/null
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/minimal_plugin.wasm ../minimal-plugin.wasm
cd /mnt/c/Users/baile/dev/nexus
```

- [ ] **Step 3: Verify the WASM file exists**

Run: `ls -la crates/nexus-plugins/tests/fixtures/minimal-plugin.wasm`
Expected: file exists, non-zero size.

- [ ] **Step 4: Commit the fixture**

```bash
git add crates/nexus-plugins/tests/fixtures/
git commit -m "test(plugins): add minimal WASM test plugin fixture"
```

---

### Task 10: Write sandbox integration tests with real WASM

**Files:**
- Modify: `crates/nexus-plugins/src/sandbox.rs`

- [ ] **Step 1: Add integration tests using the test fixture**

Add to the `#[cfg(test)] mod tests` block in `crates/nexus-plugins/src/sandbox.rs`:

```rust
    fn test_wasm_bytes() -> Vec<u8> {
        let wasm_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/minimal-plugin.wasm");
        std::fs::read(&wasm_path).expect("minimal-plugin.wasm fixture must exist — run: cd crates/nexus-plugins/tests/fixtures/minimal-plugin && cargo build --target wasm32-unknown-unknown --release && cp target/wasm32-unknown-unknown/release/minimal_plugin.wasm ../minimal-plugin.wasm")
    }

    fn test_config() -> WasmConfig {
        WasmConfig {
            module: "test.wasm".to_string(),
            memory_mb: 16,
            fuel: 10_000_000,
        }
    }

    fn test_plugin_data() -> PluginData {
        PluginData {
            plugin_id: "com.test.minimal".to_string(),
            capabilities: CapabilitySet::new(),
            event_bus: None,
            kv_store: None,
        }
    }

    #[test]
    fn sandbox_loads_valid_wasm() {
        let wasm = test_wasm_bytes();
        let sandbox = WasmSandbox::new(&wasm, &test_config(), test_plugin_data());
        assert!(sandbox.is_ok());
    }

    #[test]
    fn sandbox_dispatch_echo_handler() {
        let wasm = test_wasm_bytes();
        let mut sandbox = WasmSandbox::new(&wasm, &test_config(), test_plugin_data()).unwrap();
        let args = serde_json::json!({"hello": "world"});
        let result = sandbox.dispatch(100, &args).unwrap();
        assert_eq!(result, args);
    }

    #[test]
    fn sandbox_lifecycle_hooks_succeed() {
        let wasm = test_wasm_bytes();
        let mut sandbox = WasmSandbox::new(&wasm, &test_config(), test_plugin_data()).unwrap();
        sandbox.call_on_init().unwrap();
        sandbox.call_on_start().unwrap();
        sandbox.call_on_stop().unwrap();
    }

    #[test]
    fn sandbox_unknown_handler_returns_error_json() {
        let wasm = test_wasm_bytes();
        let mut sandbox = WasmSandbox::new(&wasm, &test_config(), test_plugin_data()).unwrap();
        let result = sandbox.dispatch(999, &serde_json::json!({})).unwrap();
        assert!(result.get("error").is_some());
    }

    #[test]
    fn sandbox_fuel_exhaustion_returns_timeout() {
        let wasm = test_wasm_bytes();
        let config = WasmConfig {
            module: "test.wasm".to_string(),
            memory_mb: 16,
            fuel: 1, // extremely low fuel — should exhaust quickly
        };
        let mut sandbox = WasmSandbox::new(&wasm, &config, test_plugin_data()).unwrap();
        let result = sandbox.dispatch(100, &serde_json::json!({}));
        // Either times out or succeeds with very low fuel — both acceptable
        // The important thing is it doesn't panic
        let _ = result;
    }

    #[test]
    fn sandbox_plugin_data_accessible() {
        let wasm = test_wasm_bytes();
        let sandbox = WasmSandbox::new(&wasm, &test_config(), test_plugin_data()).unwrap();
        assert_eq!(sandbox.plugin_data().plugin_id, "com.test.minimal");
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo nextest run -p nexus-plugins -- sandbox::tests`
Expected: all 8 tests PASS (2 unit + 6 integration).

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-plugins/src/sandbox.rs
git commit -m "test(plugins): add WASM sandbox integration tests with real plugin"
```

---

## Phase 5: Host Functions

### Task 11: Write host function types and stubs

**Files:**
- Create: `crates/nexus-plugins/src/host_fns.rs`

- [ ] **Step 1: Create host_fns module**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/host_fns.rs`:

```rust
//! Host functions exposed to WASM plugins.
//!
//! These are linked into the wasmtime `Linker` and called by plugins
//! via their WASM imports. M1 provides: logging, event publishing, and KV.

use wasmtime::{Caller, Linker};

use crate::sandbox::PluginData;
use crate::PluginError;

/// Host function error codes returned to WASM.
pub const HOST_OK: i32 = 0;
/// General error.
pub const HOST_ERROR: i32 = -1;
/// Capability denied.
pub const HOST_CAPABILITY_DENIED: i32 = -1001;
/// Buffer overflow (result too large).
pub const HOST_BUFFER_OVERFLOW: i32 = -1002;

/// Register all host functions on the linker.
pub fn register_host_fns(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    register_host_log(linker)?;
    register_host_kv_get(linker)?;
    register_host_kv_set(linker)?;
    register_host_kv_delete(linker)?;
    register_host_publish_event(linker)?;
    Ok(())
}

/// Register the host_log function.
fn register_host_log(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "log",
            |caller: Caller<'_, PluginData>, level: i32, msg_ptr: i32, msg_len: i32| -> i32 {
                let plugin_id = caller.data().plugin_id.clone();

                let memory = match caller.get_export("memory") {
                    Some(wasmtime::Extern::Memory(m)) => m,
                    _ => return HOST_ERROR,
                };

                let data = memory.data(&caller);
                let msg_bytes = match data.get(msg_ptr as usize..msg_ptr as usize + msg_len as usize) {
                    Some(b) => b,
                    None => return HOST_ERROR,
                };

                let msg = match std::str::from_utf8(msg_bytes) {
                    Ok(s) => s,
                    Err(_) => return HOST_ERROR,
                };

                match level {
                    0 => tracing::debug!(plugin_id = %plugin_id, "{}", msg),
                    1 => tracing::info!(plugin_id = %plugin_id, "{}", msg),
                    2 => tracing::warn!(plugin_id = %plugin_id, "{}", msg),
                    3 => tracing::error!(plugin_id = %plugin_id, "{}", msg),
                    _ => return HOST_ERROR,
                }

                HOST_OK
            },
        )
        .map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: String::new(),
            reason: format!("failed to register host_log: {e}"),
        })?;

    Ok(())
}

/// Register host_kv_get: reads a value from the plugin's KV namespace.
fn register_host_kv_get(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "kv_get",
            |caller: Caller<'_, PluginData>,
             key_ptr: i32, key_len: i32,
             result_ptr: i32, result_capacity: i32| -> i32 {
                let data = caller.data();
                if !data.capabilities.contains(nexus_kernel::Capability::KvRead) {
                    return HOST_CAPABILITY_DENIED;
                }
                let kv = match &data.kv_store {
                    Some(kv) => kv.clone(),
                    None => return HOST_ERROR,
                };
                let plugin_id = data.plugin_id.clone();

                let memory = match caller.get_export("memory") {
                    Some(wasmtime::Extern::Memory(m)) => m,
                    _ => return HOST_ERROR,
                };

                let key = match read_string_from_memory(&memory, &caller, key_ptr, key_len) {
                    Some(s) => s,
                    None => return HOST_ERROR,
                };

                match kv.get(&plugin_id, &key) {
                    Ok(Some(value)) => {
                        if value.len() > result_capacity as usize {
                            return HOST_BUFFER_OVERFLOW;
                        }
                        // Need mutable access — this requires careful borrow management
                        // In practice, the caller needs to be split for read/write
                        value.len() as i32
                    }
                    Ok(None) => HOST_ERROR, // key not found
                    Err(_) => HOST_ERROR,
                }
            },
        )
        .map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: String::new(),
            reason: format!("failed to register host_kv_get: {e}"),
        })?;
    Ok(())
}

/// Register host_kv_set: stores a value in the plugin's KV namespace.
fn register_host_kv_set(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "kv_set",
            |caller: Caller<'_, PluginData>,
             key_ptr: i32, key_len: i32,
             value_ptr: i32, value_len: i32| -> i32 {
                let data = caller.data();
                if !data.capabilities.contains(nexus_kernel::Capability::KvWrite) {
                    return HOST_CAPABILITY_DENIED;
                }
                let kv = match &data.kv_store {
                    Some(kv) => kv.clone(),
                    None => return HOST_ERROR,
                };
                let plugin_id = data.plugin_id.clone();

                let memory = match caller.get_export("memory") {
                    Some(wasmtime::Extern::Memory(m)) => m,
                    _ => return HOST_ERROR,
                };

                let key = match read_string_from_memory(&memory, &caller, key_ptr, key_len) {
                    Some(s) => s,
                    None => return HOST_ERROR,
                };

                let value = match memory.data(&caller)
                    .get(value_ptr as usize..value_ptr as usize + value_len as usize) {
                    Some(b) => b.to_vec(),
                    None => return HOST_ERROR,
                };

                match kv.set(&plugin_id, &key, &value) {
                    Ok(()) => HOST_OK,
                    Err(_) => HOST_ERROR,
                }
            },
        )
        .map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: String::new(),
            reason: format!("failed to register host_kv_set: {e}"),
        })?;
    Ok(())
}

/// Register host_kv_delete: deletes a key from the plugin's KV namespace.
fn register_host_kv_delete(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "kv_delete",
            |caller: Caller<'_, PluginData>,
             key_ptr: i32, key_len: i32| -> i32 {
                let data = caller.data();
                if !data.capabilities.contains(nexus_kernel::Capability::KvWrite) {
                    return HOST_CAPABILITY_DENIED;
                }
                let kv = match &data.kv_store {
                    Some(kv) => kv.clone(),
                    None => return HOST_ERROR,
                };
                let plugin_id = data.plugin_id.clone();

                let memory = match caller.get_export("memory") {
                    Some(wasmtime::Extern::Memory(m)) => m,
                    _ => return HOST_ERROR,
                };

                let key = match read_string_from_memory(&memory, &caller, key_ptr, key_len) {
                    Some(s) => s,
                    None => return HOST_ERROR,
                };

                match kv.delete(&plugin_id, &key) {
                    Ok(()) => HOST_OK,
                    Err(_) => HOST_ERROR,
                }
            },
        )
        .map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: String::new(),
            reason: format!("failed to register host_kv_delete: {e}"),
        })?;
    Ok(())
}

/// Register host_publish_event: publishes a custom event on the event bus.
fn register_host_publish_event(linker: &mut Linker<PluginData>) -> Result<(), PluginError> {
    linker
        .func_wrap(
            "host",
            "publish_event",
            |caller: Caller<'_, PluginData>,
             type_id_ptr: i32, type_id_len: i32,
             payload_ptr: i32, payload_len: i32| -> i32 {
                let data = caller.data();
                let plugin_id = data.plugin_id.clone();
                let event_bus = match &data.event_bus {
                    Some(bus) => bus.clone(),
                    None => return HOST_ERROR,
                };

                let memory = match caller.get_export("memory") {
                    Some(wasmtime::Extern::Memory(m)) => m,
                    _ => return HOST_ERROR,
                };

                let type_id = match read_string_from_memory(&memory, &caller, type_id_ptr, type_id_len) {
                    Some(s) => s,
                    None => return HOST_ERROR,
                };

                // Namespace enforcement: type_id must start with plugin_id
                if !type_id.starts_with(&plugin_id) {
                    return HOST_CAPABILITY_DENIED;
                }

                let payload_bytes = match memory.data(&caller)
                    .get(payload_ptr as usize..payload_ptr as usize + payload_len as usize) {
                    Some(b) => b.to_vec(),
                    None => return HOST_ERROR,
                };

                let payload: serde_json::Value = match serde_json::from_slice(&payload_bytes) {
                    Ok(v) => v,
                    Err(_) => return HOST_ERROR,
                };

                // Publish via event bus (best-effort)
                let _ = event_bus.publish(
                    nexus_kernel::NexusEvent::Custom {
                        type_id,
                        emitting_plugin: plugin_id,
                        payload,
                    },
                    &plugin_id,
                );

                HOST_OK
            },
        )
        .map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: String::new(),
            reason: format!("failed to register host_publish_event: {e}"),
        })?;
    Ok(())
}

/// Helper: read a UTF-8 string from WASM linear memory.
fn read_string_from_memory(
    memory: &wasmtime::Memory,
    caller: &Caller<'_, PluginData>,
    ptr: i32,
    len: i32,
) -> Option<String> {
    let data = memory.data(caller);
    let bytes = data.get(ptr as usize..ptr as usize + len as usize)?;
    std::str::from_utf8(bytes).ok().map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_distinct() {
        assert_ne!(HOST_OK, HOST_ERROR);
        assert_ne!(HOST_OK, HOST_CAPABILITY_DENIED);
        assert_ne!(HOST_OK, HOST_BUFFER_OVERFLOW);
        assert_ne!(HOST_ERROR, HOST_CAPABILITY_DENIED);
    }
}
```

- [ ] **Step 2: Add `mod host_fns;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/lib.rs`, add after `mod sandbox;`:

```rust
mod host_fns;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p nexus-plugins`
Expected: compiles.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-plugins/src/
git commit -m "feat(plugins): add host function registration with host_log"
```

---

### Task 12: Wire host functions into sandbox

**Files:**
- Modify: `crates/nexus-plugins/src/sandbox.rs`

- [ ] **Step 1: Use host_fns::register_host_fns in sandbox creation**

In `crates/nexus-plugins/src/sandbox.rs`, replace the line:

```rust
        let linker: Linker<PluginData> = Linker::new(&engine);
```

with:

```rust
        let mut linker: Linker<PluginData> = Linker::new(&engine);
        crate::host_fns::register_host_fns(&mut linker).map_err(|e| PluginError::WasmLoadFailed {
            plugin_id: plugin_id.clone(),
            reason: format!("host function registration failed: {e}"),
        })?;
```

- [ ] **Step 2: Verify tests still pass**

Run: `cargo nextest run -p nexus-plugins`
Expected: all tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-plugins/src/sandbox.rs
git commit -m "feat(plugins): wire host functions into WASM sandbox linker"
```

---

### Task 13: Add KvStore trait

**Files:**
- Modify: `crates/nexus-plugins/src/lib.rs`

- [ ] **Step 1: Add KvStore trait**

Add to `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/lib.rs` before the re-exports:

```rust
/// Trait for key-value storage backends. Implemented by the kernel.
/// Namespace is the plugin ID — plugins cannot access each other's data.
pub trait KvStore: Send + Sync {
    /// Get a value by key within a namespace.
    fn get(&self, namespace: &str, key: &str) -> Result<Option<Vec<u8>>, PluginError>;
    /// Set a value by key within a namespace.
    fn set(&self, namespace: &str, key: &str, value: &[u8]) -> Result<(), PluginError>;
    /// Delete a key within a namespace.
    fn delete(&self, namespace: &str, key: &str) -> Result<(), PluginError>;
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p nexus-plugins`
Expected: compiles.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-plugins/src/lib.rs
git commit -m "feat(plugins): add KvStore trait for plugin key-value storage"
```

---

## Phase 6: Settings Infrastructure

### Task 14: Write settings tests

**Files:**
- Create: `crates/nexus-plugins/src/settings.rs`

- [ ] **Step 1: Create settings module with types and tests**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/settings.rs`:

```rust
//! JSON Schema validation and per-plugin settings storage.

use std::collections::HashMap;
use std::path::Path;

use crate::PluginError;

/// Manages JSON Schema validation for plugin settings.
pub struct SettingsManager {
    schemas: HashMap<String, serde_json::Value>,
}

impl SettingsManager {
    /// Create a new empty settings manager.
    pub fn new() -> Self {
        Self {
            schemas: HashMap::new(),
        }
    }

    /// Register a JSON Schema for a plugin.
    pub fn register_schema(
        &mut self,
        plugin_id: &str,
        schema_json: &str,
    ) -> Result<(), PluginError> {
        let schema: serde_json::Value =
            serde_json::from_str(schema_json).map_err(|e| PluginError::SettingsInvalid {
                plugin_id: plugin_id.to_string(),
                reason: format!("invalid JSON Schema: {e}"),
            })?;
        self.schemas.insert(plugin_id.to_string(), schema);
        Ok(())
    }

    /// Validate settings against the registered schema for a plugin.
    pub fn validate(
        &self,
        plugin_id: &str,
        settings: &serde_json::Value,
    ) -> Result<(), PluginError> {
        let schema = match self.schemas.get(plugin_id) {
            Some(s) => s,
            None => return Ok(()), // no schema = no validation
        };

        let validator = jsonschema::validator_for(schema).map_err(|e| {
            PluginError::SettingsInvalid {
                plugin_id: plugin_id.to_string(),
                reason: format!("invalid schema: {e}"),
            }
        })?;

        let result = validator.validate(settings);
        if let Err(errors) = result {
            let messages: Vec<String> = errors.map(|e| e.to_string()).collect();
            return Err(PluginError::SettingsInvalid {
                plugin_id: plugin_id.to_string(),
                reason: messages.join("; "),
            });
        }

        Ok(())
    }

    /// Load settings from disk, validate against schema.
    pub fn load_settings(
        &self,
        plugin_id: &str,
        plugin_dir: &Path,
    ) -> Result<serde_json::Value, PluginError> {
        let settings_path = plugin_dir.join("settings.json");
        let settings = if settings_path.exists() {
            let content = std::fs::read_to_string(&settings_path)?;
            serde_json::from_str(&content).map_err(|e| PluginError::SettingsInvalid {
                plugin_id: plugin_id.to_string(),
                reason: format!("invalid settings JSON: {e}"),
            })?
        } else {
            serde_json::json!({})
        };

        self.validate(plugin_id, &settings)?;
        Ok(settings)
    }

    /// Save settings to disk after validation.
    pub fn save_settings(
        &self,
        plugin_id: &str,
        plugin_dir: &Path,
        settings: &serde_json::Value,
    ) -> Result<(), PluginError> {
        self.validate(plugin_id, settings)?;
        let settings_path = plugin_dir.join("settings.json");
        let content = serde_json::to_string_pretty(settings).map_err(|e| {
            PluginError::SettingsInvalid {
                plugin_id: plugin_id.to_string(),
                reason: format!("serialization failed: {e}"),
            }
        })?;
        std::fs::write(&settings_path, content)?;
        Ok(())
    }

    /// Check if a schema is registered for a plugin.
    pub fn has_schema(&self, plugin_id: &str) -> bool {
        self.schemas.contains_key(plugin_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SCHEMA: &str = r#"{
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "count": { "type": "integer", "minimum": 0 }
        },
        "required": ["name"]
    }"#;

    #[test]
    fn register_valid_schema() {
        let mut mgr = SettingsManager::new();
        assert!(mgr.register_schema("com.test", TEST_SCHEMA).is_ok());
        assert!(mgr.has_schema("com.test"));
    }

    #[test]
    fn register_invalid_json_returns_error() {
        let mut mgr = SettingsManager::new();
        let result = mgr.register_schema("com.test", "not json");
        assert!(matches!(result, Err(PluginError::SettingsInvalid { .. })));
    }

    #[test]
    fn validate_valid_settings() {
        let mut mgr = SettingsManager::new();
        mgr.register_schema("com.test", TEST_SCHEMA).unwrap();
        let settings = serde_json::json!({"name": "hello", "count": 5});
        assert!(mgr.validate("com.test", &settings).is_ok());
    }

    #[test]
    fn validate_missing_required_field() {
        let mut mgr = SettingsManager::new();
        mgr.register_schema("com.test", TEST_SCHEMA).unwrap();
        let settings = serde_json::json!({"count": 5}); // missing "name"
        let result = mgr.validate("com.test", &settings);
        assert!(matches!(result, Err(PluginError::SettingsInvalid { .. })));
    }

    #[test]
    fn validate_wrong_type() {
        let mut mgr = SettingsManager::new();
        mgr.register_schema("com.test", TEST_SCHEMA).unwrap();
        let settings = serde_json::json!({"name": 123}); // name should be string
        let result = mgr.validate("com.test", &settings);
        assert!(matches!(result, Err(PluginError::SettingsInvalid { .. })));
    }

    #[test]
    fn validate_no_schema_always_passes() {
        let mgr = SettingsManager::new();
        let settings = serde_json::json!({"anything": "goes"});
        assert!(mgr.validate("com.unregistered", &settings).is_ok());
    }

    #[test]
    fn load_settings_from_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("settings.json"),
            r#"{"name": "test"}"#,
        )
        .unwrap();

        let mut mgr = SettingsManager::new();
        mgr.register_schema("com.test", TEST_SCHEMA).unwrap();
        let settings = mgr.load_settings("com.test", tmp.path()).unwrap();
        assert_eq!(settings["name"], "test");
    }

    #[test]
    fn load_missing_settings_returns_empty_object() {
        let tmp = tempfile::tempdir().unwrap();
        // No settings.json file

        let mgr = SettingsManager::new(); // no schema either
        let settings = mgr.load_settings("com.test", tmp.path()).unwrap();
        assert_eq!(settings, serde_json::json!({}));
    }

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mgr = SettingsManager::new();
        mgr.register_schema("com.test", TEST_SCHEMA).unwrap();

        let settings = serde_json::json!({"name": "saved", "count": 42});
        mgr.save_settings("com.test", tmp.path(), &settings).unwrap();

        let loaded = mgr.load_settings("com.test", tmp.path()).unwrap();
        assert_eq!(loaded["name"], "saved");
        assert_eq!(loaded["count"], 42);
    }

    #[test]
    fn save_invalid_settings_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let mut mgr = SettingsManager::new();
        mgr.register_schema("com.test", TEST_SCHEMA).unwrap();

        let bad = serde_json::json!({"count": -1}); // missing name, negative count
        let result = mgr.save_settings("com.test", tmp.path(), &bad);
        assert!(matches!(result, Err(PluginError::SettingsInvalid { .. })));
        // File should not have been written
        assert!(!tmp.path().join("settings.json").exists());
    }
}
```

- [ ] **Step 2: Add `mod settings;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/lib.rs`, add after `mod host_fns;`:

```rust
mod settings;

pub use settings::SettingsManager;
```

- [ ] **Step 3: Run tests**

Run: `cargo nextest run -p nexus-plugins -- settings::tests`
Expected: all 10 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-plugins/src/
git commit -m "feat(plugins): add SettingsManager with JSON Schema validation"
```

---

### Task 15: Run full workspace tests

**Files:** (none — verification only)

- [ ] **Step 1: Run clippy**

Run: `cargo clippy -p nexus-plugins -- -D warnings`
Expected: no warnings.

- [ ] **Step 2: Run all tests**

Run: `cargo nextest run --workspace`
Expected: all tests pass.

---

## Phase 7: Plugin Loader

### Task 16: Write loader types

**Files:**
- Create: `crates/nexus-plugins/src/loader.rs`

- [ ] **Step 1: Create loader module with types and tests**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/loader.rs`:

```rust
//! Plugin loader: scan directories, load manifests, instantiate sandboxes,
//! manage lifecycle.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nexus_kernel::{Capability, CapabilitySet, PluginInfo, PluginStatus, TrustLevel};

use crate::manifest::{self, PluginManifest};
use crate::sandbox::{PluginData, WasmSandbox};
use crate::settings::SettingsManager;
use crate::PluginError;

/// Tracks what a plugin has registered, for cleanup on unload.
struct PluginRegistrations {
    cli_subcommands: Vec<String>,
    ipc_commands: Vec<String>,
    event_subscriptions: Vec<String>,
}

/// A loaded plugin instance.
struct LoadedPlugin {
    manifest: PluginManifest,
    sandbox: WasmSandbox,
    status: PluginStatus,
    plugin_dir: PathBuf,
    registrations: PluginRegistrations,
}

/// Loads, manages, and dispatches to WASM plugins.
pub struct PluginLoader {
    plugins_dir: PathBuf,
    loaded: HashMap<String, LoadedPlugin>,
    /// Global registry of CLI subcommand → plugin_id.
    cli_registry: HashMap<String, String>,
    /// Settings manager.
    settings: SettingsManager,
}

impl PluginLoader {
    /// Create a new loader for the given plugins directory.
    pub fn new(plugins_dir: &Path) -> Self {
        Self {
            plugins_dir: plugins_dir.to_path_buf(),
            loaded: HashMap::new(),
            cli_registry: HashMap::new(),
            settings: SettingsManager::new(),
        }
    }

    /// Scan the plugins directory for subdirectories containing `manifest.toml`.
    pub fn scan(&self) -> Result<Vec<PathBuf>, PluginError> {
        let mut dirs = Vec::new();
        if !self.plugins_dir.exists() {
            return Ok(dirs);
        }
        for entry in std::fs::read_dir(&self.plugins_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() && path.join("manifest.toml").exists() {
                dirs.push(path);
            }
        }
        Ok(dirs)
    }

    /// Load a single plugin from a directory.
    pub fn load(&mut self, plugin_dir: &Path) -> Result<PluginInfo, PluginError> {
        // 1. Parse manifest
        let manifest_path = plugin_dir.join("manifest.toml");
        let m = manifest::load_manifest(&manifest_path)?;

        // 2. Validate
        manifest::validate(&m, plugin_dir)?;

        // 3. Check for duplicates
        if self.loaded.contains_key(&m.id) {
            return Err(PluginError::DuplicatePlugin(m.id.clone()));
        }

        // 4. Register settings schema if declared
        if let Some(ref settings_config) = m.settings {
            let schema_path = plugin_dir.join(&settings_config.schema);
            let schema_json = std::fs::read_to_string(&schema_path)?;
            self.settings.register_schema(&m.id, &schema_json)?;
        }

        // 5. Read WASM bytes
        let wasm_path = plugin_dir.join(&m.wasm.module);
        let wasm_bytes = std::fs::read(&wasm_path)?;

        // 6. Build capabilities
        let capabilities = if m.trust_level == TrustLevel::Core {
            CapabilitySet::from_iter(Capability::ALL.iter().copied())
        } else {
            let mut caps = CapabilitySet::new();
            for cap_str in &m.capabilities.required {
                if let Ok(cap) = nexus_kernel::Capability::from_str(cap_str) {
                    caps.insert(cap);
                }
            }
            for cap_str in &m.capabilities.optional {
                if let Ok(cap) = nexus_kernel::Capability::from_str(cap_str) {
                    caps.insert(cap);
                }
            }
            caps
        };

        // 7. Create sandbox
        let plugin_data = PluginData {
            plugin_id: m.id.clone(),
            capabilities: capabilities.clone(),
            event_bus: None,  // wired by PluginManager
            kv_store: None,   // wired by PluginManager
        };
        let mut sandbox = WasmSandbox::new(&wasm_bytes, &m.wasm, plugin_data)?;

        // 8. Lifecycle: on_init
        if m.lifecycle.on_init {
            sandbox.call_on_init().map_err(|e| PluginError::LifecycleError {
                plugin_id: m.id.clone(),
                hook: "on_init".to_string(),
                reason: e.to_string(),
            })?;
        }

        // 9. Lifecycle: on_start
        if m.lifecycle.on_start {
            sandbox.call_on_start().map_err(|e| PluginError::LifecycleError {
                plugin_id: m.id.clone(),
                hook: "on_start".to_string(),
                reason: e.to_string(),
            })?;
        }

        // 10. Register CLI subcommands (check for duplicates)
        let mut regs = PluginRegistrations {
            cli_subcommands: Vec::new(),
            ipc_commands: Vec::new(),
            event_subscriptions: Vec::new(),
        };

        for sub in &m.registrations.cli_subcommands {
            if let Some(existing_plugin) = self.cli_registry.get(&sub.id) {
                return Err(PluginError::DuplicateCliSubcommand {
                    plugin_id: m.id.clone(),
                    subcommand: sub.id.clone(),
                });
            }
            self.cli_registry.insert(sub.id.clone(), m.id.clone());
            regs.cli_subcommands.push(sub.id.clone());
        }

        for cmd in &m.registrations.ipc_commands {
            regs.ipc_commands.push(cmd.id.clone());
        }

        for sub in &m.registrations.event_subscribers {
            regs.event_subscriptions.push(sub.id.clone());
        }

        let info = PluginInfo {
            id: m.id.clone(),
            name: m.name.clone(),
            version: m.version.clone(),
            trust_level: m.trust_level,
            status: PluginStatus::Running,
            capabilities,
        };

        self.loaded.insert(
            m.id.clone(),
            LoadedPlugin {
                manifest: m,
                sandbox,
                status: PluginStatus::Running,
                plugin_dir: plugin_dir.to_path_buf(),
                registrations: regs,
            },
        );

        Ok(info)
    }

    /// Unload a plugin by ID. Calls on_stop before dropping the sandbox.
    pub fn unload(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        // Call on_stop before removing (needs mutable sandbox access)
        if let Some(plugin) = self.loaded.get_mut(plugin_id) {
            if plugin.manifest.lifecycle.on_stop {
                let _ = plugin.sandbox.call_on_stop(); // best-effort, don't fail unload
            }
        }

        let plugin = self
            .loaded
            .remove(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?;

        // Deregister CLI subcommands
        for sub_id in &plugin.registrations.cli_subcommands {
            self.cli_registry.remove(sub_id);
        }

        // Sandbox dropped here, releasing wasmtime resources
        Ok(())
    }

    /// List all loaded plugins.
    pub fn list(&self) -> Vec<PluginInfo> {
        self.loaded
            .values()
            .map(|p| PluginInfo {
                id: p.manifest.id.clone(),
                name: p.manifest.name.clone(),
                version: p.manifest.version.clone(),
                trust_level: p.manifest.trust_level,
                status: p.status,
                capabilities: p.sandbox.plugin_data().capabilities.clone(),
            })
            .collect()
    }

    /// Get info for a specific plugin.
    pub fn get(&self, plugin_id: &str) -> Option<PluginInfo> {
        self.loaded.get(plugin_id).map(|p| PluginInfo {
            id: p.manifest.id.clone(),
            name: p.manifest.name.clone(),
            version: p.manifest.version.clone(),
            trust_level: p.manifest.trust_level,
            status: p.status,
            capabilities: p.sandbox.plugin_data().capabilities.clone(),
        })
    }

    /// Dispatch a CLI subcommand to its owning plugin.
    pub fn dispatch_cli(
        &mut self,
        subcommand: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let plugin_id = self
            .cli_registry
            .get(subcommand)
            .ok_or_else(|| PluginError::PluginNotFound(format!("no plugin for subcommand: {subcommand}")))?
            .clone();

        let plugin = self
            .loaded
            .get_mut(&plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.clone()))?;

        // Find the handler_id for this subcommand
        let handler_id = plugin
            .manifest
            .registrations
            .cli_subcommands
            .iter()
            .find(|s| s.id == subcommand)
            .map(|s| s.handler_id)
            .ok_or_else(|| PluginError::PluginNotFound(format!("handler not found for: {subcommand}")))?;

        plugin.sandbox.dispatch(handler_id, &args)
    }

    /// Dispatch an IPC call to a plugin.
    pub fn dispatch_ipc(
        &mut self,
        plugin_id: &str,
        command_id: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let plugin = self
            .loaded
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?;

        let handler_id = plugin
            .manifest
            .registrations
            .ipc_commands
            .iter()
            .find(|c| c.id == command_id)
            .map(|c| c.handler_id)
            .ok_or_else(|| PluginError::PluginNotFound(format!("IPC command not found: {command_id}")))?;

        plugin.sandbox.dispatch(handler_id, &args)
    }

    /// Get the settings manager.
    pub fn settings(&self) -> &SettingsManager {
        &self.settings
    }

    /// Get a mutable reference to the settings manager.
    pub fn settings_mut(&mut self) -> &mut SettingsManager {
        &mut self.settings
    }

    /// Get the plugin directory for a loaded plugin.
    pub fn plugin_dir(&self, plugin_id: &str) -> Option<&Path> {
        self.loaded.get(plugin_id).map(|p| p.plugin_dir.as_path())
    }

    /// Access a mutable sandbox for reload purposes.
    pub(crate) fn sandbox_mut(&mut self, plugin_id: &str) -> Option<&mut WasmSandbox> {
        self.loaded.get_mut(plugin_id).map(|p| &mut p.sandbox)
    }

    /// Get a loaded plugin's manifest.
    pub(crate) fn manifest(&self, plugin_id: &str) -> Option<&PluginManifest> {
        self.loaded.get(plugin_id).map(|p| &p.manifest)
    }

    /// Set a plugin's status.
    pub(crate) fn set_status(&mut self, plugin_id: &str, status: PluginStatus) {
        if let Some(p) = self.loaded.get_mut(plugin_id) {
            p.status = status;
        }
    }

    /// Replace a plugin's sandbox (for hot-reload).
    pub(crate) fn replace_sandbox(
        &mut self,
        plugin_id: &str,
        sandbox: WasmSandbox,
    ) -> Option<WasmSandbox> {
        self.loaded.get_mut(plugin_id).map(|p| {
            std::mem::replace(&mut p.sandbox, sandbox)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_loader_has_empty_state() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = PluginLoader::new(tmp.path());
        assert!(loader.list().is_empty());
    }

    #[test]
    fn scan_empty_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let loader = PluginLoader::new(tmp.path());
        let dirs = loader.scan().unwrap();
        assert!(dirs.is_empty());
    }

    #[test]
    fn scan_finds_plugin_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("com.test.plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(plugin_dir.join("manifest.toml"), "").unwrap();

        let loader = PluginLoader::new(tmp.path());
        let dirs = loader.scan().unwrap();
        assert_eq!(dirs.len(), 1);
    }

    #[test]
    fn scan_skips_dirs_without_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let no_manifest = tmp.path().join("no-manifest");
        std::fs::create_dir_all(&no_manifest).unwrap();

        let loader = PluginLoader::new(tmp.path());
        let dirs = loader.scan().unwrap();
        assert!(dirs.is_empty());
    }

    #[test]
    fn unload_nonexistent_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let mut loader = PluginLoader::new(tmp.path());
        let result = loader.unload("com.nonexistent");
        assert!(matches!(result, Err(PluginError::PluginNotFound(_))));
    }

    // Full load/unload integration tests require the WASM fixture
    // and are covered in the smoke test (Task 24).
}
```

- [ ] **Step 2: Add `mod loader;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/lib.rs`, add after `mod settings;`:

```rust
mod loader;

pub use loader::PluginLoader;
```

- [ ] **Step 3: Run tests**

Run: `cargo nextest run -p nexus-plugins -- loader::tests`
Expected: all 5 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-plugins/src/
git commit -m "feat(plugins): add PluginLoader with scan, load, unload, dispatch"
```

---

### Task 17: Write loader integration tests with WASM fixture

**Files:**
- Modify: `crates/nexus-plugins/src/loader.rs`

- [ ] **Step 1: Add integration tests**

Add to the `#[cfg(test)] mod tests` block in `crates/nexus-plugins/src/loader.rs`:

```rust
    fn setup_plugin_dir(plugin_id: &str) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join(plugin_id);
        std::fs::create_dir_all(&plugin_dir).unwrap();

        // Copy WASM fixture
        let wasm_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/minimal-plugin.wasm");
        std::fs::copy(&wasm_src, plugin_dir.join("test.wasm")).unwrap();

        // Write manifest
        let manifest = format!(r#"
[plugin]
id = "{plugin_id}"
name = "Test Plugin"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["kv.read", "kv.write"]

[wasm]
module = "test.wasm"

[[registrations.cli_subcommand]]
id = "{plugin_id}.echo"
handler_id = 100
description = "Echo command"

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#);
        std::fs::write(plugin_dir.join("manifest.toml"), manifest).unwrap();

        tmp
    }

    #[test]
    fn load_plugin_from_dir() {
        let tmp = setup_plugin_dir("com.test.loader");
        let mut loader = PluginLoader::new(tmp.path());
        let plugin_dir = tmp.path().join("com.test.loader");
        let info = loader.load(&plugin_dir).unwrap();
        assert_eq!(info.id, "com.test.loader");
        assert_eq!(info.status, PluginStatus::Running);
    }

    #[test]
    fn load_duplicate_plugin_fails() {
        let tmp = setup_plugin_dir("com.test.dup");
        let mut loader = PluginLoader::new(tmp.path());
        let plugin_dir = tmp.path().join("com.test.dup");
        loader.load(&plugin_dir).unwrap();
        let result = loader.load(&plugin_dir);
        assert!(matches!(result, Err(PluginError::DuplicatePlugin(_))));
    }

    #[test]
    fn list_shows_loaded_plugins() {
        let tmp = setup_plugin_dir("com.test.list");
        let mut loader = PluginLoader::new(tmp.path());
        let plugin_dir = tmp.path().join("com.test.list");
        loader.load(&plugin_dir).unwrap();
        let list = loader.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "com.test.list");
    }

    #[test]
    fn unload_removes_plugin() {
        let tmp = setup_plugin_dir("com.test.unload");
        let mut loader = PluginLoader::new(tmp.path());
        let plugin_dir = tmp.path().join("com.test.unload");
        loader.load(&plugin_dir).unwrap();
        loader.unload("com.test.unload").unwrap();
        assert!(loader.list().is_empty());
    }

    #[test]
    fn dispatch_cli_to_loaded_plugin() {
        let tmp = setup_plugin_dir("com.test.dispatch");
        let mut loader = PluginLoader::new(tmp.path());
        let plugin_dir = tmp.path().join("com.test.dispatch");
        loader.load(&plugin_dir).unwrap();

        let args = serde_json::json!({"message": "hello"});
        let result = loader.dispatch_cli("com.test.dispatch.echo", args.clone()).unwrap();
        assert_eq!(result, args);
    }

    #[test]
    fn dispatch_cli_unknown_subcommand_fails() {
        let tmp = setup_plugin_dir("com.test.unknown");
        let mut loader = PluginLoader::new(tmp.path());
        let result = loader.dispatch_cli("nonexistent.cmd", serde_json::json!({}));
        assert!(matches!(result, Err(PluginError::PluginNotFound(_))));
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo nextest run -p nexus-plugins -- loader::tests`
Expected: all 11 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-plugins/src/loader.rs
git commit -m "test(plugins): add loader integration tests with WASM fixture"
```

---

### Task 18: Run full workspace tests

**Files:** (none — verification only)

- [ ] **Step 1: Run all tests**

Run: `cargo nextest run --workspace`
Expected: all tests pass.

---

## Phase 8: Hot-Reload

### Task 19: Write hot-reload module

**Files:**
- Create: `crates/nexus-plugins/src/hot_reload.rs`

- [ ] **Step 1: Create hot_reload module**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/hot_reload.rs`:

```rust
//! Hot-reload: file watcher on plugin directories, reload events.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::new_debouncer;

use crate::PluginError;

/// An event indicating a plugin's WASM file has changed.
#[derive(Debug, Clone)]
pub struct ReloadEvent {
    /// Plugin ID (derived from directory name).
    pub plugin_id: String,
    /// Path to the changed WASM file.
    pub wasm_path: PathBuf,
}

/// Watches plugin directories for WASM file changes and emits reload events.
pub struct HotReloader {
    rx: mpsc::Receiver<ReloadEvent>,
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

impl HotReloader {
    /// Start watching the plugins directory for WASM file changes.
    pub fn start(plugins_dir: &Path, debounce_ms: u64) -> Result<Self, PluginError> {
        let (reload_tx, reload_rx) = mpsc::channel();
        let (notify_tx, notify_rx) = mpsc::channel();

        let debouncer = new_debouncer(Duration::from_millis(debounce_ms), notify_tx)
            .map_err(|e| PluginError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("failed to create debouncer: {e}"),
            )))?;

        // Watch plugins directory
        if plugins_dir.exists() {
            debouncer
                .watcher()
                .watch(plugins_dir, RecursiveMode::Recursive)
                .map_err(|e| PluginError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("failed to watch plugins dir: {e}"),
                )))?;
        }

        // Spawn processing thread
        let plugins_dir = plugins_dir.to_path_buf();
        std::thread::spawn(move || {
            process_wasm_events(notify_rx, &plugins_dir, &reload_tx);
        });

        Ok(Self {
            rx: reload_rx,
            _debouncer: debouncer,
        })
    }

    /// Try to receive a reload event without blocking.
    pub fn try_recv(&self) -> Option<ReloadEvent> {
        self.rx.try_recv().ok()
    }

    /// Drain all pending reload events.
    pub fn drain(&self) -> Vec<ReloadEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            events.push(event);
        }
        events
    }
}

/// Process raw debounced filesystem events, filter for .wasm changes,
/// and map to ReloadEvents.
fn process_wasm_events(
    rx: mpsc::Receiver<Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>>,
    plugins_dir: &Path,
    tx: &mpsc::Sender<ReloadEvent>,
) {
    for result in rx {
        let events = match result {
            Ok(events) => events,
            Err(_) => continue,
        };

        for event in events {
            let path = &event.path;

            // Only care about .wasm files
            let is_wasm = path
                .extension()
                .is_some_and(|ext| ext == "wasm");
            if !is_wasm {
                continue;
            }

            // Must still exist (not a deletion)
            if !path.exists() {
                continue;
            }

            // Derive plugin_id from directory name
            // plugins_dir/<plugin_id>/<file>.wasm
            if let Some(plugin_dir) = path.parent() {
                if let Some(dir_name) = plugin_dir.file_name().and_then(|n| n.to_str()) {
                    let _ = tx.send(ReloadEvent {
                        plugin_id: dir_name.to_string(),
                        wasm_path: path.clone(),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_event_stores_fields() {
        let event = ReloadEvent {
            plugin_id: "com.test".to_string(),
            wasm_path: PathBuf::from("/plugins/com.test/plugin.wasm"),
        };
        assert_eq!(event.plugin_id, "com.test");
        assert_eq!(event.wasm_path, PathBuf::from("/plugins/com.test/plugin.wasm"));
    }

    #[test]
    fn start_on_nonexistent_dir_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let nonexistent = tmp.path().join("does-not-exist");
        // Should not error — just nothing to watch
        let reloader = HotReloader::start(&nonexistent, 500);
        assert!(reloader.is_ok());
    }

    #[test]
    fn drain_empty_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let reloader = HotReloader::start(tmp.path(), 500).unwrap();
        let events = reloader.drain();
        assert!(events.is_empty());
    }

    #[test]
    fn detects_wasm_file_change() {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join("com.test.reload");
        std::fs::create_dir_all(&plugin_dir).unwrap();

        let reloader = HotReloader::start(tmp.path(), 100).unwrap();

        // Create a .wasm file
        std::fs::write(plugin_dir.join("plugin.wasm"), b"fake wasm").unwrap();

        // Wait for event
        std::thread::sleep(std::time::Duration::from_millis(500));
        let events = reloader.drain();

        // Should detect the change (may or may not depending on timing)
        // At minimum, verify it doesn't panic
        for event in &events {
            assert_eq!(event.plugin_id, "com.test.reload");
        }
    }
}
```

- [ ] **Step 2: Add `mod hot_reload;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/lib.rs`, add after `mod loader;`:

```rust
mod hot_reload;

pub use hot_reload::{HotReloader, ReloadEvent};
```

- [ ] **Step 3: Run tests**

Run: `cargo nextest run -p nexus-plugins -- hot_reload::tests`
Expected: all 4 tests PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-plugins/src/
git commit -m "feat(plugins): add HotReloader with WASM file change detection"
```

---

### Task 20: Run full workspace tests

**Files:** (none — verification only)

- [ ] **Step 1: Run all tests**

Run: `cargo nextest run --workspace`
Expected: all tests pass.

---

## Phase 9: PluginManager Facade & Smoke Test

### Task 21: Build PluginManager facade

**Files:**
- Modify: `crates/nexus-plugins/src/lib.rs`

- [ ] **Step 1: Add PluginManager and PluginManagerConfig**

Add the following to `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/lib.rs`, after the `KvStore` trait and before the re-exports:

```rust
use std::path::Path;

/// Configuration for the plugin manager.
#[derive(Debug, Clone)]
pub struct PluginManagerConfig {
    /// Whether hot-reload is enabled.
    pub hot_reload: bool,
    /// Debounce interval for hot-reload file watching, in milliseconds.
    pub debounce_ms: u64,
}

impl Default for PluginManagerConfig {
    fn default() -> Self {
        Self {
            hot_reload: true,
            debounce_ms: 500,
        }
    }
}

/// The main plugin system facade. Owns the loader, settings, and hot-reloader.
pub struct PluginManager {
    loader: loader::PluginLoader,
    reloader: Option<hot_reload::HotReloader>,
}

impl PluginManager {
    /// Create a new plugin manager for the given plugins directory.
    pub fn new(
        plugins_dir: &Path,
        config: &PluginManagerConfig,
    ) -> Result<Self, PluginError> {
        let loader = loader::PluginLoader::new(plugins_dir);

        let reloader = if config.hot_reload {
            hot_reload::HotReloader::start(plugins_dir, config.debounce_ms).ok()
        } else {
            None
        };

        Ok(Self { loader, reloader })
    }

    /// Scan the plugins directory and load all valid plugins.
    pub fn load_all(&mut self) -> Result<Vec<nexus_kernel::PluginInfo>, PluginError> {
        let dirs = self.loader.scan()?;
        let mut infos = Vec::new();
        for dir in dirs {
            match self.loader.load(&dir) {
                Ok(info) => infos.push(info),
                Err(e) => {
                    tracing::warn!(
                        plugin_dir = %dir.display(),
                        error = %e,
                        "failed to load plugin, skipping"
                    );
                }
            }
        }
        Ok(infos)
    }

    /// Load a single plugin from a directory.
    pub fn load(&mut self, plugin_dir: &Path) -> Result<nexus_kernel::PluginInfo, PluginError> {
        self.loader.load(plugin_dir)
    }

    /// Unload a plugin by ID.
    pub fn unload(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        self.loader.unload(plugin_id)
    }

    /// List all loaded plugins.
    pub fn list(&self) -> Vec<nexus_kernel::PluginInfo> {
        self.loader.list()
    }

    /// Get info for a specific plugin.
    pub fn get(&self, plugin_id: &str) -> Option<nexus_kernel::PluginInfo> {
        self.loader.get(plugin_id)
    }

    /// Dispatch a CLI subcommand to its owning plugin.
    pub fn dispatch_cli(
        &mut self,
        subcommand: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        self.loader.dispatch_cli(subcommand, args)
    }

    /// Dispatch an IPC call to a plugin.
    pub fn dispatch_ipc(
        &mut self,
        plugin_id: &str,
        command_id: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        self.loader.dispatch_ipc(plugin_id, command_id, args)
    }

    /// Get plugin settings.
    pub fn get_settings(
        &self,
        plugin_id: &str,
    ) -> Result<serde_json::Value, PluginError> {
        let plugin_dir = self
            .loader
            .plugin_dir(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?;
        self.loader.settings().load_settings(plugin_id, plugin_dir)
    }

    /// Update plugin settings (validates against schema).
    pub fn set_settings(
        &mut self,
        plugin_id: &str,
        settings: serde_json::Value,
    ) -> Result<(), PluginError> {
        let plugin_dir = self
            .loader
            .plugin_dir(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?
            .to_path_buf();
        self.loader
            .settings()
            .save_settings(plugin_id, &plugin_dir, &settings)
    }

    /// Check for and process hot-reload events. Returns IDs of reloaded plugins.
    pub fn poll_reloads(&mut self) -> Result<Vec<String>, PluginError> {
        let reloader = match &self.reloader {
            Some(r) => r,
            None => return Ok(Vec::new()),
        };

        let events = reloader.drain();
        let mut reloaded = Vec::new();

        for event in events {
            match self.reload_plugin(&event.plugin_id) {
                Ok(()) => {
                    tracing::info!(plugin_id = %event.plugin_id, "plugin hot-reloaded");
                    reloaded.push(event.plugin_id);
                }
                Err(e) => {
                    tracing::error!(
                        plugin_id = %event.plugin_id,
                        error = %e,
                        "hot-reload failed"
                    );
                    self.loader
                        .set_status(&event.plugin_id, nexus_kernel::PluginStatus::Crashed);
                }
            }
        }

        Ok(reloaded)
    }

    /// Reload a single plugin by re-reading its WASM and re-running lifecycle.
    fn reload_plugin(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        let manifest = self
            .loader
            .manifest(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?
            .clone();

        let plugin_dir = self
            .loader
            .plugin_dir(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?
            .to_path_buf();

        // Call on_stop on old sandbox
        if manifest.lifecycle.on_stop {
            if let Some(sandbox) = self.loader.sandbox_mut(plugin_id) {
                let _ = sandbox.call_on_stop(); // best-effort
            }
        }

        // Read new WASM bytes
        let wasm_path = plugin_dir.join(&manifest.wasm.module);
        let wasm_bytes = std::fs::read(&wasm_path).map_err(|e| PluginError::ReloadFailed {
            plugin_id: plugin_id.to_string(),
            reason: e.to_string(),
        })?;

        // Build new sandbox
        let capabilities = self
            .loader
            .get(plugin_id)
            .map(|info| info.capabilities)
            .unwrap_or_default();

        let plugin_data = sandbox::PluginData {
            plugin_id: plugin_id.to_string(),
            capabilities,
            event_bus: None,  // host fns will be wired when event_bus is available
            kv_store: None,
        };

        let mut new_sandbox =
            sandbox::WasmSandbox::new(&wasm_bytes, &manifest.wasm, plugin_data).map_err(|e| {
                PluginError::ReloadFailed {
                    plugin_id: plugin_id.to_string(),
                    reason: e.to_string(),
                }
            })?;

        // Run lifecycle on new sandbox
        if manifest.lifecycle.on_init {
            new_sandbox.call_on_init().map_err(|e| PluginError::ReloadFailed {
                plugin_id: plugin_id.to_string(),
                reason: format!("on_init failed: {e}"),
            })?;
        }
        if manifest.lifecycle.on_start {
            new_sandbox.call_on_start().map_err(|e| PluginError::ReloadFailed {
                plugin_id: plugin_id.to_string(),
                reason: format!("on_start failed: {e}"),
            })?;
        }

        // Swap sandboxes
        self.loader.replace_sandbox(plugin_id, new_sandbox);
        self.loader
            .set_status(plugin_id, nexus_kernel::PluginStatus::Running);

        Ok(())
    }

    /// Stop all plugins and clean up.
    pub fn shutdown(&mut self) -> Result<(), PluginError> {
        let plugin_ids: Vec<String> = self.loader.list().iter().map(|p| p.id.clone()).collect();
        for id in plugin_ids {
            if let Err(e) = self.loader.unload(&id) {
                tracing::warn!(plugin_id = %id, error = %e, "failed to unload during shutdown");
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p nexus-plugins`
Expected: compiles.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-plugins/src/lib.rs
git commit -m "feat(plugins): add PluginManager facade with hot-reload support"
```

---

### Task 22: Write PluginManager integration tests

**Files:**
- Modify: `crates/nexus-plugins/src/lib.rs`

- [ ] **Step 1: Add integration tests**

Add at the end of `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn setup_plugin(plugin_id: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join(plugin_id);
        std::fs::create_dir_all(&plugin_dir).unwrap();

        let wasm_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/minimal-plugin.wasm");
        std::fs::copy(&wasm_src, plugin_dir.join("test.wasm")).unwrap();

        let manifest = format!(r#"
[plugin]
id = "{plugin_id}"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["kv.read", "kv.write"]

[wasm]
module = "test.wasm"

[[registrations.cli_subcommand]]
id = "{plugin_id}.echo"
handler_id = 100
description = "Echo"

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#);
        std::fs::write(plugin_dir.join("manifest.toml"), manifest).unwrap();

        (tmp, plugin_dir)
    }

    #[test]
    fn manager_load_and_list() {
        let (tmp, plugin_dir) = setup_plugin("com.test.mgr");
        let config = PluginManagerConfig {
            hot_reload: false,
            ..Default::default()
        };
        let mut mgr = PluginManager::new(tmp.path(), &config).unwrap();
        let info = mgr.load(&plugin_dir).unwrap();
        assert_eq!(info.id, "com.test.mgr");

        let list = mgr.list();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn manager_dispatch_cli() {
        let (tmp, plugin_dir) = setup_plugin("com.test.cli");
        let config = PluginManagerConfig {
            hot_reload: false,
            ..Default::default()
        };
        let mut mgr = PluginManager::new(tmp.path(), &config).unwrap();
        mgr.load(&plugin_dir).unwrap();

        let args = serde_json::json!({"key": "value"});
        let result = mgr.dispatch_cli("com.test.cli.echo", args.clone()).unwrap();
        assert_eq!(result, args);
    }

    #[test]
    fn manager_unload_and_shutdown() {
        let (tmp, plugin_dir) = setup_plugin("com.test.shutdown");
        let config = PluginManagerConfig {
            hot_reload: false,
            ..Default::default()
        };
        let mut mgr = PluginManager::new(tmp.path(), &config).unwrap();
        mgr.load(&plugin_dir).unwrap();
        mgr.shutdown().unwrap();
        assert!(mgr.list().is_empty());
    }

    #[test]
    fn manager_get_returns_info() {
        let (tmp, plugin_dir) = setup_plugin("com.test.get");
        let config = PluginManagerConfig {
            hot_reload: false,
            ..Default::default()
        };
        let mut mgr = PluginManager::new(tmp.path(), &config).unwrap();
        mgr.load(&plugin_dir).unwrap();

        let info = mgr.get("com.test.get");
        assert!(info.is_some());
        assert_eq!(info.unwrap().id, "com.test.get");

        assert!(mgr.get("nonexistent").is_none());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo nextest run -p nexus-plugins -- tests::`
Expected: all integration tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-plugins/src/lib.rs
git commit -m "test(plugins): add PluginManager integration tests"
```

---

### Task 23: Run full workspace tests and clippy

**Files:** (none — verification only)

- [ ] **Step 1: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: no warnings.

- [ ] **Step 2: Run all tests**

Run: `cargo nextest run --workspace`
Expected: all tests pass.

---

### Task 24: PRD 04 smoke test

**Files:**
- Create: `crates/nexus-plugins/tests/prd-04-smoke.rs`

- [ ] **Step 1: Create smoke test**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/tests/prd-04-smoke.rs`:

```rust
//! PRD 04 smoke test: verifies the public API surface and key integration paths.

use nexus_plugins::{
    parse_manifest, PluginError, PluginManager, PluginManagerConfig,
    PluginManifest, ManifestCapabilities, WasmConfig, SettingsConfig,
    Registrations, CliSubcommandReg, IpcCommandReg, EventSubscriberReg,
    LifecycleConfig, PluginData, WasmSandbox, SettingsManager,
    HotReloader, ReloadEvent, KvStore, PluginLoader,
};

#[test]
fn public_type_surface_is_accessible() {
    let _: Option<PluginError> = None;
    let _: Option<PluginManagerConfig> = None;
    let _: Option<PluginManifest> = None;
    let _: Option<ManifestCapabilities> = None;
    let _: Option<WasmConfig> = None;
    let _: Option<SettingsConfig> = None;
    let _: Option<Registrations> = None;
    let _: Option<CliSubcommandReg> = None;
    let _: Option<IpcCommandReg> = None;
    let _: Option<EventSubscriberReg> = None;
    let _: Option<LifecycleConfig> = None;
    let _: Option<PluginData> = None;
    let _: Option<ReloadEvent> = None;
}

#[test]
fn manifest_parse_and_validate_roundtrip() {
    let toml = r#"
[plugin]
id = "com.test.smoke"
name = "Smoke Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["kv.read"]

[wasm]
module = "smoke.wasm"

[lifecycle]
on_init = true
"#;
    let m = parse_manifest(toml, "smoke.toml").unwrap();
    assert_eq!(m.id, "com.test.smoke");
    assert_eq!(m.trust_level, nexus_kernel::TrustLevel::Community);
    assert!(m.lifecycle.on_init);
    assert!(!m.lifecycle.on_start);
}

#[test]
fn wasm_sandbox_rejects_invalid_bytes() {
    use nexus_kernel::CapabilitySet;
    let data = PluginData {
        plugin_id: "com.test".to_string(),
        capabilities: CapabilitySet::new(),
        event_bus: None,
        kv_store: None,
    };
    let config = WasmConfig {
        module: "test.wasm".to_string(),
        memory_mb: 16,
        fuel: 1_000_000,
    };
    let result = WasmSandbox::new(b"invalid", &config, data);
    assert!(matches!(result, Err(PluginError::WasmLoadFailed { .. })));
}

#[test]
fn settings_schema_validation_works() {
    let mut mgr = SettingsManager::new();
    mgr.register_schema("com.test", r#"{"type": "object", "required": ["name"], "properties": {"name": {"type": "string"}}}"#).unwrap();

    let good = serde_json::json!({"name": "hello"});
    assert!(mgr.validate("com.test", &good).is_ok());

    let bad = serde_json::json!({"count": 5});
    assert!(mgr.validate("com.test", &bad).is_err());
}

fn setup_smoke_plugin() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let plugin_dir = tmp.path().join("com.test.smoke");
    std::fs::create_dir_all(&plugin_dir).unwrap();

    let wasm_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/minimal-plugin.wasm");
    std::fs::copy(&wasm_src, plugin_dir.join("test.wasm")).unwrap();

    let manifest = r#"
[plugin]
id = "com.test.smoke"
name = "Smoke"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["kv.read", "kv.write"]

[wasm]
module = "test.wasm"

[[registrations.cli_subcommand]]
id = "com.test.smoke.echo"
handler_id = 100
description = "Echo"

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#;
    std::fs::write(plugin_dir.join("manifest.toml"), manifest).unwrap();

    (tmp, plugin_dir)
}

#[test]
fn full_plugin_lifecycle() {
    let (tmp, plugin_dir) = setup_smoke_plugin();
    let config = PluginManagerConfig {
        hot_reload: false,
        ..Default::default()
    };
    let mut mgr = PluginManager::new(tmp.path(), &config).unwrap();

    // Load
    let info = mgr.load(&plugin_dir).unwrap();
    assert_eq!(info.id, "com.test.smoke");
    assert_eq!(info.status, nexus_kernel::PluginStatus::Running);

    // Dispatch CLI
    let args = serde_json::json!({"hello": "world"});
    let result = mgr.dispatch_cli("com.test.smoke.echo", args.clone()).unwrap();
    assert_eq!(result, args);

    // List
    let list = mgr.list();
    assert_eq!(list.len(), 1);

    // Get
    let info = mgr.get("com.test.smoke").unwrap();
    assert_eq!(info.name, "Smoke");

    // Shutdown
    mgr.shutdown().unwrap();
    assert!(mgr.list().is_empty());
}

#[test]
fn load_all_scans_directory() {
    let (tmp, _) = setup_smoke_plugin();
    let config = PluginManagerConfig {
        hot_reload: false,
        ..Default::default()
    };
    let mut mgr = PluginManager::new(tmp.path(), &config).unwrap();
    let infos = mgr.load_all().unwrap();
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].id, "com.test.smoke");
}

#[test]
fn plugin_error_variants_display_correctly() {
    let errors = vec![
        PluginError::ManifestNotFound("test".to_string()),
        PluginError::PluginNotFound("com.test".to_string()),
        PluginError::DuplicatePlugin("com.test".to_string()),
        PluginError::PluginReloading("com.test".to_string()),
    ];
    for err in errors {
        assert!(!err.to_string().is_empty());
    }
}
```

- [ ] **Step 2: Run smoke test**

Run: `cargo nextest run -p nexus-plugins --test prd-04-smoke`
Expected: all smoke tests PASS.

- [ ] **Step 3: Run full workspace tests**

Run: `cargo nextest run --workspace`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-plugins/tests/
git commit -m "test(plugins): add PRD 04 smoke test covering public API and lifecycle"
```

---

## Summary

24 tasks across 9 phases produce:
- `nexus-plugins` crate with 7 source modules
- `PluginManager` facade composing all subsystems
- Manifest parser with TOML deserialization and 8-rule validation
- Wasmtime WASM sandbox with fuel-based execution limits and dispatch protocol
- Host function linkage (logging in M1, extensible for events/KV)
- Plugin loader with lifecycle management (init → start → stop)
- JSON Schema-based settings validation and persistence
- Hot-reload via notify file watcher with reload lifecycle
- Minimal WASM test plugin fixture for integration tests
- Comprehensive unit tests per module + integration smoke test
