//! Plugin loader: scans a directory for plugins, loads WASM sandboxes,
//! registers CLI/IPC dispatch tables, and manages plugin lifecycle.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nexus_kernel::{Capability, CapabilitySet, PluginInfo, PluginStatus, TrustLevel};

use crate::manifest::{self, PluginManifest};
use crate::sandbox::{PluginData, WasmSandbox};
use crate::settings::SettingsManager;
use crate::PluginError;

// ─── Internal structs ─────────────────────────────────────────────────────────

struct PluginRegistrations {
    cli_subcommands: Vec<String>,
    #[allow(dead_code)]
    ipc_commands: Vec<String>,
    #[allow(dead_code)]
    event_subscriptions: Vec<String>,
}

struct LoadedPlugin {
    manifest: PluginManifest,
    sandbox: WasmSandbox,
    status: PluginStatus,
    plugin_dir: PathBuf,
    registrations: PluginRegistrations,
}

// ─── PluginLoader ─────────────────────────────────────────────────────────────

/// Manages loading, unloading, and dispatching to WASM plugins.
///
/// Maintains a registry of loaded plugins keyed by their manifest ID, and a
/// separate CLI-dispatch table mapping subcommand IDs to plugin IDs.
pub struct PluginLoader {
    plugins_dir: PathBuf,
    loaded: HashMap<String, LoadedPlugin>,
    /// Maps `subcommand_id` → `plugin_id`
    cli_registry: HashMap<String, String>,
    settings: SettingsManager,
}

impl PluginLoader {
    /// Create a new, empty `PluginLoader` rooted at `plugins_dir`.
    ///
    /// No plugins are loaded at construction time; call [`scan`](Self::scan)
    /// and [`load`](Self::load) to populate the loader.
    #[must_use]
    pub fn new(plugins_dir: &Path) -> Self {
        Self {
            plugins_dir: plugins_dir.to_path_buf(),
            loaded: HashMap::new(),
            cli_registry: HashMap::new(),
            settings: SettingsManager::new(),
        }
    }

    /// Walk `plugins_dir` and return the paths of subdirectories that contain
    /// a `manifest.toml` file.
    ///
    /// # Errors
    /// Returns [`PluginError::Io`] if the plugins directory cannot be read.
    pub fn scan(&self) -> Result<Vec<PathBuf>, PluginError> {
        let mut found = Vec::new();

        let read_dir = match std::fs::read_dir(&self.plugins_dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(found),
            Err(e) => return Err(PluginError::Io(e)),
        };

        for entry in read_dir {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() && path.join("manifest.toml").exists() {
                found.push(path);
            }
        }

        Ok(found)
    }

    /// Load a plugin from `plugin_dir`.
    ///
    /// # Steps
    /// 1. Parse `manifest.toml` via [`manifest::load_manifest`].
    /// 2. Validate the manifest via [`manifest::validate`].
    /// 3. Reject duplicate plugin IDs.
    /// 4. Register settings schema with [`SettingsManager`] if declared.
    /// 5. Read the WASM bytes.
    /// 6. Build [`PluginData`] with the appropriate capabilities.
    /// 7. Create [`WasmSandbox`].
    /// 8. Call `on_init` and `on_start` lifecycle hooks if declared.
    /// 9. Register CLI subcommands, rejecting conflicts.
    /// 10. Set status to [`PluginStatus::Running`] and return [`PluginInfo`].
    ///
    /// # Errors
    /// Returns [`PluginError`] on any failure.
    pub fn load(&mut self, plugin_dir: &Path) -> Result<PluginInfo, PluginError> {
        // Step 1: Parse manifest
        let manifest_path = plugin_dir.join("manifest.toml");
        let manifest = manifest::load_manifest(&manifest_path)?;

        // Step 2: Validate
        manifest::validate(&manifest, plugin_dir)?;

        // Step 3: Reject duplicate plugin ID
        let plugin_id = manifest.id.clone();
        if self.loaded.contains_key(&plugin_id) {
            return Err(PluginError::DuplicatePlugin(plugin_id));
        }

        // Step 4: Register settings schema if declared
        if let Some(ref settings_cfg) = manifest.settings {
            let schema_path = plugin_dir.join(&settings_cfg.schema);
            let schema_json = std::fs::read_to_string(&schema_path)?;
            self.settings.register_schema(&plugin_id, &schema_json)?;
        }

        // Step 5: Read WASM bytes
        let wasm_path = plugin_dir.join(&manifest.wasm.module);
        let wasm_bytes = std::fs::read(&wasm_path)?;

        // Step 6: Build PluginData with capabilities
        let capabilities = build_capabilities(&manifest);
        let plugin_data = PluginData {
            plugin_id: plugin_id.clone(),
            capabilities: capabilities.clone(),
        };

        // Step 7: Create WasmSandbox
        let mut sandbox = WasmSandbox::new(&wasm_bytes, &manifest.wasm, plugin_data)?;

        // Step 8: Call lifecycle hooks
        if manifest.lifecycle.on_init {
            sandbox.call_on_init()?;
        }
        if manifest.lifecycle.on_start {
            sandbox.call_on_start()?;
        }

        // Step 9: Register CLI subcommands (reject duplicates)
        for sub in &manifest.registrations.cli_subcommands {
            if let Some(existing_plugin) = self.cli_registry.get(&sub.id) {
                return Err(PluginError::DuplicateCliSubcommand {
                    plugin_id: plugin_id.clone(),
                    subcommand: format!(
                        "{} (already registered by {})",
                        sub.id, existing_plugin
                    ),
                });
            }
        }
        let mut registered_cli: Vec<String> = Vec::new();
        for sub in &manifest.registrations.cli_subcommands {
            self.cli_registry
                .insert(sub.id.clone(), plugin_id.clone());
            registered_cli.push(sub.id.clone());
        }

        // Collect IPC and event registrations
        let registered_ipc: Vec<String> = manifest
            .registrations
            .ipc_commands
            .iter()
            .map(|r| r.id.clone())
            .collect();
        let registered_events: Vec<String> = manifest
            .registrations
            .event_subscribers
            .iter()
            .map(|r| r.id.clone())
            .collect();

        let info = plugin_info_from(&manifest, PluginStatus::Running, &capabilities);

        self.loaded.insert(
            plugin_id,
            LoadedPlugin {
                manifest,
                sandbox,
                status: PluginStatus::Running,
                plugin_dir: plugin_dir.to_path_buf(),
                registrations: PluginRegistrations {
                    cli_subcommands: registered_cli,
                    ipc_commands: registered_ipc,
                    event_subscriptions: registered_events,
                },
            },
        );

        Ok(info)
    }

    /// Unload the plugin with the given `plugin_id`.
    ///
    /// If the plugin declared `on_stop`, it is called best-effort (errors are
    /// ignored so the plugin is always removed).
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if no plugin with `plugin_id`
    /// is loaded.
    pub fn unload(&mut self, plugin_id: &str) -> Result<(), PluginError> {
        let mut loaded = self
            .loaded
            .remove(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?;

        // Best-effort on_stop
        if loaded.manifest.lifecycle.on_stop {
            let _ = loaded.sandbox.call_on_stop();
        }

        // Deregister CLI subcommands
        for sub_id in &loaded.registrations.cli_subcommands {
            self.cli_registry.remove(sub_id);
        }

        Ok(())
    }

    /// Return a snapshot of all currently-loaded plugins.
    #[must_use]
    pub fn list(&self) -> Vec<PluginInfo> {
        self.loaded
            .values()
            .map(|lp| plugin_info_from(&lp.manifest, lp.status, &lp.sandbox.plugin_data().capabilities))
            .collect()
    }

    /// Look up a single plugin by ID, returning a [`PluginInfo`] snapshot.
    #[must_use]
    pub fn get(&self, plugin_id: &str) -> Option<PluginInfo> {
        self.loaded.get(plugin_id).map(|lp| {
            plugin_info_from(&lp.manifest, lp.status, &lp.sandbox.plugin_data().capabilities)
        })
    }

    /// Dispatch a CLI subcommand call.
    ///
    /// Looks up the plugin from the CLI registry by `subcommand`, finds the
    /// handler ID from the manifest, and calls `sandbox.dispatch`.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the subcommand is unknown.
    /// Propagates sandbox dispatch errors.
    pub fn dispatch_cli(
        &mut self,
        subcommand: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let plugin_id = self
            .cli_registry
            .get(subcommand)
            .cloned()
            .ok_or_else(|| PluginError::PluginNotFound(subcommand.to_string()))?;

        let lp = self
            .loaded
            .get_mut(&plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.clone()))?;

        let handler_id = lp
            .manifest
            .registrations
            .cli_subcommands
            .iter()
            .find(|r| r.id == subcommand)
            .map(|r| r.handler_id)
            .ok_or_else(|| PluginError::PluginNotFound(subcommand.to_string()))?;

        lp.sandbox.dispatch(handler_id, args)
    }

    /// Dispatch an IPC command call.
    ///
    /// Looks up the plugin by `plugin_id`, finds the handler ID for
    /// `command_id` in the manifest, and calls `sandbox.dispatch`.
    ///
    /// # Errors
    /// Returns [`PluginError::PluginNotFound`] if the plugin or command is
    /// unknown. Propagates sandbox dispatch errors.
    pub fn dispatch_ipc(
        &mut self,
        plugin_id: &str,
        command_id: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let lp = self
            .loaded
            .get_mut(plugin_id)
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.to_string()))?;

        let handler_id = lp
            .manifest
            .registrations
            .ipc_commands
            .iter()
            .find(|r| r.id == command_id)
            .map(|r| r.handler_id)
            .ok_or_else(|| PluginError::PluginNotFound(command_id.to_string()))?;

        lp.sandbox.dispatch(handler_id, args)
    }

    /// Return a reference to the [`SettingsManager`].
    #[must_use]
    pub fn settings(&self) -> &SettingsManager {
        &self.settings
    }

    /// Return the plugin directory for `plugin_id`, if it is loaded.
    #[must_use]
    pub fn plugin_dir(&self, plugin_id: &str) -> Option<&Path> {
        self.loaded
            .get(plugin_id)
            .map(|lp| lp.plugin_dir.as_path())
    }

    // ─── Internal helpers for hot-reload ──────────────────────────────────────

    /// Return a mutable reference to the [`WasmSandbox`] for `plugin_id`.
    #[allow(dead_code)]
    pub(crate) fn sandbox_mut(&mut self, plugin_id: &str) -> Option<&mut WasmSandbox> {
        self.loaded.get_mut(plugin_id).map(|lp| &mut lp.sandbox)
    }

    /// Return a reference to the [`PluginManifest`] for `plugin_id`.
    #[allow(dead_code)]
    pub(crate) fn manifest(&self, plugin_id: &str) -> Option<&PluginManifest> {
        self.loaded.get(plugin_id).map(|lp| &lp.manifest)
    }

    /// Update the [`PluginStatus`] for `plugin_id`.
    #[allow(dead_code)]
    pub(crate) fn set_status(&mut self, plugin_id: &str, status: PluginStatus) {
        if let Some(lp) = self.loaded.get_mut(plugin_id) {
            lp.status = status;
        }
    }

    /// Replace the [`WasmSandbox`] for `plugin_id`, returning the old one.
    #[allow(dead_code)]
    pub(crate) fn replace_sandbox(
        &mut self,
        plugin_id: &str,
        sandbox: WasmSandbox,
    ) -> Option<WasmSandbox> {
        self.loaded
            .get_mut(plugin_id)
            .map(|lp| std::mem::replace(&mut lp.sandbox, sandbox))
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn build_capabilities(manifest: &PluginManifest) -> CapabilitySet {
    match manifest.trust_level {
        TrustLevel::Core => CapabilitySet::from_iter(Capability::ALL.iter().copied()),
        TrustLevel::Community => {
            let caps: Vec<Capability> = manifest
                .capabilities
                .required
                .iter()
                .chain(manifest.capabilities.optional.iter())
                .filter_map(|s| Capability::from_str(s).ok())
                .collect();
            CapabilitySet::from_iter(caps)
        }
    }
}

fn plugin_info_from(
    manifest: &PluginManifest,
    status: PluginStatus,
    capabilities: &CapabilitySet,
) -> PluginInfo {
    PluginInfo {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        trust_level: manifest.trust_level,
        status,
        capabilities: capabilities.clone(),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod unit_tests {
    use super::*;

    fn plugins_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn new_loader_has_empty_state() {
        let dir = plugins_dir();
        let loader = PluginLoader::new(dir.path());
        assert!(loader.loaded.is_empty());
        assert!(loader.cli_registry.is_empty());
        assert!(loader.list().is_empty());
    }

    #[test]
    fn scan_empty_directory() {
        let dir = plugins_dir();
        let loader = PluginLoader::new(dir.path());
        let found = loader.scan().unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn scan_finds_plugin_dirs() {
        let dir = plugins_dir();
        let plugin_dir = dir.path().join("my.plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(plugin_dir.join("manifest.toml"), b"").unwrap();

        let loader = PluginLoader::new(dir.path());
        let found = loader.scan().unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0], plugin_dir);
    }

    #[test]
    fn scan_skips_dirs_without_manifest() {
        let dir = plugins_dir();
        // Dir without manifest.toml
        std::fs::create_dir_all(dir.path().join("no-manifest")).unwrap();
        // Dir with manifest.toml
        let with_manifest = dir.path().join("has-manifest");
        std::fs::create_dir_all(&with_manifest).unwrap();
        std::fs::write(with_manifest.join("manifest.toml"), b"").unwrap();

        let loader = PluginLoader::new(dir.path());
        let found = loader.scan().unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("has-manifest"));
    }

    #[test]
    fn unload_nonexistent_returns_error() {
        let dir = plugins_dir();
        let mut loader = PluginLoader::new(dir.path());
        let err = loader.unload("com.example.missing").unwrap_err();
        assert!(
            matches!(err, PluginError::PluginNotFound(_)),
            "expected PluginNotFound, got {err:?}"
        );
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    fn setup_plugin_dir(plugin_id: &str) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let plugin_dir = tmp.path().join(plugin_id);
        std::fs::create_dir_all(&plugin_dir).unwrap();

        // Copy WASM fixture
        let wasm_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/minimal-plugin.wasm");
        std::fs::copy(&wasm_src, plugin_dir.join("test.wasm")).unwrap();

        let manifest = format!(
            r#"
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
"#
        );
        std::fs::write(plugin_dir.join("manifest.toml"), manifest).unwrap();
        tmp
    }

    #[test]
    fn load_plugin_from_dir() {
        let tmp = setup_plugin_dir("com.test.load");
        let plugin_dir = tmp.path().join("com.test.load");
        let mut loader = PluginLoader::new(tmp.path());
        let info = loader.load(&plugin_dir).unwrap();
        assert_eq!(info.id, "com.test.load");
        assert_eq!(info.status, PluginStatus::Running);
    }

    #[test]
    fn load_duplicate_plugin_fails() {
        let tmp = setup_plugin_dir("com.test.dupe");
        let plugin_dir = tmp.path().join("com.test.dupe");
        let mut loader = PluginLoader::new(tmp.path());
        loader.load(&plugin_dir).unwrap();

        let err = loader.load(&plugin_dir).unwrap_err();
        assert!(
            matches!(err, PluginError::DuplicatePlugin(_)),
            "expected DuplicatePlugin, got {err:?}"
        );
    }

    #[test]
    fn list_shows_loaded_plugins() {
        let tmp = setup_plugin_dir("com.test.list");
        let plugin_dir = tmp.path().join("com.test.list");
        let mut loader = PluginLoader::new(tmp.path());
        assert!(loader.list().is_empty());

        loader.load(&plugin_dir).unwrap();
        let list = loader.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "com.test.list");
    }

    #[test]
    fn unload_removes_plugin() {
        let tmp = setup_plugin_dir("com.test.unload");
        let plugin_dir = tmp.path().join("com.test.unload");
        let mut loader = PluginLoader::new(tmp.path());
        loader.load(&plugin_dir).unwrap();
        assert_eq!(loader.list().len(), 1);

        loader.unload("com.test.unload").unwrap();
        assert!(loader.list().is_empty());
        assert!(loader.get("com.test.unload").is_none());
        // CLI subcommand should also be deregistered
        assert!(!loader.cli_registry.contains_key("com.test.unload.echo"));
    }

    #[test]
    fn dispatch_cli_to_loaded_plugin() {
        let tmp = setup_plugin_dir("com.test.dispatch");
        let plugin_dir = tmp.path().join("com.test.dispatch");
        let mut loader = PluginLoader::new(tmp.path());
        loader.load(&plugin_dir).unwrap();

        let args = serde_json::json!({"hello": "world"});
        let result = loader
            .dispatch_cli("com.test.dispatch.echo", &args)
            .unwrap();
        assert_eq!(result, args, "echo handler should return args unchanged");
    }

    #[test]
    fn dispatch_cli_unknown_subcommand_fails() {
        let tmp = setup_plugin_dir("com.test.unknown");
        let plugin_dir = tmp.path().join("com.test.unknown");
        let mut loader = PluginLoader::new(tmp.path());
        loader.load(&plugin_dir).unwrap();

        let err = loader
            .dispatch_cli("com.test.unknown.nonexistent", &serde_json::json!({}))
            .unwrap_err();
        assert!(
            matches!(err, PluginError::PluginNotFound(_)),
            "expected PluginNotFound, got {err:?}"
        );
    }
}
