//! Workspace layout data model and tree manipulation API (PRD §5).
//!
//! A [`WorkspaceLayout`] holds the user's current arrangement of panes, tabs,
//! sidebars, and the bottom panel. The root is a recursive [`LayoutNode`]
//! tree: every inner node is a [`LayoutNode::Split`] with ordered children
//! and proportional sizes, every leaf is a [`LayoutNode::Leaf`] carrying a
//! tab list.
//!
//! The mutation API on [`WorkspaceLayout`] (see [`WorkspaceLayout::split_pane`]
//! etc.) never panics on valid ids — unknown ids return
//! [`ThemeError::PaneNotFound`] / [`ThemeError::TabNotFound`]. Invariants
//! preserved across all mutations:
//!
//! - A [`LayoutNode::Split`] always has ≥ 2 children. When removal would
//!   leave 1, the split is collapsed to that child.
//! - `children.len() == sizes.len()` for every split.
//! - Sum of `sizes` is normalised to approximately 1.0 after any removal.
//! - `focused_pane_id`, when set, references a leaf that still exists.
//! - `active_tab_id` on a leaf, when set, references a tab in that leaf's
//!   `tabs` list.
//!
//! Serialisation is JSON in camelCase matching the TypeScript interfaces in
//! PRD §5.1, with the `{ "type": "split" | "leaf" }` discriminator from §5.2.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Result, ThemeError};

/// Branded identifier for a pane (split or leaf). Serializes as a bare string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
pub struct PaneId(pub String);

impl PaneId {
    /// Generate a fresh pane id for a leaf.
    #[must_use]
    pub fn new_leaf() -> Self {
        Self(format!("pane-{}", uuid::Uuid::now_v7()))
    }

    /// Generate a fresh pane id for a split.
    #[must_use]
    pub fn new_split() -> Self {
        Self(format!("split-{}", uuid::Uuid::now_v7()))
    }

    /// Borrow the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for PaneId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for PaneId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Branded identifier for a tab. Serializes as a bare string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
pub struct TabId(pub String);

impl TabId {
    /// Generate a fresh tab id.
    #[must_use]
    pub fn new() -> Self {
        Self(format!("tab-{}", uuid::Uuid::now_v7()))
    }

    /// Borrow the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for TabId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<String> for TabId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for TabId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Orientation of a split node — horizontal (`Row`) or vertical (`Column`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Horizontal split: children laid left-to-right.
    Row,
    /// Vertical split: children stacked top-to-bottom.
    Column,
}

/// Which kind of content a tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    /// Code / markdown editor.
    Editor,
    /// Rendered preview (HTML, markdown, …).
    Preview,
    /// Terminal / PTY.
    Terminal,
    /// Side panel content (docs, chat, settings).
    Sidepanel,
    /// Plugin-defined custom surface.
    Custom,
}

/// A single tab inside a leaf pane or bottom panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    /// Stable tab id.
    pub id: TabId,
    /// Display label (file name, "Terminal", …).
    pub label: String,
    /// Optional icon id for the tab bar.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub icon: Option<String>,
    /// What kind of content this tab shows.
    pub surface: Surface,
    /// Pinned tabs are non-closable and render before unpinned ones.
    pub pinned: bool,
    /// Content locator (e.g. `file:///…`, `terminal://0`).
    pub content_type: String,
    /// Unsaved-changes indicator.
    pub is_dirty: bool,
}

/// One node in the workspace layout tree.
///
/// Serializes with a `"type"` discriminator matching the TypeScript union
/// in PRD §5.1 (`"split"` or `"leaf"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum LayoutNode {
    /// Internal node: two or more children arranged in [`Direction`].
    Split {
        /// Stable id for this split.
        id: PaneId,
        /// Orientation of the split.
        direction: Direction,
        /// Child nodes, left-to-right or top-to-bottom.
        children: Vec<LayoutNode>,
        /// Proportional sizes; `sizes[i]` is `children[i]`'s share of the
        /// parent axis. Sum normalised to ~1.0.
        sizes: Vec<f32>,
    },
    /// Leaf node: a tabbed pane.
    Leaf {
        /// Stable id for this leaf.
        id: PaneId,
        /// Currently-focused tab within this leaf.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        active_tab_id: Option<TabId>,
        /// Tabs in display order.
        #[serde(default)]
        tabs: Vec<Tab>,
        /// When true, the pane is hidden but its size in the layout is
        /// preserved (see PRD §6.4).
        #[serde(default)]
        collapsed: bool,
        /// Minimum size in pixels along the parent axis.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        min_size: Option<u32>,
    },
}

/// Subset of [`PaneNode`] fields exposed for convenience when the caller
/// only needs read access to a leaf's tabs.
///
/// (The PRD TypeScript models leaves as a separate `PaneNode` struct. In
/// Rust we encode them as an enum variant for ergonomics; this alias lets
/// integration code still pattern-match in PRD terms.)
pub type PaneNode = LayoutNode;

/// Which side of the window a side panel docks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum SidePanelSide {
    /// Left side of the window.
    Left,
    /// Right side of the window.
    Right,
}

/// One panel within a side panel (Explorer, Search, Outline, …).
///
/// A panel has three surfaces (matching the Obsidian reference):
///
/// 1. **Selector** in the side panel's panel-selector toolbar — toggles
///    visibility. Owned by the side panel.
/// 2. **Toolbar** ([`Self::toolbar`]) — panel-local action icons rendered
///    above the content. Owned by this panel's contributing plugin.
/// 3. **Content** ([`Self::content_type`]) — the React component registered
///    under that id in the UI contribution registry. Owned by this panel's
///    contributing plugin.
///
/// The data model only carries references; the UI contribution registry
/// (landing with §8 / §13) resolves icons, toolbar actions, and content
/// components at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Panel {
    /// Panel id (`"explorer"`, `"search"`, …).
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Icon id.
    pub icon: String,
    /// Owning plugin id, if the panel came from a plugin.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub plugin: Option<String>,
    /// Whether the panel is currently shown.
    pub visible: bool,
    /// Panel-local toolbar items rendered above the content area. Empty
    /// for panels that don't need controls (Explorer, Bookmarks, …).
    #[serde(default)]
    pub toolbar: Vec<PanelToolbarItem>,
    /// Content-component id resolved by the UI contribution registry.
    /// `None` for panels whose content is rendered by core (legacy) or
    /// that only exist as placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

/// One action icon in a panel's local toolbar (sort, filter, search,
/// view-mode, …).
///
/// Shape mirrors [`RibbonItem`]; the same [`RibbonAction`] variants apply
/// since clicking a toolbar icon is conceptually the same dispatch —
/// "toggle a sub-panel", "invoke a command", or "open a view".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PanelToolbarItem {
    /// Stable id (`"outline.collapse-all"`, `"tags.sort-asc"`, …).
    pub id: String,
    /// Icon id resolved by the UI.
    pub icon: String,
    /// Hover tooltip and accessible label.
    pub tooltip: String,
    /// What clicking the item does.
    pub action: RibbonAction,
    /// Owning plugin id. `None` for core.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub plugin: Option<String>,
}

/// A single icon button in a side panel's footer (right-aligned area —
/// help, settings, plugin-contributed actions).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FooterAction {
    /// Stable id (`"workspace.help"`, `"workspace.settings"`, …).
    pub id: String,
    /// Icon id resolved by the UI.
    pub icon: String,
    /// Hover tooltip and accessible label.
    pub tooltip: String,
    /// What clicking the item does.
    pub action: RibbonAction,
    /// Owning plugin id. `None` for core.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub plugin: Option<String>,
}

/// Floating status bar entry rendered at the bottom-right of the
/// workspace. Mixes counters (`text` only), icon-only actions (`icon` +
/// `action`), and combined items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StatusBarItem {
    /// Stable id (`"editor.word-count"`, `"sync.status"`, …).
    pub id: String,
    /// Text shown to the right of the icon, if any (`"2,348 words"`).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub text: Option<String>,
    /// Icon id, if any.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub icon: Option<String>,
    /// Click action. `None` makes the item non-interactive (counter).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub action: Option<RibbonAction>,
    /// Owning plugin id. `None` for core.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub plugin: Option<String>,
}

/// Footer row pinned to the bottom of a side panel. Typically only the
/// left side panel carries one (forge switcher + help + settings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SidePanelFooter {
    /// Show the forge / workspace switcher on the left of the footer
    /// row. The active forge name is filled in at runtime — the preset
    /// just opts in.
    #[serde(default)]
    pub show_forge_selector: bool,
    /// Right-aligned action icons (core ships help + settings; plugins
    /// can append more).
    #[serde(default)]
    pub actions: Vec<FooterAction>,
}

/// What a ribbon icon does when clicked.
///
/// Ribbon items are references: `command` / `view_id` are resolved at runtime
/// through a UI contribution registry that core and plugins populate. The
/// data model never owns the *meaning* of an id — only the list of items to
/// show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RibbonAction {
    /// Toggle the panel with this id inside the side panel whose
    /// selector toolbar (or the top-level ribbon) dispatched this action.
    TogglePanel {
        /// Target panel id (matches [`Panel::id`]).
        #[serde(rename = "panelId")]
        panel_id: String,
    },
    /// Invoke a named command registered in the UI contribution registry.
    InvokeCommand {
        /// Command id (`"workspace.new-note"`, `"git.commit"`, …).
        command: String,
    },
    /// Open a registered view (editor, canvas, graph, …) in the focused pane.
    OpenView {
        /// View id registered by core or a plugin.
        #[serde(rename = "viewId")]
        view_id: String,
    },
}

/// A single icon on the workspace's activity ribbon (the narrow icon rail
/// docked at the far-left edge of the window, independent of either side
/// panel).
///
/// Ribbon items are just references — the icon to render, the tooltip to
/// show, and the action to dispatch. The actual implementation lives in the
/// UI contribution registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RibbonItem {
    /// Stable id of this ribbon entry (`"explorer"`, `"git.status"`, …).
    pub id: String,
    /// Icon id resolved by the UI.
    pub icon: String,
    /// Hover tooltip and accessible label.
    pub tooltip: String,
    /// What clicking the item does.
    pub action: RibbonAction,
    /// Owning plugin id, if the item came from a plugin. `None` for core.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub plugin: Option<String>,
}

/// A docked side panel (left or right).
///
/// A side panel has three stacked surfaces:
///
///   1. **Panel-selector toolbar** — derived from [`Self::panels`]; rendered
///      horizontally at the top. Clicking a selector toggles that panel's
///      [`Panel::visible`] flag.
///   2. **Panel-local toolbar** — [`Panel::toolbar`] of the active panel.
///   3. **Content** — [`Panel::content_type`] of the active panel.
///
/// The activity ribbon is *not* part of the side panel — it lives on
/// [`WorkspaceLayout::ribbon`] as a workspace-level concern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SidePanel {
    /// Which side this side panel lives on.
    pub side: SidePanelSide,
    /// Width in pixels when expanded.
    pub width: u32,
    /// Side panel is fully hidden.
    pub collapsed: bool,
    /// Show icons only (no titles).
    pub mini_mode: bool,
    /// Registered panels. Each contributes one entry to the top selector
    /// toolbar and, when active, its own [`Panel::toolbar`] + content.
    pub panels: Vec<Panel>,
    /// Panel ids in display order.
    pub panel_order: Vec<String>,
    /// Optional footer pinned at the bottom (forge selector + actions).
    /// Typically only set on the left side panel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer: Option<SidePanelFooter>,
}

impl SidePanel {
    /// Empty side panel on the given side, default width 280 px, collapsed.
    #[must_use]
    pub fn collapsed(side: SidePanelSide) -> Self {
        Self {
            side,
            width: 280,
            collapsed: true,
            mini_mode: false,
            panels: Vec::new(),
            panel_order: Vec::new(),
            footer: None,
        }
    }
}

/// Bottom panel (terminal, process manager, diagnostics).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BottomPanel {
    /// Height in pixels.
    pub height: u32,
    /// Whether the panel is hidden.
    pub collapsed: bool,
    /// Tabs inside the panel (terminal, logs, etc.).
    pub tabs: Vec<Tab>,
}

impl Default for BottomPanel {
    fn default() -> Self {
        Self {
            height: 200,
            collapsed: true,
            tabs: Vec::new(),
        }
    }
}

/// Discovery metadata for the layout file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LayoutMetadata {
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// ISO 8601 last-modified timestamp; bumped on every mutation.
    pub last_modified: String,
    /// Window width at last save.
    pub width: u32,
    /// Window height at last save.
    pub height: u32,
}

impl LayoutMetadata {
    /// Fresh metadata with both timestamps set to "now".
    #[must_use]
    pub fn now(width: u32, height: u32) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            created_at: now.clone(),
            last_modified: now,
            width,
            height,
        }
    }
}

/// Top-level workspace layout: root tree + side panels + bottom panel + ribbon + metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLayout {
    /// Layout id (stable across saves).
    pub id: String,
    /// User-visible name (e.g. "Coding", "Writing").
    pub name: String,
    /// Schema version (currently `"1.0"`).
    pub version: String,
    /// Root of the pane tree.
    pub root: LayoutNode,
    /// Workspace activity ribbon (the far-left vertical rail). Contains
    /// plugin/view shortcuts (graph, calendar, terminal, …), *not* side
    /// panel selectors — those are derived from each side panel's own
    /// [`Panel`] list.
    #[serde(default)]
    pub ribbon: Vec<RibbonItem>,
    /// Floating status-bar items rendered at the bottom-right of the
    /// workspace. Counters, sync status, plugin-contributed badges, etc.
    #[serde(default)]
    pub status_bar: Vec<StatusBarItem>,
    /// Left dock.
    pub left_side_panel: SidePanel,
    /// Right dock.
    pub right_side_panel: SidePanel,
    /// Bottom panel.
    pub bottom_panel: BottomPanel,
    /// Id of the pane receiving keyboard focus.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub focused_pane_id: Option<PaneId>,
    /// Discovery metadata.
    pub metadata: LayoutMetadata,
}

impl Default for WorkspaceLayout {
    fn default() -> Self {
        let root_id = PaneId::new_leaf();
        let root = LayoutNode::Leaf {
            id: root_id.clone(),
            active_tab_id: None,
            tabs: Vec::new(),
            collapsed: false,
            min_size: None,
        };
        Self {
            id: format!("workspace-{}", uuid::Uuid::now_v7()),
            name: "Default".to_string(),
            version: "1.0".to_string(),
            root,
            ribbon: Vec::new(),
            status_bar: Vec::new(),
            left_side_panel: SidePanel::collapsed(SidePanelSide::Left),
            right_side_panel: SidePanel::collapsed(SidePanelSide::Right),
            bottom_panel: BottomPanel::default(),
            focused_pane_id: Some(root_id),
            metadata: LayoutMetadata::now(1920, 1080),
        }
    }
}

// ---------------------------------------------------------------------------
// Tree helpers (node traversal, not mutation API)
// ---------------------------------------------------------------------------

impl LayoutNode {
    /// Id of this node (split or leaf).
    #[must_use]
    pub fn id(&self) -> &PaneId {
        match self {
            Self::Split { id, .. } | Self::Leaf { id, .. } => id,
        }
    }

    /// `true` if this node is a leaf.
    #[must_use]
    pub fn is_leaf(&self) -> bool {
        matches!(self, Self::Leaf { .. })
    }

    /// Find a leaf by id, read-only.
    #[must_use]
    pub fn find_leaf(&self, pane_id: &PaneId) -> Option<&Self> {
        match self {
            Self::Leaf { id, .. } if id == pane_id => Some(self),
            Self::Leaf { .. } => None,
            Self::Split { children, .. } => children.iter().find_map(|c| c.find_leaf(pane_id)),
        }
    }

    /// Find a leaf by id, mutable.
    pub fn find_leaf_mut(&mut self, pane_id: &PaneId) -> Option<&mut Self> {
        match self {
            Self::Leaf { id, .. } if id == pane_id => Some(self),
            Self::Leaf { .. } => None,
            Self::Split { children, .. } => {
                children.iter_mut().find_map(|c| c.find_leaf_mut(pane_id))
            }
        }
    }

    /// Find the leaf containing the tab with `tab_id`, mutable.
    pub fn find_leaf_with_tab_mut(&mut self, tab_id: &TabId) -> Option<&mut Self> {
        match self {
            Self::Leaf { tabs, .. } if tabs.iter().any(|t| &t.id == tab_id) => Some(self),
            Self::Leaf { .. } => None,
            Self::Split { children, .. } => children
                .iter_mut()
                .find_map(|c| c.find_leaf_with_tab_mut(tab_id)),
        }
    }

    /// Find a split by id, mutable.
    pub fn find_split_mut(&mut self, split_id: &PaneId) -> Option<&mut Self> {
        match self {
            Self::Leaf { .. } => None,
            Self::Split { id, .. } if id == split_id => Some(self),
            Self::Split { children, .. } => {
                children.iter_mut().find_map(|c| c.find_split_mut(split_id))
            }
        }
    }

    /// Run `f` on this node and every descendant.
    pub fn visit<F: FnMut(&Self)>(&self, f: &mut F) {
        f(self);
        if let Self::Split { children, .. } = self {
            for c in children {
                c.visit(f);
            }
        }
    }

    /// Collect every leaf's pane id, in traversal order.
    #[must_use]
    pub fn leaf_ids(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.visit(&mut |n| {
            if let Self::Leaf { id, .. } = n {
                out.push(id.clone());
            }
        });
        out
    }
}

// ---------------------------------------------------------------------------
// Mutation API (PRD §5.3)
// ---------------------------------------------------------------------------

impl WorkspaceLayout {
    /// Split the leaf `pane_id` in two along `direction`, inserting a fresh
    /// empty sibling leaf on the right/bottom. Returns the new pane's id.
    ///
    /// # Errors
    /// Returns [`ThemeError::PaneNotFound`] if `pane_id` doesn't identify a
    /// leaf anywhere in the tree, or [`ThemeError::NodeKindMismatch`] if it
    /// identifies a split instead.
    pub fn split_pane(&mut self, pane_id: &PaneId, direction: Direction) -> Result<PaneId> {
        if let Some(node) = self.root.find_leaf(pane_id) {
            debug_assert!(node.is_leaf());
        } else {
            // Distinguish "no such id" from "exists but is a split".
            return Err(match self.root.find_split_mut(pane_id) {
                Some(_) => ThemeError::NodeKindMismatch {
                    id: pane_id.0.clone(),
                    expected: "leaf",
                    actual: "split",
                },
                None => ThemeError::PaneNotFound(pane_id.0.clone()),
            });
        }

        let new_pane_id = PaneId::new_leaf();
        let new_leaf = Self::fresh_leaf(new_pane_id.clone());

        let placeholder = Self::placeholder_leaf();
        let old_root = std::mem::replace(&mut self.root, placeholder);
        let (new_root, carry) = split_in_subtree(old_root, pane_id, direction, new_leaf);
        debug_assert!(carry.is_none(), "split target should have been found");
        self.root = new_root;
        self.touch();
        Ok(new_pane_id)
    }

    /// Close the leaf with `pane_id`. Collapses any resulting 1-child split
    /// into its remaining child.
    ///
    /// # Errors
    /// Returns [`ThemeError::PaneNotFound`] if `pane_id` doesn't match any
    /// leaf, or an error if removing would leave the tree empty — callers
    /// must keep at least one leaf.
    pub fn close_pane(&mut self, pane_id: &PaneId) -> Result<()> {
        let placeholder = Self::placeholder_leaf();
        let old_root = std::mem::replace(&mut self.root, placeholder);
        let (new_root, found) = rewrite_remove(old_root, pane_id);

        if !found {
            // Restore and report.
            self.root = new_root.unwrap_or_else(Self::placeholder_leaf);
            return Err(ThemeError::PaneNotFound(pane_id.0.clone()));
        }

        let new_root = new_root.unwrap_or_else(|| {
            // Removing the last leaf — keep one empty leaf so the layout
            // never degenerates.
            Self::fresh_leaf(PaneId::new_leaf())
        });

        // Clear focus if it pointed at the removed pane.
        if self.focused_pane_id.as_ref() == Some(pane_id) {
            self.focused_pane_id = new_root.leaf_ids().into_iter().next();
        }
        self.root = new_root;
        self.touch();
        Ok(())
    }

    /// Update the proportional sizes of a split node in place.
    ///
    /// # Errors
    /// Returns [`ThemeError::PaneNotFound`] if `split_id` is unknown,
    /// [`ThemeError::NodeKindMismatch`] if it identifies a leaf, or
    /// [`ThemeError::InvalidSplitSizes`] if the length or sum is wrong.
    pub fn set_split_sizes(&mut self, split_id: &PaneId, sizes: Vec<f32>) -> Result<()> {
        // Disambiguate id kind first so we return a precise error for leaves.
        if self.root.find_split_mut(split_id).is_none() {
            return Err(if self.root.find_leaf(split_id).is_some() {
                ThemeError::NodeKindMismatch {
                    id: split_id.0.clone(),
                    expected: "split",
                    actual: "leaf",
                }
            } else {
                ThemeError::PaneNotFound(split_id.0.clone())
            });
        }

        for s in &sizes {
            if !s.is_finite() || *s < 0.0 {
                return Err(ThemeError::InvalidSplitSizes {
                    children: sizes.len(),
                    sizes,
                    reason: "each size must be finite and ≥ 0",
                });
            }
        }

        let Some(LayoutNode::Split {
            children,
            sizes: dest,
            ..
        }) = self.root.find_split_mut(split_id)
        else {
            unreachable!("checked above");
        };

        if sizes.len() != children.len() {
            let n = children.len();
            return Err(ThemeError::InvalidSplitSizes {
                children: n,
                sizes,
                reason: "sizes length must equal children length",
            });
        }

        let sum: f32 = sizes.iter().sum();
        if !(0.99..=1.01).contains(&sum) {
            let n = children.len();
            return Err(ThemeError::InvalidSplitSizes {
                children: n,
                sizes,
                reason: "sizes must sum to ~1.0",
            });
        }

        *dest = sizes;
        self.touch();
        Ok(())
    }

    /// Append `tab` to the leaf `pane_id`, make it the active tab, and
    /// return its id.
    ///
    /// # Errors
    /// Returns [`ThemeError::PaneNotFound`] if `pane_id` isn't a leaf.
    pub fn add_tab(&mut self, pane_id: &PaneId, tab: Tab) -> Result<TabId> {
        let tab_id = tab.id.clone();
        match self.root.find_leaf_mut(pane_id) {
            Some(LayoutNode::Leaf {
                tabs,
                active_tab_id,
                ..
            }) => {
                tabs.push(tab);
                *active_tab_id = Some(tab_id.clone());
                self.touch();
                Ok(tab_id)
            }
            Some(LayoutNode::Split { .. }) | None => {
                Err(ThemeError::PaneNotFound(pane_id.0.clone()))
            }
        }
    }

    /// Close the tab with `tab_id` wherever it lives.
    ///
    /// If it was the active tab, the neighbour to its left becomes active
    /// (or the right if it was the first tab).
    ///
    /// # Errors
    /// Returns [`ThemeError::TabNotFound`] if no leaf contains the tab.
    pub fn close_tab(&mut self, tab_id: &TabId) -> Result<()> {
        let leaf = self
            .root
            .find_leaf_with_tab_mut(tab_id)
            .ok_or_else(|| ThemeError::TabNotFound(tab_id.0.clone()))?;

        let LayoutNode::Leaf {
            tabs,
            active_tab_id,
            ..
        } = leaf
        else {
            unreachable!("find_leaf_with_tab_mut always returns a Leaf");
        };

        let Some(idx) = tabs.iter().position(|t| &t.id == tab_id) else {
            return Err(ThemeError::TabNotFound(tab_id.0.clone()));
        };
        tabs.remove(idx);

        if active_tab_id.as_ref() == Some(tab_id) {
            *active_tab_id = if tabs.is_empty() {
                None
            } else {
                let new_idx = idx.saturating_sub(1).min(tabs.len() - 1);
                Some(tabs[new_idx].id.clone())
            };
        }
        self.touch();
        Ok(())
    }

    /// Set the focused pane. The pane must exist as a leaf.
    ///
    /// # Errors
    /// Returns [`ThemeError::PaneNotFound`] if `pane_id` isn't a leaf.
    pub fn focus_pane(&mut self, pane_id: &PaneId) -> Result<()> {
        if self.root.find_leaf(pane_id).is_none() {
            return Err(ThemeError::PaneNotFound(pane_id.0.clone()));
        }
        self.focused_pane_id = Some(pane_id.clone());
        self.touch();
        Ok(())
    }

    /// Focus the tab with `tab_id`: sets the containing leaf's active tab
    /// and focuses that leaf.
    ///
    /// # Errors
    /// Returns [`ThemeError::TabNotFound`] if no leaf contains the tab.
    pub fn focus_tab(&mut self, tab_id: &TabId) -> Result<()> {
        let leaf = self
            .root
            .find_leaf_with_tab_mut(tab_id)
            .ok_or_else(|| ThemeError::TabNotFound(tab_id.0.clone()))?;
        let LayoutNode::Leaf {
            id, active_tab_id, ..
        } = leaf
        else {
            unreachable!();
        };
        *active_tab_id = Some(tab_id.clone());
        let focused = id.clone();
        self.focused_pane_id = Some(focused);
        self.touch();
        Ok(())
    }

    /// Collapse / expand the side panel on `side`.
    pub fn collapse_side_panel(&mut self, side: SidePanelSide, collapsed: bool) {
        self.side_panel_mut(side).collapsed = collapsed;
        self.touch();
    }

    /// Toggle mini-mode (icon-only) on a side panel.
    pub fn set_mini_mode(&mut self, side: SidePanelSide, enabled: bool) {
        self.side_panel_mut(side).mini_mode = enabled;
        self.touch();
    }

    /// Resize the bottom panel.
    pub fn resize_bottom_panel(&mut self, height: u32) {
        self.bottom_panel.height = height;
        self.touch();
    }

    fn side_panel_mut(&mut self, side: SidePanelSide) -> &mut SidePanel {
        match side {
            SidePanelSide::Left => &mut self.left_side_panel,
            SidePanelSide::Right => &mut self.right_side_panel,
        }
    }

    fn touch(&mut self) {
        self.metadata.last_modified = chrono::Utc::now().to_rfc3339();
    }

    fn fresh_leaf(id: PaneId) -> LayoutNode {
        LayoutNode::Leaf {
            id,
            active_tab_id: None,
            tabs: Vec::new(),
            collapsed: false,
            min_size: None,
        }
    }

    fn placeholder_leaf() -> LayoutNode {
        LayoutNode::Leaf {
            id: PaneId("__placeholder__".to_string()),
            active_tab_id: None,
            tabs: Vec::new(),
            collapsed: false,
            min_size: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

impl WorkspaceLayout {
    /// Serialize to pretty-printed JSON.
    ///
    /// # Errors
    /// Propagates [`serde_json`] encoding errors — effectively impossible
    /// for well-formed `WorkspaceLayout` values.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|e| ThemeError::LayoutJson {
            path: PathBuf::new(),
            source: e,
        })
    }

    /// Parse JSON into a [`WorkspaceLayout`].
    ///
    /// # Errors
    /// Returns [`ThemeError::LayoutJson`] on parse failure.
    pub fn from_json(src: &str) -> Result<Self> {
        serde_json::from_str(src).map_err(|e| ThemeError::LayoutJson {
            path: PathBuf::new(),
            source: e,
        })
    }

    /// Write the layout as pretty JSON to `path`.
    ///
    /// # Errors
    /// Returns [`ThemeError::Io`] on write failure, or
    /// [`ThemeError::LayoutJson`] on serialize failure.
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let json = serde_json::to_string_pretty(self).map_err(|e| ThemeError::LayoutJson {
            path: path.to_path_buf(),
            source: e,
        })?;
        fs::write(path, json).map_err(|e| ThemeError::io(path, e))
    }

    /// Read a layout from a JSON file.
    ///
    /// # Errors
    /// Returns [`ThemeError::Io`] on read failure or
    /// [`ThemeError::LayoutJson`] on parse failure.
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let src = fs::read_to_string(path).map_err(|e| ThemeError::io(path, e))?;
        serde_json::from_str(&src).map_err(|e| ThemeError::LayoutJson {
            path: path.to_path_buf(),
            source: e,
        })
    }
}

// ---------------------------------------------------------------------------
// Recursive rewrite helpers
// ---------------------------------------------------------------------------

/// Walk `node` looking for the leaf `target`; if found, wrap it in a new
/// [`LayoutNode::Split`] alongside `new_leaf` in the given direction.
///
/// Returns `(rewritten_tree, carry)` where `carry` is `Some(new_leaf)` if
/// the target was NOT found (caller gets the leaf back unused) and `None`
/// if the split was applied.
fn split_in_subtree(
    node: LayoutNode,
    target: &PaneId,
    direction: Direction,
    new_leaf: LayoutNode,
) -> (LayoutNode, Option<LayoutNode>) {
    match node {
        LayoutNode::Leaf {
            id,
            active_tab_id,
            tabs,
            collapsed,
            min_size,
        } if &id == target => {
            let leaf = LayoutNode::Leaf {
                id,
                active_tab_id,
                tabs,
                collapsed,
                min_size,
            };
            let split = LayoutNode::Split {
                id: PaneId::new_split(),
                direction,
                children: vec![leaf, new_leaf],
                sizes: vec![0.5, 0.5],
            };
            (split, None)
        }
        LayoutNode::Leaf { .. } => (node, Some(new_leaf)),
        LayoutNode::Split {
            id,
            direction: dir,
            children,
            sizes,
        } => {
            let mut carry = Some(new_leaf);
            let mut new_children = Vec::with_capacity(children.len());
            for child in children {
                if let Some(nl) = carry.take() {
                    let (c, returned) = split_in_subtree(child, target, direction, nl);
                    new_children.push(c);
                    carry = returned;
                } else {
                    new_children.push(child);
                }
            }
            (
                LayoutNode::Split {
                    id,
                    direction: dir,
                    children: new_children,
                    sizes,
                },
                carry,
            )
        }
    }
}

/// Walk `node` and remove the leaf with `target` id if present. Returns
/// `(replacement, found)` where `replacement` is `None` iff the whole
/// subtree should be removed from its parent.
fn rewrite_remove(node: LayoutNode, target: &PaneId) -> (Option<LayoutNode>, bool) {
    match node {
        LayoutNode::Leaf { ref id, .. } if id == target => (None, true),
        LayoutNode::Leaf { .. } => (Some(node), false),
        LayoutNode::Split {
            id,
            direction,
            children,
            sizes,
        } => {
            let mut new_children: Vec<LayoutNode> = Vec::with_capacity(children.len());
            let mut new_sizes: Vec<f32> = Vec::with_capacity(sizes.len());
            let mut found = false;
            for (i, child) in children.into_iter().enumerate() {
                if found {
                    new_children.push(child);
                    if let Some(s) = sizes.get(i) {
                        new_sizes.push(*s);
                    }
                    continue;
                }
                let (maybe_child, f) = rewrite_remove(child, target);
                found = found || f;
                if let Some(c) = maybe_child {
                    new_children.push(c);
                    if let Some(s) = sizes.get(i) {
                        new_sizes.push(*s);
                    }
                }
            }

            let result = match new_children.len() {
                0 => None,
                1 => Some(
                    new_children
                        .into_iter()
                        .next()
                        .expect("len() == 1 verified by match arm"),
                ),
                _ => {
                    renormalize_sizes(&mut new_sizes);
                    Some(LayoutNode::Split {
                        id,
                        direction,
                        children: new_children,
                        sizes: new_sizes,
                    })
                }
            };
            (result, found)
        }
    }
}

/// Normalise `sizes` in place so they sum to 1.0. If the sum is 0 (or NaN),
/// replaces with an even split.
fn renormalize_sizes(sizes: &mut [f32]) {
    if sizes.is_empty() {
        return;
    }
    let sum: f32 = sizes.iter().sum();
    if sum.is_finite() && sum > 0.0 {
        for s in sizes.iter_mut() {
            *s /= sum;
        }
    } else {
        #[allow(clippy::cast_precision_loss)]
        let even = 1.0 / sizes.len() as f32;
        for s in sizes.iter_mut() {
            *s = even;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tab(label: &str) -> Tab {
        Tab {
            id: TabId::new(),
            label: label.to_string(),
            icon: None,
            surface: Surface::Editor,
            pinned: false,
            content_type: format!("file:///tmp/{label}"),
            is_dirty: false,
        }
    }

    #[test]
    fn default_layout_has_single_focused_leaf() {
        let layout = WorkspaceLayout::default();
        assert!(layout.root.is_leaf());
        assert!(layout.focused_pane_id.is_some());
        assert_eq!(layout.left_side_panel.side, SidePanelSide::Left);
        assert_eq!(layout.right_side_panel.side, SidePanelSide::Right);
        assert!(layout.left_side_panel.collapsed);
        assert!(layout.bottom_panel.collapsed);
    }

    #[test]
    fn json_roundtrip_preserves_structure() {
        let mut layout = WorkspaceLayout::default();
        let root_id = layout.root.id().clone();
        let tab = sample_tab("main.rs");
        layout.add_tab(&root_id, tab).unwrap();
        layout.split_pane(&root_id, Direction::Row).unwrap();

        let json = layout.to_json().unwrap();
        let decoded = WorkspaceLayout::from_json(&json).unwrap();
        assert_eq!(layout, decoded);
    }

    #[test]
    fn json_uses_camel_case_and_type_discriminator() {
        let layout = WorkspaceLayout::default();
        let json = layout.to_json().unwrap();
        assert!(json.contains("\"type\": \"leaf\""));
        assert!(json.contains("\"leftSidePanel\""));
        assert!(json.contains("\"bottomPanel\""));
    }

    #[test]
    fn split_pane_creates_split_with_two_leaves() {
        let mut layout = WorkspaceLayout::default();
        let root_id = layout.root.id().clone();
        let new_id = layout.split_pane(&root_id, Direction::Row).unwrap();

        match &layout.root {
            LayoutNode::Split {
                direction,
                children,
                sizes,
                ..
            } => {
                assert_eq!(*direction, Direction::Row);
                assert_eq!(children.len(), 2);
                assert_eq!(sizes, &vec![0.5, 0.5]);
                let ids: Vec<_> = children.iter().map(|c| c.id().clone()).collect();
                assert!(ids.contains(&root_id));
                assert!(ids.contains(&new_id));
            }
            LayoutNode::Leaf { .. } => panic!("root should now be a split"),
        }
    }

    #[test]
    fn split_pane_on_unknown_id_errors() {
        let mut layout = WorkspaceLayout::default();
        let err = layout
            .split_pane(&PaneId::from("nonexistent"), Direction::Row)
            .unwrap_err();
        assert!(matches!(err, ThemeError::PaneNotFound(_)));
    }

    #[test]
    fn close_pane_collapses_single_child_split() {
        let mut layout = WorkspaceLayout::default();
        let root_id = layout.root.id().clone();
        let other = layout.split_pane(&root_id, Direction::Column).unwrap();
        // After split, root is a split with 2 children. Close one.
        layout.close_pane(&other).unwrap();
        // Root should be back to a single leaf.
        assert!(layout.root.is_leaf());
        assert_eq!(layout.root.id(), &root_id);
    }

    #[test]
    fn close_pane_never_produces_empty_tree() {
        let mut layout = WorkspaceLayout::default();
        let only = layout.root.id().clone();
        // Closing the lone leaf should replace it with a fresh empty leaf.
        layout.close_pane(&only).unwrap();
        assert!(layout.root.is_leaf());
        assert_ne!(layout.root.id(), &only);
    }

    #[test]
    fn close_pane_unknown_errors() {
        let mut layout = WorkspaceLayout::default();
        let err = layout.close_pane(&PaneId::from("ghost")).unwrap_err();
        assert!(matches!(err, ThemeError::PaneNotFound(_)));
    }

    #[test]
    fn add_and_close_tab() {
        let mut layout = WorkspaceLayout::default();
        let pane = layout.root.id().clone();
        let t1 = sample_tab("a");
        let t2 = sample_tab("b");
        let id1 = t1.id.clone();
        let id2 = t2.id.clone();
        layout.add_tab(&pane, t1).unwrap();
        layout.add_tab(&pane, t2).unwrap();

        if let LayoutNode::Leaf {
            tabs,
            active_tab_id,
            ..
        } = &layout.root
        {
            assert_eq!(tabs.len(), 2);
            assert_eq!(active_tab_id.as_ref(), Some(&id2));
        } else {
            panic!();
        }

        layout.close_tab(&id2).unwrap();
        if let LayoutNode::Leaf {
            tabs,
            active_tab_id,
            ..
        } = &layout.root
        {
            assert_eq!(tabs.len(), 1);
            // Active tab should have fallen back to the remaining one.
            assert_eq!(active_tab_id.as_ref(), Some(&id1));
        }
    }

    #[test]
    fn close_unknown_tab_errors() {
        let mut layout = WorkspaceLayout::default();
        let err = layout.close_tab(&TabId::from("no-such")).unwrap_err();
        assert!(matches!(err, ThemeError::TabNotFound(_)));
    }

    #[test]
    fn set_split_sizes_validates_length_and_sum() {
        let mut layout = WorkspaceLayout::default();
        let root_leaf = layout.root.id().clone();
        layout.split_pane(&root_leaf, Direction::Row).unwrap();
        let split_id = layout.root.id().clone();

        layout.set_split_sizes(&split_id, vec![0.3, 0.7]).unwrap();
        match &layout.root {
            LayoutNode::Split { sizes, .. } => {
                assert_eq!(sizes, &vec![0.3, 0.7]);
            }
            LayoutNode::Leaf { .. } => panic!(),
        }

        let err = layout.set_split_sizes(&split_id, vec![0.5]).unwrap_err();
        assert!(matches!(err, ThemeError::InvalidSplitSizes { .. }));

        let err = layout
            .set_split_sizes(&split_id, vec![0.2, 0.2])
            .unwrap_err();
        assert!(matches!(err, ThemeError::InvalidSplitSizes { .. }));

        let err = layout
            .set_split_sizes(&root_leaf, vec![0.5, 0.5])
            .unwrap_err();
        assert!(matches!(err, ThemeError::NodeKindMismatch { .. }));
    }

    #[test]
    fn focus_pane_requires_leaf() {
        let mut layout = WorkspaceLayout::default();
        let leaf = layout.root.id().clone();
        layout.focus_pane(&leaf).unwrap();
        assert_eq!(layout.focused_pane_id.as_ref(), Some(&leaf));

        let err = layout.focus_pane(&PaneId::from("nope")).unwrap_err();
        assert!(matches!(err, ThemeError::PaneNotFound(_)));
    }

    #[test]
    fn focus_tab_updates_pane_and_tab() {
        let mut layout = WorkspaceLayout::default();
        let pane = layout.root.id().clone();
        let tab = sample_tab("x");
        let tab_id = tab.id.clone();
        layout.add_tab(&pane, tab).unwrap();
        // Add a second tab so the first isn't automatically active.
        let other = sample_tab("y");
        layout.add_tab(&pane, other).unwrap();

        layout.focus_tab(&tab_id).unwrap();
        if let LayoutNode::Leaf { active_tab_id, .. } = &layout.root {
            assert_eq!(active_tab_id.as_ref(), Some(&tab_id));
        }
        assert_eq!(layout.focused_pane_id.as_ref(), Some(&pane));
    }

    #[test]
    fn side_panel_and_bottom_panel_mutations() {
        let mut layout = WorkspaceLayout::default();
        layout.collapse_side_panel(SidePanelSide::Left, false);
        layout.set_mini_mode(SidePanelSide::Left, true);
        layout.resize_bottom_panel(300);
        assert!(!layout.left_side_panel.collapsed);
        assert!(layout.left_side_panel.mini_mode);
        assert_eq!(layout.bottom_panel.height, 300);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let layout = WorkspaceLayout::default();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        layout.save_to_file(tmp.path()).unwrap();
        let loaded = WorkspaceLayout::load_from_file(tmp.path()).unwrap();
        assert_eq!(layout, loaded);
    }

    #[test]
    fn close_pane_removes_nested_leaf() {
        let mut layout = WorkspaceLayout::default();
        let a = layout.root.id().clone();
        let b = layout.split_pane(&a, Direction::Row).unwrap();
        let c = layout.split_pane(&b, Direction::Column).unwrap();
        // Tree: Split row { a, Split col { b, c } }
        layout.close_pane(&c).unwrap();
        // The inner col-split collapses to just b. Outer row-split still has a, b.
        match &layout.root {
            LayoutNode::Split { children, .. } => {
                assert_eq!(children.len(), 2);
                let ids: Vec<_> = children.iter().map(|c| c.id().clone()).collect();
                assert!(ids.contains(&a));
                assert!(ids.contains(&b));
            }
            LayoutNode::Leaf { .. } => panic!("root should still be a split"),
        }
    }

    #[test]
    fn renormalize_handles_degenerate_sum() {
        let mut sizes = vec![0.0, 0.0];
        renormalize_sizes(&mut sizes);
        assert_eq!(sizes, vec![0.5, 0.5]);
    }

    #[test]
    fn find_helpers_traverse_recursively() {
        let mut layout = WorkspaceLayout::default();
        let a = layout.root.id().clone();
        let b = layout.split_pane(&a, Direction::Row).unwrap();
        assert!(layout.root.find_leaf(&a).is_some());
        assert!(layout.root.find_leaf(&b).is_some());
        assert!(layout.root.find_leaf(&PaneId::from("none")).is_none());
        // Root is now a split.
        let split_id = layout.root.id().clone();
        assert!(layout.root.find_split_mut(&split_id).is_some());
        assert!(layout.root.find_split_mut(&a).is_none());
    }
}
