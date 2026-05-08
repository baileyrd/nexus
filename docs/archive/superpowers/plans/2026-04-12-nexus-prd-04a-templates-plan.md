# Nexus PRD 04a — Plugin Templates (M1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `scaffold` module to `nexus-plugins` that generates plugin project files from embedded templates, with core and community variants.

**Architecture:** Single new module `scaffold.rs` inside the existing `nexus-plugins` crate. Templates embedded as `&str` constants, placeholder substitution via `str::replace()`. No new dependencies.

**Tech Stack:** Rust (edition 2024), `regex-lite` (already in deps for ID validation).

**Parent docs:**
- [`2026-04-12-nexus-prd-04a-templates-design.md`](../specs/2026-04-12-nexus-prd-04a-templates-design.md) — **the contract this plan implements**

---

## Prerequisites

1. PRD 04 (plugins crate) is complete and tests pass.
2. Verify: `cargo nextest run --workspace` passes (321 tests).

---

## File Structure

```
crates/nexus-plugins/src/
└── scaffold.rs          # new file: template generation + embedded templates
```

Modifications:
- `crates/nexus-plugins/src/lib.rs`: add `mod scaffold;` and re-exports

---

## Task Overview

3 tasks across 2 phases:

1. Phase 1: Scaffold module with templates (Tasks 1–2)
2. Phase 2: Smoke test integration (Task 3)

---

## Phase 1: Scaffold Module

### Task 1: Create scaffold module with types, templates, and function

**Files:**
- Create: `crates/nexus-plugins/src/scaffold.rs`
- Modify: `crates/nexus-plugins/src/lib.rs`

- [ ] **Step 1: Create `scaffold.rs` with types, embedded templates, and scaffold function**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/scaffold.rs`:

```rust
//! Plugin template scaffolding.
//!
//! Generates new plugin projects from embedded templates.

use std::path::Path;

use crate::PluginError;

/// Which template variant to generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginTemplate {
    /// Core plugin — gets all capabilities, unlimited fuel.
    Core,
    /// Community plugin — declares required capabilities, fuel-limited.
    Community,
}

/// Configuration for scaffolding a new plugin.
#[derive(Debug, Clone)]
pub struct ScaffoldConfig {
    /// Plugin ID in reverse-DNS format (e.g., "com.example.my-plugin").
    pub plugin_id: String,
    /// Human-readable plugin name (e.g., "My Plugin").
    pub plugin_name: String,
    /// Author name.
    pub author: String,
    /// One-line description.
    pub description: String,
}

// ── Embedded templates ───────────────────────────────────────────────────────

const CARGO_TOML_TEMPLATE: &str = r#"[package]
name = "{{plugin-id}}"
version = "0.1.0"
edition = "2021"
authors = ["{{author}}"]
description = "{{description}}"

[lib]
crate-type = ["cdylib"]

[profile.release]
opt-level = "s"
lto = true
"#;

const MANIFEST_CORE_TEMPLATE: &str = r#"[plugin]
id = "{{plugin-id}}"
name = "{{plugin-name}}"
version = "0.1.0"
trust_level = "core"
api_version = "1"

# Core plugins receive all capabilities by default.
# List here only for documentation.
[capabilities]
required = []
optional = []

[wasm]
module = "{{plugin-id}}.wasm"
memory_mb = 16
fuel = 0

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#;

const MANIFEST_COMMUNITY_TEMPLATE: &str = r#"[plugin]
id = "{{plugin-id}}"
name = "{{plugin-name}}"
version = "0.1.0"
trust_level = "community"
api_version = "1"

# Declare every capability your plugin needs.
# Required: blocks plugin load if denied.
# Optional: allows degraded operation if denied.
[capabilities]
required = ["kv.read", "kv.write"]
optional = []

[wasm]
module = "{{plugin-id}}.wasm"
memory_mb = 16
fuel = 10000000

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#;

const LIB_RS_TEMPLATE: &str = r#"//! {{plugin-name}} — a Nexus plugin.
//!
//! Build: cargo build --target wasm32-unknown-unknown --release
//! The output WASM file is your plugin binary.

use std::alloc::{alloc, Layout};

// ── Memory allocator (required by the Nexus host) ────────────────────────────

/// Allocate `size` bytes in WASM linear memory. Called by the host to pass
/// arguments into the plugin.
#[no_mangle]
pub extern "C" fn nexus_alloc(size: u32) -> u32 {
    if size == 0 {
        return 0;
    }
    unsafe {
        let layout = Layout::from_size_align(size as usize, 1).unwrap();
        alloc(layout) as u32
    }
}

// ── Dispatch (required by the Nexus host) ────────────────────────────────────

/// Main entry point called by the Nexus host.
///
/// The host calls this with a `handler_id` and JSON arguments serialized
/// into WASM linear memory at `args_ptr` / `args_len`.
///
/// Return value is a packed u64: `(result_ptr << 32) | result_len`.
/// The host reads the JSON result from that memory region.
///
/// # Handler IDs
///
/// - 0: on_init (called once after load)
/// - 1: on_start (called after init, plugin is now "running")
/// - 2: on_stop (called before unload — persist state here)
/// - 100+: your custom handlers (registered in manifest.toml)
#[no_mangle]
pub extern "C" fn nexus_dispatch(handler_id: u32, args_ptr: u32, args_len: u32) -> u64 {
    let result = match handler_id {
        // ── Lifecycle hooks ──────────────────────────────────────────────
        0 => handle_init(args_ptr, args_len),
        1 => handle_start(args_ptr, args_len),
        2 => handle_stop(args_ptr, args_len),

        // ── Your handlers ────────────────────────────────────────────────
        // Add your own handler IDs here. Register them in manifest.toml
        // under [[registrations.cli_subcommand]], [[registrations.ipc_command]],
        // or [[registrations.event_subscriber]].
        100 => handle_example(args_ptr, args_len),

        _ => br#"{"error":"unknown handler"}"#.to_vec(),
    };

    write_result(&result)
}

// ── Lifecycle handlers ───────────────────────────────────────────────────────

fn handle_init(_args_ptr: u32, _args_len: u32) -> Vec<u8> {
    // Called once after the plugin is loaded.
    // Use this to initialize state. Read persisted state from KV
    // via the host_kv_get host function if needed.
    b"{}".to_vec()
}

fn handle_start(_args_ptr: u32, _args_len: u32) -> Vec<u8> {
    // Called after init. The plugin is now "running" and will
    // receive events and IPC calls.
    b"{}".to_vec()
}

fn handle_stop(_args_ptr: u32, _args_len: u32) -> Vec<u8> {
    // Called before the plugin is unloaded. Persist any state
    // via the host_kv_set host function here.
    b"{}".to_vec()
}

// ── Example handler ──────────────────────────────────────────────────────────

fn handle_example(args_ptr: u32, args_len: u32) -> Vec<u8> {
    // Example: echo the input arguments back as the result.
    if args_len == 0 {
        return b"{}".to_vec();
    }
    unsafe {
        std::slice::from_raw_parts(args_ptr as *const u8, args_len as usize).to_vec()
    }
}

// ── Result helper ────────────────────────────────────────────────────────────

/// Write a result byte slice into WASM memory and return the packed pointer.
fn write_result(result: &[u8]) -> u64 {
    let ptr = nexus_alloc(result.len() as u32);
    if ptr != 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(result.as_ptr(), ptr as *mut u8, result.len());
        }
    }
    ((ptr as u64) << 32) | (result.len() as u64)
}
"#;

// ── Scaffold function ────────────────────────────────────────────────────────

/// Generate a new plugin project from a template.
///
/// Creates `output_dir` and writes all project files.
///
/// # Errors
///
/// Returns [`PluginError::ManifestValidation`] if `plugin_id` is invalid.
/// Returns [`PluginError::Io`] if the directory already exists and is non-empty.
pub fn scaffold(
    output_dir: &Path,
    template: PluginTemplate,
    config: &ScaffoldConfig,
) -> Result<(), PluginError> {
    // Validate plugin ID
    let id_re = regex_lite::Regex::new(
        r"^[a-z0-9]+([-._][a-z0-9]+)*\.[a-z0-9]+([-._][a-z0-9]+)*$",
    )
    .expect("valid regex");
    if !id_re.is_match(&config.plugin_id) {
        return Err(PluginError::ManifestValidation {
            plugin_id: config.plugin_id.clone(),
            reason: format!(
                "plugin ID '{}' does not match required format",
                config.plugin_id
            ),
        });
    }

    // Check output dir
    if output_dir.exists() {
        let is_empty = output_dir
            .read_dir()
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if !is_empty {
            return Err(PluginError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("directory already exists and is non-empty: {}", output_dir.display()),
            )));
        }
    }

    // Create directories
    std::fs::create_dir_all(output_dir.join("src"))?;

    // Select manifest template
    let manifest_template = match template {
        PluginTemplate::Core => MANIFEST_CORE_TEMPLATE,
        PluginTemplate::Community => MANIFEST_COMMUNITY_TEMPLATE,
    };

    // Apply substitutions and write files
    let sub = |s: &str| -> String {
        s.replace("{{plugin-id}}", &config.plugin_id)
            .replace("{{plugin-name}}", &config.plugin_name)
            .replace("{{author}}", &config.author)
            .replace("{{description}}", &config.description)
    };

    std::fs::write(output_dir.join("Cargo.toml"), sub(CARGO_TOML_TEMPLATE))?;
    std::fs::write(output_dir.join("manifest.toml"), sub(manifest_template))?;
    std::fs::write(output_dir.join("src/lib.rs"), sub(LIB_RS_TEMPLATE))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ScaffoldConfig {
        ScaffoldConfig {
            plugin_id: "com.example.test".to_string(),
            plugin_name: "Test Plugin".to_string(),
            author: "Jane Doe".to_string(),
            description: "A test plugin.".to_string(),
        }
    }

    #[test]
    fn scaffold_creates_directory_structure() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("my-plugin");
        scaffold(&out, PluginTemplate::Community, &test_config()).unwrap();

        assert!(out.join("Cargo.toml").is_file());
        assert!(out.join("manifest.toml").is_file());
        assert!(out.join("src/lib.rs").is_file());
    }

    #[test]
    fn scaffold_community_manifest_has_community_trust_level() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("community");
        scaffold(&out, PluginTemplate::Community, &test_config()).unwrap();

        let manifest = std::fs::read_to_string(out.join("manifest.toml")).unwrap();
        assert!(manifest.contains(r#"trust_level = "community""#));
        assert!(manifest.contains("fuel = 10000000"));
    }

    #[test]
    fn scaffold_core_manifest_has_core_trust_level() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("core");
        scaffold(&out, PluginTemplate::Core, &test_config()).unwrap();

        let manifest = std::fs::read_to_string(out.join("manifest.toml")).unwrap();
        assert!(manifest.contains(r#"trust_level = "core""#));
        assert!(manifest.contains("fuel = 0"));
    }

    #[test]
    fn scaffold_substitutes_plugin_id() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("sub-id");
        scaffold(&out, PluginTemplate::Community, &test_config()).unwrap();

        let manifest = std::fs::read_to_string(out.join("manifest.toml")).unwrap();
        assert!(manifest.contains("com.example.test"));
        assert!(!manifest.contains("{{plugin-id}}"));

        let cargo = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("com.example.test"));
    }

    #[test]
    fn scaffold_substitutes_author() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("sub-author");
        scaffold(&out, PluginTemplate::Community, &test_config()).unwrap();

        let cargo = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("Jane Doe"));
    }

    #[test]
    fn scaffold_rejects_invalid_plugin_id() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("bad-id");
        let mut config = test_config();
        config.plugin_id = "INVALID_ID".to_string();

        let result = scaffold(&out, PluginTemplate::Community, &config);
        assert!(matches!(result, Err(PluginError::ManifestValidation { .. })));
        assert!(!out.exists());
    }
}
```

- [ ] **Step 2: Add `mod scaffold;` to `lib.rs`**

Edit `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/src/lib.rs`, add after the last `mod` declaration:

```rust
mod scaffold;

pub use scaffold::{scaffold, PluginTemplate, ScaffoldConfig};
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo nextest run -p nexus-plugins -- scaffold::tests`
Expected: all 6 tests PASS.

- [ ] **Step 4: Run clippy**

Run: `cargo clippy -p nexus-plugins -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/nexus-plugins/src/
git commit -m "feat(plugins): add scaffold module with core and community plugin templates"
```

---

### Task 2: Verify workspace

**Files:** (none — verification only)

- [ ] **Step 1: Run all tests**

Run: `cargo nextest run --workspace`
Expected: all tests pass (321 existing + 6 new scaffold tests).

---

## Phase 2: Smoke Test Update

### Task 3: Add scaffold tests to PRD 04 smoke test

**Files:**
- Modify: `crates/nexus-plugins/tests/prd-04-smoke.rs`

- [ ] **Step 1: Add scaffold smoke tests**

Add to the end of `/mnt/c/Users/baile/dev/nexus/crates/nexus-plugins/tests/prd-04-smoke.rs`:

```rust
#[test]
fn scaffold_types_accessible() {
    let _: Option<nexus_plugins::PluginTemplate> = None;
    let _: Option<nexus_plugins::ScaffoldConfig> = None;
}

#[test]
fn scaffold_generates_compilable_project() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("smoke-plugin");

    let config = nexus_plugins::ScaffoldConfig {
        plugin_id: "com.test.scaffold.smoke".to_string(),
        plugin_name: "Scaffold Smoke".to_string(),
        author: "Tester".to_string(),
        description: "Smoke test plugin.".to_string(),
    };

    nexus_plugins::scaffold(&out, nexus_plugins::PluginTemplate::Community, &config).unwrap();

    // Verify all files exist and contain expected content
    let manifest = std::fs::read_to_string(out.join("manifest.toml")).unwrap();
    assert!(manifest.contains("com.test.scaffold.smoke"));
    assert!(manifest.contains("community"));

    let lib_rs = std::fs::read_to_string(out.join("src/lib.rs")).unwrap();
    assert!(lib_rs.contains("nexus_dispatch"));
    assert!(lib_rs.contains("nexus_alloc"));
    assert!(lib_rs.contains("handle_init"));
}
```

- [ ] **Step 2: Run smoke test**

Run: `cargo nextest run -p nexus-plugins --test prd-04-smoke`
Expected: all smoke tests PASS.

- [ ] **Step 3: Run full workspace**

Run: `cargo nextest run --workspace`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-plugins/tests/prd-04-smoke.rs
git commit -m "test(plugins): add scaffold smoke tests to PRD 04 smoke test"
```

---

## Summary

3 tasks across 2 phases produce:
- `scaffold.rs` module with embedded templates (Cargo.toml, manifest.toml, src/lib.rs)
- `scaffold()` function with ID validation and placeholder substitution
- Two template variants (core: unlimited fuel, community: declared capabilities)
- 6 unit tests + 2 smoke tests
