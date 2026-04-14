//! Theme resolution cascade.
//!
//! Produces a final [`ResolvedTheme`] by merging, in order (lower → higher
//! precedence, per PRD §3.1):
//!
//! 1. Base defaults ([`crate::variables::default_variables`])
//! 2. Selected theme's `[variables]`
//! 3. Selected theme's `[platforms.<current>]` overrides
//! 4. Enabled snippets (in the order the user provides them), filtered by
//!    the active [`ThemeMode`]
//! 5. Plugin overrides (plain `VariableMap` passed by the caller)
//!
//! `var(--nx-foo)` references in the final map are left intact so the
//! frontend's CSS engine can resolve them at render time. Callers that need
//! a flat map can run each value through [`crate::variables::substitute`].

use serde::{Deserialize, Serialize};

use crate::variables::{default_variables, VariableMap};
use crate::{CssSnippet, Platform, Theme, ThemeMode};

/// Everything the resolver needs to build a [`ResolvedTheme`].
#[derive(Debug, Clone, Copy)]
pub struct ResolverInput<'a> {
    /// The selected theme package.
    pub theme: &'a Theme,
    /// Current light/dark mode (not `System`; caller resolves `System` first).
    pub mode: ThemeMode,
    /// Current host platform.
    pub platform: Platform,
    /// Snippets the user has enabled, in cascade order.
    pub snippets: &'a [CssSnippet],
    /// Plugin-supplied variable overrides, applied last.
    pub plugin_overrides: &'a VariableMap,
}

impl<'a> ResolverInput<'a> {
    /// Build a minimal input with no snippets or overrides.
    #[must_use]
    pub fn new(theme: &'a Theme, mode: ThemeMode, platform: Platform) -> Self {
        const EMPTY_SNIPPETS: &[CssSnippet] = &[];
        // We cannot make a `&'static VariableMap` easily without a helper, so
        // callers that need overrides should build `ResolverInput` directly.
        static EMPTY_OVERRIDES: std::sync::OnceLock<VariableMap> = std::sync::OnceLock::new();
        Self {
            theme,
            mode,
            platform,
            snippets: EMPTY_SNIPPETS,
            plugin_overrides: EMPTY_OVERRIDES.get_or_init(VariableMap::new),
        }
    }
}

/// Output of the resolver — the final variable map plus provenance metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedTheme {
    /// Theme id the resolution was based on.
    pub theme_id: String,
    /// Mode the resolution used (`Light` or `Dark` — never `System`).
    pub mode: ThemeMode,
    /// Platform whose overrides were applied.
    pub platform: Platform,
    /// Snippet ids that contributed, in cascade order.
    pub applied_snippets: Vec<String>,
    /// Final merged variable map.
    pub variables: VariableMap,
}

/// Run the full cascade and return a [`ResolvedTheme`].
#[must_use]
pub fn resolve(input: &ResolverInput<'_>) -> ResolvedTheme {
    let ResolverInput {
        theme,
        mode,
        platform,
        snippets,
        plugin_overrides,
    } = *input;

    let mut vars = default_variables();

    // 2. Theme-level overrides.
    for (k, v) in &theme.manifest.variables {
        vars.insert(k.clone(), v.clone());
    }

    // 3. Platform-specific overrides from the theme.
    for (k, v) in theme.manifest.platforms.for_platform(platform) {
        vars.insert(k.clone(), v.clone());
    }

    // 4. Snippets (filtered by mode).
    let mut applied = Vec::new();
    let snippet_mode = match mode {
        ThemeMode::Dark => crate::snippet::SnippetMode::Dark,
        // Treat `System` as Light for snippet filtering; callers that care
        // must resolve `System` to a concrete mode before calling the resolver.
        ThemeMode::Light | ThemeMode::System => crate::snippet::SnippetMode::Light,
    };
    for snippet in snippets {
        if !snippet.applies_to(snippet_mode) {
            continue;
        }
        for (k, v) in &snippet.variables {
            vars.insert(k.clone(), v.clone());
        }
        applied.push(snippet.id.clone());
    }

    // 5. Plugin overrides.
    for (k, v) in plugin_overrides {
        vars.insert(k.clone(), v.clone());
    }

    ResolvedTheme {
        theme_id: theme.id.clone(),
        mode,
        platform,
        applied_snippets: applied,
        variables: vars,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snippet::CssSnippet;

    fn light_theme() -> Theme {
        Theme::builtins().into_iter().next().unwrap()
    }

    fn dark_theme() -> Theme {
        Theme::builtins().into_iter().nth(1).unwrap()
    }

    #[test]
    fn theme_overrides_defaults() {
        let theme = dark_theme();
        let resolved = resolve(&ResolverInput::new(&theme, ThemeMode::Dark, Platform::Linux));
        // Dark theme sets bg-primary to #1A1A1A, overriding the #FFFFFF default.
        assert_eq!(resolved.variables["--nx-bg-primary"], "#1A1A1A");
    }

    #[test]
    fn platform_overrides_win_over_theme_variables() {
        let theme = light_theme();
        let resolved_linux = resolve(&ResolverInput::new(&theme, ThemeMode::Light, Platform::Linux));
        let resolved_macos = resolve(&ResolverInput::new(&theme, ThemeMode::Light, Platform::Macos));
        assert_eq!(resolved_linux.variables["--nx-color-primary"], "#4A90E2");
        assert_eq!(resolved_macos.variables["--nx-color-primary"], "#006AFF");
    }

    #[test]
    fn snippets_override_platform_and_theme() {
        let theme = light_theme();
        let snippet = CssSnippet::parse(
            "neon",
            "/* Name: N\nDescription: D\nMode: all */\n:root { --nx-color-primary: #00FF00; }",
        )
        .unwrap();
        let overrides = VariableMap::new();
        let input = ResolverInput {
            theme: &theme,
            mode: ThemeMode::Light,
            platform: Platform::Macos,
            snippets: &[snippet],
            plugin_overrides: &overrides,
        };
        let resolved = resolve(&input);
        assert_eq!(resolved.variables["--nx-color-primary"], "#00FF00");
        assert_eq!(resolved.applied_snippets, vec!["neon".to_string()]);
    }

    #[test]
    fn plugin_overrides_win_last() {
        let theme = light_theme();
        let mut overrides = VariableMap::new();
        overrides.insert("--nx-color-primary".into(), "#ABCDEF".into());
        let input = ResolverInput {
            theme: &theme,
            mode: ThemeMode::Light,
            platform: Platform::Linux,
            snippets: &[],
            plugin_overrides: &overrides,
        };
        let resolved = resolve(&input);
        assert_eq!(resolved.variables["--nx-color-primary"], "#ABCDEF");
    }

    #[test]
    fn snippet_filtered_out_by_mode() {
        let theme = light_theme();
        let snippet = CssSnippet::parse(
            "only-dark",
            "/* Name: N\nDescription: D\nMode: dark */\n:root { --nx-color-primary: #00FF00; }",
        )
        .unwrap();
        let overrides = VariableMap::new();
        let input = ResolverInput {
            theme: &theme,
            mode: ThemeMode::Light,
            platform: Platform::Linux,
            snippets: &[snippet],
            plugin_overrides: &overrides,
        };
        let resolved = resolve(&input);
        assert_eq!(resolved.variables["--nx-color-primary"], "#4A90E2");
        assert!(resolved.applied_snippets.is_empty());
    }

    #[test]
    fn resolved_theme_records_provenance() {
        let theme = dark_theme();
        let resolved = resolve(&ResolverInput::new(&theme, ThemeMode::Dark, Platform::Macos));
        assert_eq!(resolved.theme_id, "nexus-dark");
        assert_eq!(resolved.mode, ThemeMode::Dark);
        assert_eq!(resolved.platform, Platform::Macos);
    }
}
