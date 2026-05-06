//! Kernel configuration.

use std::path::PathBuf;

use crate::error::ConfigError;

/// Configuration for a Kernel instance.
///
/// Load from disk via `KernelConfig::load`, or construct programmatically
/// (typically via `KernelConfig::for_testing` in tests).
#[derive(Debug, Clone)]
pub struct KernelConfig {
    /// Root directory of the forge (workspace).
    pub forge_root: PathBuf,

    /// Event bus ring buffer capacity. Slow subscribers receive
    /// `RecvError::Lagged(n)` if they fall more than this many events behind.
    pub event_bus_capacity: usize,

    /// Directories to search for plugin manifests. Default:
    /// `[<forge_root>/.nexus/plugins]`.
    pub plugin_search_paths: Vec<PathBuf>,

    /// Enable hot-reload of plugins when their WASM files change on disk.
    pub hot_reload_enabled: bool,

    /// Per-hook deadline for `on_init` / `on_start` lifecycle hooks
    /// (BL-095). A hook exceeding this returns
    /// `PluginError::LifecycleTimeout` and the worker thread is
    /// detached so a hung plugin cannot block bootstrap. Default 30s;
    /// `0` disables the watchdog (hooks run inline).
    pub lifecycle_timeout_secs: u64,
}

impl KernelConfig {
    /// Programmatic construction for tests. Uses defaults for everything
    /// except `forge_root`.
    #[must_use]
    pub fn for_testing(forge_root: PathBuf) -> Self {
        Self {
            forge_root,
            ..Self::default()
        }
    }

    /// Load from `<forge_root>/.nexus/config.toml`, falling back to defaults
    /// for any fields not specified. Returns a default config (without error)
    /// if the file doesn't exist.
    ///
    /// # Errors
    /// - `ConfigError::TomlParse` if the file exists but is not valid TOML.
    /// - `ConfigError::Invalid` if a field has an out-of-range value or the
    ///   file can't be read.
    pub fn load(forge_root: &std::path::Path) -> std::result::Result<Self, ConfigError> {
        let config_path = forge_root.join(".nexus").join("config.toml");

        // If no config file exists, return defaults with forge_root set.
        if !config_path.exists() {
            return Ok(Self {
                forge_root: forge_root.to_path_buf(),
                ..Self::default()
            });
        }

        // Read and parse.
        let content = std::fs::read_to_string(&config_path).map_err(|e| ConfigError::Invalid {
            path: config_path.clone(),
            reason: format!("failed to read: {e}"),
        })?;

        let raw: RawConfig = toml::from_str(&content).map_err(|source| ConfigError::TomlParse {
            path: config_path.clone(),
            source,
        })?;

        if raw.event_bus_capacity == Some(0) {
            return Err(ConfigError::Invalid {
                path: config_path,
                reason: "event_bus_capacity must be > 0".to_string(),
            });
        }

        Ok(Self {
            forge_root: forge_root.to_path_buf(),
            event_bus_capacity: raw.event_bus_capacity.unwrap_or(2048),
            plugin_search_paths: raw.plugin_search_paths.unwrap_or_default(),
            hot_reload_enabled: raw.hot_reload_enabled.unwrap_or(true),
            lifecycle_timeout_secs: raw.lifecycle_timeout_secs.unwrap_or(30),
        })
    }
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            forge_root: PathBuf::from("."),
            event_bus_capacity: 2048,
            plugin_search_paths: vec![],
            hot_reload_enabled: true,
            lifecycle_timeout_secs: 30,
        }
    }
}

/// Raw TOML shape for deserialization. All fields optional so missing
/// values fall back to defaults.
#[derive(Debug, serde::Deserialize)]
struct RawConfig {
    event_bus_capacity: Option<usize>,
    plugin_search_paths: Option<Vec<PathBuf>>,
    hot_reload_enabled: Option<bool>,
    lifecycle_timeout_secs: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_expected_values() {
        let cfg = KernelConfig::default();
        assert_eq!(cfg.event_bus_capacity, 2048);
        assert!(cfg.hot_reload_enabled);
        assert!(cfg.plugin_search_paths.is_empty());
    }

    #[test]
    fn for_testing_sets_forge_root() {
        let cfg = KernelConfig::for_testing(PathBuf::from("/tmp/test"));
        assert_eq!(cfg.forge_root, PathBuf::from("/tmp/test"));
        assert_eq!(cfg.event_bus_capacity, 2048);
    }

    #[test]
    fn config_is_clone() {
        let cfg = KernelConfig::default();
        let _cloned = cfg.clone();
    }

    #[test]
    fn load_missing_file_returns_defaults() {
        // `tempfile::tempdir()` over `env::temp_dir().join(static-name)`:
        // per-process unique paths + RAII cleanup so concurrent test
        // runs (parallel `cargo test`, multi-shard CI) don't race on
        // the same directory. See issue #81.
        let tmp = tempfile::tempdir().unwrap();

        let cfg = KernelConfig::load(tmp.path()).unwrap();
        assert_eq!(cfg.forge_root, tmp.path());
        assert_eq!(cfg.event_bus_capacity, 2048);
        assert!(cfg.hot_reload_enabled);
    }

    #[test]
    fn load_valid_config_applies_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".nexus")).unwrap();
        std::fs::write(
            tmp.path().join(".nexus/config.toml"),
            "event_bus_capacity = 4096\nhot_reload_enabled = false\n",
        )
        .unwrap();

        let cfg = KernelConfig::load(tmp.path()).unwrap();
        assert_eq!(cfg.event_bus_capacity, 4096);
        assert!(!cfg.hot_reload_enabled);
    }

    #[test]
    fn load_malformed_toml_returns_parse_error() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".nexus")).unwrap();
        std::fs::write(
            tmp.path().join(".nexus/config.toml"),
            "this is not valid toml = = =",
        )
        .unwrap();

        let err = KernelConfig::load(tmp.path()).unwrap_err();
        assert!(matches!(err, ConfigError::TomlParse { .. }));
    }

    #[test]
    fn load_zero_capacity_returns_invalid_error() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".nexus")).unwrap();
        std::fs::write(
            tmp.path().join(".nexus/config.toml"),
            "event_bus_capacity = 0\n",
        )
        .unwrap();

        let err = KernelConfig::load(tmp.path()).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }));
    }
}
