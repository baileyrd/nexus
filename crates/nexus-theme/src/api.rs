//! Transport-agnostic IPC shim.
//!
//! [`ThemeEngine`] owns the runtime state (discovered themes + snippets,
//! current selection) and exposes command-shaped functions that a future
//! Tauri layer can wrap with `#[tauri::command]` one-liners. All inputs are
//! JSON-serializable; all outputs implement [`serde::Serialize`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::resolver::{resolve, ResolverInput};
use crate::snippet::{CssSnippet, SnippetMode, SnippetScope};
use crate::theme::{Theme, ThemeMetadata, BUILTIN_DARK_ID, BUILTIN_LIGHT_ID};
use crate::variables::VariableMap;
use crate::{Platform, ResolvedTheme, Result, ThemeError, ThemeMode};

/// Response shape for [`ThemeEngine::apply_theme`] — matches PRD §10.1
/// `AppliedTheme`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct AppliedTheme {
    /// Theme id that was applied.
    pub id: String,
    /// Human-readable theme name.
    pub name: String,
    /// Fully-resolved variable map (base → theme → platform → snippets → plugins).
    pub variables: VariableMap,
}

/// Listing-friendly snippet description.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SnippetMetadata {
    /// Snippet id (filename stem).
    pub id: String,
    /// Snippet name from the header.
    pub name: String,
    /// Snippet description from the header.
    pub description: String,
    /// Mode filter.
    pub mode: SnippetMode,
    /// Scope (global or per-surface selector).
    pub scope: SnippetScope,
    /// Whether the snippet is currently enabled.
    pub enabled: bool,
}

/// Config snapshot persisted to disk (see PRD §3.2 step 6).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ThemeConfig {
    /// Selected theme id.
    pub theme_id: String,
    /// Selected mode.
    pub mode: ThemeMode,
    /// Enabled snippet ids, in cascade order.
    pub enabled_snippets: Vec<String>,
}

/// Runtime owner of theme + snippet state, plus all the command shims.
///
/// Usage:
/// ```no_run
/// use nexus_theme::api::ThemeEngine;
/// let mut engine = ThemeEngine::with_dirs("/some/themes", "/some/snippets").unwrap();
/// let applied = engine.apply_theme("nexus-dark").unwrap();
/// println!("applied {} variables", applied.variables.len());
/// ```
#[derive(Debug, Clone)]
pub struct ThemeEngine {
    themes: BTreeMap<String, Theme>,
    snippets: BTreeMap<String, CssSnippet>,
    themes_dir: Option<PathBuf>,
    snippets_dir: Option<PathBuf>,
    current_theme_id: String,
    mode: ThemeMode,
    platform: Platform,
    enabled_snippet_ids: Vec<String>,
    plugin_overrides: VariableMap,
}

impl Default for ThemeEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeEngine {
    /// Engine with only the bundled built-in themes and no snippets.
    #[must_use]
    pub fn new() -> Self {
        let themes = Theme::builtins()
            .into_iter()
            .map(|t| (t.id.clone(), t))
            .collect();
        Self {
            themes,
            snippets: BTreeMap::new(),
            themes_dir: None,
            snippets_dir: None,
            current_theme_id: BUILTIN_LIGHT_ID.to_string(),
            mode: ThemeMode::default(),
            platform: Platform::current(),
            enabled_snippet_ids: Vec::new(),
            plugin_overrides: VariableMap::new(),
        }
    }

    /// Engine that also scans `themes_dir` and `snippets_dir` for
    /// user-installed packages. Missing directories are treated as empty.
    ///
    /// # Errors
    /// Returns an error if either directory exists but fails to read for
    /// reasons other than "not found".
    pub fn with_dirs(
        themes_dir: impl AsRef<Path>,
        snippets_dir: impl AsRef<Path>,
    ) -> Result<Self> {
        let mut engine = Self::new();
        engine.themes_dir = Some(themes_dir.as_ref().to_path_buf());
        engine.snippets_dir = Some(snippets_dir.as_ref().to_path_buf());
        engine.reload()?;
        Ok(engine)
    }

    /// Rescan the configured theme and snippet directories. Built-in themes
    /// are always preserved.
    ///
    /// # Errors
    /// Propagates directory read errors other than "not found".
    pub fn reload(&mut self) -> Result<()> {
        self.themes = Theme::builtins()
            .into_iter()
            .map(|t| (t.id.clone(), t))
            .collect();

        if let Some(dir) = &self.themes_dir {
            for theme in Theme::discover(dir)? {
                self.themes.insert(theme.id.clone(), theme);
            }
        }

        if let Some(dir) = &self.snippets_dir {
            self.snippets = CssSnippet::discover(dir)?
                .into_iter()
                .map(|s| (s.id.clone(), s))
                .collect();
            // Drop enabled ids that no longer exist on disk.
            self.enabled_snippet_ids
                .retain(|id| self.snippets.contains_key(id));
        }

        if !self.themes.contains_key(&self.current_theme_id) {
            self.current_theme_id = BUILTIN_LIGHT_ID.to_string();
        }
        Ok(())
    }

    /// Override the platform used by the resolver. Defaults to the build
    /// target's platform.
    pub fn set_platform(&mut self, platform: Platform) {
        self.platform = platform;
    }

    /// Merge plugin overrides on top of everything else in the cascade.
    pub fn set_plugin_overrides(&mut self, overrides: VariableMap) {
        self.plugin_overrides = overrides;
    }

    /// All available themes (built-ins + discovered), alphabetical.
    #[must_use]
    pub fn get_available_themes(&self) -> Vec<ThemeMetadata> {
        self.themes.values().map(Theme::metadata).collect()
    }

    /// All available snippets with their `enabled` flag filled in.
    #[must_use]
    pub fn get_available_snippets(&self) -> Vec<SnippetMetadata> {
        self.snippets
            .values()
            .map(|s| SnippetMetadata {
                id: s.id.clone(),
                name: s.name.clone(),
                description: s.description.clone(),
                mode: s.mode,
                scope: s.scope.clone(),
                enabled: self.enabled_snippet_ids.contains(&s.id),
            })
            .collect()
    }

    /// Switch the active theme and return the resulting [`AppliedTheme`].
    ///
    /// # Errors
    /// Returns [`ThemeError::ThemeNotFound`] if `theme_id` is unknown.
    pub fn apply_theme(&mut self, theme_id: &str) -> Result<AppliedTheme> {
        if !self.themes.contains_key(theme_id) {
            return Err(ThemeError::ThemeNotFound(theme_id.to_string()));
        }
        self.current_theme_id = theme_id.to_string();
        let resolved = self.compute();
        Ok(AppliedTheme {
            id: resolved.theme_id,
            name: self.themes[theme_id].manifest.theme.name.clone(),
            variables: resolved.variables,
        })
    }

    /// Set light/dark/system and recompute.
    pub fn set_mode(&mut self, mode: ThemeMode) -> AppliedTheme {
        self.mode = mode;
        let resolved = self.compute();
        AppliedTheme {
            name: self.themes[&resolved.theme_id]
                .manifest
                .theme
                .name
                .clone(),
            id: resolved.theme_id,
            variables: resolved.variables,
        }
    }

    /// Toggle a snippet on/off; returns the new ordered list of enabled ids.
    ///
    /// # Errors
    /// Returns [`ThemeError::SnippetNotFound`] if `snippet_id` is unknown.
    pub fn toggle_snippet(&mut self, snippet_id: &str) -> Result<Vec<String>> {
        if !self.snippets.contains_key(snippet_id) {
            return Err(ThemeError::SnippetNotFound(snippet_id.to_string()));
        }
        if let Some(pos) = self
            .enabled_snippet_ids
            .iter()
            .position(|id| id == snippet_id)
        {
            self.enabled_snippet_ids.remove(pos);
        } else {
            self.enabled_snippet_ids.push(snippet_id.to_string());
        }
        Ok(self.enabled_snippet_ids.clone())
    }

    /// Replace the ordered list of enabled snippet ids.
    ///
    /// # Errors
    /// Returns [`ThemeError::SnippetNotFound`] if any id is unknown.
    pub fn reorder_snippets(&mut self, ids: Vec<String>) -> Result<()> {
        for id in &ids {
            if !self.snippets.contains_key(id) {
                return Err(ThemeError::SnippetNotFound(id.clone()));
            }
        }
        self.enabled_snippet_ids = ids;
        Ok(())
    }

    /// Stateless cascade: given a theme id and a list of enabled snippet ids,
    /// return the merged [`VariableMap`]. Used by the PRD §10.1
    /// `compute_variables` IPC command.
    ///
    /// # Errors
    /// Returns [`ThemeError::ThemeNotFound`] or [`ThemeError::SnippetNotFound`]
    /// for unknown ids.
    pub fn compute_variables(
        &self,
        theme_id: &str,
        enabled_snippets: &[String],
    ) -> Result<VariableMap> {
        let theme = self
            .themes
            .get(theme_id)
            .ok_or_else(|| ThemeError::ThemeNotFound(theme_id.to_string()))?;
        let mut snippets = Vec::with_capacity(enabled_snippets.len());
        for id in enabled_snippets {
            let snippet = self
                .snippets
                .get(id)
                .ok_or_else(|| ThemeError::SnippetNotFound(id.clone()))?;
            snippets.push(snippet.clone());
        }

        let resolved = resolve(&ResolverInput {
            theme,
            mode: self.mode,
            platform: self.platform,
            snippets: &snippets,
            plugin_overrides: &self.plugin_overrides,
        });
        Ok(resolved.variables)
    }

    /// Current in-memory config snapshot — ready to serialize to
    /// `~/.nexus/theme-config.json`.
    #[must_use]
    pub fn config(&self) -> ThemeConfig {
        ThemeConfig {
            theme_id: self.current_theme_id.clone(),
            mode: self.mode,
            enabled_snippets: self.enabled_snippet_ids.clone(),
        }
    }

    /// Restore state from a persisted [`ThemeConfig`]. Unknown ids are
    /// silently dropped (matching PRD §3.2 "graceful fallback" behaviour).
    pub fn apply_config(&mut self, cfg: ThemeConfig) {
        if self.themes.contains_key(&cfg.theme_id) {
            self.current_theme_id = cfg.theme_id;
        }
        self.mode = cfg.mode;
        self.enabled_snippet_ids = cfg
            .enabled_snippets
            .into_iter()
            .filter(|id| self.snippets.contains_key(id))
            .collect();
    }

    /// Compute the current [`ResolvedTheme`] from the engine's state.
    #[must_use]
    pub fn compute(&self) -> ResolvedTheme {
        let theme = &self.themes[&self.current_theme_id];
        let snippets: Vec<CssSnippet> = self
            .enabled_snippet_ids
            .iter()
            .filter_map(|id| self.snippets.get(id).cloned())
            .collect();
        resolve(&ResolverInput {
            theme,
            mode: self.mode,
            platform: self.platform,
            snippets: &snippets,
            plugin_overrides: &self.plugin_overrides,
        })
    }
}

/// Convenience: id of the default startup theme.
#[must_use]
pub fn default_theme_id() -> &'static str {
    BUILTIN_LIGHT_ID
}

/// Convenience: id of the bundled dark theme.
#[must_use]
pub fn dark_theme_id() -> &'static str {
    BUILTIN_DARK_ID
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_engine_has_builtins() {
        let engine = ThemeEngine::new();
        let ids: Vec<_> = engine
            .get_available_themes()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert!(ids.contains(&"nexus-light".to_string()));
        assert!(ids.contains(&"nexus-dark".to_string()));
    }

    #[test]
    fn apply_theme_switches_current() {
        let mut engine = ThemeEngine::new();
        let applied = engine.apply_theme("nexus-dark").unwrap();
        assert_eq!(applied.id, "nexus-dark");
        assert_eq!(engine.config().theme_id, "nexus-dark");
        // Dark-mode-only variable should be present.
        assert_eq!(applied.variables["--nx-bg-primary"], "#1A1A1A");
    }

    #[test]
    fn apply_unknown_theme_errors() {
        let mut engine = ThemeEngine::new();
        let err = engine.apply_theme("nope").unwrap_err();
        assert!(matches!(err, ThemeError::ThemeNotFound(_)));
    }

    #[test]
    fn toggle_snippet_flips_enabled_state() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("neon.css"),
            "/* Name: Neon\nDescription: d\nMode: all */\n:root { --nx-color-primary: #0F0; }",
        )
        .unwrap();
        let themes_dir = tempfile::tempdir().unwrap();

        let mut engine = ThemeEngine::with_dirs(themes_dir.path(), dir.path()).unwrap();
        let ids = engine.toggle_snippet("neon").unwrap();
        assert_eq!(ids, vec!["neon".to_string()]);

        let resolved = engine.compute();
        assert_eq!(resolved.variables["--nx-color-primary"], "#0F0");

        let ids = engine.toggle_snippet("neon").unwrap();
        assert!(ids.is_empty());
    }

    #[test]
    fn compute_variables_is_stateless() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("neon.css"),
            "/* Name: Neon\nDescription: d\nMode: all */\n:root { --nx-color-primary: #0F0; }",
        )
        .unwrap();
        let themes_dir = tempfile::tempdir().unwrap();
        let engine = ThemeEngine::with_dirs(themes_dir.path(), dir.path()).unwrap();

        let vars = engine
            .compute_variables("nexus-light", &["neon".to_string()])
            .unwrap();
        assert_eq!(vars["--nx-color-primary"], "#0F0");

        // State is unchanged.
        assert!(engine.config().enabled_snippets.is_empty());
    }

    #[test]
    fn apply_config_restores_state() {
        let mut engine = ThemeEngine::new();
        let cfg = ThemeConfig {
            theme_id: "nexus-dark".into(),
            mode: ThemeMode::Dark,
            enabled_snippets: vec![],
        };
        engine.apply_config(cfg);
        assert_eq!(engine.config().theme_id, "nexus-dark");
        assert_eq!(engine.config().mode, ThemeMode::Dark);
    }

    #[test]
    fn apply_config_drops_unknown_theme_id() {
        let mut engine = ThemeEngine::new();
        engine.apply_config(ThemeConfig {
            theme_id: "ghost".into(),
            mode: ThemeMode::Light,
            enabled_snippets: vec![],
        });
        // current_theme_id unchanged from default.
        assert_eq!(engine.config().theme_id, "nexus-light");
    }

    #[test]
    fn reorder_snippets_validates_ids() {
        let mut engine = ThemeEngine::new();
        let err = engine
            .reorder_snippets(vec!["ghost".into()])
            .unwrap_err();
        assert!(matches!(err, ThemeError::SnippetNotFound(_)));
    }

    #[test]
    fn set_plugin_overrides_wins() {
        let mut engine = ThemeEngine::new();
        let mut overrides = VariableMap::new();
        overrides.insert("--nx-color-primary".into(), "#FACADE".into());
        engine.set_plugin_overrides(overrides);
        let resolved = engine.compute();
        assert_eq!(resolved.variables["--nx-color-primary"], "#FACADE");
    }
}
