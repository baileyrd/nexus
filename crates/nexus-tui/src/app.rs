//! Application state for nexus-tui.
//!
//! Defines the top-level [`TuiApp`] struct and all supporting state types:
//! [`TreeState`], [`ViewerState`], [`SearchState`], [`FindState`].

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use nexus_bootstrap::storage as ipc;
use nexus_bootstrap::storage::{BacklinkResult, SearchResult, TaskFilter, TaskRecord};
use nexus_bootstrap::terminal as term_ipc;
use nexus_bootstrap::terminal::OutputLine;
use nexus_bootstrap::{build_tui_runtime, Runtime};
use nexus_kernel::{EventFilter, EventSubscription, NexusEvent, PluginContext};
use ratatui::widgets::ListState;
use tokio::runtime::Runtime as TokioRuntime;
use tokio::task::JoinHandle;

use crate::streaming::{
    matches_start_event, parse_chunk_event, parse_done_event, STREAM_CHUNK_TOPIC,
    STREAM_DONE_TOPIC, STREAM_START_TOPIC,
};

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
    /// Terminal input mode — keystrokes go to the PTY.
    Terminal,
    /// AIG-07 — AI chat input mode. Keystrokes go to the prompt at
    /// the bottom of the AI panel.
    AiInput,
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

// ── KernelStatsState ─────────────────────────────────────────────────────────

/// BL-137 — state for the kernel-stats overlay (toggled with Shift+K).
/// Reads `com.nexus.security::metrics_snapshot` on each open so the
/// view is a fresh point-in-time capture rather than a streaming
/// dashboard. The raw JSON value is cached in `snapshot` to keep
/// `ui::kernel_stats::render` allocation-free.
pub struct KernelStatsState {
    /// Whether the overlay is currently visible.
    pub visible: bool,
    /// Latest snapshot fetched from
    /// `com.nexus.security::metrics_snapshot`. `None` until the first
    /// successful fetch, or after an error (with the error in
    /// `last_error`).
    pub snapshot: Option<serde_json::Value>,
    /// Error message from the most recent fetch attempt, if any.
    pub last_error: Option<String>,
}

impl KernelStatsState {
    /// Create a fresh, hidden state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            visible: false,
            snapshot: None,
            last_error: None,
        }
    }
}

impl Default for KernelStatsState {
    fn default() -> Self {
        Self::new()
    }
}

// ── TaskViewState ────────────────────────────────────────────────────────────

/// A single task entry for display in the task list view.
#[derive(Debug, Clone)]
pub struct TaskEntry {
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

// ── StatusInfo ───────────────────────────────────────────────────────────────

/// Cached status bar statistics.
pub struct StatusInfo {
    /// Total number of files in the forge.
    pub file_count: usize,
    /// Total number of links (graph edges).
    pub link_count: usize,
    /// Number of pending (incomplete) tasks.
    pub pending_task_count: usize,
    /// Git branch name and dirty flag, if in a git repo.
    pub git_branch: Option<(String, bool)>,
}

impl StatusInfo {
    /// Create a zeroed `StatusInfo`.
    pub fn new() -> Self {
        Self {
            file_count: 0,
            link_count: 0,
            pending_task_count: 0,
            git_branch: None,
        }
    }
}

impl Default for StatusInfo {
    fn default() -> Self {
        Self::new()
    }
}

// ── TerminalPanelState ───────────────────────────────────────────────────────

/// State for the in-TUI terminal panel (PRD-09 §14.1, backed by
/// `com.nexus.terminal` core-plugin dispatch via
/// [`nexus_bootstrap::terminal`]). The panel replaces the viewer when
/// active, mirroring the existing `TaskViewState` pattern.
pub struct TerminalPanelState {
    /// Whether the panel currently replaces the viewer.
    pub active: bool,
    /// Opaque session id issued by the terminal core plugin. `None`
    /// until the user first opens the panel — we spawn on demand so
    /// sessions don't leak when the panel is never used.
    pub session_id: Option<String>,
    /// Cached snapshot of the session's line buffer. Refreshed on
    /// every pump tick while the panel is visible.
    pub lines: Vec<OutputLine>,
    /// User input buffer for the current prompt. Flushed to the PTY
    /// on Enter.
    pub input: String,
    /// Last observed line count so the next refresh can detect whether
    /// new output arrived. Avoids re-cloning the whole `lines` vec
    /// when nothing changed.
    pub last_line_count: usize,
    /// Diagnostic ring of the last N key events the terminal-mode
    /// handler observed. Shown in the panel title while debugging
    /// input routing — empty on shell builds that don't care.
    pub key_log: Vec<String>,
}

impl TerminalPanelState {
    /// Create a fresh, inactive panel with no session.
    pub fn new() -> Self {
        Self {
            active: false,
            session_id: None,
            lines: Vec::new(),
            input: String::new(),
            last_line_count: 0,
            key_log: Vec::new(),
        }
    }

    /// Record a diagnostic entry for the most recent key event
    /// observed by the terminal-mode input handler. Kept at 5 entries
    /// max so the title bar stays readable.
    pub fn log_key(&mut self, entry: String) {
        self.key_log.push(entry);
        if self.key_log.len() > 5 {
            self.key_log.remove(0);
        }
    }
}

impl Default for TerminalPanelState {
    fn default() -> Self {
        Self::new()
    }
}

// ── AiPanelState (AIG-07) ─────────────────────────────────────────────────────

/// One turn in the AI chat transcript. The role drives rendering
/// (user vs assistant prefix); the content is markdown emitted by
/// the model — rendered as plain text in the TUI for now since
/// ratatui doesn't ship a markdown renderer.
#[derive(Debug, Clone)]
pub struct AiMessage {
    /// Either `"user"` or `"assistant"`.
    pub role: AiRole,
    /// Free-form text. For assistant turns, this is the model's
    /// reply; user turns hold the submitted question verbatim.
    pub text: String,
}

/// Role of an [`AiMessage`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiRole {
    User,
    Assistant,
}

/// In-flight streaming session — one per active AI submit. The pump
/// drains the subscription between renders, appending each chunk to
/// the placeholder assistant message so the user sees the reply
/// arrive in real time. AIG-07 follow-up.
pub struct StreamingSession {
    /// Session id passed to `stream_ask` and matched against
    /// `session_id` on incoming bus events. Distinct from any other
    /// concurrent session that might publish on the same topics.
    pub session_id: String,
    /// Subscription to the chunk topic (kernel-owned shared topic).
    /// Drained via `try_recv` from the main thread on every pump tick.
    pub subscription: EventSubscription,
    /// Tokio task driving the IPC call. `is_finished` is polled non-
    /// blocking; the result is harvested only when `true`.
    pub join: JoinHandle<Result<serde_json::Value, String>>,
    /// Index into `messages` of the placeholder assistant message
    /// that streaming chunks are appended to. Captured at submit
    /// time so the pump can locate the buffer without a linear scan.
    pub placeholder_idx: usize,
    /// Flips to `true` once the first chunk lands so the status line
    /// can switch from "thinking…" to "streaming…".
    pub started: bool,
}

/// State of the right-pane AI chat surface (AIG-07).
pub struct AiPanelState {
    /// True when the panel is visible. Toggled with `a` in Normal
    /// mode.
    pub active: bool,
    /// Conversation transcript, oldest first. The current `ask`
    /// handler is single-turn (it doesn't take prior context), so
    /// these are kept for display purposes only — multi-turn
    /// follow-up is a follow-up improvement.
    pub messages: Vec<AiMessage>,
    /// Current prompt buffer. Submitted to `ask` on Enter.
    pub input: String,
    /// Caret position within `input` (char index, not byte).
    pub cursor: usize,
    /// True while a `com.nexus.ai::ask` call is being awaited via
    /// `rt.block_on`. The render loop pre-paints "Thinking…" so the
    /// freeze is at least narrated; streaming token feedback is
    /// deferred (would require an `Arc<KernelPluginContext>` on the
    /// Runtime, which is a larger refactor).
    pub in_flight: bool,
    /// Most recent error (transport / no provider configured /
    /// kernel error). Cleared at the start of every new submit.
    pub last_error: Option<String>,
    /// Provider/model status string from `com.nexus.ai::status`.
    /// Populated on first activation; refreshed on demand. `None`
    /// while not yet loaded; `Some(text)` for the rendered string.
    pub provider_status: Option<String>,
    /// Vertical scroll offset for the transcript. Up/Down arrows in
    /// panel-Normal mode adjust it; auto-pinned to the bottom when
    /// a new message arrives.
    pub scroll: u16,
    /// AIG-07 — active streaming session, if any. `Some` between
    /// `submit_ai` and the pump observing the IPC task's completion.
    pub streaming: Option<StreamingSession>,
}

impl AiPanelState {
    /// Create a fresh, inactive panel.
    pub fn new() -> Self {
        Self {
            active: false,
            messages: Vec::new(),
            input: String::new(),
            cursor: 0,
            in_flight: false,
            last_error: None,
            provider_status: None,
            scroll: 0,
            streaming: None,
        }
    }

    /// Insert a single character at the caret. Used by `Mode::AiInput`.
    pub fn insert_char(&mut self, c: char) {
        let byte = self.char_index_to_byte(self.cursor);
        self.input.insert(byte, c);
        self.cursor += 1;
    }

    /// Backspace: delete the character before the caret.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let byte_end = self.char_index_to_byte(self.cursor);
        let byte_start = self.char_index_to_byte(self.cursor - 1);
        self.input.replace_range(byte_start..byte_end, "");
        self.cursor -= 1;
    }

    /// Translate a char index into a byte offset within `input`.
    /// Needed because Rust strings are byte-indexed but the cursor is
    /// expressed in chars (so emoji / combining marks don't break).
    fn char_index_to_byte(&self, char_idx: usize) -> usize {
        self.input
            .char_indices()
            .nth(char_idx)
            .map_or(self.input.len(), |(b, _)| b)
    }
}

impl Default for AiPanelState {
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
    /// Terminal panel state.
    pub terminal: TerminalPanelState,
    /// AIG-07 — AI chat panel state.
    pub ai: AiPanelState,
    /// BL-137 — kernel-stats overlay state.
    pub kernel_stats: KernelStatsState,
    /// Cached status bar statistics.
    pub status_info: StatusInfo,
    /// Nexus runtime providing the kernel plugin context used for all storage
    /// operations. Held behind `runtime.context.ipc_call`.
    ///
    /// AIG-07 — wrapped in `Arc` so the streaming pump can hand a clone
    /// to the spawned IPC task without a borrow-checker fight; every
    /// existing helper that takes `&Runtime` still works via
    /// `&*self.runtime`.
    pub runtime: Arc<Runtime>,
    /// Tokio runtime used to block on async `ipc_call`s from the sync event
    /// loop. A multi-threaded runtime is required for `spawn_blocking` tasks
    /// inside the kernel's ipc dispatcher.
    pub rt: TokioRuntime,
    /// Path to the forge root.
    pub forge_root: PathBuf,
    /// Set to `true` to request a clean exit on the next event loop tick.
    pub should_quit: bool,
    /// BL-129 — background dream-cycle scheduler. `None` when the
    /// scheduler failed to spawn (logged at warn). Held purely for
    /// its `Drop` impl: when the `App` is dropped the scheduler
    /// signals its worker thread and joins it. `#[allow(dead_code)]`
    /// because the field is never read after construction.
    #[allow(dead_code)]
    pub dream_cycle: Option<nexus_bootstrap::dream_cycle::DreamCycleScheduler>,
}

impl TuiApp {
    /// Create a new `TuiApp` by opening the forge at `forge_root`.
    ///
    /// # Errors
    ///
    /// Returns an error if the forge cannot be opened or the initial tree
    /// population fails.
    pub fn new(forge_root: PathBuf) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .context("failed to start tokio runtime")?;
        let runtime = Arc::new(
            build_tui_runtime(forge_root.clone())
                .with_context(|| format!("failed to build runtime at {}", forge_root.display()))?,
        );

        // BL-129 — spawn the dream-cycle scheduler. The handle gates
        // its own work on `[dream_cycle].enabled`, so a forge that
        // hasn't opted in does nothing beyond a 60s config-poll loop.
        let dream_cycle = match nexus_bootstrap::dream_cycle::spawn(&runtime, forge_root.clone()) {
            Ok(h) => Some(h),
            Err(e) => {
                tracing::warn!(error = %e, "dream_cycle: scheduler not spawned");
                None
            }
        };

        let mut app = Self {
            mode: Mode::Normal,
            focus: Focus::FileTree,
            tree: TreeState::new(),
            viewer: ViewerState::new(),
            search: SearchState::new(),
            find: FindState::new(),
            backlinks: BacklinksState::new(),
            task_view: TaskViewState::new(),
            terminal: TerminalPanelState::new(),
            ai: AiPanelState::new(),
            kernel_stats: KernelStatsState::new(),
            status_info: StatusInfo::new(),
            runtime,
            rt,
            forge_root,
            should_quit: false,
            dream_cycle,
        };

        app.refresh_tree()?;
        app.refresh_status();
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
        let invoker = self.runtime.invoker();
        let records = self
            .rt
            .block_on(ipc::query_files(&*invoker))
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
        let invoker = self.runtime.invoker();
        let bytes = self
            .rt
            .block_on(ipc::read_file(&*invoker, &path))
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
        let invoker = self.runtime.invoker();
        match self.rt.block_on(ipc::backlinks(&*invoker, &path)) {
            Ok(results) => self.backlinks.load(results),
            Err(_) => self.backlinks.load(Vec::new()),
        }
    }

    /// BL-137 — toggle the kernel-stats overlay. On opening, fetches
    /// a fresh `com.nexus.security::metrics_snapshot` so the panel
    /// shows a current point-in-time capture rather than the previous
    /// session's snapshot.
    pub fn toggle_kernel_stats(&mut self) {
        self.kernel_stats.visible = !self.kernel_stats.visible;
        if !self.kernel_stats.visible {
            return;
        }
        use std::time::Duration;
        use nexus_kernel::PluginContext;
        let result = self.rt.block_on(self.runtime.context.ipc_call(
            "com.nexus.security",
            "metrics_snapshot",
            serde_json::json!({}),
            Duration::from_secs(5),
        ));
        match result {
            Ok(value) => {
                self.kernel_stats.snapshot = if value.is_null() { None } else { Some(value) };
                self.kernel_stats.last_error = None;
            }
            Err(e) => {
                self.kernel_stats.snapshot = None;
                self.kernel_stats.last_error = Some(e.to_string());
            }
        }
    }

    /// Load all tasks into the task view state.
    pub fn load_tasks(&mut self) {
        let invoker = self.runtime.invoker();
        match self
            .rt
            .block_on(ipc::query_tasks(&*invoker, &TaskFilter::default()))
        {
            Ok(records) => self.task_view.load(records),
            Err(_) => self.task_view.load(Vec::new()),
        }
    }

    // ── Terminal panel ────────────────────────────────────────────────────────

    /// Open the terminal panel, spawning a PTY session on first open.
    /// Subsequent opens reuse the existing session so scrollback
    /// survives a hide/show toggle.
    pub fn open_terminal(&mut self) {
        self.terminal.active = true;
        if self.terminal.session_id.is_none() {
            let args = term_ipc::CreateSessionArgs {
                name: Some("tui-terminal".into()),
                working_dir: Some(self.forge_root.display().to_string()),
                ..Default::default()
            };
            let invoker = self.runtime.invoker();
            match self.rt.block_on(term_ipc::create_session(&*invoker, args)) {
                Ok(id) => {
                    self.terminal.session_id = Some(id);
                    self.terminal.lines.clear();
                    self.terminal.last_line_count = 0;
                    self.terminal.input.clear();
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to open terminal session");
                    self.terminal.active = false;
                }
            }
        }
    }

    /// Hide the terminal panel without closing the underlying session
    /// — scrollback survives and the next open restores it. Users
    /// explicitly close via [`Self::kill_terminal`].
    pub fn hide_terminal(&mut self) {
        self.terminal.active = false;
    }

    /// Close the terminal session outright. Called when the user hits
    /// Ctrl+D in the terminal panel or quits the app.
    pub fn kill_terminal(&mut self) {
        if let Some(id) = self.terminal.session_id.take() {
            let invoker = self.runtime.invoker();
            if let Err(e) = self.rt.block_on(term_ipc::close_session(&*invoker, &id)) {
                tracing::debug!(error = %e, "terminal close_session returned error (child may already be gone)");
            }
        }
        self.terminal.active = false;
        self.terminal.lines.clear();
        self.terminal.input.clear();
        self.terminal.last_line_count = 0;
    }

    /// Pump the PTY once (short timeout) and refresh the cached line
    /// snapshot. Called from the TUI event loop every few frames so
    /// long-running commands surface output without blocking input.
    pub fn pump_terminal(&mut self) {
        let Some(id) = self.terminal.session_id.clone() else {
            return;
        };
        // Short timeout: we want to return to input handling quickly.
        let invoker = self.runtime.invoker();
        if let Err(e) = self.rt.block_on(term_ipc::pump(&*invoker, &id, 50)) {
            tracing::debug!(error = %e, "terminal pump failed");
            return;
        }
        match self
            .rt
            .block_on(term_ipc::read_output(&*invoker, &id, None, None))
        {
            Ok(lines) => {
                if lines.len() != self.terminal.last_line_count {
                    self.terminal.last_line_count = lines.len();
                    self.terminal.lines = lines;
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "terminal read_output failed");
            }
        }
    }

    /// Flush the user's current input buffer as a complete command
    /// (appending a newline) to the PTY. Clears the buffer on success.
    pub fn submit_terminal_input(&mut self) {
        let Some(id) = self.terminal.session_id.clone() else {
            return;
        };
        let line = std::mem::take(&mut self.terminal.input);
        let invoker = self.runtime.invoker();
        if let Err(e) = self.rt.block_on(term_ipc::send_input(&*invoker, &id, &line)) {
            tracing::warn!(error = %e, "terminal send_input failed");
        }
    }

    /// Write raw bytes to the PTY — used for control sequences like
    /// Ctrl+C (`\x03`), Ctrl+D (`\x04`), arrow keys, …
    pub fn send_terminal_raw(&mut self, data: &[u8]) {
        let Some(id) = self.terminal.session_id.clone() else {
            return;
        };
        let invoker = self.runtime.invoker();
        if let Err(e) = self.rt.block_on(term_ipc::send_raw_input(&*invoker, &id, data)) {
            tracing::warn!(error = %e, "terminal send_raw_input failed");
        }
    }

    /// Refresh cached status bar statistics from the storage engine.
    pub fn refresh_status(&mut self) {
        let file_count = self
            .tree
            .entries
            .iter()
            .filter(|e| !e.is_dir)
            .count();

        let invoker = self.runtime.invoker();
        let link_count = self
            .rt
            .block_on(ipc::graph_stats(&*invoker))
            .map(|s| s.edge_count)
            .unwrap_or(0);

        let pending_task_count = self
            .rt
            .block_on(ipc::query_tasks(
                &*invoker,
                &TaskFilter {
                    completed: Some(false),
                    file_path: None,
                },
            ))
            .map(|tasks| tasks.len())
            .unwrap_or(0);

        let git_branch = nexus_git::GitEngine::open(&self.forge_root)
            .ok()
            .and_then(|engine| engine.state().ok())
            .map(|state| {
                let branch = state.branch.unwrap_or_else(|| "detached".to_string());
                (branch, state.is_dirty)
            });

        self.status_info = StatusInfo {
            file_count,
            link_count,
            pending_task_count,
            git_branch,
        };
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

    // ── AIG-07 — AI chat panel ──────────────────────────────────────

    /// Toggle the AI chat panel. First activation kicks off a status
    /// refresh so the header shows the configured provider.
    pub fn toggle_ai_panel(&mut self) {
        self.ai.active = !self.ai.active;
        if self.ai.active && self.ai.provider_status.is_none() {
            self.refresh_ai_status();
        }
    }

    /// Pull the active provider/model from `com.nexus.ai::status`.
    /// Best-effort: a missing or errored response leaves the
    /// header showing "(no provider)".
    pub fn refresh_ai_status(&mut self) {
        let result = self.rt.block_on(async {
            self.runtime
                .context
                .ipc_call(
                    "com.nexus.ai",
                    "status",
                    serde_json::json!({}),
                    Duration::from_secs(5),
                )
                .await
        });
        match result {
            Ok(value) => {
                let provider = value
                    .get("ai_provider")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let model = value
                    .get("ai_model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let label = match (provider, model) {
                    ("", _) => "(no provider)".to_string(),
                    (p, "") => p.to_string(),
                    (p, m) => format!("{p} / {m}"),
                };
                self.ai.provider_status = Some(label);
            }
            Err(_) => {
                self.ai.provider_status = Some("(no provider)".to_string());
            }
        }
    }

    /// Submit the current prompt to `com.nexus.ai::stream_ask`.
    ///
    /// AIG-07 — non-blocking. Subscribes to the chunk topic, spawns
    /// the IPC call onto `self.rt`, inserts a placeholder assistant
    /// message, and returns immediately. The render loop's
    /// [`Self::pump_ai`] drains chunks between frames and harvests
    /// the IPC result when the spawned task completes.
    ///
    /// The full transcript is passed through `stream_ask`'s
    /// `messages` field so the model sees prior turns; the kernel
    /// handler treats the last `user` message as the RAG retrieval
    /// question.
    pub fn submit_ai(&mut self) {
        let question = self.ai.input.trim().to_string();
        if question.is_empty() || self.ai.in_flight {
            return;
        }
        self.ai.messages.push(AiMessage {
            role: AiRole::User,
            text: question.clone(),
        });
        self.ai.input.clear();
        self.ai.cursor = 0;
        self.ai.last_error = None;
        self.ai.in_flight = true;
        // Auto-pin to the bottom whenever a new turn lands.
        self.ai.scroll = u16::MAX;

        let messages: Vec<serde_json::Value> = self
            .ai
            .messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": match m.role {
                        AiRole::User => "user",
                        AiRole::Assistant => "assistant",
                    },
                    "content": m.text,
                })
            })
            .collect();

        // Insert a placeholder assistant message that pump_ai will
        // grow as chunks arrive. Captured by index because the vec
        // can't grow under it before pump_ai sees the final result
        // (other turns are blocked on `in_flight`).
        self.ai.messages.push(AiMessage {
            role: AiRole::Assistant,
            text: String::new(),
        });
        let placeholder_idx = self.ai.messages.len() - 1;

        // Subscribe BEFORE firing the call so chunks published
        // between the spawn and our first pump still reach us
        // (broadcast channels drop pre-subscription events).
        let session_id = uuid::Uuid::new_v4().to_string();
        let subscription = self.runtime.kernel.event_bus().subscribe(
            EventFilter::CustomPrefix("com.nexus.ai.stream_".to_string()),
        );

        // Spawn the IPC call on the multi-threaded tokio runtime.
        // The future captures `runtime` (Arc clone) by move so it
        // owns its borrow for its full lifetime.
        let runtime = Arc::clone(&self.runtime);
        let sid_for_call = session_id.clone();
        let timeout = Duration::from_secs(180);
        let join = self.rt.spawn(async move {
            runtime
                .context
                .ipc_call(
                    "com.nexus.ai",
                    "stream_ask",
                    serde_json::json!({
                        "messages": messages,
                        "session_id": sid_for_call,
                    }),
                    timeout,
                )
                .await
                .map_err(|e| e.to_string())
        });

        self.ai.streaming = Some(StreamingSession {
            session_id,
            subscription,
            join,
            placeholder_idx,
            started: false,
        });
    }

    /// AIG-07 — drain any pending stream events into the placeholder
    /// assistant message and harvest the IPC result if the spawned
    /// task has completed.
    ///
    /// Called from the render loop on every tick (cheap when no
    /// streaming session is active). Non-blocking: uses `try_recv`
    /// on the subscription and `is_finished` on the join handle.
    /// Only blocks (briefly) when the join handle is already
    /// finished, to harvest the final value.
    pub fn pump_ai(&mut self) {
        let Some(session) = self.ai.streaming.as_mut() else {
            return;
        };

        // Drain pending bus events. We try-recv until empty so a
        // burst of chunks lands in one frame rather than dripping
        // out one-per-tick (which would feel laggy).
        loop {
            match session.subscription.try_recv() {
                Ok(Some(event)) => {
                    let NexusEvent::Custom { type_id, payload, .. } = &event.event else {
                        continue;
                    };
                    if type_id == STREAM_START_TOPIC
                        && matches_start_event(payload, &session.session_id)
                    {
                        session.started = true;
                        continue;
                    }
                    if type_id == STREAM_CHUNK_TOPIC {
                        if let Some(chunk) = parse_chunk_event(payload, &session.session_id) {
                            session.started = true;
                            if let Some(msg) = self.ai.messages.get_mut(session.placeholder_idx) {
                                msg.text.push_str(&chunk);
                            }
                            self.ai.scroll = u16::MAX;
                        }
                        continue;
                    }
                    if type_id == STREAM_DONE_TOPIC {
                        if let Some(text) = parse_done_event(payload, &session.session_id) {
                            // The done event carries the final
                            // post-processed text. Replacing the
                            // accumulated chunks here keeps the
                            // visible text in sync with whatever
                            // `trim` / `stop` the kernel applied.
                            if let Some(msg) = self.ai.messages.get_mut(session.placeholder_idx) {
                                msg.text = text;
                            }
                            self.ai.scroll = u16::MAX;
                        }
                        continue;
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    tracing::debug!(error = %err, "ai stream subscription drain error");
                    break;
                }
            }
        }

        // Harvest the IPC result if the task has completed. The
        // result drives error reporting and ensures the placeholder
        // is reconciled against the IPC's authoritative response
        // shape (covers the case where the bus never delivered a
        // `stream_done` — kernel error, transport drop, etc.).
        if !session.join.is_finished() {
            return;
        }
        // Take the session out so we drop the subscription before
        // any UI mutation; the join harvest can't observe pre-take
        // state because we're the only mutator.
        let mut session = self.ai.streaming.take().expect("checked Some above");
        let join_result = self
            .rt
            .block_on(async { (&mut session.join).await });
        self.ai.in_flight = false;

        let outcome = match join_result {
            Ok(Ok(value)) => extract_stream_ask_text(&value),
            Ok(Err(msg)) => Err(msg),
            Err(join_err) => Err(format!("ai task join: {join_err}")),
        };
        match outcome {
            Ok(answer) => {
                if let Some(msg) = self.ai.messages.get_mut(session.placeholder_idx) {
                    // Use the IPC's final text as the source of
                    // truth — chunk events alone may have produced a
                    // pre-trim version. If the placeholder was empty
                    // (provider never streamed), this is the only
                    // text the user will see.
                    if !answer.is_empty() {
                        msg.text = answer;
                    } else if msg.text.is_empty() {
                        // Empty IPC + empty chunks → drop the
                        // placeholder rather than leave a blank
                        // bubble in the transcript.
                        self.ai.messages.remove(session.placeholder_idx);
                    }
                }
                self.ai.scroll = u16::MAX;
            }
            Err(err) => {
                // Error path: drop the placeholder and surface the
                // error in the status line so the transcript stays
                // clean.
                if let Some(msg) = self.ai.messages.get(session.placeholder_idx) {
                    if msg.text.is_empty() {
                        self.ai.messages.remove(session.placeholder_idx);
                    }
                }
                self.ai.last_error = Some(err);
            }
        }
    }
}

/// AIG-07 follow-up — pull the assistant text out of a
/// `com.nexus.ai::stream_ask` final result. Mirrors the
/// `AiStreamAskResult.text` shape; falls back to the legacy `answer`
/// field (so a one-shot `ask` response still parses) and to a bare
/// string for kernel error paths that surface the message verbatim.
fn extract_stream_ask_text(value: &serde_json::Value) -> Result<String, String> {
    if let Some(s) = value.get("text").and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    if let Some(s) = value.get("answer").and_then(|v| v.as_str()) {
        return Ok(s.to_string());
    }
    if let Some(s) = value.as_str() {
        return Ok(s.to_string());
    }
    Err(format!(
        "stream_ask: unrecognised response shape ({})",
        value
    ))
}

#[cfg(test)]
mod aig07_tests {
    use super::*;

    // ── extract_stream_ask_text ────────────────────────────────────

    #[test]
    fn extract_stream_ask_text_pulls_text_field() {
        let v = serde_json::json!({
            "session_id": "abc",
            "text": "the model said this",
            "sources": []
        });
        assert_eq!(
            extract_stream_ask_text(&v).unwrap(),
            "the model said this".to_string(),
        );
    }

    #[test]
    fn extract_stream_ask_text_falls_back_to_legacy_answer_field() {
        // Forward-compat: an old `ask` response shape still parses,
        // so the TUI doesn't break if it hits a legacy plugin build.
        let v = serde_json::json!({
            "answer": "legacy reply",
            "citations": []
        });
        assert_eq!(
            extract_stream_ask_text(&v).unwrap(),
            "legacy reply".to_string(),
        );
    }

    #[test]
    fn extract_stream_ask_text_falls_back_to_bare_string() {
        // Forward-compat: some kernel error paths return a string
        // directly. The TUI surfaces it as the answer rather than
        // an "unrecognised shape" error.
        let v = serde_json::Value::String("plain reply".into());
        assert_eq!(
            extract_stream_ask_text(&v).unwrap(),
            "plain reply".to_string(),
        );
    }

    #[test]
    fn extract_stream_ask_text_rejects_unknown_shape() {
        let v = serde_json::json!({ "foo": 1 });
        let err = extract_stream_ask_text(&v).unwrap_err();
        assert!(err.contains("unrecognised"), "got: {err}");
    }

    // ── AiPanelState input editing ─────────────────────────────────

    #[test]
    fn insert_char_appends_at_end() {
        let mut s = AiPanelState::new();
        for c in "hello".chars() {
            s.insert_char(c);
        }
        assert_eq!(s.input, "hello");
        assert_eq!(s.cursor, 5);
    }

    #[test]
    fn insert_char_at_caret_within_existing_text() {
        let mut s = AiPanelState::new();
        for c in "hlo".chars() {
            s.insert_char(c);
        }
        // Move caret to position 1 (between 'h' and 'l') and insert.
        s.cursor = 1;
        s.insert_char('e');
        s.insert_char('l');
        assert_eq!(s.input, "hello");
        assert_eq!(s.cursor, 3);
    }

    #[test]
    fn insert_char_supports_multibyte_chars() {
        let mut s = AiPanelState::new();
        s.insert_char('é');
        s.insert_char('a');
        assert_eq!(s.input, "éa");
        assert_eq!(s.cursor, 2);
        // Backspace should delete a full grapheme even though `é`
        // is multi-byte.
        s.backspace();
        assert_eq!(s.input, "é");
        assert_eq!(s.cursor, 1);
    }

    #[test]
    fn backspace_at_zero_is_a_noop() {
        let mut s = AiPanelState::new();
        s.backspace();
        assert_eq!(s.input, "");
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn backspace_removes_char_before_caret() {
        let mut s = AiPanelState::new();
        for c in "abc".chars() {
            s.insert_char(c);
        }
        s.cursor = 2; // between 'b' and 'c'
        s.backspace();
        assert_eq!(s.input, "ac");
        assert_eq!(s.cursor, 1);
    }
}
