use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use nexus_bootstrap::dap_contribution_wiring::{
    unwire_dap_contributions_for_plugin, wire_dap_contributions, DapWireOutcome,
};
use nexus_bootstrap::lsp_contribution_wiring::{
    unwire_lsp_contributions_for_plugin, wire_lsp_contributions, LspWireOutcome,
};
use nexus_bootstrap::mcp_contribution_wiring::{
    unwire_mcp_contributions_for_plugin, wire_mcp_contributions, McpWireOutcome,
};
use nexus_bootstrap::{Runtime, build_cli_runtime};
use nexus_kernel::EventBus;
use nexus_plugins::{PluginManager, PluginManagerConfig, PluginManifest};
use tokio::runtime::Runtime as TokioRuntime;

use crate::output::OutputFormat;

/// Central application state, owning all subsystems with lazy initialisation.
pub struct App {
    forge_root: PathBuf,
    format: OutputFormat,
    /// Safe mode — skip every community plugin at load (F-8.2.2).
    safe_mode: bool,
    /// Kernel event bus handle retained for the community plugin manager.
    event_bus: Arc<EventBus>,
    /// Community-plugin manager (lazy).
    plugins: Option<PluginManager>,
    /// Bootstrap-assembled runtime (lazy).
    runtime: Option<Runtime>,
    /// Tokio runtime used to block on async `ipc_call`s.
    rt: Option<TokioRuntime>,
}

impl App {
    /// Create a new `App` with the given forge root and output format.
    ///
    /// Subsystems are not opened until first use.
    pub fn new(forge_root: PathBuf, format: OutputFormat) -> Self {
        Self {
            forge_root,
            format,
            safe_mode: false,
            event_bus: Arc::new(EventBus::new(256)),
            plugins: None,
            runtime: None,
            rt: None,
        }
    }

    /// Enable safe mode so the plugin manager skips community plugins
    /// at load time (F-8.2.2). Must be set before `plugins()` is called.
    pub fn set_safe_mode(&mut self, enabled: bool) {
        self.safe_mode = enabled;
    }

    /// Lazily build the Nexus runtime (kernel + all core plugins + CLI as a
    /// Core plugin) and return a reference plus a Tokio runtime for blocking
    /// on async `ipc_call`s.
    ///
    /// First-use opens the storage engine inside the plugin, so the forge
    /// directory must already exist. Subcommands that run *before* forge
    /// init (e.g. `forge init`) must not call this — they use
    /// [`nexus_bootstrap::init_forge`] first.
    ///
    /// # Errors
    ///
    /// Returns an error if the runtime or Tokio runtime cannot be built.
    pub fn runtime(&mut self) -> Result<(&Runtime, &TokioRuntime)> {
        if self.runtime.is_none() {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .context("failed to start tokio runtime")?;
            let runtime = build_cli_runtime(self.forge_root.clone())
                .with_context(|| format!("failed to build runtime at {}", self.forge_root.display()))?;
            self.runtime = Some(runtime);
            self.rt = Some(rt);
        }
        Ok((
            self.runtime.as_ref().expect("just initialised"),
            self.rt.as_ref().expect("just initialised"),
        ))
    }

    /// Return the forge root directory.
    pub fn forge_root(&self) -> &Path {
        &self.forge_root
    }

    /// Return the configured output format.
    pub fn format(&self) -> OutputFormat {
        self.format
    }

    /// Create the plugin manager lazily (creates on first call, reuses after).
    ///
    /// The plugins directory is `.forge/plugins/` relative to the forge root.
    ///
    /// # Errors
    ///
    /// Returns an error if the plugin manager cannot be created.
    pub fn plugins(&mut self) -> Result<&mut PluginManager> {
        if self.plugins.is_none() {
            let plugins_dir = self.forge_root.join(".forge").join("plugins");
            let config = PluginManagerConfig {
                hot_reload: false,
                safe_mode: self.safe_mode,
                ..Default::default()
            };
            let mut manager =
                PluginManager::new(&plugins_dir, &config).with_context(|| {
                    format!(
                        "failed to create plugin manager at '{}'",
                        plugins_dir.display()
                    )
                })?;
            // Wire the kernel bus so community plugins can subscribe to events.
            manager.set_event_bus(Arc::clone(&self.event_bus));
            self.plugins = Some(manager);
        }
        Ok(self.plugins.as_mut().expect("just initialised"))
    }

    /// BL-113 Phase 1d — issue `com.nexus.dap::register_adapter` IPC
    /// calls for every DAP contribution declared by currently-loaded
    /// community plugins, so any later DAP-host call (`list_adapters`,
    /// `launch`, …) sees the merged adapter set.
    ///
    /// Assumes [`Self::plugins`] has already loaded the community
    /// plugins (`plugins.load_all()`). Plugins with no DAP
    /// contributions cost a single manifest iteration; only plugins
    /// that declared `[[registrations.protocol_hosts.dap]]` entries
    /// trigger an IPC dispatch.
    ///
    /// Best-effort: per-adapter rejections / transport errors are
    /// returned in the outcome list (see
    /// [`nexus_bootstrap::dap_contribution_wiring::DapWireStatus`])
    /// rather than aborting the pass.
    ///
    /// # Errors
    /// Propagates errors from runtime / Tokio initialisation. Per-call
    /// IPC failures do not surface here — see the outcome list.
    pub fn wire_dap_contributions(&mut self) -> Result<Vec<DapWireOutcome>> {
        // Step 1: snapshot currently-loaded manifests. Clones because
        // the `&PluginManager` borrow has to end before we can re-borrow
        // `&mut self` for `runtime()`.
        let manifests: Vec<PluginManifest> = {
            let plugins = self.plugins()?;
            plugins
                .list()
                .iter()
                .filter_map(|info| plugins.manifest(&info.id).cloned())
                .collect()
        };
        if manifests.is_empty() {
            return Ok(Vec::new());
        }

        // Step 2: dispatch through the runtime's IPC surface.
        let (runtime, rt) = self.runtime()?;
        let manifest_refs: Vec<&PluginManifest> = manifests.iter().collect();
        let outcomes = rt.block_on(wire_dap_contributions(&runtime.context, &manifest_refs));
        Ok(outcomes)
    }

    /// BL-113 Phase 1e — wire just one plugin's DAP contributions.
    /// Used by `nexus plugin enable <id>` after the enable lifecycle
    /// hook fires, so the just-enabled plugin's contributed adapters
    /// become visible to the DAP host.
    ///
    /// Returns an empty outcome list when the plugin is unknown to the
    /// manager or declares no DAP contributions — the caller decides
    /// whether to surface either as an error.
    ///
    /// # Errors
    /// Propagates errors from runtime / Tokio initialisation. Per-call
    /// IPC failures do not surface here — see the outcome list.
    pub fn wire_dap_contributions_for_plugin(
        &mut self,
        plugin_id: &str,
    ) -> Result<Vec<DapWireOutcome>> {
        let manifest: Option<PluginManifest> = {
            let plugins = self.plugins()?;
            plugins.manifest(plugin_id).cloned()
        };
        let Some(manifest) = manifest else {
            return Ok(Vec::new());
        };
        if manifest.registrations.protocol_hosts.dap.is_empty() {
            return Ok(Vec::new());
        }
        let (runtime, rt) = self.runtime()?;
        let outcomes = rt.block_on(wire_dap_contributions(&runtime.context, &[&manifest]));
        Ok(outcomes)
    }

    /// BL-113 Phase 1e — unwire just one plugin's DAP contributions.
    /// Used by `nexus plugin disable <id>` immediately before the
    /// disable lifecycle hook fires, so the about-to-be-disabled
    /// plugin's contributed adapters are removed from the DAP host's
    /// runtime map.
    ///
    /// Returns an empty outcome list when the plugin is unknown to the
    /// manager or declares no DAP contributions.
    ///
    /// # Errors
    /// Propagates errors from runtime / Tokio initialisation. Per-call
    /// IPC failures do not surface here — see the outcome list.
    pub fn unwire_dap_contributions_for_plugin(
        &mut self,
        plugin_id: &str,
    ) -> Result<Vec<DapWireOutcome>> {
        let manifest: Option<PluginManifest> = {
            let plugins = self.plugins()?;
            plugins.manifest(plugin_id).cloned()
        };
        let Some(manifest) = manifest else {
            return Ok(Vec::new());
        };
        if manifest.registrations.protocol_hosts.dap.is_empty() {
            return Ok(Vec::new());
        }
        let (runtime, rt) = self.runtime()?;
        let outcomes = rt.block_on(unwire_dap_contributions_for_plugin(
            &runtime.context,
            &manifest,
        ));
        Ok(outcomes)
    }

    // ── BL-113 Phase 2b — LSP variants ──────────────────────────────────────

    /// BL-113 Phase 2b — issue `com.nexus.lsp::register_server` IPC
    /// calls for every LSP contribution declared by currently-loaded
    /// community plugins.
    ///
    /// # Errors
    /// Propagates errors from runtime / Tokio initialisation. Per-call
    /// IPC failures do not surface here — see the outcome list.
    pub fn wire_lsp_contributions(&mut self) -> Result<Vec<LspWireOutcome>> {
        let manifests: Vec<PluginManifest> = {
            let plugins = self.plugins()?;
            plugins
                .list()
                .iter()
                .filter_map(|info| plugins.manifest(&info.id).cloned())
                .collect()
        };
        if manifests.is_empty() {
            return Ok(Vec::new());
        }
        let (runtime, rt) = self.runtime()?;
        let manifest_refs: Vec<&PluginManifest> = manifests.iter().collect();
        let outcomes = rt.block_on(wire_lsp_contributions(&runtime.context, &manifest_refs));
        Ok(outcomes)
    }

    /// BL-113 Phase 2b — wire one plugin's LSP contributions after
    /// `nexus plugin enable <id>`.
    ///
    /// # Errors
    /// Propagates errors from runtime / Tokio initialisation.
    pub fn wire_lsp_contributions_for_plugin(
        &mut self,
        plugin_id: &str,
    ) -> Result<Vec<LspWireOutcome>> {
        let manifest: Option<PluginManifest> = {
            let plugins = self.plugins()?;
            plugins.manifest(plugin_id).cloned()
        };
        let Some(manifest) = manifest else {
            return Ok(Vec::new());
        };
        if manifest.registrations.protocol_hosts.lsp.is_empty() {
            return Ok(Vec::new());
        }
        let (runtime, rt) = self.runtime()?;
        let outcomes = rt.block_on(wire_lsp_contributions(&runtime.context, &[&manifest]));
        Ok(outcomes)
    }

    /// BL-113 Phase 2b — unwire one plugin's LSP contributions before
    /// `nexus plugin disable <id>`.
    ///
    /// # Errors
    /// Propagates errors from runtime / Tokio initialisation.
    pub fn unwire_lsp_contributions_for_plugin(
        &mut self,
        plugin_id: &str,
    ) -> Result<Vec<LspWireOutcome>> {
        let manifest: Option<PluginManifest> = {
            let plugins = self.plugins()?;
            plugins.manifest(plugin_id).cloned()
        };
        let Some(manifest) = manifest else {
            return Ok(Vec::new());
        };
        if manifest.registrations.protocol_hosts.lsp.is_empty() {
            return Ok(Vec::new());
        }
        let (runtime, rt) = self.runtime()?;
        let outcomes = rt.block_on(unwire_lsp_contributions_for_plugin(
            &runtime.context,
            &manifest,
        ));
        Ok(outcomes)
    }

    // ── BL-113 Phase 3b — MCP variants ──────────────────────────────────────

    /// BL-113 Phase 3b — issue `com.nexus.mcp.host::register_server`
    /// IPC calls for every MCP contribution declared by
    /// currently-loaded community plugins.
    ///
    /// # Errors
    /// Propagates errors from runtime / Tokio initialisation. Per-call
    /// IPC failures do not surface here — see the outcome list.
    pub fn wire_mcp_contributions(&mut self) -> Result<Vec<McpWireOutcome>> {
        let manifests: Vec<PluginManifest> = {
            let plugins = self.plugins()?;
            plugins
                .list()
                .iter()
                .filter_map(|info| plugins.manifest(&info.id).cloned())
                .collect()
        };
        if manifests.is_empty() {
            return Ok(Vec::new());
        }
        let (runtime, rt) = self.runtime()?;
        let manifest_refs: Vec<&PluginManifest> = manifests.iter().collect();
        let outcomes = rt.block_on(wire_mcp_contributions(&runtime.context, &manifest_refs));
        Ok(outcomes)
    }

    /// BL-113 Phase 3b — wire one plugin's MCP contributions after
    /// `nexus plugin enable <id>`.
    ///
    /// # Errors
    /// Propagates errors from runtime / Tokio initialisation.
    pub fn wire_mcp_contributions_for_plugin(
        &mut self,
        plugin_id: &str,
    ) -> Result<Vec<McpWireOutcome>> {
        let manifest: Option<PluginManifest> = {
            let plugins = self.plugins()?;
            plugins.manifest(plugin_id).cloned()
        };
        let Some(manifest) = manifest else {
            return Ok(Vec::new());
        };
        if manifest.registrations.protocol_hosts.mcp.is_empty() {
            return Ok(Vec::new());
        }
        let (runtime, rt) = self.runtime()?;
        let outcomes = rt.block_on(wire_mcp_contributions(&runtime.context, &[&manifest]));
        Ok(outcomes)
    }

    /// BL-113 Phase 3b — unwire one plugin's MCP contributions before
    /// `nexus plugin disable <id>`.
    ///
    /// # Errors
    /// Propagates errors from runtime / Tokio initialisation.
    pub fn unwire_mcp_contributions_for_plugin(
        &mut self,
        plugin_id: &str,
    ) -> Result<Vec<McpWireOutcome>> {
        let manifest: Option<PluginManifest> = {
            let plugins = self.plugins()?;
            plugins.manifest(plugin_id).cloned()
        };
        let Some(manifest) = manifest else {
            return Ok(Vec::new());
        };
        if manifest.registrations.protocol_hosts.mcp.is_empty() {
            return Ok(Vec::new());
        }
        let (runtime, rt) = self.runtime()?;
        let outcomes = rt.block_on(unwire_mcp_contributions_for_plugin(
            &runtime.context,
            &manifest,
        ));
        Ok(outcomes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_forge() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        nexus_storage::StorageEngine::init(dir.path()).unwrap();
        dir
    }

    #[test]
    fn wire_dap_contributions_is_a_noop_when_no_community_plugins_are_installed() {
        let forge = empty_forge();
        let mut app = App::new(forge.path().to_path_buf(), OutputFormat::Text);
        let outcomes = app.wire_dap_contributions().expect("wire ok");
        assert!(outcomes.is_empty(), "outcomes: {outcomes:?}");
    }

    #[test]
    fn wire_for_unknown_plugin_returns_empty_outcomes() {
        let forge = empty_forge();
        let mut app = App::new(forge.path().to_path_buf(), OutputFormat::Text);
        let outcomes = app
            .wire_dap_contributions_for_plugin("community.does-not-exist")
            .expect("wire ok");
        assert!(outcomes.is_empty());
    }

    #[test]
    fn unwire_for_unknown_plugin_returns_empty_outcomes() {
        let forge = empty_forge();
        let mut app = App::new(forge.path().to_path_buf(), OutputFormat::Text);
        let outcomes = app
            .unwire_dap_contributions_for_plugin("community.does-not-exist")
            .expect("unwire ok");
        assert!(outcomes.is_empty());
    }

    #[test]
    fn wire_lsp_contributions_is_a_noop_when_no_community_plugins_are_installed() {
        let forge = empty_forge();
        let mut app = App::new(forge.path().to_path_buf(), OutputFormat::Text);
        let outcomes = app.wire_lsp_contributions().expect("wire ok");
        assert!(outcomes.is_empty(), "outcomes: {outcomes:?}");
    }

    #[test]
    fn wire_mcp_contributions_is_a_noop_when_no_community_plugins_are_installed() {
        let forge = empty_forge();
        let mut app = App::new(forge.path().to_path_buf(), OutputFormat::Text);
        let outcomes = app.wire_mcp_contributions().expect("wire ok");
        assert!(outcomes.is_empty(), "outcomes: {outcomes:?}");
    }
}
