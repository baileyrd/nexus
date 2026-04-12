//! Plugin error types for the Nexus plugin subsystem.

/// Errors that can occur within the plugin subsystem, covering manifest
/// loading and validation, WASM execution, lifecycle hooks, capability
/// enforcement, registry management, settings, and hot-reload.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// The manifest file was not found at the given path.
    #[error("manifest not found: {0}")]
    ManifestNotFound(
        /// The path that was searched.
        String,
    ),

    /// The manifest file exists but could not be parsed or decoded.
    #[error("manifest invalid at {path}: {reason}")]
    ManifestInvalid {
        /// The path of the invalid manifest.
        path: String,
        /// A human-readable description of why the manifest is invalid.
        reason: String,
    },

    /// The manifest was parsed but failed semantic validation rules.
    #[error("manifest validation failed for {plugin_id}: {reason}")]
    ManifestValidation {
        /// The plugin identifier whose manifest failed validation.
        plugin_id: String,
        /// A human-readable description of the validation failure.
        reason: String,
    },

    /// The WASM module for the plugin could not be loaded or compiled.
    #[error("WASM load failed for {plugin_id}: {reason}")]
    WasmLoadFailed {
        /// The plugin identifier whose WASM module failed to load.
        plugin_id: String,
        /// A human-readable description of the load failure.
        reason: String,
    },

    /// A plugin invocation exceeded the configured execution time limit.
    #[error("execution timeout for {plugin_id}")]
    ExecutionTimeout {
        /// The plugin identifier that timed out.
        plugin_id: String,
    },

    /// A plugin invocation returned an error or trapped.
    #[error("execution failed for {plugin_id}: {reason}")]
    ExecutionFailed {
        /// The plugin identifier whose execution failed.
        plugin_id: String,
        /// A human-readable description of the execution failure.
        reason: String,
    },

    /// An error occurred during a plugin lifecycle hook (e.g. `on_load`, `on_unload`).
    #[error("lifecycle error for {plugin_id} in {hook}: {reason}")]
    LifecycleError {
        /// The plugin identifier that encountered the lifecycle error.
        plugin_id: String,
        /// The name of the lifecycle hook that failed.
        hook: String,
        /// A human-readable description of the lifecycle error.
        reason: String,
    },

    /// A plugin attempted to use a capability it was not granted.
    #[error("capability denied for {plugin_id}: {capability}")]
    CapabilityDenied {
        /// The plugin identifier that was denied.
        plugin_id: String,
        /// The capability that was requested but not granted.
        capability: String,
    },

    /// No plugin with the given identifier is registered.
    #[error("plugin not found: {0}")]
    PluginNotFound(
        /// The plugin identifier that was not found.
        String,
    ),

    /// A plugin with the given identifier is already registered.
    #[error("duplicate plugin: {0}")]
    DuplicatePlugin(
        /// The plugin identifier that was duplicated.
        String,
    ),

    /// Two or more plugins register the same CLI subcommand name.
    #[error("duplicate CLI subcommand '{subcommand}' from {plugin_id}")]
    DuplicateCliSubcommand {
        /// The plugin identifier that attempted to register a conflicting subcommand.
        plugin_id: String,
        /// The subcommand name that was already registered.
        subcommand: String,
    },

    /// The plugin's settings did not pass validation.
    #[error("settings invalid for {plugin_id}: {reason}")]
    SettingsInvalid {
        /// The plugin identifier whose settings are invalid.
        plugin_id: String,
        /// A human-readable description of the settings validation failure.
        reason: String,
    },

    /// A hot-reload attempt for the plugin failed.
    #[error("reload failed for {plugin_id}: {reason}")]
    ReloadFailed {
        /// The plugin identifier that failed to reload.
        plugin_id: String,
        /// A human-readable description of the reload failure.
        reason: String,
    },

    /// The plugin is currently in the middle of a reload and cannot be used.
    #[error("plugin reloading: {0}")]
    PluginReloading(
        /// The plugin identifier that is currently reloading.
        String,
    ),

    /// An underlying I/O error occurred.
    #[error("I/O error: {0}")]
    Io(
        /// The underlying I/O error.
        #[from]
        std::io::Error,
    ),
}

#[cfg(test)]
mod tests {
    use super::PluginError;

    #[test]
    fn manifest_not_found_display() {
        let err = PluginError::ManifestNotFound("plugins/foo/manifest.toml".to_string());
        let msg = err.to_string();
        assert!(msg.contains("manifest not found"), "got: {msg}");
        assert!(msg.contains("plugins/foo/manifest.toml"), "got: {msg}");
    }

    #[test]
    fn manifest_invalid_display() {
        let err = PluginError::ManifestInvalid {
            path: "/etc/plugin.toml".to_string(),
            reason: "missing field `id`".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("manifest invalid at"), "got: {msg}");
        assert!(msg.contains("/etc/plugin.toml"), "got: {msg}");
        assert!(msg.contains("missing field `id`"), "got: {msg}");
    }

    #[test]
    fn manifest_validation_display() {
        let err = PluginError::ManifestValidation {
            plugin_id: "my-plugin".to_string(),
            reason: "version must be semver".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("manifest validation failed"), "got: {msg}");
        assert!(msg.contains("my-plugin"), "got: {msg}");
        assert!(msg.contains("version must be semver"), "got: {msg}");
    }

    #[test]
    fn wasm_load_failed_display() {
        let err = PluginError::WasmLoadFailed {
            plugin_id: "wasm-plugin".to_string(),
            reason: "invalid magic number".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("WASM load failed"), "got: {msg}");
        assert!(msg.contains("wasm-plugin"), "got: {msg}");
        assert!(msg.contains("invalid magic number"), "got: {msg}");
    }

    #[test]
    fn execution_timeout_display() {
        let err = PluginError::ExecutionTimeout {
            plugin_id: "slow-plugin".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("execution timeout"), "got: {msg}");
        assert!(msg.contains("slow-plugin"), "got: {msg}");
    }

    #[test]
    fn execution_failed_display() {
        let err = PluginError::ExecutionFailed {
            plugin_id: "bad-plugin".to_string(),
            reason: "trap: unreachable".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("execution failed"), "got: {msg}");
        assert!(msg.contains("bad-plugin"), "got: {msg}");
        assert!(msg.contains("trap: unreachable"), "got: {msg}");
    }

    #[test]
    fn lifecycle_error_display() {
        let err = PluginError::LifecycleError {
            plugin_id: "hook-plugin".to_string(),
            hook: "on_load".to_string(),
            reason: "db connection failed".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("lifecycle error"), "got: {msg}");
        assert!(msg.contains("hook-plugin"), "got: {msg}");
        assert!(msg.contains("on_load"), "got: {msg}");
        assert!(msg.contains("db connection failed"), "got: {msg}");
    }

    #[test]
    fn capability_denied_display() {
        let err = PluginError::CapabilityDenied {
            plugin_id: "sneaky-plugin".to_string(),
            capability: "fs:write".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("capability denied"), "got: {msg}");
        assert!(msg.contains("sneaky-plugin"), "got: {msg}");
        assert!(msg.contains("fs:write"), "got: {msg}");
    }

    #[test]
    fn plugin_not_found_display() {
        let err = PluginError::PluginNotFound("missing-plugin".to_string());
        let msg = err.to_string();
        assert!(msg.contains("plugin not found"), "got: {msg}");
        assert!(msg.contains("missing-plugin"), "got: {msg}");
    }

    #[test]
    fn duplicate_plugin_display() {
        let err = PluginError::DuplicatePlugin("dupe-plugin".to_string());
        let msg = err.to_string();
        assert!(msg.contains("duplicate plugin"), "got: {msg}");
        assert!(msg.contains("dupe-plugin"), "got: {msg}");
    }

    #[test]
    fn duplicate_cli_subcommand_display() {
        let err = PluginError::DuplicateCliSubcommand {
            plugin_id: "cli-plugin".to_string(),
            subcommand: "deploy".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("duplicate CLI subcommand"), "got: {msg}");
        assert!(msg.contains("cli-plugin"), "got: {msg}");
        assert!(msg.contains("deploy"), "got: {msg}");
    }

    #[test]
    fn settings_invalid_display() {
        let err = PluginError::SettingsInvalid {
            plugin_id: "cfg-plugin".to_string(),
            reason: "timeout must be positive".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("settings invalid"), "got: {msg}");
        assert!(msg.contains("cfg-plugin"), "got: {msg}");
        assert!(msg.contains("timeout must be positive"), "got: {msg}");
    }

    #[test]
    fn reload_failed_display() {
        let err = PluginError::ReloadFailed {
            plugin_id: "hot-plugin".to_string(),
            reason: "checksum mismatch".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("reload failed"), "got: {msg}");
        assert!(msg.contains("hot-plugin"), "got: {msg}");
        assert!(msg.contains("checksum mismatch"), "got: {msg}");
    }

    #[test]
    fn plugin_reloading_display() {
        let err = PluginError::PluginReloading("busy-plugin".to_string());
        let msg = err.to_string();
        assert!(msg.contains("plugin reloading"), "got: {msg}");
        assert!(msg.contains("busy-plugin"), "got: {msg}");
    }

    #[test]
    fn io_error_from_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file gone");
        let plugin_err: PluginError = io_err.into();
        let msg = plugin_err.to_string();
        assert!(msg.contains("I/O error"), "got: {msg}");
        assert!(msg.contains("file gone"), "got: {msg}");
    }
}
