//! Theme manifest format — TOML schema from PRD §2.2.
//!
//! A `NEXUS.toml` file at the root of a theme directory defines one theme
//! package: its metadata, the list of CSS variable overrides, optional
//! platform-specific overrides, and discovery tags.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Platform, VariableMap};

/// Root schema for a theme's `NEXUS.toml` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeManifest {
    /// Required `[theme]` header (metadata + visual info + feature flags).
    pub theme: ThemeHeader,

    /// CSS variable overrides; keys are `--nx-*` variable names.
    #[serde(default)]
    pub variables: VariableMap,

    /// Optional typography block.
    #[serde(default)]
    pub typography: Option<TypographyBlock>,

    /// Optional platform-specific variable overrides.
    #[serde(default)]
    pub platforms: PlatformOverrides,

    /// Other themes or plugins this theme requires.
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,

    /// Discovery metadata (keywords, contrast level, use cases).
    #[serde(default)]
    pub tags: TagBlock,

    /// Changelog: version string → human description.
    #[serde(default)]
    pub version_history: BTreeMap<String, String>,
}

/// `[theme]` table — metadata + feature flags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeHeader {
    /// Human-readable name (e.g. "Default Light").
    pub name: String,

    /// Semver theme version.
    pub version: String,

    /// Theme author.
    pub author: String,

    /// Short description shown in the theme picker.
    pub description: String,

    /// SPDX license identifier.
    #[serde(default = "default_license")]
    pub license: String,

    /// Minimum Nexus version this theme requires.
    #[serde(default = "default_min_version")]
    pub nexus_min_version: String,

    /// Maximum Nexus version, or `"*"` for any.
    #[serde(default = "default_wildcard")]
    pub nexus_max_version: String,

    /// Long-form name for the picker UI (falls back to [`Self::name`]).
    #[serde(default)]
    pub display_name: Option<String>,

    /// Base64-encoded 32×32 PNG or URL.
    #[serde(default)]
    pub icon: Option<String>,

    /// Broad category for filtering in the picker.
    #[serde(default)]
    pub category: ThemeCategory,

    /// Modes (`light`, `dark`) this theme supports.
    #[serde(default)]
    pub supports: Vec<String>,

    /// Platforms for which [`PlatformOverrides`] has entries.
    #[serde(default)]
    pub platform_specific: Vec<String>,
}

/// Broad category for a theme — used for filtering in the picker UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeCategory {
    /// Light mode theme.
    #[default]
    Light,
    /// Dark mode theme.
    Dark,
    /// Sepia / low-blue-light theme.
    Sepia,
    /// Increased-contrast theme for accessibility.
    HighContrast,
    /// Anything else.
    Custom,
}

/// `[typography]` table.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TypographyBlock {
    /// `font-family` stack for sans-serif text.
    #[serde(default)]
    pub sans_font: Option<String>,
    /// `font-family` stack for monospace text (code blocks, editor).
    #[serde(default)]
    pub mono_font: Option<String>,
    /// `font-family` stack for serif text (optional).
    #[serde(default)]
    pub serif_font: Option<String>,
    /// Remote `@import` URLs for fonts (e.g. Google Fonts).
    #[serde(default)]
    pub font_imports: Vec<String>,
}

/// `[platforms.*]` tables.
///
/// Each inner map is `--nx-var-name` → `value`, applied on top of the theme's
/// default `[variables]` when the current [`Platform`] matches.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformOverrides {
    /// `[platforms.macos]` overrides.
    #[serde(default)]
    pub macos: VariableMap,
    /// `[platforms.windows]` overrides.
    #[serde(default)]
    pub windows: VariableMap,
    /// `[platforms.linux]` overrides.
    #[serde(default)]
    pub linux: VariableMap,
}

impl PlatformOverrides {
    /// Returns the overrides for `platform`, or an empty map.
    #[must_use]
    pub fn for_platform(&self, platform: Platform) -> &VariableMap {
        match platform {
            Platform::Macos => &self.macos,
            Platform::Windows => &self.windows,
            Platform::Linux => &self.linux,
        }
    }
}

/// `[tags]` table.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TagBlock {
    /// Keywords for search / discovery.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// `"cool"`, `"warm"`, `"neutral"`, …
    #[serde(default)]
    pub color_temperature: Option<String>,
    /// `"aa"`, `"aaa"`, `"low"`.
    #[serde(default)]
    pub contrast_level: Option<String>,
    /// Suggested use cases (`"coding"`, `"writing"`, …).
    #[serde(default)]
    pub use_case: Vec<String>,
}

fn default_license() -> String {
    "MIT".to_string()
}

fn default_min_version() -> String {
    "0.1.0".to_string()
}

fn default_wildcard() -> String {
    "*".to_string()
}

impl ThemeManifest {
    /// Parse a manifest from a TOML source string.
    ///
    /// # Errors
    /// Returns [`crate::ThemeError::Manifest`] if the TOML is malformed.
    /// The caller wraps it with an originating path for better error messages.
    pub fn from_toml(src: &str) -> std::result::Result<Self, toml::de::Error> {
        toml::from_str(src)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"
[theme]
name = "Default Light"
version = "1.0.0"
author = "Anthropic"
description = "Clean, accessible light theme for focused work"
license = "MIT"
category = "light"
supports = ["light"]
platform_specific = ["macos"]

[variables]
"--nx-color-primary" = "#4A90E2"
"--nx-bg-primary" = "#FFFFFF"

[typography]
sans_font = "system-ui, sans-serif"

[platforms.macos]
"--nx-color-primary" = "#006AFF"

[tags]
keywords = ["light", "minimal"]
contrast_level = "aa"
use_case = ["writing", "coding"]

[version_history]
"1.0.0" = "Initial release"
"##;

    #[test]
    fn parses_full_manifest() {
        let m = ThemeManifest::from_toml(SAMPLE).unwrap();
        assert_eq!(m.theme.name, "Default Light");
        assert_eq!(m.theme.category, ThemeCategory::Light);
        assert_eq!(m.variables["--nx-color-primary"], "#4A90E2");
        assert_eq!(
            m.platforms.macos["--nx-color-primary"],
            "#006AFF"
        );
        assert_eq!(
            m.typography.as_ref().unwrap().sans_font.as_deref(),
            Some("system-ui, sans-serif"),
        );
        assert_eq!(m.tags.contrast_level.as_deref(), Some("aa"));
        assert_eq!(m.version_history["1.0.0"], "Initial release");
    }

    #[test]
    fn minimal_manifest_uses_defaults() {
        let src = r#"
[theme]
name = "Bare"
version = "0.1.0"
author = "me"
description = "minimal"
"#;
        let m = ThemeManifest::from_toml(src).unwrap();
        assert_eq!(m.theme.license, "MIT");
        assert_eq!(m.theme.nexus_min_version, "0.1.0");
        assert_eq!(m.theme.nexus_max_version, "*");
        assert_eq!(m.theme.category, ThemeCategory::Light);
        assert!(m.variables.is_empty());
        assert!(m.platforms.macos.is_empty());
    }

    #[test]
    fn platform_overrides_lookup() {
        let m = ThemeManifest::from_toml(SAMPLE).unwrap();
        assert!(m.platforms.for_platform(Platform::Macos).contains_key("--nx-color-primary"));
        assert!(m.platforms.for_platform(Platform::Windows).is_empty());
    }

    #[test]
    fn roundtrip_serialization() {
        let m = ThemeManifest::from_toml(SAMPLE).unwrap();
        let out = toml::to_string(&m).unwrap();
        let m2 = ThemeManifest::from_toml(&out).unwrap();
        assert_eq!(m.theme.name, m2.theme.name);
        assert_eq!(m.variables, m2.variables);
    }
}
