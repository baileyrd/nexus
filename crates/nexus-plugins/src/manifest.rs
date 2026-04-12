//! Plugin manifest data types, TOML parser, and semantic validation.
//!
//! The entry points are [`parse_manifest`], [`load_manifest`], and
//! [`validate`]. All three are re-exported from the crate root.

use std::path::Path;

use nexus_kernel::TrustLevel;
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
#[derive(Debug, Clone)]
pub struct LifecycleConfig {
    /// Whether the plugin exports an `on_init` handler.
    pub on_init: bool,
    /// Whether the plugin exports an `on_start` handler.
    pub on_start: bool,
    /// Whether the plugin exports an `on_stop` handler.
    pub on_stop: bool,
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
}

fn default_memory_mb() -> u32 {
    16
}
fn default_fuel() -> u64 {
    10_000_000
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
    #[serde(default)]
    on_init: bool,
    #[serde(default)]
    on_start: bool,
    #[serde(default)]
    on_stop: bool,
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
            on_init: raw.lifecycle.on_init,
            on_start: raw.lifecycle.on_start,
            on_stop: raw.lifecycle.on_stop,
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

/// Validate a parsed [`PluginManifest`] against semantic rules.
///
/// `plugin_dir` is the directory that contains the plugin's files (WASM
/// module, settings schema, etc.).
///
/// # Errors
/// Returns [`PluginError::ManifestValidation`] describing the first rule that
/// is violated.
pub fn validate(_manifest: &PluginManifest, _plugin_dir: &Path) -> Result<(), PluginError> {
    // Implemented in a follow-up commit.
    Ok(())
}
