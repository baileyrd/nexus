//! Declarative layout presets.
//!
//! Layout presets (Obsidian, Vibe, Dev, …) are TOML files instead of Rust
//! constructors so that:
//!
//! 1. Plugins can ship presets by dropping a `*.layout.toml` file in their
//!    preset directory — no recompile.
//! 2. Users can author their own under `<forge>/.nexus/layouts/`.
//! 3. The three file-and-author conventions (themes, snippets, presets) share
//!    the same pattern.
//!
//! Core presets are embedded into the binary via [`include_str!`] so they
//! load without a filesystem. [`PresetRegistry`] collects all three sources
//! (embedded / user / plugin) behind one lookup.
//!
//! ## Schema
//!
//! A preset file mirrors the subset of [`WorkspaceLayout`] the author cares
//! about — `name`, the `root` tree, the two sidebars, and the bottom panel.
//! Runtime-only fields (`id`, `metadata`, `focusedPaneId`) are filled in by
//! [`LayoutPreset::instantiate`] when the preset is loaded. A minimal preset
//! is therefore:
//!
//! ```toml
//! name = "Writing"
//!
//! [root]
//! type = "leaf"
//! id = "editor"
//! tabs = []
//! collapsed = false
//!
//! [leftSidePanel]
//! side = "left"
//! width = 280
//! collapsed = true
//! miniMode = false
//! panels = []
//! panelOrder = []
//!
//! [rightSidePanel]
//! side = "right"
//! width = 280
//! collapsed = true
//! miniMode = false
//! panels = []
//! panelOrder = []
//!
//! [bottomPanel]
//! height = 200
//! collapsed = true
//! tabs = []
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::layout::{
    BottomPanel, LayoutMetadata, LayoutNode, PaneId, RibbonItem, SidePanel, WorkspaceLayout,
};
use crate::{Result, ThemeError};

/// Authored preset schema — the TOML shape users and plugins write.
///
/// Converted to a live [`WorkspaceLayout`] at load time via
/// [`Self::instantiate`], which assigns a fresh workspace id and stamps
/// fresh timestamps in [`LayoutMetadata`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutPreset {
    /// Stable id (`"obsidian"`, `"vibe"`, `"dev"`, …). Also the lookup key in
    /// [`PresetRegistry`].
    pub id: String,
    /// User-visible display name (`"Obsidian"`).
    pub name: String,
    /// One-line description shown in the picker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Root pane tree.
    pub root: LayoutNode,
    /// Workspace-level activity ribbon (far-left vertical rail). Empty
    /// presets omit the key entirely.
    #[serde(default)]
    pub ribbon: Vec<RibbonItem>,
    /// Left dock.
    pub left_side_panel: SidePanel,
    /// Right dock.
    pub right_side_panel: SidePanel,
    /// Bottom panel.
    pub bottom_panel: BottomPanel,
    /// Which pane starts focused. Must reference a leaf in `root`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_pane_id: Option<PaneId>,
}

impl LayoutPreset {
    /// Hydrate into a live [`WorkspaceLayout`] with fresh workspace id and
    /// metadata timestamps. Pane ids from the preset are preserved — they
    /// identify panes within this specific instantiation, not globally.
    #[must_use]
    pub fn instantiate(&self) -> WorkspaceLayout {
        WorkspaceLayout {
            id: format!("workspace-{}", uuid::Uuid::now_v7()),
            name: self.name.clone(),
            version: "1.0".to_string(),
            root: self.root.clone(),
            ribbon: self.ribbon.clone(),
            left_side_panel: self.left_side_panel.clone(),
            right_side_panel: self.right_side_panel.clone(),
            bottom_panel: self.bottom_panel.clone(),
            focused_pane_id: self.focused_pane_id.clone(),
            metadata: LayoutMetadata::now(1920, 1080),
        }
    }
}

/// Parse a preset from TOML text. The `path` is used purely for error
/// messages and can be a synthetic marker for embedded presets (e.g.
/// `"<embedded:obsidian>"`).
///
/// # Errors
///
/// Returns [`ThemeError::PresetToml`] if the TOML is malformed or doesn't
/// match the schema.
pub fn parse_preset(toml_text: &str, path: impl Into<PathBuf>) -> Result<LayoutPreset> {
    toml::from_str(toml_text).map_err(|source| ThemeError::PresetToml {
        path: path.into(),
        source,
    })
}

/// Summary metadata for a registered preset; returned by
/// [`PresetRegistry::list`] so UIs can show a picker without parsing every
/// file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../app/src/bindings/")]
pub struct PresetInfo {
    /// Stable id for lookup.
    pub id: String,
    /// User-visible name.
    pub name: String,
    /// Description if provided by the preset.
    pub description: Option<String>,
    /// Where this preset came from.
    pub source: PresetSourceKind,
}

/// Origin tag for a registered preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../../app/src/bindings/")]
pub enum PresetSourceKind {
    /// Shipped with the binary via `include_str!`.
    Embedded,
    /// Authored by the user in `<forge>/.nexus/layouts/`.
    User,
    /// Contributed by a plugin.
    Plugin,
}

#[derive(Debug, Clone)]
enum PresetSource {
    Embedded {
        id: &'static str,
        toml: &'static str,
    },
    File {
        kind: PresetSourceKind,
        path: PathBuf,
    },
}

impl PresetSource {
    fn kind(&self) -> PresetSourceKind {
        match self {
            Self::Embedded { .. } => PresetSourceKind::Embedded,
            Self::File { kind, .. } => *kind,
        }
    }
}

/// Registry of available layout presets, backed by embedded / user / plugin
/// sources.
///
/// Lookups are lazy: the TOML is parsed on each [`Self::get`]. That keeps
/// memory bounded and ensures hot-edited user preset files pick up on the
/// next load without a cache invalidation step.
#[derive(Debug, Default)]
pub struct PresetRegistry {
    sources: BTreeMap<String, PresetSource>,
}

impl PresetRegistry {
    /// Fresh registry containing only the core presets bundled with the
    /// binary. Call [`Self::scan_user_dir`] or
    /// [`Self::register_plugin_preset`] to add more.
    #[must_use]
    pub fn with_core_presets() -> Self {
        let mut registry = Self::default();
        for (id, toml) in core_presets::ALL {
            registry.sources.insert(
                (*id).to_string(),
                PresetSource::Embedded { id, toml },
            );
        }
        registry
    }

    /// Scan a directory for `*.layout.toml` files and register each as a
    /// user preset. Non-existent directories are treated as empty (returns
    /// `Ok(0)`). File-level errors are collected into a rolled-up
    /// [`ThemeError::PresetToml`] after all readable files are registered.
    ///
    /// Returns the number of presets registered.
    ///
    /// # Errors
    ///
    /// Returns [`ThemeError::Io`] if the directory can't be read or a
    /// filename inside it can't be stat'd.
    pub fn scan_user_dir(&mut self, dir: impl AsRef<Path>) -> Result<usize> {
        self.scan_dir(dir.as_ref(), PresetSourceKind::User)
    }

    /// Register a single preset file shipped by a plugin. The `id` in the
    /// TOML must match the file's stem.
    ///
    /// # Errors
    ///
    /// Returns [`ThemeError::Io`] if the path isn't readable.
    pub fn register_plugin_preset(&mut self, path: impl Into<PathBuf>) -> Result<()> {
        let path = path.into();
        let id = preset_id_from_path(&path)?;
        self.sources.insert(
            id,
            PresetSource::File {
                kind: PresetSourceKind::Plugin,
                path,
            },
        );
        Ok(())
    }

    fn scan_dir(&mut self, dir: &Path, kind: PresetSourceKind) -> Result<usize> {
        if !dir.exists() {
            return Ok(0);
        }
        let entries =
            std::fs::read_dir(dir).map_err(|source| ThemeError::io(dir.to_path_buf(), source))?;
        let mut count = 0;
        for entry in entries {
            let entry = entry.map_err(|source| ThemeError::io(dir.to_path_buf(), source))?;
            let path = entry.path();
            if !is_preset_file(&path) {
                continue;
            }
            let id = preset_id_from_path(&path)?;
            self.sources
                .insert(id, PresetSource::File { kind, path });
            count += 1;
        }
        Ok(count)
    }

    /// Look up a preset by id and hydrate it into a fresh
    /// [`WorkspaceLayout`].
    ///
    /// # Errors
    ///
    /// - [`ThemeError::PresetNotFound`] if `id` isn't registered.
    /// - [`ThemeError::Io`] / [`ThemeError::PresetToml`] if the backing file
    ///   can't be read or parsed.
    pub fn get(&self, id: &str) -> Result<WorkspaceLayout> {
        let source = self
            .sources
            .get(id)
            .ok_or_else(|| ThemeError::PresetNotFound(id.to_string()))?;
        let preset = self.load(source)?;
        Ok(preset.instantiate())
    }

    /// List every registered preset with summary metadata. Sorted by id so
    /// the picker order is deterministic.
    ///
    /// Parse errors in individual presets are skipped — a malformed user
    /// preset should not hide the rest of the picker. Callers that want to
    /// surface parse errors should call [`Self::get`] on specific ids.
    #[must_use]
    pub fn list(&self) -> Vec<PresetInfo> {
        self.sources
            .iter()
            .filter_map(|(id, source)| {
                let preset = self.load(source).ok()?;
                Some(PresetInfo {
                    id: id.clone(),
                    name: preset.name,
                    description: preset.description,
                    source: source.kind(),
                })
            })
            .collect()
    }

    fn load(&self, source: &PresetSource) -> Result<LayoutPreset> {
        match source {
            PresetSource::Embedded { id, toml } => parse_preset(toml, format!("<embedded:{id}>")),
            PresetSource::File { path, .. } => {
                let text = std::fs::read_to_string(path)
                    .map_err(|source| ThemeError::io(path.clone(), source))?;
                let mut preset = parse_preset(&text, path.clone())?;
                // Keep the id from the filename authoritative — this lets
                // authors rename the file without editing the TOML.
                preset.id = preset_id_from_path(path)?;
                Ok(preset)
            }
        }
    }
}

fn is_preset_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    name.ends_with(".layout.toml")
}

fn preset_id_from_path(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|name| name.strip_suffix(".layout.toml"))
        .map(str::to_string)
        .ok_or_else(|| {
            ThemeError::io(
                path.to_path_buf(),
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "preset file must be named `<id>.layout.toml`",
                ),
            )
        })
}

/// Core presets shipped with the binary.
mod core_presets {
    pub static ALL: &[(&str, &str)] = &[
        ("writing", include_str!("../presets/writing.layout.toml")),
        (
            "reviewing",
            include_str!("../presets/reviewing.layout.toml"),
        ),
        ("coding", include_str!("../presets/coding.layout.toml")),
        ("obsidian", include_str!("../presets/obsidian.layout.toml")),
        ("vibe", include_str!("../presets/vibe.layout.toml")),
        ("dev", include_str!("../presets/dev.layout.toml")),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_presets_all_parse() {
        let registry = PresetRegistry::with_core_presets();
        let ids = ["writing", "reviewing", "coding", "obsidian", "vibe", "dev"];
        for id in ids {
            let layout = registry
                .get(id)
                .unwrap_or_else(|e| panic!("preset {id} failed to load: {e}"));
            assert_eq!(layout.version, "1.0");
            assert!(!layout.id.is_empty());
            assert!(!layout.name.is_empty());
        }
    }

    #[test]
    fn list_returns_every_core_preset_sorted() {
        let registry = PresetRegistry::with_core_presets();
        let infos = registry.list();
        let ids: Vec<&str> = infos.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["coding", "dev", "obsidian", "reviewing", "vibe", "writing"]
        );
        for info in &infos {
            assert_eq!(info.source, PresetSourceKind::Embedded);
        }
    }

    #[test]
    fn get_unknown_preset_errors() {
        let registry = PresetRegistry::with_core_presets();
        let err = registry.get("nope").unwrap_err();
        assert!(matches!(err, ThemeError::PresetNotFound(id) if id == "nope"));
    }

    #[test]
    fn instantiate_generates_fresh_workspace_id() {
        let registry = PresetRegistry::with_core_presets();
        let a = registry.get("writing").unwrap();
        let b = registry.get("writing").unwrap();
        assert_ne!(a.id, b.id, "each instantiation should have a unique id");
    }

    #[test]
    fn scan_user_dir_picks_up_layout_toml_files() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("my-custom.layout.toml");
        std::fs::write(
            &file,
            r#"
id = "my-custom"
name = "My Custom"

[root]
type = "leaf"
id = "main"
tabs = []
collapsed = false

[leftSidePanel]
side = "left"
width = 280
collapsed = true
miniMode = false
panels = []
panelOrder = []

[rightSidePanel]
side = "right"
width = 280
collapsed = true
miniMode = false
panels = []
panelOrder = []

[bottomPanel]
height = 200
collapsed = true
tabs = []
"#,
        )
        .unwrap();

        let mut registry = PresetRegistry::with_core_presets();
        let count = registry.scan_user_dir(tmp.path()).unwrap();
        assert_eq!(count, 1);

        let layout = registry.get("my-custom").unwrap();
        assert_eq!(layout.name, "My Custom");

        let info = registry
            .list()
            .into_iter()
            .find(|i| i.id == "my-custom")
            .unwrap();
        assert_eq!(info.source, PresetSourceKind::User);
    }

    #[test]
    fn scan_nonexistent_dir_is_ok() {
        let mut registry = PresetRegistry::default();
        let count = registry
            .scan_user_dir(PathBuf::from("/definitely/does/not/exist"))
            .unwrap();
        assert_eq!(count, 0);
    }
}
