//! Named-layout persistence (PRD §5.3).
//!
//! [`LayoutManager`] wraps a filesystem directory of saved layouts, each
//! stored as a pretty-printed JSON file. IDs are file stems; the
//! [`WorkspaceLayout::id`](crate::WorkspaceLayout) field is canonical.
//!
//! Built-in presets (writing/reviewing/coding/obsidian/vibe/dev) live in
//! [`crate::preset`] as TOML files — see [`crate::PresetRegistry`].

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::layout::WorkspaceLayout;
use crate::{Result, ThemeError};

/// Filesystem-backed store of named [`WorkspaceLayout`]s.
///
/// Layouts live as `{layouts_dir}/{id}.json`. The directory is created on
/// construction if it doesn't already exist.
#[derive(Debug, Clone)]
pub struct LayoutManager {
    layouts_dir: PathBuf,
}

/// Listing-friendly summary of a saved layout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedLayoutInfo {
    /// Layout id (file stem).
    pub id: String,
    /// Human-readable name from [`WorkspaceLayout::name`].
    pub name: String,
    /// Last-modified timestamp from the layout metadata.
    pub last_modified: String,
}

impl LayoutManager {
    /// Create a manager backed by `layouts_dir`, creating the directory
    /// if missing.
    ///
    /// # Errors
    /// Returns [`ThemeError::Io`] if the directory can't be created.
    pub fn new(layouts_dir: impl Into<PathBuf>) -> Result<Self> {
        let layouts_dir = layouts_dir.into();
        fs::create_dir_all(&layouts_dir).map_err(|e| ThemeError::io(&layouts_dir, e))?;
        Ok(Self { layouts_dir })
    }

    /// Save `layout` using its existing `id` as the filename stem. If a
    /// file with that id already exists it is overwritten.
    ///
    /// # Errors
    /// Returns [`ThemeError::Io`] or [`ThemeError::LayoutJson`].
    pub fn save(&self, layout: &WorkspaceLayout) -> Result<()> {
        let path = self.layouts_dir.join(format!("{}.json", layout.id));
        layout.save_to_file(&path)
    }

    /// Assign `name` + a fresh id to `layout`, then save.
    ///
    /// # Errors
    /// See [`Self::save`].
    pub fn save_as(&self, name: impl Into<String>, layout: &mut WorkspaceLayout) -> Result<()> {
        layout.name = name.into();
        layout.id = format!("workspace-{}", uuid::Uuid::now_v7());
        self.save(layout)
    }

    /// Load the layout with `id`.
    ///
    /// # Errors
    /// Returns [`ThemeError::Io`] if no file exists for `id`, or
    /// [`ThemeError::LayoutJson`] on parse failure.
    pub fn load(&self, id: &str) -> Result<WorkspaceLayout> {
        let path = self.layouts_dir.join(format!("{id}.json"));
        WorkspaceLayout::load_from_file(&path)
    }

    /// Delete the layout with `id`.
    ///
    /// # Errors
    /// Returns [`ThemeError::Io`] if deletion fails. A missing file is
    /// not an error — the post-condition (no such layout) is satisfied.
    pub fn delete(&self, id: &str) -> Result<()> {
        let path = self.layouts_dir.join(format!("{id}.json"));
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(ThemeError::io(path, e)),
        }
    }

    /// Return a summary of every saved layout in the directory.
    ///
    /// Files that fail to parse are logged via `tracing::warn!` and
    /// skipped — one bad file won't hide the rest.
    ///
    /// # Errors
    /// Returns [`ThemeError::Io`] if the directory listing itself fails.
    pub fn list(&self) -> Result<Vec<SavedLayoutInfo>> {
        let entries =
            fs::read_dir(&self.layouts_dir).map_err(|e| ThemeError::io(&self.layouts_dir, e))?;

        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match WorkspaceLayout::load_from_file(&path) {
                Ok(layout) => out.push(SavedLayoutInfo {
                    id: layout.id,
                    name: layout.name,
                    last_modified: layout.metadata.last_modified,
                }),
                Err(e) => tracing::warn!(?path, %e, "skipping malformed layout"),
            }
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(out)
    }

    /// Root directory used by this manager.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.layouts_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PresetRegistry;

    fn sample_layout() -> WorkspaceLayout {
        PresetRegistry::with_core_presets()
            .get("coding")
            .expect("coding preset must load")
    }

    #[test]
    fn manager_creates_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let sub = tmp.path().join("layouts");
        assert!(!sub.exists());
        let mgr = LayoutManager::new(&sub).unwrap();
        assert!(mgr.dir().exists());
    }

    #[test]
    fn save_load_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = LayoutManager::new(tmp.path()).unwrap();
        let layout = sample_layout();
        mgr.save(&layout).unwrap();
        let loaded = mgr.load(&layout.id).unwrap();
        assert_eq!(loaded, layout);
    }

    #[test]
    fn list_returns_summaries() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = LayoutManager::new(tmp.path()).unwrap();
        let registry = PresetRegistry::with_core_presets();
        let writing = registry.get("writing").unwrap();
        let coding = registry.get("coding").unwrap();
        mgr.save(&writing).unwrap();
        mgr.save(&coding).unwrap();
        let listed = mgr.list().unwrap();
        assert_eq!(listed.len(), 2);
        let names: Vec<_> = listed.iter().map(|s| s.name.clone()).collect();
        assert!(names.contains(&"Writing".to_string()));
        assert!(names.contains(&"Coding".to_string()));
    }

    #[test]
    fn delete_removes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = LayoutManager::new(tmp.path()).unwrap();
        let layout = sample_layout();
        mgr.save(&layout).unwrap();
        mgr.delete(&layout.id).unwrap();
        assert!(mgr.load(&layout.id).is_err());
    }

    #[test]
    fn delete_missing_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = LayoutManager::new(tmp.path()).unwrap();
        mgr.delete("ghost").unwrap();
    }

    #[test]
    fn list_skips_non_json_and_malformed() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = LayoutManager::new(tmp.path()).unwrap();
        std::fs::write(tmp.path().join("readme.txt"), "nope").unwrap();
        std::fs::write(tmp.path().join("bad.json"), "{ not valid").unwrap();
        let ok = sample_layout();
        mgr.save(&ok).unwrap();
        let listed = mgr.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, ok.id);
    }

    #[test]
    fn save_as_assigns_new_id_and_name() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = LayoutManager::new(tmp.path()).unwrap();
        let mut layout = WorkspaceLayout::default();
        let original_id = layout.id.clone();
        mgr.save_as("My Saved", &mut layout).unwrap();
        assert_ne!(layout.id, original_id);
        assert_eq!(layout.name, "My Saved");
        assert!(mgr.load(&layout.id).is_ok());
    }
}
