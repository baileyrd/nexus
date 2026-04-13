//! Plugin manifest data types, TOML parser, and semantic validation.
//!
//! The entry points are [`parse_manifest`], [`load_manifest`], and
//! [`validate`]. All three are re-exported from the crate root.

use std::collections::HashSet;
use std::path::Path;

use nexus_kernel::{Capability, TrustLevel};
use regex_lite::Regex;
use serde::Deserialize;

use crate::PluginError;

// ─── Public data types ────────────────────────────────────────────────────────

/// A fully-parsed plugin manifest.
///
/// Produced by [`parse_manifest`] / [`load_manifest`]; validated by
/// [`validate`].
#[derive(Debug, Clone)]
pub struct PluginManifest {
    /// Reverse-DNS plugin identifier (e.g. `com.example.my-plugin`).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Semantic version string (validated by [`validate`]).
    pub version: String,
    /// Trust level declared in the manifest.
    pub trust_level: TrustLevel,
    /// Minimum Nexus API version required (e.g. `"1"`).
    pub api_version: String,
    /// Capability declarations.
    pub capabilities: ManifestCapabilities,
    /// WASM module configuration.
    pub wasm: WasmConfig,
    /// Optional settings schema reference.
    pub settings: Option<SettingsConfig>,
    /// Extension-point registrations.
    pub registrations: Registrations,
    /// Lifecycle hook enablement.
    pub lifecycle: LifecycleConfig,
}

/// Capability strings declared in the manifest.
#[derive(Debug, Clone)]
pub struct ManifestCapabilities {
    /// Capabilities that the plugin requires. If any are denied the plugin
    /// will not load.
    pub required: Vec<String>,
    /// Capabilities that the plugin will use if available, but can operate
    /// without.
    pub optional: Vec<String>,
}

/// WASM module configuration declared in the manifest.
#[derive(Debug, Clone)]
pub struct WasmConfig {
    /// Relative path to the `.wasm` file inside the plugin directory.
    pub module: String,
    /// Linear-memory limit in mebibytes. Default: `16`.
    pub memory_mb: u32,
    /// Wasmtime fuel budget. Default: `10_000_000`.
    /// Core plugins may set this to `0` to disable metering.
    pub fuel: u64,
    /// Maximum wall-clock milliseconds a single dispatch call may take.
    /// Default: `5000`. Set to `0` to disable.
    pub max_execution_ms: u64,
}

/// Optional settings schema reference.
#[derive(Debug, Clone)]
pub struct SettingsConfig {
    /// Relative path to the JSON Schema file inside the plugin directory.
    pub schema: String,
}

/// All extension-point registrations declared by the plugin.
#[derive(Debug, Clone, Default)]
pub struct Registrations {
    /// CLI subcommand registrations.
    pub cli_subcommands: Vec<CliSubcommandReg>,
    /// IPC command registrations.
    pub ipc_commands: Vec<IpcCommandReg>,
    /// Event subscriber registrations.
    pub event_subscribers: Vec<EventSubscriberReg>,
}

/// A single CLI subcommand registration.
#[derive(Debug, Clone)]
pub struct CliSubcommandReg {
    /// Unique subcommand identifier.
    pub id: String,
    /// WASM handler function index dispatched to when this subcommand is
    /// invoked.
    pub handler_id: u32,
    /// Short description shown in `--help` output.
    pub description: String,
}

/// A single IPC command registration.
#[derive(Debug, Clone)]
pub struct IpcCommandReg {
    /// Unique IPC command identifier.
    pub id: String,
    /// WASM handler function index dispatched to for this command.
    pub handler_id: u32,
}

/// A single event subscriber registration.
#[derive(Debug, Clone)]
pub struct EventSubscriberReg {
    /// Unique subscriber identifier.
    pub id: String,
    /// Event filter expression (e.g. `"FileCreated"`).
    pub filter: String,
    /// WASM handler function index dispatched to when a matching event fires.
    pub handler_id: u32,
}

/// Lifecycle hook enablement flags.
#[derive(Debug, Clone, Default)]
pub struct LifecycleConfig {
    /// Called when the binary is loaded into memory. Handler id 3.
    pub on_load: bool,
    /// Called after dependencies are initialized. Handler id 0.
    pub on_init: bool,
    /// Called when the plugin transitions to Started. Handler id 1.
    pub on_start: bool,
    /// Called on graceful shutdown. Handler id 2.
    pub on_stop: bool,
    /// Called after on_stop; final cleanup. Handler id 6.
    pub on_unload: bool,
    /// Called when the plugin is enabled (after being disabled). Handler id 4.
    pub on_enable: bool,
    /// Called when the plugin is disabled by the user. Handler id 5.
    pub on_disable: bool,
    /// Called when the user updates the plugin's settings. Handler id 7.
    pub on_settings_changed: bool,
}

// ─── Private TOML shadow types ────────────────────────────────────────────────

#[derive(Deserialize)]
struct TomlManifest {
    plugin: TomlPlugin,
    #[serde(default)]
    capabilities: TomlCapabilities,
    wasm: TomlWasm,
    settings: Option<TomlSettings>,
    #[serde(default)]
    registrations: TomlRegistrations,
    #[serde(default)]
    lifecycle: TomlLifecycle,
}

#[derive(Deserialize)]
struct TomlPlugin {
    id: String,
    name: String,
    version: String,
    trust_level: String,
    api_version: String,
}

#[derive(Deserialize, Default)]
struct TomlCapabilities {
    #[serde(default)]
    required: Vec<String>,
    #[serde(default)]
    optional: Vec<String>,
}

#[derive(Deserialize)]
struct TomlWasm {
    module: String,
    #[serde(default = "default_memory_mb")]
    memory_mb: u32,
    #[serde(default = "default_fuel")]
    fuel: u64,
    #[serde(default = "default_max_execution_ms")]
    max_execution_ms: u64,
}

fn default_memory_mb() -> u32 {
    16
}
fn default_fuel() -> u64 {
    10_000_000
}
fn default_max_execution_ms() -> u64 {
    5_000
}

#[derive(Deserialize)]
struct TomlSettings {
    schema: String,
}

#[derive(Deserialize, Default)]
struct TomlRegistrations {
    #[serde(default, rename = "cli_subcommand")]
    cli_subcommands: Vec<TomlCliSubcommandReg>,
    #[serde(default, rename = "ipc_command")]
    ipc_commands: Vec<TomlIpcCommandReg>,
    #[serde(default, rename = "event_subscriber")]
    event_subscribers: Vec<TomlEventSubscriberReg>,
}

#[derive(Deserialize)]
struct TomlCliSubcommandReg {
    id: String,
    handler_id: u32,
    description: String,
}

#[derive(Deserialize)]
struct TomlIpcCommandReg {
    id: String,
    handler_id: u32,
}

#[derive(Deserialize)]
struct TomlEventSubscriberReg {
    id: String,
    filter: String,
    handler_id: u32,
}

#[derive(Deserialize, Default)]
struct TomlLifecycle {
    #[serde(default)] on_load: bool,
    #[serde(default)] on_init: bool,
    #[serde(default)] on_start: bool,
    #[serde(default)] on_stop: bool,
    #[serde(default)] on_unload: bool,
    #[serde(default)] on_enable: bool,
    #[serde(default)] on_disable: bool,
    #[serde(default)] on_settings_changed: bool,
}

// ─── Conversion helpers ───────────────────────────────────────────────────────

fn parse_trust_level(s: &str, path: &str) -> Result<TrustLevel, PluginError> {
    match s {
        "core" => Ok(TrustLevel::Core),
        "community" => Ok(TrustLevel::Community),
        other => Err(PluginError::ManifestInvalid {
            path: path.to_string(),
            reason: format!("unknown trust_level '{other}'; expected 'core' or 'community'"),
        }),
    }
}

fn convert(raw: TomlManifest, path: &str) -> Result<PluginManifest, PluginError> {
    let trust_level = parse_trust_level(&raw.plugin.trust_level, path)?;

    Ok(PluginManifest {
        id: raw.plugin.id,
        name: raw.plugin.name,
        version: raw.plugin.version,
        trust_level,
        api_version: raw.plugin.api_version,
        capabilities: ManifestCapabilities {
            required: raw.capabilities.required,
            optional: raw.capabilities.optional,
        },
        wasm: WasmConfig {
            module: raw.wasm.module,
            memory_mb: raw.wasm.memory_mb,
            fuel: raw.wasm.fuel,
            max_execution_ms: raw.wasm.max_execution_ms,
        },
        settings: raw.settings.map(|s| SettingsConfig { schema: s.schema }),
        registrations: Registrations {
            cli_subcommands: raw
                .registrations
                .cli_subcommands
                .into_iter()
                .map(|r| CliSubcommandReg {
                    id: r.id,
                    handler_id: r.handler_id,
                    description: r.description,
                })
                .collect(),
            ipc_commands: raw
                .registrations
                .ipc_commands
                .into_iter()
                .map(|r| IpcCommandReg {
                    id: r.id,
                    handler_id: r.handler_id,
                })
                .collect(),
            event_subscribers: raw
                .registrations
                .event_subscribers
                .into_iter()
                .map(|r| EventSubscriberReg {
                    id: r.id,
                    filter: r.filter,
                    handler_id: r.handler_id,
                })
                .collect(),
        },
        lifecycle: LifecycleConfig {
            on_load: raw.lifecycle.on_load,
            on_init: raw.lifecycle.on_init,
            on_start: raw.lifecycle.on_start,
            on_stop: raw.lifecycle.on_stop,
            on_unload: raw.lifecycle.on_unload,
            on_enable: raw.lifecycle.on_enable,
            on_disable: raw.lifecycle.on_disable,
            on_settings_changed: raw.lifecycle.on_settings_changed,
        },
    })
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Parse a plugin manifest from a TOML string.
///
/// `manifest_path` is used only for error messages; it does **not** need to
/// exist on disk.
///
/// # Errors
/// Returns [`PluginError::ManifestInvalid`] when the TOML is malformed or
/// contains an unrecognised `trust_level`.
pub fn parse_manifest(toml_str: &str, manifest_path: &str) -> Result<PluginManifest, PluginError> {
    let raw: TomlManifest = toml::from_str(toml_str).map_err(|e| PluginError::ManifestInvalid {
        path: manifest_path.to_string(),
        reason: e.to_string(),
    })?;
    convert(raw, manifest_path)
}

/// Load and parse a plugin manifest from a file on disk.
///
/// # Errors
/// Returns [`PluginError::ManifestNotFound`] when the file does not exist,
/// [`PluginError::Io`] for other I/O failures, and
/// [`PluginError::ManifestInvalid`] for parse failures.
pub fn load_manifest(manifest_path: &Path) -> Result<PluginManifest, PluginError> {
    let path_str = manifest_path.display().to_string();
    if !manifest_path.exists() {
        return Err(PluginError::ManifestNotFound(path_str));
    }
    let toml_str = std::fs::read_to_string(manifest_path)?;
    parse_manifest(&toml_str, &path_str)
}

// ─── Parsing tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod parsing_tests {
    use super::*;

    /// Minimal valid manifest TOML (only required fields).
    const MINIMAL: &str = r#"
[plugin]
id = "com.example.test"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"
"#;

    /// Full manifest TOML with every optional section and field populated.
    const FULL: &str = r#"
[plugin]
id = "com.example.test"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["fs.read", "kv.read"]
optional = ["net.http"]

[wasm]
module = "test.wasm"
memory_mb = 32
fuel = 5000000

[settings]
schema = "settings.json"

[[registrations.cli_subcommand]]
id = "test.run"
handler_id = 1
description = "Run test"

[[registrations.ipc_command]]
id = "test.query"
handler_id = 100

[[registrations.event_subscriber]]
id = "test.on-file"
filter = "FileCreated"
handler_id = 200

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#;

    #[test]
    fn parse_minimal_manifest() {
        let m = parse_manifest(MINIMAL, "manifest.toml").unwrap();
        assert_eq!(m.id, "com.example.test");
        assert_eq!(m.name, "Test");
        assert_eq!(m.version, "1.0.0");
        assert!(matches!(m.trust_level, TrustLevel::Community));
        assert_eq!(m.api_version, "1");
        assert_eq!(m.wasm.module, "test.wasm");
        assert_eq!(m.wasm.memory_mb, 16); // default
        assert_eq!(m.wasm.fuel, 10_000_000); // default
        assert!(m.settings.is_none());
        assert!(m.registrations.cli_subcommands.is_empty());
        assert!(m.registrations.ipc_commands.is_empty());
        assert!(m.registrations.event_subscribers.is_empty());
    }

    #[test]
    fn parse_full_manifest() {
        let m = parse_manifest(FULL, "manifest.toml").unwrap();
        assert_eq!(m.capabilities.required, ["fs.read", "kv.read"]);
        assert_eq!(m.capabilities.optional, ["net.http"]);
        assert_eq!(m.wasm.memory_mb, 32);
        assert_eq!(m.wasm.fuel, 5_000_000);
        assert!(m.settings.is_some());
        assert_eq!(m.settings.unwrap().schema, "settings.json");
        assert_eq!(m.registrations.cli_subcommands.len(), 1);
        assert_eq!(m.registrations.cli_subcommands[0].handler_id, 1);
        assert_eq!(m.registrations.ipc_commands.len(), 1);
        assert_eq!(m.registrations.ipc_commands[0].handler_id, 100);
        assert_eq!(m.registrations.event_subscribers.len(), 1);
        assert_eq!(m.registrations.event_subscribers[0].handler_id, 200);
        assert!(m.lifecycle.on_init);
        assert!(m.lifecycle.on_start);
        assert!(m.lifecycle.on_stop);
    }

    #[test]
    fn parse_invalid_toml_returns_error() {
        let err = parse_manifest("this is not valid toml ][", "manifest.toml").unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestInvalid { .. }),
            "expected ManifestInvalid, got {err:?}"
        );
    }

    #[test]
    fn parse_unknown_trust_level_returns_error() {
        let toml = MINIMAL.replace("community", "superuser");
        let err = parse_manifest(&toml, "manifest.toml").unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestInvalid { .. }),
            "expected ManifestInvalid, got {err:?}"
        );
    }

    #[test]
    fn parse_missing_wasm_section_returns_error() {
        let toml = r#"
[plugin]
id = "com.example.test"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"
"#;
        let err = parse_manifest(toml, "manifest.toml").unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestInvalid { .. }),
            "expected ManifestInvalid, got {err:?}"
        );
    }

    #[test]
    fn parse_empty_capabilities_defaults_to_empty() {
        let m = parse_manifest(MINIMAL, "manifest.toml").unwrap();
        assert!(m.capabilities.required.is_empty());
        assert!(m.capabilities.optional.is_empty());
    }

    #[test]
    fn parse_empty_registrations_defaults_to_empty() {
        let m = parse_manifest(MINIMAL, "manifest.toml").unwrap();
        assert!(m.registrations.cli_subcommands.is_empty());
        assert!(m.registrations.ipc_commands.is_empty());
        assert!(m.registrations.event_subscribers.is_empty());
    }

    #[test]
    fn parse_lifecycle_defaults_to_false() {
        let m = parse_manifest(MINIMAL, "manifest.toml").unwrap();
        assert!(!m.lifecycle.on_init);
        assert!(!m.lifecycle.on_start);
        assert!(!m.lifecycle.on_stop);
    }
}

/// Validate a parsed [`PluginManifest`] against semantic rules.
///
/// `plugin_dir` is the directory that contains the plugin's files (WASM
/// module, settings schema, etc.).
///
/// # Errors
/// Returns [`PluginError::ManifestValidation`] describing the first rule that
/// is violated.
///
/// # Panics
/// Panics if the internal ID validation regex fails to compile (should never
/// happen — the pattern is a compile-time constant).
pub fn validate(manifest: &PluginManifest, plugin_dir: &Path) -> Result<(), PluginError> {
    let id = &manifest.id;

    // Rule 1: ID format.
    let id_re =
        Regex::new(r"^[a-z0-9]+([-._][a-z0-9]+)*\.[a-z0-9]+([-._][a-z0-9]+)*$").unwrap();
    if !id_re.is_match(id) {
        return Err(PluginError::ManifestValidation {
            plugin_id: id.clone(),
            reason: format!(
                "plugin id '{id}' does not match the required pattern \
                 ^[a-z0-9]+([-._][a-z0-9]+)*\\.[a-z0-9]+([-._][a-z0-9]+)*$"
            ),
        });
    }

    // Rule 2: semver.
    semver::Version::parse(&manifest.version).map_err(|e| PluginError::ManifestValidation {
        plugin_id: id.clone(),
        reason: format!("version '{}' is not valid semver: {e}", manifest.version),
    })?;

    // Rule 3: all capability strings must be known.
    for cap_str in manifest
        .capabilities
        .required
        .iter()
        .chain(manifest.capabilities.optional.iter())
    {
        Capability::from_str(cap_str).map_err(|_| PluginError::ManifestValidation {
            plugin_id: id.clone(),
            reason: format!("unknown capability '{cap_str}'"),
        })?;
    }

    // Rule 4: handler_id values must be unique across all registrations.
    let mut seen_handlers: HashSet<u32> = HashSet::new();
    for h in manifest
        .registrations
        .cli_subcommands
        .iter()
        .map(|r| r.handler_id)
        .chain(
            manifest
                .registrations
                .ipc_commands
                .iter()
                .map(|r| r.handler_id),
        )
        .chain(
            manifest
                .registrations
                .event_subscribers
                .iter()
                .map(|r| r.handler_id),
        )
    {
        if !seen_handlers.insert(h) {
            return Err(PluginError::ManifestValidation {
                plugin_id: id.clone(),
                reason: format!("duplicate handler_id {h}"),
            });
        }
    }

    // Rule 5: memory_mb in [1, 256].
    if manifest.wasm.memory_mb < 1 || manifest.wasm.memory_mb > 256 {
        return Err(PluginError::ManifestValidation {
            plugin_id: id.clone(),
            reason: format!(
                "wasm.memory_mb {} is out of range [1, 256]",
                manifest.wasm.memory_mb
            ),
        });
    }

    // Rule 6: fuel > 0 unless trust_level is Core.
    if manifest.wasm.fuel == 0 && manifest.trust_level != TrustLevel::Core {
        return Err(PluginError::ManifestValidation {
            plugin_id: id.clone(),
            reason: "wasm.fuel must be > 0 for community plugins".to_string(),
        });
    }

    // Rule 7: wasm module file must exist.
    let wasm_path = plugin_dir.join(&manifest.wasm.module);
    if !wasm_path.exists() {
        return Err(PluginError::ManifestValidation {
            plugin_id: id.clone(),
            reason: format!(
                "wasm module '{}' not found in plugin directory",
                manifest.wasm.module
            ),
        });
    }

    // Rule 8: settings schema file must exist if specified.
    if let Some(settings) = &manifest.settings {
        let schema_path = plugin_dir.join(&settings.schema);
        if !schema_path.exists() {
            return Err(PluginError::ManifestValidation {
                plugin_id: id.clone(),
                reason: format!(
                    "settings schema '{}' not found in plugin directory",
                    settings.schema
                ),
            });
        }
    }

    Ok(())
}

// ─── Validation tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod validation_tests {
    use super::*;

    const MINIMAL: &str = r#"
[plugin]
id = "com.example.test"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"
"#;

    const FULL: &str = r#"
[plugin]
id = "com.example.test"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["fs.read", "kv.read"]
optional = ["net.http"]

[wasm]
module = "test.wasm"
memory_mb = 32
fuel = 5000000

[settings]
schema = "settings.json"

[[registrations.cli_subcommand]]
id = "test.run"
handler_id = 1
description = "Run test"

[[registrations.ipc_command]]
id = "test.query"
handler_id = 100

[[registrations.event_subscriber]]
id = "test.on-file"
filter = "FileCreated"
handler_id = 200

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#;

    fn make_test_plugin_dir(wasm_name: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(wasm_name), b"fake-wasm").unwrap();
        dir
    }

    fn make_test_plugin_dir_with_schema(wasm_name: &str) -> tempfile::TempDir {
        let dir = make_test_plugin_dir(wasm_name);
        std::fs::write(dir.path().join("settings.json"), b"{}").unwrap();
        dir
    }

    fn valid_manifest() -> PluginManifest {
        parse_manifest(MINIMAL, "manifest.toml").unwrap()
    }

    #[test]
    fn validate_accepts_valid_manifest() {
        let dir = make_test_plugin_dir("test.wasm");
        validate(&valid_manifest(), dir.path()).unwrap();
    }

    #[test]
    fn validate_rejects_invalid_id() {
        let dir = make_test_plugin_dir("test.wasm");
        let mut m = valid_manifest();
        m.id = "COM.Example.Test".to_string();
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestValidation { ref reason, .. } if reason.contains("pattern")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_rejects_invalid_semver() {
        let dir = make_test_plugin_dir("test.wasm");
        let mut m = valid_manifest();
        m.version = "not-a-version".to_string();
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestValidation { ref reason, .. } if reason.contains("semver")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_rejects_unknown_capability() {
        let dir = make_test_plugin_dir("test.wasm");
        let mut m = valid_manifest();
        m.capabilities.required.push("fs.teleport".to_string());
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestValidation { ref reason, .. } if reason.contains("unknown capability")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_rejects_duplicate_handler_id() {
        let dir = make_test_plugin_dir("test.wasm");
        let mut m = valid_manifest();
        m.registrations.cli_subcommands.push(CliSubcommandReg {
            id: "cmd.a".to_string(),
            handler_id: 42,
            description: "A".to_string(),
        });
        m.registrations.ipc_commands.push(IpcCommandReg {
            id: "ipc.a".to_string(),
            handler_id: 42,
        });
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestValidation { ref reason, .. } if reason.contains("duplicate handler_id")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_rejects_memory_out_of_range() {
        let dir = make_test_plugin_dir("test.wasm");
        let mut m = valid_manifest();
        m.wasm.memory_mb = 512;
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestValidation { ref reason, .. } if reason.contains("out of range")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_rejects_zero_fuel_for_community() {
        let dir = make_test_plugin_dir("test.wasm");
        let mut m = valid_manifest();
        m.wasm.fuel = 0;
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestValidation { ref reason, .. } if reason.contains("fuel")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_allows_zero_fuel_for_core() {
        let dir = make_test_plugin_dir("test.wasm");
        let mut m = valid_manifest();
        m.wasm.fuel = 0;
        m.trust_level = TrustLevel::Core;
        validate(&m, dir.path()).unwrap();
    }

    #[test]
    fn validate_rejects_missing_wasm_file() {
        let dir = make_test_plugin_dir("other.wasm");
        let err = validate(&valid_manifest(), dir.path()).unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestValidation { ref reason, .. } if reason.contains("not found")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_accepts_manifest_with_existing_settings_schema() {
        let dir = make_test_plugin_dir_with_schema("test.wasm");
        let m = parse_manifest(FULL, "manifest.toml").unwrap();
        validate(&m, dir.path()).unwrap();
    }
}
