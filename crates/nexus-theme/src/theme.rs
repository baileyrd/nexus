//! Top-level [`Theme`] loader, directory scan, and bundled built-in themes.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;

pub use crate::manifest::ThemeCategory;
use crate::manifest::ThemeManifest;
use crate::{Result, ThemeError};

/// Name of the manifest file in a theme directory.
pub const MANIFEST_FILENAME: &str = "NEXUS.toml";

/// Bundled light theme manifest — see `themes/nexus-light/NEXUS.toml`.
pub const BUILTIN_LIGHT_TOML: &str = include_str!("../themes/nexus-light/NEXUS.toml");

/// Bundled dark theme manifest — see `themes/nexus-dark/NEXUS.toml`.
pub const BUILTIN_DARK_TOML: &str = include_str!("../themes/nexus-dark/NEXUS.toml");

/// Bundled high-contrast theme manifest — see `themes/nexus-forge/NEXUS.toml`.
pub const BUILTIN_FORGE_TOML: &str = include_str!("../themes/nexus-forge/NEXUS.toml");

/// Bundled Solarized Dark theme manifest.
pub const BUILTIN_SOLARIZED_DARK_TOML: &str =
    include_str!("../themes/nexus-solarized-dark/NEXUS.toml");

/// Bundled Solarized Light theme manifest.
pub const BUILTIN_SOLARIZED_LIGHT_TOML: &str =
    include_str!("../themes/nexus-solarized-light/NEXUS.toml");

/// Bundled Nord theme manifest.
pub const BUILTIN_NORD_TOML: &str = include_str!("../themes/nexus-nord/NEXUS.toml");

/// Bundled Dracula theme manifest.
pub const BUILTIN_DRACULA_TOML: &str = include_str!("../themes/nexus-dracula/NEXUS.toml");

/// Bundled Tomorrow Night theme manifest.
pub const BUILTIN_TOMORROW_NIGHT_TOML: &str =
    include_str!("../themes/nexus-tomorrow-night/NEXUS.toml");

/// Bundled Forge Ember dark theme manifest — derived from the Forge Color
/// System v0.1 ("ember on slate").
pub const BUILTIN_EMBER_DARK_TOML: &str = include_str!("../themes/nexus-ember-dark/NEXUS.toml");

/// Bundled Forge Ember light theme manifest.
pub const BUILTIN_EMBER_LIGHT_TOML: &str = include_str!("../themes/nexus-ember-light/NEXUS.toml");

/// Bundled Manuscript theme manifest — warm parchment canvas on dark chrome.
pub const BUILTIN_MANUSCRIPT_TOML: &str = include_str!("../themes/nexus-manuscript/NEXUS.toml");

/// Identifier for the bundled light theme.
pub const BUILTIN_LIGHT_ID: &str = "nexus-light";

/// Identifier for the bundled dark theme.
pub const BUILTIN_DARK_ID: &str = "nexus-dark";

/// Identifier for the bundled high-contrast theme.
pub const BUILTIN_FORGE_ID: &str = "nexus-forge";

/// Identifier for the bundled Solarized Dark theme.
pub const BUILTIN_SOLARIZED_DARK_ID: &str = "nexus-solarized-dark";

/// Identifier for the bundled Solarized Light theme.
pub const BUILTIN_SOLARIZED_LIGHT_ID: &str = "nexus-solarized-light";

/// Identifier for the bundled Nord theme.
pub const BUILTIN_NORD_ID: &str = "nexus-nord";

/// Identifier for the bundled Dracula theme.
pub const BUILTIN_DRACULA_ID: &str = "nexus-dracula";

/// Identifier for the bundled Tomorrow Night theme.
pub const BUILTIN_TOMORROW_NIGHT_ID: &str = "nexus-tomorrow-night";

/// Identifier for the bundled Forge Ember dark theme.
pub const BUILTIN_EMBER_DARK_ID: &str = "nexus-ember-dark";

/// Identifier for the bundled Forge Ember light theme.
pub const BUILTIN_EMBER_LIGHT_ID: &str = "nexus-ember-light";

/// Identifier for the bundled Manuscript theme.
pub const BUILTIN_MANUSCRIPT_ID: &str = "nexus-manuscript";

/// Canonical list of every theme bundled with the crate. Adding a new
/// built-in theme only requires appending an `(id, toml)` tuple here —
/// [`Theme::builtins`] and the `builtins_parse` test both read from this
/// table.
pub const BUILTIN_THEMES: &[(&str, &str)] = &[
    (BUILTIN_LIGHT_ID, BUILTIN_LIGHT_TOML),
    (BUILTIN_DARK_ID, BUILTIN_DARK_TOML),
    (BUILTIN_FORGE_ID, BUILTIN_FORGE_TOML),
    (BUILTIN_SOLARIZED_DARK_ID, BUILTIN_SOLARIZED_DARK_TOML),
    (BUILTIN_SOLARIZED_LIGHT_ID, BUILTIN_SOLARIZED_LIGHT_TOML),
    (BUILTIN_NORD_ID, BUILTIN_NORD_TOML),
    (BUILTIN_DRACULA_ID, BUILTIN_DRACULA_TOML),
    (BUILTIN_TOMORROW_NIGHT_ID, BUILTIN_TOMORROW_NIGHT_TOML),
    (BUILTIN_EMBER_DARK_ID, BUILTIN_EMBER_DARK_TOML),
    (BUILTIN_EMBER_LIGHT_ID, BUILTIN_EMBER_LIGHT_TOML),
    (BUILTIN_MANUSCRIPT_ID, BUILTIN_MANUSCRIPT_TOML),
];

/// A loaded theme package — identifier + parsed manifest + source path if any.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    /// Unique identifier (directory stem, e.g. `nexus-light`).
    pub id: String,

    /// Path to the theme's source directory on disk, or `None` for built-ins.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub path: Option<PathBuf>,

    /// Whether this theme ships with the engine.
    #[serde(default)]
    pub builtin: bool,

    /// Parsed manifest.
    pub manifest: ThemeManifest,
}

/// Which light/dark mode the UI is in. The resolver may use this to decide
/// whether a [`crate::CssSnippet`] applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[cfg_attr(feature = "ts-export", derive(JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    /// Force light mode.
    Light,
    /// Force dark mode.
    Dark,
    /// Follow the host OS preference (caller resolves to Light or Dark before
    /// handing this to the resolver).
    #[default]
    System,
}

impl Theme {
    /// Load a theme from a directory containing `NEXUS.toml`.
    ///
    /// # Errors
    /// Returns [`ThemeError::Io`] if the manifest can't be read, or
    /// [`ThemeError::Manifest`] if the TOML is malformed.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let manifest_path = dir.join(MANIFEST_FILENAME);
        let source =
            fs::read_to_string(&manifest_path).map_err(|e| ThemeError::io(&manifest_path, e))?;
        let manifest =
            ThemeManifest::from_toml(&source).map_err(|source| ThemeError::Manifest {
                path: manifest_path.clone(),
                source,
            })?;
        let id = dir
            .file_name()
            .and_then(|s| s.to_str())
            .map_or_else(|| dir.display().to_string(), str::to_owned);

        Ok(Self {
            id,
            path: Some(dir.to_path_buf()),
            builtin: false,
            manifest,
        })
    }

    /// Scan `root` for theme subdirectories and load each one whose
    /// `NEXUS.toml` parses successfully. Malformed themes are logged and
    /// skipped.
    ///
    /// # Errors
    /// Returns an error if the root directory itself fails to read with
    /// something other than "not found" (missing is treated as "no themes").
    pub fn discover(root: impl AsRef<Path>) -> Result<Vec<Self>> {
        let root = root.as_ref();
        let entries = match fs::read_dir(root) {
            Ok(it) => it,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(ThemeError::io(root, e)),
        };

        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            match Self::load(&path) {
                Ok(theme) => out.push(theme),
                Err(e) => tracing::warn!(?path, %e, "skipping malformed theme"),
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    /// Returns every bundled theme. The canonical list is
    /// [`BUILTIN_THEMES`]; new entries land there and flow through here.
    ///
    /// # Panics
    /// Panics if the bundled TOML fails to parse. This is a bug in the
    /// crate's own theme files and is caught by the `builtins_parse` test.
    #[must_use]
    pub fn builtins() -> Vec<Self> {
        BUILTIN_THEMES
            .iter()
            .map(|(id, toml)| Self::builtin(id, toml))
            .collect()
    }

    fn builtin(id: &str, source: &str) -> Self {
        let manifest = ThemeManifest::from_toml(source)
            .unwrap_or_else(|e| panic!("bundled theme `{id}` failed to parse: {e}"));
        Self {
            id: id.to_string(),
            path: None,
            builtin: true,
            manifest,
        }
    }

    /// Short metadata view used by the theme-picker IPC shim.
    #[must_use]
    pub fn metadata(&self) -> ThemeMetadata {
        ThemeMetadata {
            id: self.id.clone(),
            name: self.manifest.theme.name.clone(),
            author: self.manifest.theme.author.clone(),
            description: self.manifest.theme.description.clone(),
            category: self.manifest.theme.category,
            builtin: self.builtin,
            keywords: self.manifest.tags.keywords.clone(),
        }
    }
}

/// Compact theme description returned by listing APIs — matches PRD §10.1
/// `ThemeMetadata`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ThemeMetadata {
    /// Theme id (directory stem / built-in constant).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Theme author.
    pub author: String,
    /// Short description.
    pub description: String,
    /// Category for filtering (light/dark/etc.).
    pub category: ThemeCategory,
    /// Whether the theme is bundled with the engine.
    pub builtin: bool,
    /// Search keywords from `[tags]`.
    pub keywords: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_parse() {
        let themes = Theme::builtins();
        assert_eq!(themes.len(), BUILTIN_THEMES.len());
        for (theme, (expected_id, _)) in themes.iter().zip(BUILTIN_THEMES.iter()) {
            assert_eq!(theme.id, *expected_id);
            assert!(theme.builtin);
            assert!(theme.path.is_none());
        }
        let by_id: std::collections::HashMap<_, _> =
            themes.iter().map(|t| (t.id.as_str(), t)).collect();
        assert_eq!(
            by_id[BUILTIN_LIGHT_ID].manifest.theme.category,
            ThemeCategory::Light
        );
        assert_eq!(
            by_id[BUILTIN_DARK_ID].manifest.theme.category,
            ThemeCategory::Dark
        );
        assert_eq!(
            by_id[BUILTIN_FORGE_ID].manifest.theme.category,
            ThemeCategory::Dark
        );
        assert!(by_id[BUILTIN_LIGHT_ID]
            .manifest
            .variables
            .contains_key("--nx-color-primary"));
    }

    #[test]
    fn discover_reads_theme_directories() {
        let dir = tempfile::tempdir().unwrap();
        let theme_dir = dir.path().join("my-theme");
        std::fs::create_dir(&theme_dir).unwrap();
        std::fs::write(
            theme_dir.join("NEXUS.toml"),
            r#"
[theme]
name = "My Theme"
version = "0.1.0"
author = "me"
description = "test"
"#,
        )
        .unwrap();

        let themes = Theme::discover(dir.path()).unwrap();
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].id, "my-theme");
        assert!(!themes[0].builtin);
    }

    #[test]
    fn discover_missing_dir_is_ok() {
        let out = Theme::discover("/nonexistent/themes/dir").unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn load_reports_missing_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let err = Theme::load(dir.path()).unwrap_err();
        assert!(matches!(err, ThemeError::Io { .. }));
    }

    #[test]
    fn metadata_round_trips_key_fields() {
        let theme = &Theme::builtins()[0];
        let meta = theme.metadata();
        assert_eq!(meta.id, "nexus-light");
        assert_eq!(meta.category, ThemeCategory::Light);
        assert!(meta.builtin);
    }
}
