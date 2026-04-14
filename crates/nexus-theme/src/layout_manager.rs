//! Named-layout persistence + built-in presets (PRD §5.3, §18).
//!
//! [`LayoutManager`] wraps a filesystem directory of saved layouts, each
//! stored as a pretty-printed JSON file. IDs are file stems; the
//! [`WorkspaceLayout::id`](crate::WorkspaceLayout) field is canonical.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::layout::{
    BottomPanel, Direction, LayoutMetadata, LayoutNode, PaneId, Sidebar, SidebarPanel,
    SidebarSide, WorkspaceLayout,
};
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
        fs::create_dir_all(&layouts_dir)
            .map_err(|e| ThemeError::io(&layouts_dir, e))?;
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
        let entries = fs::read_dir(&self.layouts_dir)
            .map_err(|e| ThemeError::io(&self.layouts_dir, e))?;

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

// ---------------------------------------------------------------------------
// Presets (PRD §18)
// ---------------------------------------------------------------------------

impl WorkspaceLayout {
    /// "Writing" preset — single pane, all panels collapsed.
    #[must_use]
    pub fn preset_writing() -> Self {
        let pane_id = PaneId::new_leaf();
        Self {
            id: fresh_workspace_id(),
            name: "Writing".to_string(),
            version: "1.0".to_string(),
            root: LayoutNode::Leaf {
                id: pane_id.clone(),
                active_tab_id: None,
                tabs: Vec::new(),
                collapsed: false,
                min_size: None,
            },
            left_sidebar: Sidebar::collapsed(SidebarSide::Left),
            right_sidebar: Sidebar::collapsed(SidebarSide::Right),
            bottom_panel: BottomPanel::default(),
            focused_pane_id: Some(pane_id),
            metadata: LayoutMetadata::now(1920, 1080),
        }
    }

    /// "Reviewing" preset — row split of editor + preview with a comments
    /// panel in the right sidebar.
    #[must_use]
    pub fn preset_reviewing() -> Self {
        let left = PaneId::new_leaf();
        let right = PaneId::new_leaf();
        let root = LayoutNode::Split {
            id: PaneId::new_split(),
            direction: Direction::Row,
            children: vec![
                LayoutNode::Leaf {
                    id: left.clone(),
                    active_tab_id: None,
                    tabs: Vec::new(),
                    collapsed: false,
                    min_size: None,
                },
                LayoutNode::Leaf {
                    id: right,
                    active_tab_id: None,
                    tabs: Vec::new(),
                    collapsed: false,
                    min_size: None,
                },
            ],
            sizes: vec![0.5, 0.5],
        };

        let mut right_sidebar = Sidebar::collapsed(SidebarSide::Right);
        right_sidebar.collapsed = false;
        right_sidebar.width = 320;
        right_sidebar.panels.push(SidebarPanel {
            id: "comments".to_string(),
            title: "Comments".to_string(),
            icon: "message".to_string(),
            plugin: None,
            visible: true,
        });
        right_sidebar.panel_order = vec!["comments".to_string()];

        Self {
            id: fresh_workspace_id(),
            name: "Reviewing".to_string(),
            version: "1.0".to_string(),
            root,
            left_sidebar: Sidebar::collapsed(SidebarSide::Left),
            right_sidebar,
            bottom_panel: BottomPanel::default(),
            focused_pane_id: Some(left),
            metadata: LayoutMetadata::now(1920, 1080),
        }
    }

    /// "Coding" preset — explorer on the left, editor in the centre, a
    /// debugger on the right, and a terminal in the bottom panel.
    #[must_use]
    pub fn preset_coding() -> Self {
        let editor = PaneId::new_leaf();
        let root = LayoutNode::Leaf {
            id: editor.clone(),
            active_tab_id: None,
            tabs: Vec::new(),
            collapsed: false,
            min_size: None,
        };

        let mut left = Sidebar::collapsed(SidebarSide::Left);
        left.collapsed = false;
        left.panels.push(SidebarPanel {
            id: "explorer".to_string(),
            title: "Explorer".to_string(),
            icon: "folder".to_string(),
            plugin: None,
            visible: true,
        });
        left.panel_order = vec!["explorer".to_string()];

        let mut right = Sidebar::collapsed(SidebarSide::Right);
        right.collapsed = false;
        right.panels.push(SidebarPanel {
            id: "debugger".to_string(),
            title: "Debugger".to_string(),
            icon: "bug".to_string(),
            plugin: None,
            visible: true,
        });
        right.panel_order = vec!["debugger".to_string()];

        let bottom_panel = BottomPanel {
            height: 240,
            collapsed: false,
            tabs: Vec::new(),
        };

        Self {
            id: fresh_workspace_id(),
            name: "Coding".to_string(),
            version: "1.0".to_string(),
            root,
            left_sidebar: left,
            right_sidebar: right,
            bottom_panel,
            focused_pane_id: Some(editor),
            metadata: LayoutMetadata::now(1920, 1080),
        }
    }
}

fn fresh_workspace_id() -> String {
    format!("workspace-{}", uuid::Uuid::now_v7())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let layout = WorkspaceLayout::preset_coding();
        mgr.save(&layout).unwrap();
        let loaded = mgr.load(&layout.id).unwrap();
        assert_eq!(loaded, layout);
    }

    #[test]
    fn list_returns_summaries() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = LayoutManager::new(tmp.path()).unwrap();
        let writing = WorkspaceLayout::preset_writing();
        let coding = WorkspaceLayout::preset_coding();
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
        let layout = WorkspaceLayout::preset_writing();
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
        let ok = WorkspaceLayout::preset_writing();
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

    #[test]
    fn preset_writing_is_single_leaf() {
        let l = WorkspaceLayout::preset_writing();
        assert!(l.root.is_leaf());
        assert!(l.left_sidebar.collapsed);
        assert!(l.right_sidebar.collapsed);
        assert!(l.bottom_panel.collapsed);
    }

    #[test]
    fn preset_reviewing_has_row_split_and_right_sidebar() {
        let l = WorkspaceLayout::preset_reviewing();
        match &l.root {
            LayoutNode::Split {
                direction,
                children,
                ..
            } => {
                assert_eq!(*direction, Direction::Row);
                assert_eq!(children.len(), 2);
            }
            LayoutNode::Leaf { .. } => panic!("reviewing should be a split"),
        }
        assert!(!l.right_sidebar.collapsed);
        assert_eq!(l.right_sidebar.panels.len(), 1);
        assert_eq!(l.right_sidebar.panels[0].id, "comments");
    }

    #[test]
    fn preset_coding_has_both_sidebars_and_bottom() {
        let l = WorkspaceLayout::preset_coding();
        assert!(l.root.is_leaf());
        assert!(!l.left_sidebar.collapsed);
        assert!(!l.right_sidebar.collapsed);
        assert!(!l.bottom_panel.collapsed);
        assert_eq!(l.left_sidebar.panels[0].id, "explorer");
        assert_eq!(l.right_sidebar.panels[0].id, "debugger");
    }
}
