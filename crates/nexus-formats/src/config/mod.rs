//! Forge configuration file loading and saving.
//!
//! Four config files live in `.forge/`:
//! - `app.toml`       — core application settings
//! - `workspace.json` — UI state
//! - `mcp.toml`       — MCP server configuration
//! - `ai.toml`        — AI provider / model configuration
//!
//! All TOML and JSON loaders run `${ENV_VAR}` substitution before parsing.

mod ai;
mod app;
pub mod env_subst;
mod mcp;
mod workspace;

pub use ai::{AiConfig, AiModel, AiProvider};
pub use app::{
    AppConfig, CoreSettings, EditorSettings, GitSettings, PluginSettings, PreviewSettings,
    SearchSettings,
};
pub use mcp::{McpConfig, McpServerEntry};
pub use workspace::{OpenFileEntry, PanelConfig, PanelLayout, WorkspaceState};

use std::path::Path;

use crate::error::ConfigError;

// ── Public load / save API ────────────────────────────────────────────────────

/// Load application config from `.forge/app.toml`.
///
/// Returns defaults when the file is absent. Runs env-var substitution before parsing.
///
/// # Errors
///
/// Returns [`ConfigError::TomlParse`] if the file is malformed.
pub fn load_app_config(forge_root: &Path) -> Result<AppConfig, ConfigError> {
    load_toml(forge_root, "app.toml")
}

/// Save application config to `.forge/app.toml`.
///
/// # Errors
///
/// Returns [`ConfigError`] on serialization or I/O failure.
pub fn save_app_config(forge_root: &Path, cfg: &AppConfig) -> Result<(), ConfigError> {
    save_toml(forge_root, "app.toml", cfg)
}

/// Load workspace state from `.forge/workspace.json`.
///
/// # Errors
///
/// Returns [`ConfigError::JsonParse`] if the file is malformed.
pub fn load_workspace_state(forge_root: &Path) -> Result<WorkspaceState, ConfigError> {
    load_json(forge_root, "workspace.json")
}

/// Save workspace state to `.forge/workspace.json`.
///
/// # Errors
///
/// Returns [`ConfigError`] on serialization or I/O failure.
pub fn save_workspace_state(forge_root: &Path, s: &WorkspaceState) -> Result<(), ConfigError> {
    save_json(forge_root, "workspace.json", s)
}

/// Load MCP config from `.forge/mcp.toml`.
///
/// # Errors
///
/// Returns [`ConfigError::TomlParse`] if the file is malformed.
pub fn load_mcp_config(forge_root: &Path) -> Result<McpConfig, ConfigError> {
    load_toml(forge_root, "mcp.toml")
}

/// Save MCP config to `.forge/mcp.toml`.
///
/// # Errors
///
/// Returns [`ConfigError`] on serialization or I/O failure.
pub fn save_mcp_config(forge_root: &Path, cfg: &McpConfig) -> Result<(), ConfigError> {
    save_toml(forge_root, "mcp.toml", cfg)
}

/// Load AI config from `.forge/ai.toml`.
///
/// # Errors
///
/// Returns [`ConfigError::TomlParse`] if the file is malformed.
pub fn load_ai_config(forge_root: &Path) -> Result<AiConfig, ConfigError> {
    load_toml(forge_root, "ai.toml")
}

/// Save AI config to `.forge/ai.toml`.
///
/// # Errors
///
/// Returns [`ConfigError`] on serialization or I/O failure.
pub fn save_ai_config(forge_root: &Path, cfg: &AiConfig) -> Result<(), ConfigError> {
    save_toml(forge_root, "ai.toml", cfg)
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn load_toml<T>(forge_root: &Path, filename: &str) -> Result<T, ConfigError>
where
    T: serde::de::DeserializeOwned + Default,
{
    let path = forge_root.join(".forge").join(filename);
    if !path.exists() {
        return Ok(T::default());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| ConfigError::TomlParse {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let substituted = env_subst::substitute(&raw)?;
    toml::from_str(&substituted).map_err(|e| ConfigError::TomlParse {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

fn save_toml<T: serde::Serialize>(
    forge_root: &Path,
    filename: &str,
    value: &T,
) -> Result<(), ConfigError> {
    let dir  = forge_root.join(".forge");
    std::fs::create_dir_all(&dir).map_err(|e| ConfigError::TomlParse {
        path: dir.display().to_string(),
        reason: e.to_string(),
    })?;
    let path = dir.join(filename);
    let text = toml::to_string_pretty(value).map_err(|e| ConfigError::TomlParse {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    std::fs::write(&path, text).map_err(|e| ConfigError::TomlParse {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

fn load_json<T>(forge_root: &Path, filename: &str) -> Result<T, ConfigError>
where
    T: serde::de::DeserializeOwned + Default,
{
    let path = forge_root.join(".forge").join(filename);
    if !path.exists() {
        return Ok(T::default());
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| ConfigError::JsonParse {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    let substituted = env_subst::substitute(&raw)?;
    serde_json::from_str(&substituted).map_err(|e| ConfigError::JsonParse {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

fn save_json<T: serde::Serialize>(
    forge_root: &Path,
    filename: &str,
    value: &T,
) -> Result<(), ConfigError> {
    let dir  = forge_root.join(".forge");
    std::fs::create_dir_all(&dir).map_err(|e| ConfigError::JsonParse {
        path: dir.display().to_string(),
        reason: e.to_string(),
    })?;
    let path = dir.join(filename);
    let text = serde_json::to_string_pretty(value).map_err(|e| ConfigError::JsonParse {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    std::fs::write(&path, text).map_err(|e| ConfigError::JsonParse {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn forge_dir(root: &Path) {
        std::fs::create_dir_all(root.join(".forge")).unwrap();
    }

    // ── AppConfig ─────────────────────────────────────────────────────────

    #[test]
    fn app_config_defaults_when_missing() {
        let dir = tmp();
        let cfg = load_app_config(dir.path()).unwrap();
        assert_eq!(cfg.core.name, "MyForge");
        assert_eq!(cfg.editor.font_size, 14);
    }

    #[test]
    fn save_and_load_app_config() {
        let dir = tmp();
        forge_dir(dir.path());
        let mut cfg = AppConfig::default();
        cfg.core.name = "TestForge".into();
        cfg.editor.font_size = 18;
        save_app_config(dir.path(), &cfg).unwrap();
        let loaded = load_app_config(dir.path()).unwrap();
        assert_eq!(loaded.core.name, "TestForge");
        assert_eq!(loaded.editor.font_size, 18);
    }

    #[test]
    fn partial_toml_merges_with_defaults() {
        let dir = tmp();
        forge_dir(dir.path());
        std::fs::write(
            dir.path().join(".forge/app.toml"),
            "[core]\nname = \"Partial\"\n",
        ).unwrap();
        let cfg = load_app_config(dir.path()).unwrap();
        assert_eq!(cfg.core.name, "Partial");
        assert_eq!(cfg.editor.font_size, 14); // default
    }

    // ── WorkspaceState ────────────────────────────────────────────────────

    #[test]
    fn workspace_defaults_when_missing() {
        let dir = tmp();
        let s = load_workspace_state(dir.path()).unwrap();
        assert_eq!(s.theme, "dark");
        assert!(s.active_file.is_none());
    }

    #[test]
    fn save_and_load_workspace_state() {
        let dir = tmp();
        forge_dir(dir.path());
        let mut s = WorkspaceState::default();
        s.active_file = Some("README.md".into());
        s.theme = "light".into();
        save_workspace_state(dir.path(), &s).unwrap();
        let loaded = load_workspace_state(dir.path()).unwrap();
        assert_eq!(loaded.active_file.as_deref(), Some("README.md"));
        assert_eq!(loaded.theme, "light");
    }

    // ── McpConfig ─────────────────────────────────────────────────────────

    #[test]
    fn save_and_load_mcp_config() {
        let dir = tmp();
        forge_dir(dir.path());
        let mut cfg = McpConfig::default();
        cfg.allowed_tools = vec!["search".into(), "read".into()];
        save_mcp_config(dir.path(), &cfg).unwrap();
        let loaded = load_mcp_config(dir.path()).unwrap();
        assert_eq!(loaded.allowed_tools.len(), 2);
    }

    // ── AiConfig ──────────────────────────────────────────────────────────

    #[test]
    fn save_and_load_ai_config() {
        let dir = tmp();
        forge_dir(dir.path());
        let mut cfg = AiConfig::default();
        cfg.model = "claude-opus-4-6".into();
        cfg.temperature = 0.3;
        save_ai_config(dir.path(), &cfg).unwrap();
        let loaded = load_ai_config(dir.path()).unwrap();
        assert_eq!(loaded.model, "claude-opus-4-6");
        assert!((loaded.temperature - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn env_var_substituted_in_ai_config() {
        let dir = tmp();
        forge_dir(dir.path());
        std::env::set_var("_NEXUS_TEST_API_KEY", "sk-test-value");
        std::fs::write(
            dir.path().join(".forge/ai.toml"),
            "provider = \"anthropic\"\napi_key_env = \"${_NEXUS_TEST_API_KEY}\"\n",
        ).unwrap();
        let loaded = load_ai_config(dir.path()).unwrap();
        assert_eq!(loaded.api_key_env.as_deref(), Some("sk-test-value"));
        std::env::remove_var("_NEXUS_TEST_API_KEY");
    }
}
