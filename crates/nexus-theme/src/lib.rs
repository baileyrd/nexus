//! # nexus-theme
//!
//! Theming engine for Nexus (PRD 07, Rust slice).
//!
//! Provides the pure-Rust portion of the theming subsystem: a CSS variable
//! registry with ~100 built-in defaults, a TOML manifest parser for theme
//! packages, CSS snippet discovery + parsing, and the resolution cascade that
//! merges base defaults → theme → platform overrides → snippets → plugin
//! overrides into a final [`VariableMap`].
//!
//! This crate is transport-agnostic. A Tauri layer can wrap [`api::ThemeEngine`]
//! to expose its functions as `#[tauri::command]`s; a future TUI or headless
//! preview tool can call the same functions directly. No `tauri` dependency.
//!
//! ## Modules
//!
//! | Module | Purpose |
//! |--------|---------|
//! | [`variables`] | `VariableMap`, built-in defaults (PRD §1.2), value substitution |
//! | [`manifest`]  | `ThemeManifest` TOML schema (PRD §2.2) |
//! | [`theme`]     | `Theme` loader + directory scan + bundled light/dark themes |
//! | [`snippet`]   | CSS snippet header parser + directory scan (PRD §4) |
//! | [`resolver`]  | Cascade: defaults → theme → platform → snippets → overrides (PRD §3.1) |
//! | [`api`]       | Plain-fn command shims for future IPC wiring |
//! | [`error`]     | `ThemeError` enum |

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod api;
pub mod core_plugin;
pub mod error;
pub mod layout;
pub mod layout_manager;
pub mod manifest;
pub mod preset;
pub mod resolver;
pub mod snippet;
pub mod theme;
pub mod variables;
pub mod watcher;

pub use core_plugin::ThemeCorePlugin;
pub use error::ThemeError;
pub use layout::{
    BottomPanel, Direction, FooterAction, LayoutNode, PaneId, PaneNode, Panel, PanelToolbarItem,
    RibbonAction, RibbonItem, SidePanel, SidePanelFooter, SidePanelSide, StatusBarItem, Surface,
    Tab, TabId, WorkspaceLayout,
};
pub use layout_manager::{LayoutManager, SavedLayoutInfo};
pub use manifest::ThemeManifest;
pub use preset::{LayoutPreset, PresetInfo, PresetRegistry, PresetSourceKind};
pub use resolver::{ResolvedTheme, ResolverInput};
pub use snippet::CssSnippet;
pub use theme::{Theme, ThemeCategory, ThemeMode};
pub use variables::VariableMap;
pub use watcher::{ThemeReloadEvent, ThemeWatcher};

/// Result alias for theme operations.
pub type Result<T> = std::result::Result<T, ThemeError>;

/// Target platform for platform-specific variable overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    /// Apple macOS.
    Macos,
    /// Microsoft Windows.
    Windows,
    /// Linux (any distribution; covers both Wayland and X11).
    Linux,
}

impl Platform {
    /// Returns the current build target's platform.
    ///
    /// Defaults to [`Platform::Linux`] on unknown targets so plugins can
    /// still load a sensible palette during tests.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    /// String key used in `[platforms.*]` TOML tables.
    #[must_use]
    pub const fn as_key(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }
}
