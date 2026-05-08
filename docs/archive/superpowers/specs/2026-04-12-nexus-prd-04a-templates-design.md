# Nexus PRD 04a — Plugin Templates (M1) Design Spec

**Version:** 1.0
**Date:** 2026-04-12
**Status:** Approved (brainstorming session output)
**Scope:** Scaffold module inside `nexus-plugins` that generates plugin project files from embedded templates. Two variants (core, community). Unit-tested on generated file contents.

**Parent docs:**
- [`PRDs/04a-plugin-templates.md`](../../../PRDs/04a-plugin-templates.md) — full PRD
- [`2026-04-12-nexus-prd-04-plugins-design.md`](2026-04-12-nexus-prd-04-plugins-design.md) — plugin system spec (prerequisite)
- [`2026-04-11-nexus-m1-foundation-spec.md`](2026-04-11-nexus-m1-foundation-spec.md) — M1 spec §6

---

## 1. Architecture Overview

A `scaffold` module added to the existing `nexus-plugins` crate. No new crate, no new dependencies. Templates are embedded as `&str` constants in the source file. String substitution replaces placeholders (`{{plugin-id}}`, `{{plugin-name}}`, etc.) with user-provided values.

The CLI's `nexus plugin scaffold` command (PRD 05) will call `nexus_plugins::scaffold()` to generate a new plugin project.

---

## 2. Module Structure

```
crates/nexus-plugins/src/
└── scaffold.rs          # template generation + embedded templates
```

Added to `lib.rs` as `mod scaffold;` with public re-exports.

---

## 3. Public API

```rust
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

/// Generate a new plugin project from a template.
///
/// Creates the output directory and writes all project files.
/// Returns an error if the directory already exists and is non-empty,
/// or if the plugin ID is invalid.
pub fn scaffold(
    output_dir: &Path,
    template: PluginTemplate,
    config: &ScaffoldConfig,
) -> Result<(), PluginError>;
```

---

## 4. Generated File Structure

```
<output_dir>/
├── Cargo.toml
├── manifest.toml
└── src/
    └── lib.rs
```

Single `lib.rs` rather than the PRD's 4-file split (`lib.rs`, `plugin.rs`, `events.rs`, `state.rs`). A single file with clear sections and comments is a better starting point — the user splits when their plugin grows.

---

## 5. Generated Files

### 5.1 Cargo.toml

```toml
[package]
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
```

No dependency on `nexus-kernel` or any Nexus crate — WASM plugins communicate through the host function ABI only.

### 5.2 manifest.toml

**Core variant:**

```toml
[plugin]
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
fuel = 0                         # unlimited for core plugins

[lifecycle]
on_init = true
on_start = true
on_stop = true
```

**Community variant:**

```toml
[plugin]
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
```

### 5.3 src/lib.rs

A working minimal plugin that:
- Exports `nexus_alloc(size: u32) -> u32` — allocates memory in WASM linear memory
- Exports `nexus_dispatch(handler_id: u32, args_ptr: u32, args_len: u32) -> u64` — main dispatch function
- Routes lifecycle handler IDs: 0 = on_init, 1 = on_start, 2 = on_stop
- Includes one example handler (ID 100) that echoes its args
- Has comments explaining the dispatch convention, how to add handlers, and where to add plugin logic
- Returns packed `(result_ptr << 32) | result_len` for results

The generated `lib.rs` is identical for core and community variants — the difference is only in the manifest.

---

## 6. Validation

`scaffold()` validates before generating:

1. `plugin_id` matches the manifest ID regex `^[a-z0-9]+([-._][a-z0-9]+)*\.[a-z0-9]+([-._][a-z0-9]+)*$` — returns `PluginError::ManifestValidation` if invalid
2. `output_dir` doesn't exist or is empty — returns `PluginError::Io` if non-empty directory exists

---

## 7. Template Substitution

Simple string replacement on these placeholders:

| Placeholder | Source |
|---|---|
| `{{plugin-id}}` | `config.plugin_id` |
| `{{plugin-name}}` | `config.plugin_name` |
| `{{author}}` | `config.author` |
| `{{description}}` | `config.description` |

No template engine dependency. Plain `str::replace()`.

---

## 8. Tests

Six unit tests verifying generated file contents:

1. `scaffold_creates_directory_structure` — verify Cargo.toml, manifest.toml, src/lib.rs exist
2. `scaffold_community_manifest_has_community_trust_level` — parse manifest, check trust_level
3. `scaffold_core_manifest_has_core_trust_level` — parse manifest, check trust_level and fuel = 0
4. `scaffold_substitutes_plugin_id` — verify plugin ID appears in manifest and Cargo.toml
5. `scaffold_substitutes_author` — verify author appears in Cargo.toml
6. `scaffold_rejects_invalid_plugin_id` — bad ID returns error

---

## 9. Deferred from M1

| Item | PRD Section | Rationale | Revisit |
|---|---|---|---|
| `cargo-generate` support | §2.1 | Embedded templates are simpler, no extra dep | v0.2 if templates grow complex |
| Multi-file split (plugin.rs, events.rs, state.rs) | §2, §4 | Single lib.rs is sufficient starting point | When plugins grow complex |
| `nexus plugin scaffold` CLI command | §8 | Part of PRD 05 (CLI) | PRD 05 |
| Template compilation test (generate + compile to WASM) | §9.1 | Unit tests on file contents sufficient for M1 | v0.2 |
| Icon assets (icon-light.svg, icon-dark.svg) | §2 | No UI in M1 to display them | M2 (PRD 07) |
| README.md generation | §2 | No external consumers in M1 | v0.2 |
