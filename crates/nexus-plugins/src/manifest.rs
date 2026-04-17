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

/// Declared plugin runtime tier. Populated either from the explicit
/// `runtime` field in `[plugin]` (UI F-3.3.1) or inferred from the
/// presence of the `[wasm]` / `[script]` sections for backwards
/// compatibility with older manifests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginRuntime {
    /// Native Rust core plugin (no `[wasm]` / `[script]` section).
    Native,
    /// WASM community plugin (requires `[wasm]`).
    Wasm,
    /// JS script plugin (requires `[script]`).
    Script,
}

impl PluginRuntime {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "native" => Some(Self::Native),
            "wasm" => Some(Self::Wasm),
            "script" => Some(Self::Script),
            _ => None,
        }
    }
}

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
    /// Runtime tier declared in `[plugin]` (UI F-3.3.1). Inferred from
    /// the presence of `[wasm]` / `[script]` sections when absent so
    /// pre-F-3.3.1 manifests keep loading. [`validate`] rejects any
    /// explicit declaration that disagrees with the section present.
    pub runtime: PluginRuntime,
    /// Capability declarations.
    pub capabilities: ManifestCapabilities,
    /// WASM module configuration.
    ///
    /// `None` for `trust_level = "core"` plugins and script-based community
    /// plugins. Required for WASM community plugins. Mutually exclusive
    /// with [`script`](Self::script).
    pub wasm: Option<WasmConfig>,
    /// Script (JS) module configuration.
    ///
    /// `None` for core and WASM community plugins. Mutually exclusive with
    /// [`wasm`](Self::wasm). When present, the plugin's handlers execute in
    /// the Tauri WebView rather than a WASM sandbox.
    pub script: Option<ScriptConfig>,
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

/// Script (JS) module configuration declared in the manifest.
///
/// Script plugins execute in the Tauri WebView as ES modules, loaded
/// via the `nexus-plugin://` custom protocol.
#[derive(Debug, Clone)]
pub struct ScriptConfig {
    /// Relative path to the JS entry point inside the plugin directory.
    pub module: String,
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
    /// UI palette command registrations.
    pub ui_commands: Vec<UiCommandReg>,
    /// UI side-panel registrations.
    pub ui_panels: Vec<UiPanelReg>,
    /// Per-plugin Settings-modal tab registrations.
    pub ui_settings_tabs: Vec<UiSettingsTabReg>,
    /// Workspace-ribbon icon registrations.
    pub ui_ribbon_items: Vec<UiRibbonItemReg>,
    /// Status-bar entry registrations.
    pub ui_status_items: Vec<UiStatusItemReg>,
    /// Editor slash-command registrations.
    pub slash_commands: Vec<UiSlashCommandReg>,
    /// Application menu-bar item registrations.
    pub menu_items: Vec<MenuItemReg>,
    /// URI / protocol-handler registrations.
    pub uri_handlers: Vec<UriHandlerReg>,
}

/// Which side of the workspace a plugin-contributed panel docks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanelSide {
    /// Dock on the left side panel (default when `side` is omitted).
    Left,
    /// Dock on the right side panel.
    Right,
}

impl Default for PanelSide {
    fn default() -> Self {
        Self::Left
    }
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
    /// Event filter expression (e.g. `"com.nexus.storage.file_created"`).
    pub filter: String,
    /// WASM handler function index dispatched to when a matching event fires.
    pub handler_id: u32,
}

/// A single UI command registration — a plugin-contributed entry that
/// appears in the command palette and dispatches back to the plugin when
/// invoked.
#[derive(Debug, Clone)]
pub struct UiCommandReg {
    /// Unique command identifier within the plugin.
    pub id: String,
    /// WASM handler function index dispatched to when the user invokes
    /// this command.
    pub handler_id: u32,
    /// Primary label shown in the command palette.
    pub title: String,
    /// Optional category badge (e.g. "AI", "Git").
    pub category: Option<String>,
    /// Optional Lucide icon name.
    pub icon: Option<String>,
    /// Optional default keybinding. A `+`-separated chord understood by
    /// the frontend keybinding dispatcher — e.g. `"Mod+Shift+H"`,
    /// `"Ctrl+Alt+/"`. `"Mod"` resolves to Ctrl on Linux/Windows and
    /// Cmd on macOS. Users will eventually be able to override this.
    pub keybinding: Option<String>,
}

/// A single UI side-panel registration — a plugin-contributed panel
/// that docks into the left or right side panel. The plugin's handler
/// is invoked when the panel mounts and must return a string; the
/// frontend renders that string inside the panel's content area.
#[derive(Debug, Clone)]
pub struct UiPanelReg {
    /// Unique panel identifier within the plugin.
    pub id: String,
    /// WASM handler index invoked to produce the panel's content.
    pub handler_id: u32,
    /// Human-readable panel title shown in the selector tab.
    pub title: String,
    /// Lucide icon name for the panel selector.
    pub icon: String,
    /// Which side panel to dock into. Defaults to [`PanelSide::Left`].
    pub side: PanelSide,
}

/// A single per-plugin Settings-modal tab registration. The plugin's
/// handler is invoked when the tab is shown and must return a JSON
/// object with a `content` string that the frontend renders below
/// the tab's auto-generated plugin header.
#[derive(Debug, Clone)]
pub struct UiSettingsTabReg {
    /// Unique tab identifier within the plugin.
    pub id: String,
    /// WASM handler index invoked to produce the tab's content.
    pub handler_id: u32,
    /// Human-readable tab title shown in the Settings rail.
    pub title: String,
    /// Lucide icon name for the rail entry.
    pub icon: String,
}

/// A single workspace-ribbon icon registration. The item delegates to
/// one of the plugin's own `ui_command` ids — clicking the ribbon icon
/// invokes that command through the contribution registry, so ribbon
/// entries don't need their own handler_id.
#[derive(Debug, Clone)]
pub struct UiRibbonItemReg {
    /// Unique ribbon-entry identifier within the plugin.
    pub id: String,
    /// Lucide icon name for the ribbon button.
    pub icon: String,
    /// Hover tooltip and accessible label.
    pub tooltip: String,
    /// Target `ui_command.id` (same manifest) invoked when the ribbon
    /// icon is clicked.
    pub command: String,
}

/// A single status-bar entry registration. Entries render as either a
/// plain counter (text/icon, no `command`) or a clickable button
/// (command set). At least one of `text` or `icon` must be present.
#[derive(Debug, Clone)]
pub struct UiStatusItemReg {
    /// Unique status-bar-entry identifier within the plugin.
    pub id: String,
    /// Text shown to the right of the icon. `None` for icon-only.
    pub text: Option<String>,
    /// Lucide icon name. `None` for text-only.
    pub icon: Option<String>,
    /// Hover tooltip; falls back to `text` when not set.
    pub tooltip: Option<String>,
    /// Optional `ui_command.id` (same manifest) invoked on click. When
    /// unset the entry renders as a non-interactive counter.
    pub command: Option<String>,
}

/// A single editor slash-command registration — a plugin-contributed
/// entry that appears in the `/` trigger overlay in the CodeMirror
/// editor. Selecting the entry inserts [`Self::template`] at the
/// cursor, with a `\0` byte in the template marking the final cursor
/// position. Purely declarative (no handler dispatch) in this
/// revision; dynamic handler-provided templates are a future slice.
#[derive(Debug, Clone)]
pub struct UiSlashCommandReg {
    /// Unique slash-command identifier within the plugin.
    pub id: String,
    /// Primary label shown in the slash menu.
    pub label: String,
    /// Short dimmed description shown beside the label.
    pub description: String,
    /// Extra keywords for fuzzy matching (may be empty).
    pub aliases: Vec<String>,
    /// Short text badge shown on the left of the row (e.g. `"AI"`).
    pub badge: String,
    /// Markdown template inserted when the command is selected. The
    /// first `\0` (NUL) in the template marks the final cursor
    /// position; NUL is used instead of a printable marker so
    /// templates containing `|`, `#`, etc. are not misinterpreted.
    pub template: String,
}

/// A single application menu-bar item registration. The item delegates
/// to one of the plugin's own `ui_command` ids; no direct handler_id is
/// needed (same model as ribbon items).
#[derive(Debug, Clone)]
pub struct MenuItemReg {
    /// Unique identifier within the plugin.
    pub id: String,
    /// Top-level menu to insert into — e.g. `"File"`, `"Edit"`, `"View"`,
    /// `"Help"`.
    pub menu: String,
    /// Label shown in the menu.
    pub label: String,
    /// Target `ui_command.id` (same manifest) invoked when the item is
    /// selected. Pre-qualified (`plugin:<plugin_id>:<command>`) by the
    /// aggregator so the frontend can pass it straight to
    /// `contributions.invokeCommand`.
    pub command: String,
    /// Optional display-order hint within the menu. Lower values sort first.
    pub order: Option<i32>,
    /// When `true`, a separator is rendered immediately before this item.
    pub separator_before: bool,
}

/// A single URI / protocol-handler registration. Incoming URIs whose
/// scheme matches [`Self::scheme`] are dispatched to the plugin's
/// WASM handler.
#[derive(Debug, Clone)]
pub struct UriHandlerReg {
    /// Unique identifier within the plugin.
    pub id: String,
    /// URI scheme to claim — e.g. `"nexus"`.
    pub scheme: String,
    /// WASM handler function index dispatched when a matching URI arrives.
    pub handler_id: u32,
}

/// Lifecycle hook enablement flags.
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct LifecycleConfig {
    /// Called when the binary is loaded into memory. Handler id 3.
    pub on_load: bool,
    /// Called after dependencies are initialized. Handler id 0.
    pub on_init: bool,
    /// Called when the plugin transitions to Started. Handler id 1.
    pub on_start: bool,
    /// Called on graceful shutdown. Handler id 2.
    pub on_stop: bool,
    /// Called after `on_stop`; final cleanup. Handler id 6.
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
    /// Absent for core plugins; required for WASM community plugins.
    wasm: Option<TomlWasm>,
    /// Absent for core and WASM plugins; mutually exclusive with `wasm`.
    script: Option<TomlScript>,
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
    /// Optional for backwards compatibility (UI F-3.3.1). When present it
    /// must be one of `"native"`, `"wasm"`, `"script"` and must agree with
    /// the declared sections; when absent the loader infers from sections.
    #[serde(default)]
    runtime: Option<String>,
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

#[derive(Deserialize)]
struct TomlScript {
    module: String,
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
    #[serde(default, rename = "ui_command")]
    ui_commands: Vec<TomlUiCommandReg>,
    #[serde(default, rename = "ui_panel")]
    ui_panels: Vec<TomlUiPanelReg>,
    #[serde(default, rename = "ui_settings_tab")]
    ui_settings_tabs: Vec<TomlUiSettingsTabReg>,
    #[serde(default, rename = "ui_ribbon_item")]
    ui_ribbon_items: Vec<TomlUiRibbonItemReg>,
    #[serde(default, rename = "ui_status_item")]
    ui_status_items: Vec<TomlUiStatusItemReg>,
    #[serde(default, rename = "slash_command")]
    slash_commands: Vec<TomlUiSlashCommandReg>,
    #[serde(default, rename = "menu_item")]
    menu_items: Vec<TomlMenuItemReg>,
    #[serde(default, rename = "uri_handler")]
    uri_handlers: Vec<TomlUriHandlerReg>,
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

#[derive(Deserialize)]
struct TomlUiCommandReg {
    id: String,
    handler_id: u32,
    title: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    keybinding: Option<String>,
}

#[derive(Deserialize)]
struct TomlUiPanelReg {
    id: String,
    handler_id: u32,
    title: String,
    icon: String,
    #[serde(default)]
    side: PanelSide,
}

#[derive(Deserialize)]
struct TomlUiSettingsTabReg {
    id: String,
    handler_id: u32,
    title: String,
    icon: String,
}

#[derive(Deserialize)]
struct TomlUiRibbonItemReg {
    id: String,
    icon: String,
    tooltip: String,
    command: String,
}

#[derive(Deserialize)]
struct TomlUiStatusItemReg {
    id: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    tooltip: Option<String>,
    #[serde(default)]
    command: Option<String>,
}

#[derive(Deserialize)]
struct TomlUiSlashCommandReg {
    id: String,
    label: String,
    description: String,
    #[serde(default)]
    aliases: Vec<String>,
    badge: String,
    template: String,
}

#[derive(Deserialize)]
struct TomlMenuItemReg {
    id: String,
    menu: String,
    label: String,
    command: String,
    #[serde(default)]
    order: Option<i32>,
    #[serde(default)]
    separator_before: bool,
}

#[derive(Deserialize)]
struct TomlUriHandlerReg {
    id: String,
    scheme: String,
    handler_id: u32,
}

#[derive(Deserialize, Default)]
#[allow(clippy::struct_excessive_bools)]
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

    // Derive the runtime tier. An explicit `runtime` field wins if it
    // parses; otherwise infer from the declared sections so pre-F-3.3.1
    // manifests keep loading. When a user sets `runtime` explicitly, it
    // must agree with the section that accompanies it — the conflict is
    // caught here at parse time because `validate` only sees the resolved
    // enum variant.
    let inferred = match (raw.wasm.is_some(), raw.script.is_some()) {
        (true, false) => PluginRuntime::Wasm,
        (false, true) => PluginRuntime::Script,
        (false, false) => PluginRuntime::Native,
        (true, true) => PluginRuntime::Wasm, // rule 5 rejects this in validate
    };
    let runtime = if let Some(ref r) = raw.plugin.runtime {
        let explicit = PluginRuntime::parse(r).ok_or_else(|| PluginError::ManifestInvalid {
            path: path.to_string(),
            reason: format!(
                "unknown runtime '{r}'; expected 'native', 'wasm', or 'script'"
            ),
        })?;
        if explicit != inferred {
            return Err(PluginError::ManifestInvalid {
                path: path.to_string(),
                reason: format!(
                    "plugin.runtime = {r:?} disagrees with the declared sections"
                ),
            });
        }
        explicit
    } else {
        inferred
    };

    let wasm = raw.wasm.map(|w| WasmConfig {
        module: w.module,
        memory_mb: w.memory_mb,
        fuel: w.fuel,
        max_execution_ms: w.max_execution_ms,
    });
    let script = raw.script.map(|s| ScriptConfig { module: s.module });

    Ok(PluginManifest {
        id: raw.plugin.id,
        name: raw.plugin.name,
        version: raw.plugin.version,
        trust_level,
        api_version: raw.plugin.api_version,
        runtime,
        capabilities: ManifestCapabilities {
            required: raw.capabilities.required,
            optional: raw.capabilities.optional,
        },
        wasm,
        script,
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
            ui_commands: raw
                .registrations
                .ui_commands
                .into_iter()
                .map(|r| UiCommandReg {
                    id: r.id,
                    handler_id: r.handler_id,
                    title: r.title,
                    category: r.category,
                    icon: r.icon,
                    keybinding: r.keybinding,
                })
                .collect(),
            ui_panels: raw
                .registrations
                .ui_panels
                .into_iter()
                .map(|r| UiPanelReg {
                    id: r.id,
                    handler_id: r.handler_id,
                    title: r.title,
                    icon: r.icon,
                    side: r.side,
                })
                .collect(),
            ui_settings_tabs: raw
                .registrations
                .ui_settings_tabs
                .into_iter()
                .map(|r| UiSettingsTabReg {
                    id: r.id,
                    handler_id: r.handler_id,
                    title: r.title,
                    icon: r.icon,
                })
                .collect(),
            ui_ribbon_items: raw
                .registrations
                .ui_ribbon_items
                .into_iter()
                .map(|r| UiRibbonItemReg {
                    id: r.id,
                    icon: r.icon,
                    tooltip: r.tooltip,
                    command: r.command,
                })
                .collect(),
            ui_status_items: raw
                .registrations
                .ui_status_items
                .into_iter()
                .map(|r| UiStatusItemReg {
                    id: r.id,
                    text: r.text,
                    icon: r.icon,
                    tooltip: r.tooltip,
                    command: r.command,
                })
                .collect(),
            slash_commands: raw
                .registrations
                .slash_commands
                .into_iter()
                .map(|r| UiSlashCommandReg {
                    id: r.id,
                    label: r.label,
                    description: r.description,
                    aliases: r.aliases,
                    badge: r.badge,
                    template: r.template,
                })
                .collect(),
            menu_items: raw
                .registrations
                .menu_items
                .into_iter()
                .map(|r| MenuItemReg {
                    id: r.id,
                    menu: r.menu,
                    label: r.label,
                    command: r.command,
                    order: r.order,
                    separator_before: r.separator_before,
                })
                .collect(),
            uri_handlers: raw
                .registrations
                .uri_handlers
                .into_iter()
                .map(|r| UriHandlerReg {
                    id: r.id,
                    scheme: r.scheme,
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

[[registrations.ui_command]]
id = "test.hello"
handler_id = 300
title = "Say Hi"
category = "Demo"
icon = "hand"
keybinding = "Mod+Shift+H"

[[registrations.ui_panel]]
id = "test.panel"
handler_id = 400
title = "Hello Panel"
icon = "hand"
side = "right"

[[registrations.ui_settings_tab]]
id = "test.tab"
handler_id = 500
title = "About"
icon = "info"

[[registrations.ui_ribbon_item]]
id = "test.ribbon"
icon = "hand"
tooltip = "Say hi"
command = "test.hello"

[[registrations.ui_status_item]]
id = "test.status"
text = "42 items"
icon = "hand"
tooltip = "Click to refresh"
command = "test.hello"

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
        let wasm = m.wasm.as_ref().unwrap();
        assert_eq!(wasm.module, "test.wasm");
        assert_eq!(wasm.memory_mb, 16); // default
        assert_eq!(wasm.fuel, 10_000_000); // default
        assert!(m.settings.is_none());
        assert!(m.registrations.cli_subcommands.is_empty());
        assert!(m.registrations.ipc_commands.is_empty());
        assert!(m.registrations.event_subscribers.is_empty());
        assert!(m.registrations.ui_commands.is_empty());
        assert!(m.registrations.ui_panels.is_empty());
        assert!(m.registrations.ui_settings_tabs.is_empty());
        assert!(m.registrations.ui_ribbon_items.is_empty());
        assert!(m.registrations.ui_status_items.is_empty());
    }

    #[test]
    fn parse_full_manifest() {
        let m = parse_manifest(FULL, "manifest.toml").unwrap();
        assert_eq!(m.capabilities.required, ["fs.read", "kv.read"]);
        assert_eq!(m.capabilities.optional, ["net.http"]);
        let wasm = m.wasm.as_ref().unwrap();
        assert_eq!(wasm.memory_mb, 32);
        assert_eq!(wasm.fuel, 5_000_000);
        assert!(m.settings.is_some());
        assert_eq!(m.settings.unwrap().schema, "settings.json");
        assert_eq!(m.registrations.cli_subcommands.len(), 1);
        assert_eq!(m.registrations.cli_subcommands[0].handler_id, 1);
        assert_eq!(m.registrations.ipc_commands.len(), 1);
        assert_eq!(m.registrations.ipc_commands[0].handler_id, 100);
        assert_eq!(m.registrations.event_subscribers.len(), 1);
        assert_eq!(m.registrations.event_subscribers[0].handler_id, 200);
        assert_eq!(m.registrations.ui_commands.len(), 1);
        let ui = &m.registrations.ui_commands[0];
        assert_eq!(ui.id, "test.hello");
        assert_eq!(ui.handler_id, 300);
        assert_eq!(ui.title, "Say Hi");
        assert_eq!(ui.category.as_deref(), Some("Demo"));
        assert_eq!(ui.icon.as_deref(), Some("hand"));
        assert_eq!(ui.keybinding.as_deref(), Some("Mod+Shift+H"));
        assert_eq!(m.registrations.ui_panels.len(), 1);
        let panel = &m.registrations.ui_panels[0];
        assert_eq!(panel.id, "test.panel");
        assert_eq!(panel.handler_id, 400);
        assert_eq!(panel.title, "Hello Panel");
        assert_eq!(panel.icon, "hand");
        assert_eq!(panel.side, PanelSide::Right);
        assert_eq!(m.registrations.ui_settings_tabs.len(), 1);
        let tab = &m.registrations.ui_settings_tabs[0];
        assert_eq!(tab.id, "test.tab");
        assert_eq!(tab.handler_id, 500);
        assert_eq!(tab.title, "About");
        assert_eq!(tab.icon, "info");
        assert_eq!(m.registrations.ui_ribbon_items.len(), 1);
        let ribbon = &m.registrations.ui_ribbon_items[0];
        assert_eq!(ribbon.id, "test.ribbon");
        assert_eq!(ribbon.icon, "hand");
        assert_eq!(ribbon.tooltip, "Say hi");
        assert_eq!(ribbon.command, "test.hello");
        assert_eq!(m.registrations.ui_status_items.len(), 1);
        let status = &m.registrations.ui_status_items[0];
        assert_eq!(status.id, "test.status");
        assert_eq!(status.text.as_deref(), Some("42 items"));
        assert_eq!(status.icon.as_deref(), Some("hand"));
        assert_eq!(status.tooltip.as_deref(), Some("Click to refresh"));
        assert_eq!(status.command.as_deref(), Some("test.hello"));
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
    fn parse_missing_wasm_section_yields_none() {
        // Parsing succeeds — wasm = None is valid at parse time.
        // Validation (separate step) will reject a community plugin with wasm = None.
        let toml = r#"
[plugin]
id = "com.example.test"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"
"#;
        let m = parse_manifest(toml, "manifest.toml").unwrap();
        assert!(m.wasm.is_none());
    }

    #[test]
    fn parse_core_plugin_without_wasm_section_succeeds() {
        let toml = r#"
[plugin]
id = "dev.nexus.core-example"
name = "Core Example"
version = "1.0.0"
trust_level = "core"
api_version = "1"
"#;
        let m = parse_manifest(toml, "plugin.toml").unwrap();
        assert!(matches!(m.trust_level, TrustLevel::Core));
        assert!(m.wasm.is_none());
        assert_eq!(m.runtime, PluginRuntime::Native);
    }

    #[test]
    fn runtime_inferred_from_wasm_section() {
        let m = parse_manifest(MINIMAL, "manifest.toml").unwrap();
        assert_eq!(m.runtime, PluginRuntime::Wasm);
    }

    #[test]
    fn runtime_inferred_from_script_section() {
        let toml = r#"
[plugin]
id = "com.example.script"
name = "Script"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[script]
module = "plugin.js"
"#;
        let m = parse_manifest(toml, "manifest.toml").unwrap();
        assert_eq!(m.runtime, PluginRuntime::Script);
    }

    #[test]
    fn explicit_runtime_field_parses() {
        let toml = r#"
[plugin]
id = "com.example.script"
name = "Script"
version = "1.0.0"
trust_level = "community"
api_version = "1"
runtime = "script"

[script]
module = "plugin.js"
"#;
        let m = parse_manifest(toml, "manifest.toml").unwrap();
        assert_eq!(m.runtime, PluginRuntime::Script);
    }

    #[test]
    fn explicit_runtime_must_match_section() {
        let toml = r#"
[plugin]
id = "com.example.bad"
name = "Bad"
version = "1.0.0"
trust_level = "community"
api_version = "1"
runtime = "wasm"

[script]
module = "plugin.js"
"#;
        let err = parse_manifest(toml, "manifest.toml").unwrap_err();
        assert!(matches!(err, PluginError::ManifestInvalid { .. }));
    }

    #[test]
    fn unknown_runtime_value_rejected() {
        let toml = r#"
[plugin]
id = "com.example.bad"
name = "Bad"
version = "1.0.0"
trust_level = "community"
api_version = "1"
runtime = "jvm"

[wasm]
module = "x.wasm"
"#;
        let err = parse_manifest(toml, "manifest.toml").unwrap_err();
        assert!(matches!(err, PluginError::ManifestInvalid { .. }));
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
        assert!(m.registrations.ui_commands.is_empty());
    }

    #[test]
    fn parse_ui_command_optional_fields_default_to_none() {
        let toml = r#"
[plugin]
id = "com.example.test"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.ui_command]]
id = "ui.bare"
handler_id = 10
title = "Bare Command"
"#;
        let m = parse_manifest(toml, "manifest.toml").unwrap();
        let ui = &m.registrations.ui_commands[0];
        assert!(ui.category.is_none());
        assert!(ui.icon.is_none());
        assert!(ui.keybinding.is_none());
    }

    #[test]
    fn parse_ui_panel_side_defaults_to_left() {
        let toml = r#"
[plugin]
id = "com.example.test"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.ui_panel]]
id = "ui.default-side"
handler_id = 10
title = "Panel"
icon = "hand"
"#;
        let m = parse_manifest(toml, "manifest.toml").unwrap();
        let panel = &m.registrations.ui_panels[0];
        assert_eq!(panel.side, PanelSide::Left);
    }

    #[test]
    fn parse_lifecycle_defaults_to_false() {
        let m = parse_manifest(MINIMAL, "manifest.toml").unwrap();
        assert!(!m.lifecycle.on_init);
        assert!(!m.lifecycle.on_start);
        assert!(!m.lifecycle.on_stop);
    }

    #[test]
    fn parse_slash_command_registration() {
        let toml = r##"
[plugin]
id = "com.example.test"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.slash_command]]
id = "test.summary"
label = "Generate summary"
description = "Insert a summary placeholder"
aliases = ["sum", "tldr"]
badge = "AI"
template = "# Summary\n\u0000"
"##;
        let m = parse_manifest(toml, "manifest.toml").unwrap();
        assert_eq!(m.registrations.slash_commands.len(), 1);
        let cmd = &m.registrations.slash_commands[0];
        assert_eq!(cmd.id, "test.summary");
        assert_eq!(cmd.label, "Generate summary");
        assert_eq!(cmd.description, "Insert a summary placeholder");
        assert_eq!(cmd.aliases, vec!["sum".to_string(), "tldr".to_string()]);
        assert_eq!(cmd.badge, "AI");
        assert_eq!(cmd.template, "# Summary\n\u{0}");
    }

    #[test]
    fn parse_slash_command_aliases_default_to_empty() {
        let toml = r##"
[plugin]
id = "com.example.test"
name = "Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[wasm]
module = "test.wasm"

[[registrations.slash_command]]
id = "test.hr"
label = "Divider"
description = "Insert a divider"
badge = "—"
template = "---\n\u0000"
"##;
        let m = parse_manifest(toml, "manifest.toml").unwrap();
        assert_eq!(m.registrations.slash_commands[0].aliases, Vec::<String>::new());
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
#[allow(clippy::too_many_lines)]
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
        .chain(
            manifest
                .registrations
                .ui_commands
                .iter()
                .map(|r| r.handler_id),
        )
        .chain(
            manifest
                .registrations
                .ui_panels
                .iter()
                .map(|r| r.handler_id),
        )
        .chain(
            manifest
                .registrations
                .ui_settings_tabs
                .iter()
                .map(|r| r.handler_id),
        )
        .chain(
            manifest
                .registrations
                .uri_handlers
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

    // Rules 5–7 apply to community plugins. Core plugins are native Rust.
    match manifest.trust_level {
        TrustLevel::Community => {
            // Rule 5: must have exactly one of [wasm] or [script].
            match (&manifest.wasm, &manifest.script) {
                (Some(_), Some(_)) => {
                    return Err(PluginError::ManifestValidation {
                        plugin_id: id.clone(),
                        reason: "[wasm] and [script] are mutually exclusive".to_string(),
                    });
                }
                (None, None) => {
                    return Err(PluginError::ManifestValidation {
                        plugin_id: id.clone(),
                        reason: "community plugins must declare a [wasm] or [script] section"
                            .to_string(),
                    });
                }
                _ => {}
            }

            if let Some(wasm) = &manifest.wasm {
                // Rule 5w: memory_mb in [1, 256].
                if wasm.memory_mb < 1 || wasm.memory_mb > 256 {
                    return Err(PluginError::ManifestValidation {
                        plugin_id: id.clone(),
                        reason: format!(
                            "wasm.memory_mb {} is out of range [1, 256]",
                            wasm.memory_mb
                        ),
                    });
                }

                // Rule 6w: fuel must be > 0 for community plugins.
                if wasm.fuel == 0 {
                    return Err(PluginError::ManifestValidation {
                        plugin_id: id.clone(),
                        reason: "wasm.fuel must be > 0 for community plugins".to_string(),
                    });
                }

                // Rule 7w: wasm module file must exist.
                let wasm_path = plugin_dir.join(&wasm.module);
                if !wasm_path.exists() {
                    return Err(PluginError::ManifestValidation {
                        plugin_id: id.clone(),
                        reason: format!(
                            "wasm module '{}' not found in plugin directory",
                            wasm.module
                        ),
                    });
                }
            }

            if let Some(script) = &manifest.script {
                // Rule 7s: script module file must exist.
                let script_path = plugin_dir.join(&script.module);
                if !script_path.exists() {
                    return Err(PluginError::ManifestValidation {
                        plugin_id: id.clone(),
                        reason: format!(
                            "script module '{}' not found in plugin directory",
                            script.module
                        ),
                    });
                }
            }
        }
        TrustLevel::Core => {
            // Rule 5c: core plugins must NOT have a [wasm] or [script] section.
            if manifest.wasm.is_some() {
                return Err(PluginError::ManifestValidation {
                    plugin_id: id.clone(),
                    reason: "core plugins are native Rust and must not declare a [wasm] section; \
                             remove [wasm] or change trust_level to 'community'"
                        .to_string(),
                });
            }
            if manifest.script.is_some() {
                return Err(PluginError::ManifestValidation {
                    plugin_id: id.clone(),
                    reason: "core plugins are native Rust and must not declare a [script] section; \
                             remove [script] or change trust_level to 'community'"
                        .to_string(),
                });
            }
        }
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
    fn validate_rejects_duplicate_handler_id_across_ui_and_ipc() {
        let dir = make_test_plugin_dir("test.wasm");
        let mut m = valid_manifest();
        m.registrations.ipc_commands.push(IpcCommandReg {
            id: "ipc.a".to_string(),
            handler_id: 77,
        });
        m.registrations.ui_commands.push(UiCommandReg {
            id: "ui.a".to_string(),
            handler_id: 77,
            title: "A".to_string(),
            category: None,
            icon: None,
            keybinding: None,
        });
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestValidation { ref reason, .. } if reason.contains("duplicate handler_id")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_rejects_community_without_wasm_or_script_section() {
        let dir = make_test_plugin_dir("test.wasm");
        let mut m = valid_manifest();
        m.wasm = None; // community plugin with neither wasm nor script
        m.script = None;
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestValidation { ref reason, .. } if reason.contains("[wasm] or [script]")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_rejects_memory_out_of_range() {
        let dir = make_test_plugin_dir("test.wasm");
        let mut m = valid_manifest();
        m.wasm.as_mut().unwrap().memory_mb = 512;
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
        m.wasm.as_mut().unwrap().fuel = 0;
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestValidation { ref reason, .. } if reason.contains("fuel")),
            "got {err:?}"
        );
    }

    #[test]
    fn validate_core_plugin_without_wasm_section_passes() {
        // Core plugins are native Rust — no [wasm] section required or allowed.
        let dir = tempfile::tempdir().expect("tempdir");
        let core_toml = r#"
[plugin]
id = "dev.nexus.core-test"
name = "Core Test"
version = "1.0.0"
trust_level = "core"
api_version = "1"
"#;
        let m = parse_manifest(core_toml, "plugin.toml").unwrap();
        validate(&m, dir.path()).unwrap();
    }

    #[test]
    fn validate_rejects_core_plugin_with_wasm_section() {
        // Core plugins must not declare [wasm]; that's a configuration error.
        let dir = make_test_plugin_dir("test.wasm");
        let core_toml = r#"
[plugin]
id = "dev.nexus.core-wasm-mistake"
name = "Core Oops"
version = "1.0.0"
trust_level = "core"
api_version = "1"

[wasm]
module = "test.wasm"
"#;
        let m = parse_manifest(core_toml, "plugin.toml").unwrap();
        let err = validate(&m, dir.path()).unwrap_err();
        assert!(
            matches!(err, PluginError::ManifestValidation { ref reason, .. } if reason.contains("native Rust")),
            "got {err:?}"
        );
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
