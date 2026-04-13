//! Configuration file parsing and persistence.
//!
//! Supports four config files stored in `.forge/`:
//! - `app.toml` — core application settings
//! - `workspace.json` — UI state (active file, open files, layout)
//! - `mcp.toml` — MCP server configuration
//! - `ai.toml` — AI provider and model configuration

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::StorageError;

// ── AppConfig (app.toml) ─────────────────────────────────────────────────────

/// Top-level application settings loaded from `.forge/app.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Core forge settings.
    pub core: CoreSettings,
    /// Editor behaviour settings.
    pub editor: EditorSettings,
    /// Preview rendering settings.
    pub preview: PreviewSettings,
    /// Search engine settings.
    pub search: SearchSettings,
    /// Plugin configuration.
    pub plugins: PluginSettings,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            core: CoreSettings::default(),
            editor: EditorSettings::default(),
            preview: PreviewSettings::default(),
            search: SearchSettings::default(),
            plugins: PluginSettings::default(),
        }
    }
}

/// Core forge settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CoreSettings {
    /// Display name for the forge.
    pub name: String,
    /// Default directory for new notes.
    pub default_note_dir: String,
    /// Directory for binary attachments.
    pub attachment_dir: String,
    /// Format string for daily note titles.
    pub daily_note_format: String,
    /// Default layout mode.
    pub default_layout: String,
    /// UI theme name.
    pub theme: String,
    /// UI language code.
    pub language: String,
}

impl Default for CoreSettings {
    fn default() -> Self {
        Self {
            name: "MyForge".to_string(),
            default_note_dir: "notes".to_string(),
            attachment_dir: "attachments".to_string(),
            daily_note_format: "%Y-%m-%d".to_string(),
            default_layout: "sidebar".to_string(),
            theme: "auto".to_string(),
            language: "en".to_string(),
        }
    }
}

/// Editor behaviour settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    /// Font size in pixels.
    pub font_size: u32,
    /// Font family name.
    pub font_family: String,
    /// Line height multiplier.
    pub line_height: f64,
    /// Enable vim keybindings.
    pub enable_vim_mode: bool,
    /// Auto-save on changes.
    pub auto_save: bool,
    /// Auto-save delay in milliseconds.
    pub auto_save_delay_ms: u64,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_size: 14,
            font_family: "monospace".to_string(),
            line_height: 1.6,
            enable_vim_mode: false,
            auto_save: true,
            auto_save_delay_ms: 3000,
        }
    }
}

/// Preview rendering settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewSettings {
    /// Enable Mermaid diagram rendering.
    pub enable_mermaid: bool,
    /// Enable KaTeX math rendering.
    pub enable_katex: bool,
    /// Enable syntax highlighting in code blocks.
    pub enable_highlight: bool,
    /// Enable wikilink resolution in preview.
    pub enable_wikilinks: bool,
}

impl Default for PreviewSettings {
    fn default() -> Self {
        Self {
            enable_mermaid: true,
            enable_katex: true,
            enable_highlight: true,
            enable_wikilinks: true,
        }
    }
}

/// Search engine settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchSettings {
    /// Enable full-text search indexing.
    pub enable_full_text: bool,
    /// Re-index interval in milliseconds.
    pub index_interval_ms: u64,
    /// Maximum search results to return.
    pub max_results: usize,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            enable_full_text: true,
            index_interval_ms: 5000,
            max_results: 50,
        }
    }
}

/// Plugin configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginSettings {
    /// List of enabled plugin IDs.
    pub enabled: Vec<String>,
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self {
            enabled: Vec::new(),
        }
    }
}

// ── WorkspaceState (workspace.json) ──────────────────────────────────────────

/// UI workspace state loaded from `.forge/workspace.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceState {
    /// Currently active file path (vault-relative).
    pub active_file: Option<String>,
    /// List of open file paths.
    pub open_files: Vec<OpenFileEntry>,
    /// Whether the sidebar is collapsed.
    pub sidebar_collapsed: bool,
    /// Panel layout configuration.
    pub panel_layout: PanelLayout,
    /// Recently opened files (most recent first).
    pub recent_files: Vec<String>,
    /// Last search query.
    pub search_query: String,
    /// Active theme name.
    pub theme: String,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            active_file: None,
            open_files: Vec::new(),
            sidebar_collapsed: false,
            panel_layout: PanelLayout::default(),
            recent_files: Vec::new(),
            search_query: String::new(),
            theme: "dark".to_string(),
        }
    }
}

/// An open file with cursor position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenFileEntry {
    /// Vault-relative file path.
    pub file: String,
    /// Cursor line number.
    #[serde(default)]
    pub line: u32,
    /// Cursor column number.
    #[serde(default)]
    pub column: u32,
}

/// Sidebar and panel layout dimensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelLayout {
    /// Left panel configuration.
    pub left: PanelConfig,
    /// Right panel configuration.
    pub right: PanelConfig,
}

impl Default for PanelLayout {
    fn default() -> Self {
        Self {
            left: PanelConfig { width: 250, collapsed: false },
            right: PanelConfig { width: 300, collapsed: true },
        }
    }
}

/// Configuration for a single panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelConfig {
    /// Panel width in pixels.
    pub width: u32,
    /// Whether the panel is collapsed.
    pub collapsed: bool,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self { width: 250, collapsed: false }
    }
}

// ── McpConfig (mcp.toml) ─────────────────────────────────────────────────────

/// MCP server configuration loaded from `.forge/mcp.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    /// Whether the MCP server is enabled.
    pub enabled: bool,
    /// Transport type (stdio, http).
    pub transport: String,
    /// List of tools allowed to be exposed.
    pub allowed_tools: Vec<String>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            transport: "stdio".to_string(),
            allowed_tools: Vec::new(),
        }
    }
}

// ── AiConfig (ai.toml) ──────────────────────────────────────────────────────

/// AI provider and model configuration loaded from `.forge/ai.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    /// Default AI provider name.
    pub provider: String,
    /// Default model ID.
    pub model: String,
    /// Environment variable name containing the API key.
    pub api_key_env: Option<String>,
    /// Embedding model ID.
    pub embedding_model: Option<String>,
    /// Maximum tokens for generation.
    pub max_tokens: u32,
    /// Sampling temperature.
    pub temperature: f64,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            api_key_env: Some("ANTHROPIC_API_KEY".to_string()),
            embedding_model: None,
            max_tokens: 4096,
            temperature: 0.7,
        }
    }
}

// ── Load / Save ──────────────────────────────────────────────────────────────

/// Load the application config from `.forge/app.toml`.
///
/// Returns defaults when the file is missing.
///
/// # Errors
///
/// Returns [`StorageError::CorruptFile`] when the TOML is malformed.
pub fn load_app_config(forge_root: &Path) -> Result<AppConfig, StorageError> {
    load_toml(forge_root, "app.toml")
}

/// Save the application config to `.forge/app.toml`.
///
/// # Errors
///
/// Returns [`StorageError`] on I/O or serialization failure.
pub fn save_app_config(forge_root: &Path, config: &AppConfig) -> Result<(), StorageError> {
    save_toml(forge_root, "app.toml", config)
}

/// Load workspace state from `.forge/workspace.json`.
///
/// Returns defaults when the file is missing.
///
/// # Errors
///
/// Returns [`StorageError::CorruptFile`] when the JSON is malformed.
pub fn load_workspace_state(forge_root: &Path) -> Result<WorkspaceState, StorageError> {
    load_json(forge_root, "workspace.json")
}

/// Save workspace state to `.forge/workspace.json`.
///
/// # Errors
///
/// Returns [`StorageError`] on I/O or serialization failure.
pub fn save_workspace_state(forge_root: &Path, state: &WorkspaceState) -> Result<(), StorageError> {
    save_json(forge_root, "workspace.json", state)
}

/// Load MCP config from `.forge/mcp.toml`.
///
/// Returns defaults when the file is missing.
///
/// # Errors
///
/// Returns [`StorageError::CorruptFile`] when the TOML is malformed.
pub fn load_mcp_config(forge_root: &Path) -> Result<McpConfig, StorageError> {
    load_toml(forge_root, "mcp.toml")
}

/// Save MCP config to `.forge/mcp.toml`.
///
/// # Errors
///
/// Returns [`StorageError`] on I/O or serialization failure.
pub fn save_mcp_config(forge_root: &Path, config: &McpConfig) -> Result<(), StorageError> {
    save_toml(forge_root, "mcp.toml", config)
}

/// Load AI config from `.forge/ai.toml`.
///
/// Returns defaults when the file is missing.
///
/// # Errors
///
/// Returns [`StorageError::CorruptFile`] when the TOML is malformed.
pub fn load_ai_config(forge_root: &Path) -> Result<AiConfig, StorageError> {
    load_toml(forge_root, "ai.toml")
}

/// Save AI config to `.forge/ai.toml`.
///
/// # Errors
///
/// Returns [`StorageError`] on I/O or serialization failure.
pub fn save_ai_config(forge_root: &Path, config: &AiConfig) -> Result<(), StorageError> {
    save_toml(forge_root, "ai.toml", config)
}

// ── Internal helpers ─────────────────────────────────────────────────────────

fn load_toml<T: serde::de::DeserializeOwned + Default>(
    forge_root: &Path,
    filename: &str,
) -> Result<T, StorageError> {
    let path = forge_root.join(".forge").join(filename);
    if !path.exists() {
        return Ok(T::default());
    }
    let text = std::fs::read_to_string(&path)?;
    toml::from_str(&text).map_err(|e| StorageError::CorruptFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

fn save_toml<T: serde::Serialize>(
    forge_root: &Path,
    filename: &str,
    value: &T,
) -> Result<(), StorageError> {
    let dir = forge_root.join(".forge");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(filename);
    let text = toml::to_string_pretty(value).map_err(|e| StorageError::CorruptFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    std::fs::write(&path, text)?;
    Ok(())
}

fn load_json<T: serde::de::DeserializeOwned + Default>(
    forge_root: &Path,
    filename: &str,
) -> Result<T, StorageError> {
    let path = forge_root.join(".forge").join(filename);
    if !path.exists() {
        return Ok(T::default());
    }
    let text = std::fs::read_to_string(&path)?;
    serde_json::from_str(&text).map_err(|e| StorageError::CorruptFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })
}

fn save_json<T: serde::Serialize>(
    forge_root: &Path,
    filename: &str,
    value: &T,
) -> Result<(), StorageError> {
    let dir = forge_root.join(".forge");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(filename);
    let text = serde_json::to_string_pretty(value).map_err(|e| StorageError::CorruptFile {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;
    std::fs::write(&path, text)?;
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn app_config_defaults() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.core.name, "MyForge");
        assert_eq!(cfg.editor.font_size, 14);
        assert!(cfg.editor.auto_save);
        assert!(cfg.preview.enable_mermaid);
        assert_eq!(cfg.search.max_results, 50);
    }

    #[test]
    fn app_config_toml_round_trip() {
        let cfg = AppConfig::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let parsed: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(parsed.core.name, cfg.core.name);
        assert_eq!(parsed.editor.font_size, cfg.editor.font_size);
    }

    #[test]
    fn workspace_state_json_round_trip() {
        let mut state = WorkspaceState::default();
        state.active_file = Some("notes/hello.md".to_string());
        state.open_files.push(OpenFileEntry {
            file: "notes/hello.md".to_string(),
            line: 42,
            column: 0,
        });
        let json = serde_json::to_string_pretty(&state).unwrap();
        let parsed: WorkspaceState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.active_file, state.active_file);
        assert_eq!(parsed.open_files.len(), 1);
        assert_eq!(parsed.open_files[0].line, 42);
    }

    #[test]
    fn mcp_config_toml_round_trip() {
        let cfg = McpConfig::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let parsed: McpConfig = toml::from_str(&text).unwrap();
        assert_eq!(parsed.transport, "stdio");
        assert!(parsed.enabled);
    }

    #[test]
    fn ai_config_toml_round_trip() {
        let cfg = AiConfig::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let parsed: AiConfig = toml::from_str(&text).unwrap();
        assert_eq!(parsed.provider, "anthropic");
        assert_eq!(parsed.max_tokens, 4096);
    }

    #[test]
    fn load_missing_config_returns_defaults() {
        let dir = tmp();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let cfg = load_app_config(dir.path()).unwrap();
        assert_eq!(cfg.core.name, "MyForge");
    }

    #[test]
    fn save_and_load_app_config() {
        let dir = tmp();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut cfg = AppConfig::default();
        cfg.core.name = "TestForge".to_string();
        cfg.editor.font_size = 18;
        save_app_config(dir.path(), &cfg).unwrap();
        let loaded = load_app_config(dir.path()).unwrap();
        assert_eq!(loaded.core.name, "TestForge");
        assert_eq!(loaded.editor.font_size, 18);
    }

    #[test]
    fn save_and_load_workspace_state() {
        let dir = tmp();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut state = WorkspaceState::default();
        state.active_file = Some("readme.md".to_string());
        state.theme = "light".to_string();
        save_workspace_state(dir.path(), &state).unwrap();
        let loaded = load_workspace_state(dir.path()).unwrap();
        assert_eq!(loaded.active_file, Some("readme.md".to_string()));
        assert_eq!(loaded.theme, "light");
    }

    #[test]
    fn partial_toml_merges_with_defaults() {
        let dir = tmp();
        let forge_dir = dir.path().join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();
        // Write a partial TOML that only sets core.name.
        std::fs::write(
            forge_dir.join("app.toml"),
            "[core]\nname = \"Partial\"\n",
        )
        .unwrap();
        let cfg = load_app_config(dir.path()).unwrap();
        assert_eq!(cfg.core.name, "Partial");
        // Other fields should still have defaults.
        assert_eq!(cfg.editor.font_size, 14);
        assert!(cfg.preview.enable_mermaid);
    }

    #[test]
    fn save_and_load_mcp_config() {
        let dir = tmp();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut cfg = McpConfig::default();
        cfg.enabled = false;
        cfg.allowed_tools = vec!["search".to_string(), "read".to_string()];
        save_mcp_config(dir.path(), &cfg).unwrap();
        let loaded = load_mcp_config(dir.path()).unwrap();
        assert!(!loaded.enabled);
        assert_eq!(loaded.allowed_tools.len(), 2);
    }

    #[test]
    fn save_and_load_ai_config() {
        let dir = tmp();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut cfg = AiConfig::default();
        cfg.model = "claude-opus-4-6".to_string();
        cfg.temperature = 0.5;
        save_ai_config(dir.path(), &cfg).unwrap();
        let loaded = load_ai_config(dir.path()).unwrap();
        assert_eq!(loaded.model, "claude-opus-4-6");
        assert!((loaded.temperature - 0.5).abs() < f64::EPSILON);
    }
}
