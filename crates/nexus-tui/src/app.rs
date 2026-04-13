//! Application state for nexus-tui.
//!
//! Defines the top-level [`TuiApp`] struct and all supporting state types:
//! [`TreeState`], [`ViewerState`], [`SearchState`], [`FindState`].

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use nexus_storage::{
    BacklinkResult, FileFilter, SearchResult, StorageConfig, StorageEngine, TaskFilter, TaskRecord,
};
use ratatui::widgets::ListState;

// ── Mode / Focus ──────────────────────────────────────────────────────────────

/// Current input mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Normal navigation mode.
    Normal,
    /// Full-text search overlay.
    Search,
    /// In-file find bar.
    Find,
}

/// Which pane has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// Left-hand file tree.
    FileTree,
    /// Right-hand file viewer.
    Viewer,
}

// ── TreeEntry ─────────────────────────────────────────────────────────────────

/// One row in the file tree (file or directory).
#[derive(Debug, Clone)]
pub struct TreeEntry {
    /// Vault-relative path (e.g. `"notes/hello.md"` or `"notes"`).
    pub path: String,
    /// Display name (last component).
    pub name: String,
    /// True if this entry represents a directory.
    pub is_dir: bool,
    /// True if this directory is expanded (ignored for files).
    pub is_expanded: bool,
    /// Nesting depth (root entries are 0).
    pub depth: usize,
}

// ── TreeState ─────────────────────────────────────────────────────────────────

/// Full state of the file tree pane.
pub struct TreeState {
    /// All entries (including hidden children of collapsed dirs).
    pub entries: Vec<TreeEntry>,
    /// Index into `entries` that is currently selected.
    pub selected: usize,
    /// `ratatui` list state; kept in sync with `selected`.
    pub list_state: ListState,
}

impl TreeState {
    /// Create an empty `TreeState` with selection at 0.
    pub fn new() -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            entries: Vec::new(),
            selected: 0,
            list_state,
        }
    }

    /// Update `selected` and keep `list_state` in sync.
    pub fn select(&mut self, index: usize) {
        self.selected = index;
        self.list_state.select(Some(index));
    }

    /// Move selection up by one, clamped to 0.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.select(self.selected - 1);
        }
    }

    /// Move selection down by one, clamped to `entries.len() - 1`.
    pub fn move_down(&mut self) {
        let max = self.entries.len().saturating_sub(1);
        if self.selected < max {
            self.select(self.selected + 1);
        }
    }
}

impl Default for TreeState {
    fn default() -> Self {
        Self::new()
    }
}

// ── ViewerState ───────────────────────────────────────────────────────────────

/// State of the file viewer pane.
pub struct ViewerState {
    /// Vault-relative path of the currently loaded file, if any.
    pub file_path: Option<String>,
    /// Raw file content as a UTF-8 string.
    pub content: String,
    /// Content split into individual lines.
    pub lines: Vec<String>,
    /// First visible line (0-based scroll offset).
    pub scroll_offset: usize,
}

impl ViewerState {
    /// Create an empty viewer state.
    pub fn new() -> Self {
        Self {
            file_path: None,
            content: String::new(),
            lines: Vec::new(),
            scroll_offset: 0,
        }
    }

    /// Load a file into the viewer; resets scroll to the top.
    pub fn load_content(&mut self, path: String, text: String) {
        self.lines = text.lines().map(String::from).collect();
        self.content = text;
        self.file_path = Some(path);
        self.scroll_offset = 0;
    }

    /// Scroll down by `amount` lines, clamped to the last line.
    pub fn scroll_down(&mut self, amount: usize) {
        let max = self.lines.len().saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + amount).min(max);
    }

    /// Scroll up by `amount` lines, clamped to 0.
    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    /// Jump to the first line.
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    /// Jump to the last line.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.lines.len().saturating_sub(1);
    }
}

impl Default for ViewerState {
    fn default() -> Self {
        Self::new()
    }
}

// ── SearchState ───────────────────────────────────────────────────────────────

/// State for the full-text search overlay.
pub struct SearchState {
    /// Current search query string.
    pub query: String,
    /// Results returned by the storage engine.
    pub results: Vec<SearchResult>,
    /// Index of the currently highlighted result.
    pub selected: usize,
    /// Cursor position inside `query`.
    pub cursor_pos: usize,
}

impl SearchState {
    /// Create a new, empty `SearchState`.
    pub fn new() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            cursor_pos: 0,
        }
    }

    /// Clear the query, results, and reset selection.
    pub fn clear(&mut self) {
        self.query.clear();
        self.results.clear();
        self.selected = 0;
        self.cursor_pos = 0;
    }
}

impl Default for SearchState {
    fn default() -> Self {
        Self::new()
    }
}

// ── FindState ─────────────────────────────────────────────────────────────────

/// State for in-file find (grep) bar.
pub struct FindState {
    /// Current find query string.
    pub query: String,
    /// All match locations as `(line_index, col_index)` pairs (0-based).
    pub matches: Vec<(usize, usize)>,
    /// Index of the currently highlighted match.
    pub current_match: usize,
    /// Cursor position inside `query`.
    pub cursor_pos: usize,
}

impl FindState {
    /// Create a new, empty `FindState`.
    pub fn new() -> Self {
        Self {
            query: String::new(),
            matches: Vec::new(),
            current_match: 0,
            cursor_pos: 0,
        }
    }

    /// Clear the query, matches, and reset state.
    pub fn clear(&mut self) {
        self.query.clear();
        self.matches.clear();
        self.current_match = 0;
        self.cursor_pos = 0;
    }

    /// Recompute `matches` from `lines` using a case-insensitive substring search.
    pub fn update_matches(&mut self, lines: &[String]) {
        self.matches.clear();
        self.current_match = 0;
        if self.query.is_empty() {
            return;
        }
        let needle = self.query.to_lowercase();
        for (line_idx, line) in lines.iter().enumerate() {
            let haystack = line.to_lowercase();
            let mut start = 0;
            while let Some(col) = haystack[start..].find(&needle) {
                self.matches.push((line_idx, start + col));
                start += col + needle.len();
            }
        }
    }

    /// Advance to the next match, wrapping around.
    pub fn next_match(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        self.current_match = (self.current_match + 1) % self.matches.len();
    }

    /// Go back to the previous match, wrapping around.
    pub fn prev_match(&mut self) {
        if self.matches.is_empty() {
            return;
        }
        if self.current_match == 0 {
            self.current_match = self.matches.len() - 1;
        } else {
            self.current_match -= 1;
        }
    }
}

impl Default for FindState {
    fn default() -> Self {
        Self::new()
    }
}

// ── BacklinksState ───────────────────────────────────────────────────────────

/// State for the toggleable backlinks panel.
pub struct BacklinksState {
    /// Whether the backlinks panel is visible.
    pub visible: bool,
    /// Backlink entries as `(source_path, link_text)` pairs.
    pub entries: Vec<(String, String)>,
    /// Index of the currently selected backlink entry.
    pub selected: usize,
    /// `ratatui` list state; kept in sync with `selected`.
    pub list_state: ListState,
}

impl BacklinksState {
    /// Create a new, hidden `BacklinksState`.
    pub fn new() -> Self {
        Self {
            visible: false,
            entries: Vec::new(),
            selected: 0,
            list_state: ListState::default(),
        }
    }

    /// Toggle the visibility of the backlinks panel.
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Load backlink entries from a list of `BacklinkResult`s.
    pub fn load(&mut self, results: Vec<BacklinkResult>) {
        self.entries = results
            .into_iter()
            .map(|r| (r.source_path, r.link_text))
            .collect();
        self.selected = 0;
        self.list_state.select(if self.entries.is_empty() {
            None
        } else {
            Some(0)
        });
    }
}

impl Default for BacklinksState {
    fn default() -> Self {
        Self::new()
    }
}

// ── TaskViewState ────────────────────────────────────────────────────────────

/// A single task entry for display in the task list view.
#[derive(Debug, Clone)]
pub struct TaskEntry {
    /// Database primary key.
    pub id: u64,
    /// Whether the task is completed.
    pub completed: bool,
    /// Task text content.
    pub content: String,
    /// Vault-relative file path containing this task.
    pub file_path: String,
    /// 1-indexed line number in the source file.
    pub line_number: u32,
}

/// State for the task list view that replaces the viewer when active.
pub struct TaskViewState {
    /// Whether the task view is currently active (replaces viewer).
    pub active: bool,
    /// All task entries.
    pub entries: Vec<TaskEntry>,
    /// Index of the currently selected task.
    pub selected: usize,
    /// `ratatui` list state; kept in sync with `selected`.
    pub list_state: ListState,
}

impl TaskViewState {
    /// Create a new, inactive `TaskViewState`.
    pub fn new() -> Self {
        Self {
            active: false,
            entries: Vec::new(),
            selected: 0,
            list_state: ListState::default(),
        }
    }

    /// Toggle the task view on or off.
    pub fn toggle(&mut self) {
        self.active = !self.active;
    }

    /// Load task entries from a list of `TaskRecord`s.
    pub fn load(&mut self, records: Vec<TaskRecord>) {
        self.entries = records
            .into_iter()
            .map(|r| TaskEntry {
                id: r.id,
                completed: r.completed,
                content: r.content,
                file_path: r.file_path,
                line_number: r.line_number,
            })
            .collect();
        self.selected = 0;
        self.list_state.select(if self.entries.is_empty() {
            None
        } else {
            Some(0)
        });
    }
}

impl Default for TaskViewState {
    fn default() -> Self {
        Self::new()
    }
}

// ── TuiApp ────────────────────────────────────────────────────────────────────

/// Top-level application state.
pub struct TuiApp {
    /// Current input mode.
    pub mode: Mode,
    /// Which pane has focus.
    pub focus: Focus,
    /// File tree state.
    pub tree: TreeState,
    /// Viewer pane state.
    pub viewer: ViewerState,
    /// Search overlay state.
    pub search: SearchState,
    /// In-file find state.
    pub find: FindState,
    /// Backlinks panel state.
    pub backlinks: BacklinksState,
    /// Task list view state.
    pub task_view: TaskViewState,
    /// Underlying storage engine.
    pub storage: StorageEngine,
    /// Path to the forge root.
    pub forge_root: PathBuf,
    /// Set to `true` to request a clean exit on the next event loop tick.
    pub should_quit: bool,
}

impl TuiApp {
    /// Create a new `TuiApp` by opening the forge at `forge_root`.
    ///
    /// # Errors
    ///
    /// Returns an error if the forge cannot be opened or the initial tree
    /// population fails.
    pub fn new(forge_root: PathBuf) -> Result<Self> {
        let storage = StorageEngine::open(&forge_root, &StorageConfig::default())
            .with_context(|| format!("failed to open forge at {}", forge_root.display()))?;

        let mut app = Self {
            mode: Mode::Normal,
            focus: Focus::FileTree,
            tree: TreeState::new(),
            viewer: ViewerState::new(),
            search: SearchState::new(),
            find: FindState::new(),
            backlinks: BacklinksState::new(),
            task_view: TaskViewState::new(),
            storage,
            forge_root,
            should_quit: false,
        };

        app.refresh_tree()?;
        Ok(app)
    }

    /// Rebuild the tree from the storage index.
    ///
    /// Queries all files, synthesises parent directory entries, then sorts
    /// and deduplicates the whole list.
    ///
    /// # Errors
    ///
    /// Returns an error if the storage query fails.
    pub fn refresh_tree(&mut self) -> Result<()> {
        let filter = FileFilter::default();
        let records = self
            .storage
            .query_files(&filter)
            .context("failed to query files for tree")?;

        // Collect all directory paths implied by the file paths.
        let mut dir_paths: BTreeSet<String> = BTreeSet::new();
        for rec in &records {
            let p = std::path::Path::new(&rec.path);
            let mut ancestor = p.parent();
            while let Some(dir) = ancestor {
                let dir_str = dir.to_string_lossy().to_string();
                if dir_str.is_empty() {
                    break;
                }
                dir_paths.insert(dir_str);
                ancestor = dir.parent();
            }
        }

        let mut entries: Vec<TreeEntry> = Vec::new();

        // Add directory entries.
        for dir_path in &dir_paths {
            let name = std::path::Path::new(dir_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| dir_path.clone());
            let depth = dir_path.chars().filter(|&c| c == '/').count();
            entries.push(TreeEntry {
                path: dir_path.clone(),
                name,
                is_dir: true,
                is_expanded: true,
                depth,
            });
        }

        // Add file entries.
        for rec in &records {
            let name = std::path::Path::new(&rec.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| rec.path.clone());
            let depth = rec.path.chars().filter(|&c| c == '/').count();
            entries.push(TreeEntry {
                path: rec.path.clone(),
                name,
                is_dir: false,
                is_expanded: false,
                depth,
            });
        }

        // Sort: dirs before files at the same depth, then alphabetically.
        entries.sort_by(|a, b| {
            a.path
                .split('/')
                .count()
                .cmp(&b.path.split('/').count())
                .then(b.is_dir.cmp(&a.is_dir))
                .then(a.name.cmp(&b.name))
        });

        // Preserve is_expanded state from the previous tree.
        let prev_collapsed: BTreeSet<String> = self
            .tree
            .entries
            .iter()
            .filter(|e| e.is_dir && !e.is_expanded)
            .map(|e| e.path.clone())
            .collect();

        for entry in &mut entries {
            if entry.is_dir && prev_collapsed.contains(&entry.path) {
                entry.is_expanded = false;
            }
        }

        // Reset selection if the tree has grown/shrunk.
        let prev_selected_path = self
            .tree
            .entries
            .get(self.tree.selected)
            .map(|e| e.path.clone());

        self.tree.entries = entries;

        // Restore selection by path, falling back to 0.
        if let Some(prev_path) = prev_selected_path {
            if let Some(idx) = self
                .tree
                .entries
                .iter()
                .position(|e| e.path == prev_path)
            {
                self.tree.select(idx);
            } else {
                self.tree.select(0);
            }
        } else {
            self.tree.select(0);
        }

        Ok(())
    }

    /// Open the currently selected file into the viewer.
    ///
    /// If the selected entry is a directory, does nothing.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read from the storage engine.
    pub fn open_selected_file(&mut self) -> Result<()> {
        let visible = self.visible_entries();
        let Some(entry) = visible.get(self.tree.selected) else {
            return Ok(());
        };
        if entry.is_dir {
            return Ok(());
        }
        let path = entry.path.clone();
        let bytes = self
            .storage
            .read_file(&path)
            .with_context(|| format!("failed to read file '{path}'"))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        self.viewer.load_content(path, text);
        self.focus = Focus::Viewer;
        // Refresh backlinks for the newly opened file.
        if self.backlinks.visible {
            self.load_backlinks();
        }
        Ok(())
    }

    /// Load backlinks for the currently viewed file into the backlinks panel.
    ///
    /// Does nothing if no file is loaded in the viewer.
    pub fn load_backlinks(&mut self) {
        let path = match self.viewer.file_path.as_deref() {
            Some(p) => p.to_owned(),
            None => {
                self.backlinks.load(Vec::new());
                return;
            }
        };
        match self.storage.backlinks(&path) {
            Ok(results) => self.backlinks.load(results),
            Err(_) => self.backlinks.load(Vec::new()),
        }
    }

    /// Load all tasks into the task view state.
    pub fn load_tasks(&mut self) {
        match self.storage.query_tasks(&TaskFilter::default()) {
            Ok(records) => self.task_view.load(records),
            Err(_) => self.task_view.load(Vec::new()),
        }
    }

    /// Toggle the expanded/collapsed state of the selected directory entry.
    ///
    /// If the selected visible entry is not a directory, does nothing.
    pub fn toggle_dir(&mut self) {
        let visible = self.visible_entries();
        let Some(entry) = visible.get(self.tree.selected) else {
            return;
        };
        if !entry.is_dir {
            return;
        }
        let path = entry.path.clone();
        if let Some(e) = self.tree.entries.iter_mut().find(|e| e.path == path) {
            e.is_expanded = !e.is_expanded;
        }
    }

    /// Return the subset of `tree.entries` that should be visible given the
    /// current expand/collapse state of directories.
    ///
    /// Entries whose parent directory (or any ancestor) is collapsed are
    /// excluded.
    pub fn visible_entries(&self) -> Vec<&TreeEntry> {
        let mut result = Vec::new();
        // Set of collapsed directory paths.
        let collapsed: BTreeSet<&str> = self
            .tree
            .entries
            .iter()
            .filter(|e| e.is_dir && !e.is_expanded)
            .map(|e| e.path.as_str())
            .collect();

        'outer: for entry in &self.tree.entries {
            // Check whether any ancestor is collapsed.
            let p = std::path::Path::new(&entry.path);
            let mut ancestor = p.parent();
            while let Some(dir) = ancestor {
                let dir_str = dir.to_string_lossy();
                if dir_str.is_empty() {
                    break;
                }
                if collapsed.contains(dir_str.as_ref()) {
                    continue 'outer;
                }
                ancestor = dir.parent();
            }
            result.push(entry);
        }
        result
    }
}
