//! Configuration file parsing and persistence.
//!
//! Thin wrapper over [`nexus_formats::config`]. Types and parsing logic live
//! in `nexus-formats`; this module re-exports them and adapts the error type
//! to [`StorageError`] so internal consumers (and the `config_read` /
//! `config_reset` IPC handlers) keep their existing error contract.
//!
//! Four config files live in `.forge/`:
//! - `app.toml`       — core application settings
//! - `workspace.json` — UI state
//! - `mcp.toml`       — MCP server configuration
//! - `ai.toml`        — AI provider and model configuration
//!
//! TOML / JSON loads run `${ENV_VAR}` substitution before parsing.

use std::path::Path;

pub use nexus_formats::config::{
    AiConfig, AiModel, AiProvider, AppConfig, CoreSettings, DreamCycleSettings, EditorSettings,
    GitSettings, McpConfig, McpServerEntry, OpenFileEntry, PanelConfig, PanelLayout,
    PluginSettings, PreviewSettings, SearchSettings, WorkspaceState,
};

use crate::StorageError;

// ── Load / Save ──────────────────────────────────────────────────────────────

/// Load the application config from `.forge/app.toml`. Returns defaults when
/// the file is absent.
///
/// # Errors
///
/// Returns [`StorageError::Config`] when the TOML is malformed.
pub fn load_app_config(forge_root: &Path) -> Result<AppConfig, StorageError> {
    Ok(nexus_formats::config::load_app_config(forge_root)?)
}

/// Save the application config to `.forge/app.toml`.
///
/// # Errors
///
/// Returns [`StorageError`] on I/O or serialization failure.
pub fn save_app_config(forge_root: &Path, config: &AppConfig) -> Result<(), StorageError> {
    Ok(nexus_formats::config::save_app_config(forge_root, config)?)
}

/// Load workspace state from `.forge/workspace.json`. Returns defaults when
/// the file is absent.
///
/// # Errors
///
/// Returns [`StorageError::Config`] when the JSON is malformed.
pub fn load_workspace_state(forge_root: &Path) -> Result<WorkspaceState, StorageError> {
    Ok(nexus_formats::config::load_workspace_state(forge_root)?)
}

/// Save workspace state to `.forge/workspace.json`.
///
/// # Errors
///
/// Returns [`StorageError`] on I/O or serialization failure.
pub fn save_workspace_state(
    forge_root: &Path,
    state: &WorkspaceState,
) -> Result<(), StorageError> {
    Ok(nexus_formats::config::save_workspace_state(forge_root, state)?)
}

/// Load MCP config from `.forge/mcp.toml`. Returns defaults when the file is
/// absent.
///
/// # Errors
///
/// Returns [`StorageError::Config`] when the TOML is malformed.
pub fn load_mcp_config(forge_root: &Path) -> Result<McpConfig, StorageError> {
    Ok(nexus_formats::config::load_mcp_config(forge_root)?)
}

/// Save MCP config to `.forge/mcp.toml`.
///
/// # Errors
///
/// Returns [`StorageError`] on I/O or serialization failure.
pub fn save_mcp_config(forge_root: &Path, config: &McpConfig) -> Result<(), StorageError> {
    Ok(nexus_formats::config::save_mcp_config(forge_root, config)?)
}

/// Load AI config from `.forge/ai.toml`. Returns defaults when the file is
/// absent.
///
/// # Errors
///
/// Returns [`StorageError::Config`] when the TOML is malformed.
pub fn load_ai_config(forge_root: &Path) -> Result<AiConfig, StorageError> {
    Ok(nexus_formats::config::load_ai_config(forge_root)?)
}

/// Save AI config to `.forge/ai.toml`.
///
/// # Errors
///
/// Returns [`StorageError`] on I/O or serialization failure.
pub fn save_ai_config(forge_root: &Path, config: &AiConfig) -> Result<(), StorageError> {
    Ok(nexus_formats::config::save_ai_config(forge_root, config)?)
}
