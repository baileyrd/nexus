//! The screen buffer (L06 state): the authoritative grid of cells, the cursor,
//! scrolling region, alternate screen, and scrollback history.
//!
//! The [`Grid`] exposes a semantic API (`put_char`, `set_cursor`, `scroll_up`,
//! …) that the [`AnsiParser`](super::parser::AnsiParser) drives, and produces a
//! [`DirtyFrame`] snapshot for the renderer.

use std::collections::VecDeque;
#[cfg(any(test, feature = "gui"))]
use std::collections::HashMap;

use super::cell::{Cell, DEFAULT_BG, DEFAULT_FG, Pen, WIDE_TRAILER, char_width};
use unicode_segmentation::UnicodeSegmentation;

use super::sixel::SixelImage;

/// Default maximum number of lines retained in the scrollback history. Older
/// lines are dropped as new ones arrive. Overridable per-grid via
/// [`Grid::set_scrollback_max`] (the `scrollback` config key).
pub const SCROLLBACK_MAX: usize = 10_000;

/// Maximum number of prompt marks (OSC 133;A) retained for navigation. Older
/// marks are dropped once exceeded — far more than any real session needs.
const PROMPT_MARKS_MAX: usize = 1024;

/// Maximum number of distinct hyperlink URIs interned from OSC 8. Past this,
/// further links are dropped (rendered as plain text) rather than growing the
/// table without bound.
const LINK_MAX: usize = 4096;

/// Maximum number of distinct grapheme-continuation strings interned from
/// multi-scalar glyphs. Past this, further continuations are dropped (the base
/// glyph still renders) rather than growing the table without bound.
const CLUSTER_MAX: usize = 8192;

/// A text selection in viewport cell coordinates (`(col, row)`), set by the
/// windowed front-end during a mouse drag and read by the renderer (to invert
/// the highlighted cells) and by [`Grid::selected_text`] (clipboard copy).
/// `anchor` is where the drag began, `head` where it is now; the pair is
/// normalized into row-major order on read, so either drag direction works.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Cell where the drag started.
    pub anchor: (usize, usize),
    /// Cell under the pointer now.
    pub head: (usize, usize),
}

/// In-window scrollback search results (windowed front-end). Matches span the
/// scrollback + live screen joined into logical lines, so a query can cross a
/// soft wrap; the renderer highlights them and next/prev steps through.
#[cfg(any(test, feature = "gui"))]
#[derive(Default)]
pub(crate) struct Search {
    /// Absolute row → highlighted col spans `(start, end_exclusive, match_index)`.
    rows: HashMap<usize, Vec<(usize, usize, usize)>>,
    /// Per match, the cell to scroll into view: `(absolute_row, col)`.
    anchors: Vec<(usize, usize)>,
    /// The active match — highlighted distinctly and the target of next/prev.
    current: usize,
}

/// Cap on retained search matches, bounding work and highlight storage.
#[cfg(any(test, feature = "gui"))]
const SEARCH_MAX: usize = 2000;

/// Find every (non-overlapping) occurrence of `q` in one logical line's `text`
/// (with parallel per-char `at` cell positions), recording each as an anchor
/// plus per-row highlight spans in `st`. Trailing spaces are ignored.
#[cfg(any(test, feature = "gui"))]
fn find_matches(text: &[char], at: &[(usize, usize)], q: &[char], st: &mut Search) {
    let mut len = text.len();
    while len > 0 && text[len - 1] == ' ' {
        len -= 1;
    }
    if q.is_empty() || q.len() > len {
        return;
    }
    let mut i = 0;
    while i + q.len() <= len {
        if text[i..i + q.len()] != *q {
            i += 1;
            continue;
        }
        let mi = st.anchors.len();
        st.anchors.push(at[i]);
        let mut j = i;
        while j < i + q.len() {
            let row = at[j].0;
            let start = at[j].1;
            let mut end = at[j].1 + char_width(text[j]).max(1);
            let mut k = j + 1;
            while k < i + q.len() && at[k].0 == row {
                end = at[k].1 + char_width(text[k]).max(1);
                k += 1;
            }
            st.rows.entry(row).or_default().push((start, end, mi));
            j = k;
        }
        i += q.len();
        if st.anchors.len() >= SEARCH_MAX {
            return;
        }
    }
}

/// The authoritative screen buffer: a row-major grid of [`Cell`]s plus a
/// per-row damage (`dirty`) tracker and a cursor position.
pub struct Grid {
    /// Number of columns.
    pub cols: usize,
    /// Number of rows.
    pub rows: usize,
    /// Row-major cell storage; `len() == cols * rows`.
    pub cells: Vec<Cell>,
    /// Per-row damage flags; `len() == rows`.
    pub dirty: Vec<bool>,
    /// Cursor position as `(col, row)`, both zero-based.
    pub cursor: (usize, usize),
    /// Cursor position stashed by a save (`DECSC` / `CSI s`) and restored by
    /// `DECRC` / `CSI u`.
    pub saved_cursor: (usize, usize),
    /// Top row of the scrolling region (inclusive, 0-based).
    pub scroll_top: usize,
    /// Bottom row of the scrolling region (inclusive, 0-based).
    pub scroll_bottom: usize,
    /// When `Some`, the alternate screen is active and this holds the primary
    /// screen to restore on exit. Kept the same dimensions as the live buffer.
    primary: Option<SavedScreen>,
    /// Monotonic counter bumped on every parsed batch; lets the renderer
    /// reason about frame freshness.
    pub epoch: u64,
    /// Window title last set by the child via OSC 0/2. The renderer forwards
    /// changes to the host terminal's title bar; empty until the child sets one.
    pub title: String,
    /// Working directory last reported by the child via OSC 7 (typically a
    /// `file://host/path` URI). Captured for future use (e.g. "open new tab in
    /// the same directory"); empty until reported.
    pub cwd: String,
    /// Lines that have scrolled off the top of the primary screen, oldest at the
    /// front. Bounded by [`SCROLLBACK_MAX`]. Each [`Line`] carries the cells plus
    /// its soft-wrap bit, so a resize can rejoin wrapped runs and re-wrap them to
    /// the new width. Rewrapped to the live width on every resize.
    pub(crate) scrollback: VecDeque<Line>,
    /// The live scrollback cap: [`SCROLLBACK_MAX`] unless overridden by the
    /// `scrollback` config key via [`Grid::set_scrollback_max`].
    pub(crate) scrollback_max: usize,
    /// How many lines the viewport is scrolled up into [`Grid::scrollback`].
    /// `0` is the live view (bottom); the renderer composites history above the
    /// live grid when this is non-zero.
    pub view_offset: usize,
    /// Active text selection (windowed front-end only), in viewport cell
    /// coordinates; `None` when nothing is selected. Pure view state, like
    /// [`Grid::view_offset`] — the TUI renderer ignores it (the host owns
    /// selection there).
    pub selection: Option<Selection>,
    /// Active scrollback search (windowed front-end); `None` when not searching.
    /// Pure view state like [`Grid::selection`]; the TUI ignores it.
    #[cfg(any(test, feature = "gui"))]
    search: Option<Search>,
    /// Bytes destined for the *host* terminal (not the grid): OSC 52 clipboard
    /// requests forwarded verbatim. The renderer drains these via
    /// [`Grid::take_host_out`] each frame and writes them to its stdout.
    pub(crate) host_out: Vec<u8>,
    /// Interned hyperlink URIs from OSC 8; a [`Cell::link`] of `n` refers to
    /// `links[n - 1]` (`0` means no link). Append-only and bounded by
    /// [`LINK_MAX`], so ids stay stable for cells held in scrollback.
    pub(crate) links: Vec<String>,
    /// Interned grapheme-continuation strings; a [`Cell::cluster`] of `n` refers
    /// to `clusters[n - 1]` (`0` means a lone `ch`). Append-only and bounded by
    /// [`CLUSTER_MAX`], so ids stay stable for cells held in scrollback.
    pub(crate) clusters: Vec<String>,
    /// The hyperlink id stamped onto cells written while an OSC 8 link is open
    /// (`0` when none). Set by the parser via [`Grid::set_link`].
    current_link: u16,
    /// Columns at which a horizontal tab stops; `len() == cols`. Defaults to
    /// every 8th column and is modified by `HTS` / `TBC`. A resize preserves
    /// stops within the surviving width and defaults the new columns.
    tab_stops: Vec<bool>,
    /// Whether the text cursor is visible (DECTCEM `?25`, default on). The
    /// renderer shows/hides the host cursor accordingly — independent of the
    /// separate hide it applies while browsing scrollback.
    pub cursor_visible: bool,
    /// Whether bracketed paste (DEC `?2004`) is enabled by the child. In TUI
    /// mode it is also relayed to the host; the windowed front-end reads it to
    /// decide whether to wrap pasted text in `ESC[200~` / `ESC[201~`.
    pub bracketed_paste: bool,
    /// Mouse reporting enabled by the child (DECSET `?1000`/`?1002`/`?1003`),
    /// plus extended-format bits (`?1006`/`?1015`/`?1016`). The window backend
    /// uses this to route clicks/drags/scrolls back to the child as encoded
    /// input bytes instead of handling them locally.
    pub mouse_modes: MouseModes,
    /// Text the child asked to place on the system clipboard via OSC 52 set,
    /// pending pickup by the window backend (which owns the clipboard); `None`
    /// when nothing is pending. The TUI relays OSC 52 to the host and ignores it.
    pub clipboard_set: Option<String>,
    /// Set when the child queried the clipboard (`OSC 52 ; … ; ?`); the window
    /// backend answers from the system clipboard and clears it.
    pub clipboard_query: bool,
    /// Desktop notifications (OSC 9 / OSC 777) the child requested, drained by
    /// the windowed front-end which raises them via the OS; the TUI relays the
    /// OSC to the host. Each entry is `(title, body)`; an empty title means the
    /// front-end picks a default. Bounded by [`Grid::push_notification`].
    pub notifications: Vec<(String, String)>,
    /// The active IME composition (preedit) text, shown reverse-video at the
    /// cursor by the windowed front-end; empty when not composing. Set by the
    /// window backend on `WindowEvent::Ime`, not by the parser.
    #[cfg_attr(not(feature = "gui"), allow(dead_code))]
    pub ime_preedit: String,
    /// Total lines ever scrolled into history — a stable serial anchoring
    /// [`GridImage`]s so they track scroll/scrollback (windowed front-end only).
    #[cfg(any(test, feature = "gui"))]
    pub(crate) total_scrolled: usize,
    /// Pixel images (Sixel/Kitty) drawn over their reserved half-block cells by
    /// the windowed CPU renderer; the TUI and GPU use the half-block cells.
    #[cfg(any(test, feature = "gui"))]
    pub(crate) images: Vec<GridImage>,
    /// Whether autowrap (DECAWM `?7`, default on) is enabled. When off, a glyph
    /// printed at the right margin overwrites the last column instead of
    /// wrapping to the next line.
    pub(crate) autowrap: bool,
    /// Whether origin mode (DECOM `?6`, default off) is enabled. When on,
    /// absolute cursor positioning (`CUP`/`HVP`/`VPA`) is relative to the scroll
    /// region top and the cursor is confined to the region.
    pub(crate) origin_mode: bool,
    /// Whether insert mode (IRM, ANSI mode `4`, default off) is enabled. When
    /// on, a printed glyph shifts the rest of the row right instead of
    /// overwriting.
    pub(crate) insert_mode: bool,
    /// Default foreground/background colors (OSC 10/11), mirrored from the
    /// parser's palette. The background doubles as the erase-fill color, so a
    /// theme set via OSC 11 colors cleared regions, not just freshly written text.
    default_fg: u32,
    default_bg: u32,
    /// Cursor color (OSC 12 / the `cursor` config key), mirrored from the
    /// parser's palette like the defaults above. The windowed renderers paint
    /// the block cursor in it; the TUI host owns its own cursor.
    pub cursor_color: u32,
    /// The cursor's rendered shape (DECSCUSR `CSI Ps SP q` or the `cursor_style`
    /// config key). The windowed renderers draw it; the TUI relays DECSCUSR to
    /// the host, which owns its own cursor.
    pub cursor_shape: CursorShape,
    /// Whether the cursor blinks (DECSCUSR odd params / the `cursor_blink` config
    /// key). The windowed event loop animates it; the TUI relays it to the host.
    pub cursor_blink: bool,
    /// The power-on cursor shape/blink (from config), restored by RIS/DECSTR.
    default_cursor_shape: CursorShape,
    default_cursor_blink: bool,
    /// Logical line indices (counting from the oldest retained scrollback line)
    /// of shell prompt starts reported via OSC 133;A, kept sorted. Powers
    /// prompt-to-prompt scrollback navigation. Bounded by [`PROMPT_MARKS_MAX`].
    prompt_marks: Vec<usize>,
    /// Structured side-channel (L13) session state: the resource subscriptions a
    /// connected client has registered for change notifications. Carried here
    /// because it must outlive any single channel OSC and ride alongside the grid
    /// state whose changes it reports.
    #[cfg(feature = "l13")]
    pub(crate) channel: super::channel::ChannelState,
    /// Optional status-line overlay set via the L13 `render` protocol; composited
    /// over the bottom row by every renderer. `None` (no allocation) when unset.
    status_line: Option<StatusLine>,
    /// Exit code of the last finished command, reported via OSC 133;D. Surfaced
    /// to a structured client as the `terminal://exit` resource; `None` until a
    /// command finishes (or when the shell omits the code).
    #[cfg(feature = "l13")]
    last_exit: Option<i32>,
    /// Absolute logical line (`scrollback.len() + cursor row`) where the running
    /// command's output began (OSC 133;C), or `None` outside a command. Decays
    /// with scrollback eviction and is remapped across a resize (it rides the
    /// reflow like a prompt mark), so a mid-command resize keeps the capture.
    #[cfg(feature = "l13")]
    command_start: Option<usize>,
    /// Text of the last finished command's output, captured at OSC 133;D and
    /// surfaced as the `terminal://command` resource.
    #[cfg(feature = "l13")]
    last_command_output: Option<String>,
    /// One entry per OSC 133;D (command finished), each carrying that command's
    /// exit code and captured output. Drained FIFO by
    /// [`Grid::pop_finished_command`] so a host emits exactly one
    /// command-finished signal per completion — even when several commands
    /// finish inside a single byte batch (a bool flag would coalesce them and
    /// lose all but the last). Nexus RFC 0003 PR-4.
    #[cfg(feature = "l13")]
    finished_commands: VecDeque<(Option<i32>, Option<String>)>,
    /// Per-row line size attributes (DECDWL/DECDHL); `len() == rows`. The
    /// renderer relays each to the host so double-width/height lines display
    /// correctly, and they shift with the rows they label as the screen scrolls.
    line_attrs: Vec<LineAttr>,
    /// Per-row soft-wrap flags; `len() == rows`. `wrapped[y]` is set when DECAWM
    /// autowrap overflowed row `y` into row `y + 1`, marking the two as one
    /// logical line. Travels with the row through scrolls (alongside
    /// [`Grid::line_attrs`]) and into [`Grid::scrollback`], and is the signal the
    /// reflow uses to rejoin wrapped runs on resize. A *hard* line break (LF,
    /// NEL, IND) leaves it clear.
    wrapped: Vec<bool>,
}

/// Build the default tab-stop table for a `cols`-wide grid: a stop at every
/// 8th column (0, 8, 16, …), matching the classic 8-column default.
fn default_tab_stops(cols: usize) -> Vec<bool> {
    (0..cols).map(|i| i % 8 == 0).collect()
}

/// Per-row size attribute set by `ESC # 3/4/5/6` (DECDHL/DECDWL/DECSWL). A
/// double-width or double-height line renders at twice the cell width, so only
/// the left half of the columns is displayed; the renderer relays the attribute
/// to the host terminal, which does the scaling.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LineAttr {
    /// Normal single-width, single-height (`ESC # 5`, the default).
    #[default]
    Single,
    /// Double-width, single-height (`ESC # 6`).
    DoubleWidth,
    /// Top half of a double-width, double-height line (`ESC # 3`).
    DoubleTop,
    /// Bottom half of a double-width, double-height line (`ESC # 4`).
    DoubleBottom,
}

/// Which DEC private mode selected the alternate screen, which determines the
/// cursor save/restore behaviour on exit (per xterm: only `1049` does it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AltMode {
    /// `?47` — bare buffer swap, no cursor save/restore.
    Dec47,
    /// `?1047` — buffer swap, no cursor save/restore.
    Dec1047,
    /// `?1049` — save cursor on entry (DECSC-style), restore on exit.
    Dec1049,
}

/// Active mouse-reporting mode envelope kept on the grid. The window backend
/// consults this to decide whether a click/drag/scroll is forwarded to the
/// child as an encoded input sequence or handled locally (selection, scroll).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MouseModes {
    /// Base mode: click (`1000`), drag (`1002`), any-event (`1003`).
    pub base: usize,
    /// Extended encoding flags combined from `1005`/`1006`/`1015`/`1016`.
    pub extended: u8,
}

impl MouseModes {
    /// Whether any mouse reporting is active.
    pub fn active(self) -> bool {
        self.base != 0
    }
}

/// The cursor's rendered shape, set by DECSCUSR (`CSI Ps SP q`) or the
/// `cursor_style` config key.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorShape {
    /// Filled block covering the whole cell (the default).
    #[default]
    Block,
    /// A horizontal bar along the bottom of the cell.
    Underline,
    /// A vertical bar (I-beam) at the left of the cell.
    Bar,
}

/// Map a DEC private parameter to its alternate-screen mode, if any.
pub(crate) fn alt_mode(param: usize) -> Option<AltMode> {
    match param {
        47 => Some(AltMode::Dec47),
        1047 => Some(AltMode::Dec1047),
        1049 => Some(AltMode::Dec1049),
        _ => None,
    }
}

/// A stashed screen buffer plus its cursor, used to swap the primary screen out
/// while the alternate screen is active.
struct SavedScreen {
    cells: Vec<Cell>,
    cursor: (usize, usize),
    /// The DECSC/`CSI s` register at the time of the switch, kept separate from
    /// the alternate screen's so the two buffers don't share one save slot.
    saved_cursor: (usize, usize),
    /// The mode that activated the alternate screen, governing exit behaviour.
    mode: AltMode,
    /// Line size attributes of the stashed screen, swapped back on exit.
    line_attrs: Vec<LineAttr>,
    /// Soft-wrap flags of the stashed screen, swapped back on exit. Parallel to
    /// [`SavedScreen::line_attrs`].
    wrapped: Vec<bool>,
}

/// One stored line of scrollback history: the cells as they left the screen plus
/// the soft-wrap bit that says whether the next stored line is a continuation of
/// the same logical line (set by DECAWM autowrap, clear on a hard line break).
pub(crate) struct Line {
    pub cells: Vec<Cell>,
    pub wrapped: bool,
}

/// Clip/extend `old` (sized `old_cols`×`old_rows`) into a fresh `cols`×`rows`
/// buffer, preserving the top-left overlap and blank-filling any new area. Used
/// for the *alternate* screen, whose full-screen apps repaint on resize — there
/// is no logical-line history there to rejoin, so a plain clip is correct.
fn reflow_clip(old: &[Cell], old_cols: usize, old_rows: usize, cols: usize, rows: usize) -> Vec<Cell> {
    let mut new = vec![Cell::blank(); cols * rows];
    let copy_rows = rows.min(old_rows);
    let copy_cols = cols.min(old_cols);
    for y in 0..copy_rows {
        let src = y * old_cols;
        let dst = y * cols;
        new[dst..dst + copy_cols].copy_from_slice(&old[src..src + copy_cols]);
    }
    new
}

/// A cell that contributes nothing to a logical line's content — a plain space
/// in default rendition with no link or grapheme tail. Trailing runs of these
/// are trimmed when rejoining wrapped rows so re-wrapping doesn't preserve dead
/// padding. `flags == 0` excludes both [`WIDE_TRAILER`] and any SGR attribute,
/// so a colored/attributed blank is *kept* (its background is content).
fn is_padding(c: &Cell) -> bool {
    c.ch == ' ' && c.cluster == 0 && c.link == 0 && c.flags == 0
}

/// A pixel image (Sixel/Kitty) placed on the grid: source pixels plus the cell
/// footprint reserved for it, anchored by `serial` (its top cell row's stable
/// scroll position) so it tracks scrollback. Used by the CPU renderer only.
#[cfg(any(test, feature = "gui"))]
pub(crate) struct GridImage {
    pub(crate) serial: usize,
    pub(crate) col: usize,
    pub(crate) cols: usize,
    pub(crate) rows: usize,
    pub(crate) pw: usize,
    pub(crate) ph: usize,
    /// Source pixels, row-major `pw × ph`; `None` = transparent (skipped).
    pub(crate) pixels: Vec<Option<u32>>,
}

/// The product of a wrap-aware reflow: the rebuilt history, the new on-screen
/// buffer with its per-row metadata, the relocated cursor, and the remapped
/// prompt marks.
struct Reflowed {
    scrollback: VecDeque<Line>,
    cells: Vec<Cell>,
    wrapped: Vec<bool>,
    line_attrs: Vec<LineAttr>,
    cursor: (usize, usize),
    prompt_marks: Vec<usize>,
    /// Remapped command-capture anchor; only consumed under the `l13` feature.
    #[cfg_attr(not(feature = "l13"), allow(dead_code))]
    command_start: Option<usize>,
}

/// Re-wrap the combined history `scrollback ++ screen` into `new_cols`×`new_rows`.
///
/// The buffer is a sequence of physical rows; consecutive rows joined by the
/// soft-wrap bit form one *logical line*. We flatten those runs back into
/// logical lines (dropping the one-column filler a wide glyph leaves when it
/// wraps, and trimming trailing padding), re-wrap each to the new width without
/// splitting a double-width glyph across the margin, then re-window the result so
/// the cursor's line stays on screen. The cursor and the OSC-133 prompt marks
/// (physical-row indices into the old `scrollback ++ screen`) are carried through
/// by tracking, respectively, the cursor's offset within its logical line and a
/// physical-row → logical-line → new-physical-row mapping.
#[allow(clippy::too_many_arguments)]
fn reflow_history(
    scrollback: &VecDeque<Line>,
    screen: &[Cell],
    screen_wrapped: &[bool],
    screen_attrs: &[LineAttr],
    old_cols: usize,
    old_rows: usize,
    cursor: (usize, usize),
    prompt_marks: &[usize],
    command_start: Option<usize>,
    new_cols: usize,
    new_rows: usize,
    blank: Cell,
    scrollback_max: usize,
) -> Reflowed {
    let hist = scrollback.len();
    let old_total = hist + old_rows;
    let cursor_phys = hist + cursor.1.min(old_rows.saturating_sub(1));
    // Borrow physical row `i` as (cells, wrapped, line attr). Scrollback lines
    // have no size attribute (a double-width line is meaningless once scrolled
    // off) and may be narrower than `old_cols`; both are handled by the joiner.
    let phys = |i: usize| -> (&[Cell], bool, LineAttr) {
        if i < hist {
            (&scrollback[i].cells, scrollback[i].wrapped, LineAttr::Single)
        } else {
            let y = i - hist;
            (
                &screen[y * old_cols..y * old_cols + old_cols],
                screen_wrapped[y],
                screen_attrs[y],
            )
        }
    };

    // --- Phase 1: flatten physical rows into logical lines. ---
    let mut logical: Vec<Vec<Cell>> = Vec::new();
    let mut logical_attr: Vec<LineAttr> = Vec::new();
    let mut phys_to_logical: Vec<usize> = Vec::with_capacity(old_total);
    let mut cur: Vec<Cell> = Vec::new();
    let mut cur_attr = LineAttr::Single;
    let mut cur_started = false;
    let mut cursor_logical = 0usize;
    let mut cursor_off = 0usize;
    for i in 0..old_total {
        let (cells, wrapped, attr) = phys(i);
        let li = logical.len(); // index this physical row's logical line will get
        phys_to_logical.push(li);
        if !cur_started {
            cur_attr = attr;
            cur_started = true;
        }
        if i == cursor_phys {
            cursor_logical = li;
            cursor_off = cur.len() + cursor.0.min(old_cols);
        }
        // Append this row's content. A wrapped row that ends in one padding cell
        // because a wide glyph didn't fit drops that filler — detected precisely
        // by the next row beginning with a width-2 glyph.
        let mut take = cells.len();
        if wrapped && take > 0 && is_padding(&cells[take - 1]) {
            let next_wide = i + 1 < old_total && {
                let (nc, _, _) = phys(i + 1);
                !nc.is_empty() && char_width(nc[0].ch) == 2
            };
            if next_wide {
                take -= 1;
            }
        }
        cur.extend_from_slice(&cells[..take]);
        if wrapped {
            continue; // logical line continues on the next physical row
        }
        // Hard break: close the logical line, trimming trailing padding but never
        // past the cursor's column on the cursor's own line.
        let min_keep = if li == cursor_logical { cursor_off } else { 0 };
        let mut end = cur.len();
        while end > min_keep && is_padding(&cur[end - 1]) {
            end -= 1;
        }
        cur.truncate(end);
        logical.push(std::mem::take(&mut cur));
        logical_attr.push(cur_attr);
        cur_started = false;
    }
    if cur_started {
        // A trailing wrapped run with no closing hard break (shouldn't normally
        // happen, since a wrap always creates the next row): keep it anyway.
        let min_keep = if logical.len() == cursor_logical { cursor_off } else { 0 };
        let mut end = cur.len();
        while end > min_keep && is_padding(&cur[end - 1]) {
            end -= 1;
        }
        cur.truncate(end);
        logical.push(cur);
        logical_attr.push(cur_attr);
    }

    // Drop blank logical lines trailing *below* the cursor: those are the screen's
    // empty tail (padding under the last output), not history. Lines at or above
    // the cursor — including intentional mid-content blank lines — are kept, so a
    // resize doesn't sweep real content into scrollback just to fill the window.
    while logical.len() > cursor_logical + 1 && logical.last().is_some_and(|l| l.is_empty()) {
        logical.pop();
        logical_attr.pop();
    }

    // --- Phase 2: re-wrap each logical line to the new width. ---
    let mut out_cells: Vec<Vec<Cell>> = Vec::new();
    let mut out_wrapped: Vec<bool> = Vec::new();
    let mut out_attr: Vec<LineAttr> = Vec::new();
    let mut logical_first_row: Vec<usize> = vec![0; logical.len()];
    let mut ncur = (0usize, 0usize);
    let mut cursor_set = false;
    for (li, line) in logical.iter().enumerate() {
        logical_first_row[li] = out_cells.len();
        let attr = logical_attr[li];
        if line.is_empty() {
            if li == cursor_logical && !cursor_set {
                ncur = (out_cells.len(), 0);
                cursor_set = true;
            }
            out_cells.push(vec![blank; new_cols]);
            out_wrapped.push(false);
            out_attr.push(attr);
            continue;
        }
        let mut i = 0;
        let mut seg_start = 0;
        while i < line.len() {
            let mut take = new_cols.min(line.len() - i);
            // Don't cut a double-width glyph from its trailer: if the cell just
            // past the segment is a trailer, push its head to the next row.
            if i + take < line.len() && line[i + take].flags & WIDE_TRAILER != 0 && take > 1 {
                take -= 1;
            }
            if take == 0 {
                take = 1; // degenerate new_cols == 1 vs a wide glyph: force progress
            }
            let mut row = vec![blank; new_cols];
            let n = take.min(new_cols);
            row[..n].copy_from_slice(&line[i..i + n]);
            seg_start = i;
            if li == cursor_logical && !cursor_set && cursor_off >= i && cursor_off < i + take {
                ncur = (out_cells.len(), (cursor_off - i).min(new_cols - 1));
                cursor_set = true;
            }
            let more = i + take < line.len();
            out_cells.push(row);
            out_wrapped.push(more);
            out_attr.push(attr);
            i += take;
        }
        if li == cursor_logical && !cursor_set {
            // Cursor sat at or past the last cell of its (trimmed) line.
            let col = (cursor_off - seg_start).min(new_cols - 1);
            ncur = (out_cells.len() - 1, col);
            cursor_set = true;
        }
    }
    let total = out_cells.len();
    if !cursor_set {
        ncur = (total.saturating_sub(1), 0);
    }

    // --- Phase 3: re-window into screen + scrollback, keeping the cursor visible. ---
    let mut screen_top = total.saturating_sub(new_rows);
    if ncur.0 < screen_top {
        screen_top = ncur.0; // cursor would be above the window: pull the window up
    }
    let dropped = screen_top.saturating_sub(scrollback_max);
    let mut new_scrollback: VecDeque<Line> = VecDeque::new();
    let mut cells = vec![blank; new_cols * new_rows];
    let mut wrapped = vec![false; new_rows];
    let mut line_attrs = vec![LineAttr::Single; new_rows];
    for (r, row) in out_cells.into_iter().enumerate() {
        if r < dropped {
            continue; // oldest history beyond the scrollback cap
        }
        if r < screen_top {
            new_scrollback.push_back(Line {
                cells: row,
                wrapped: out_wrapped[r],
            });
        } else if r < screen_top + new_rows {
            let y = r - screen_top;
            cells[y * new_cols..(y + 1) * new_cols].copy_from_slice(&row);
            wrapped[y] = out_wrapped[r];
            line_attrs[y] = out_attr[r];
        }
        // r >= screen_top + new_rows: below the cursor's window, dropped.
    }
    let cy = ncur.0.saturating_sub(screen_top).min(new_rows - 1);
    let cx = ncur.1.min(new_cols - 1);

    // --- Phase 4: remap line indices (old physical row → logical → new row). ---
    // A physical row maps to the first new row of its logical line, minus the
    // scrollback dropped off the front; a row past the kept logical lines (a
    // dropped trailing blank) or below the cap has no image.
    let remap = |m: usize| -> Option<usize> {
        if m < phys_to_logical.len() && phys_to_logical[m] < logical_first_row.len() {
            let outrow = logical_first_row[phys_to_logical[m]];
            (outrow >= dropped).then(|| outrow - dropped)
        } else {
            None
        }
    };
    let mut new_marks: Vec<usize> = prompt_marks.iter().filter_map(|&m| remap(m)).collect();
    new_marks.sort_unstable();
    new_marks.dedup();
    if new_marks.len() > PROMPT_MARKS_MAX {
        new_marks.drain(0..new_marks.len() - PROMPT_MARKS_MAX);
    }
    let new_command_start = command_start.and_then(remap);

    Reflowed {
        scrollback: new_scrollback,
        cells,
        wrapped,
        line_attrs,
        cursor: (cx, cy),
        prompt_marks: new_marks,
        command_start: new_command_start,
    }
}

/// A terminal-owned status-line overlay (L13 `render` protocol): one row of
/// pre-rendered cells the renderer composites over the bottom of the live
/// screen, independent of the child's output stream. Kept pre-rendered so the
/// hot render path just borrows the slice; re-laid from `text`/`fg`/`bg` on a
/// resize.
pub(crate) struct StatusLine {
    text: String,
    fg: u32,
    bg: u32,
    cells: Vec<Cell>,
}

impl StatusLine {
    /// Lay `text` out into exactly `cols` cells in `fg`/`bg`: advance wide glyphs
    /// by two columns with a flagged trailer, drop zero-width scalars, stop at the
    /// margin, and pad the tail with blanks in `bg`.
    fn lay_out(text: &str, fg: u32, bg: u32, cols: usize) -> Vec<Cell> {
        let blank = Cell { ch: ' ', cluster: 0, fg, bg, flags: 0, link: 0 };
        let mut cells = vec![blank; cols];
        let mut x = 0;
        for ch in text.chars() {
            let w = char_width(ch);
            if w == 0 {
                continue;
            }
            if x + w > cols {
                break;
            }
            cells[x] = Cell { ch, cluster: 0, fg, bg, flags: 0, link: 0 };
            if w == 2 {
                cells[x + 1] = Cell { ch: ' ', cluster: 0, fg, bg, flags: WIDE_TRAILER, link: 0 };
            }
            x += w;
        }
        cells
    }

    #[cfg(feature = "l13")]
    fn new(text: String, fg: u32, bg: u32, cols: usize) -> Self {
        let cells = Self::lay_out(&text, fg, bg, cols);
        StatusLine { text, fg, bg, cells }
    }

    /// Re-lay the existing text/colors at a new width (after a resize).
    fn relayout(&mut self, cols: usize) {
        self.cells = Self::lay_out(&self.text, self.fg, self.bg, cols);
    }
}

impl Grid {
    /// Create a `cols`×`rows` grid filled with blank cells.
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            cols,
            rows,
            cells: vec![Cell::blank(); cols * rows],
            dirty: vec![false; rows],
            cursor: (0, 0),
            saved_cursor: (0, 0),
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
            primary: None,
            epoch: 0,
            title: String::new(),
            cwd: String::new(),
            scrollback: VecDeque::new(),
            scrollback_max: SCROLLBACK_MAX,
            view_offset: 0,
            selection: None,
            #[cfg(any(test, feature = "gui"))]
            search: None,
            #[cfg(any(test, feature = "gui"))]
            total_scrolled: 0,
            #[cfg(any(test, feature = "gui"))]
            images: Vec::new(),
            host_out: Vec::new(),
            links: Vec::new(),
            clusters: Vec::new(),
            current_link: 0,
            tab_stops: default_tab_stops(cols),
            cursor_visible: true,
            bracketed_paste: false,
            clipboard_set: None,
            clipboard_query: false,
            notifications: Vec::new(),
            ime_preedit: String::new(),
            mouse_modes: MouseModes::default(),
            autowrap: true,
            origin_mode: false,
            insert_mode: false,
            default_fg: DEFAULT_FG,
            default_bg: DEFAULT_BG,
            cursor_color: DEFAULT_FG,
            cursor_shape: CursorShape::default(),
            cursor_blink: false,
            default_cursor_shape: CursorShape::default(),
            default_cursor_blink: false,
            line_attrs: vec![LineAttr::Single; rows],
            wrapped: vec![false; rows],
            prompt_marks: Vec::new(),
            #[cfg(feature = "l13")]
            channel: super::channel::ChannelState::default(),
            status_line: None,
            #[cfg(feature = "l13")]
            last_exit: None,
            #[cfg(feature = "l13")]
            command_start: None,
            #[cfg(feature = "l13")]
            last_command_output: None,
            #[cfg(feature = "l13")]
            finished_commands: VecDeque::new(),
        }
    }

    /// Write `cell` at `(x, y)`, marking the row dirty. Out-of-bounds writes
    /// are silently ignored (the caller is responsible for clamping).
    ///
    /// If the write lands on one half of an existing double-width glyph, the
    /// orphaned partner cell is blanked so no stale head/trailer is left behind.
    pub fn set_cell(&mut self, x: usize, y: usize, cell: Cell) {
        if x >= self.cols || y >= self.rows {
            return;
        }
        let idx = y * self.cols + x;
        if self.cells[idx].flags & WIDE_TRAILER != 0 {
            // Overwriting the trailing half: blank the head to its left.
            if x >= 1 {
                self.cells[idx - 1] = Cell::blank();
            }
        } else if x + 1 < self.cols && self.cells[idx + 1].flags & WIDE_TRAILER != 0 {
            // Overwriting the leading half: blank the orphaned trailer to its right.
            self.cells[idx + 1] = Cell::blank();
        }
        self.cells[idx] = cell;
        self.dirty[y] = true;
    }

    /// Move the cursor to `(x, y)`, clamping into the grid so a malformed or
    /// oversized positioning sequence can never park the cursor off-screen.
    pub fn set_cursor(&mut self, x: usize, y: usize) {
        self.cursor = (
            x.min(self.cols.saturating_sub(1)),
            y.min(self.rows.saturating_sub(1)),
        );
    }

    /// Absolute cursor positioning that honors origin mode (DECOM). `col`/`row`
    /// are 0-based. With origin mode on, `row` is relative to the scroll region
    /// top and the cursor is confined to the region; otherwise it is screen-
    /// absolute. Used by `CUP`/`HVP` and `VPA`.
    pub(crate) fn set_cursor_abs(&mut self, col: usize, row: usize) {
        let y = if self.origin_mode {
            (self.scroll_top + row).min(self.scroll_bottom)
        } else {
            row.min(self.rows.saturating_sub(1))
        };
        self.cursor = (col.min(self.cols.saturating_sub(1)), y);
    }

    /// Move the cursor up `n` rows (`CUU`). The scroll region's top margin is a
    /// floor when the cursor starts at or below it, so cursor-up can't escape
    /// the region; above the region it floors at row 0.
    pub(crate) fn cursor_up(&mut self, n: usize) {
        let floor = if self.cursor.1 >= self.scroll_top {
            self.scroll_top
        } else {
            0
        };
        self.cursor.1 = self.cursor.1.saturating_sub(n).max(floor);
    }

    /// Move the cursor down `n` rows (`CUD`). The scroll region's bottom margin
    /// is a ceiling when the cursor starts at or above it; below the region it
    /// ceilings at the last row.
    pub(crate) fn cursor_down(&mut self, n: usize) {
        let ceil = if self.cursor.1 <= self.scroll_bottom {
            self.scroll_bottom
        } else {
            self.rows.saturating_sub(1)
        };
        self.cursor.1 = self.cursor.1.saturating_add(n).min(ceil);
    }

    /// The cursor home position: the top-left of the screen, or of the scroll
    /// region when origin mode is on.
    fn home_position(&self) -> (usize, usize) {
        (0, if self.origin_mode { self.scroll_top } else { 0 })
    }

    /// Enable or disable origin mode (DECOM), moving the cursor to the (now
    /// possibly origin-relative) home position as the spec requires.
    pub(crate) fn set_origin_mode(&mut self, on: bool) {
        self.origin_mode = on;
        self.cursor = self.home_position();
    }

    /// Move the cursor to column 0 of the current row.
    pub fn carriage_return(&mut self) {
        self.cursor.0 = 0;
    }

    /// Advance the cursor one row. At the bottom of the scrolling region this
    /// scrolls the region up instead of moving the cursor past it.
    pub fn newline(&mut self) {
        if self.cursor.1 == self.scroll_bottom {
            self.scroll_up();
        } else if self.cursor.1 + 1 < self.rows {
            self.cursor.1 += 1;
        }
    }

    /// Render an RGB(A) image at the cursor as truecolor half-block glyphs
    /// (`▀`/`▄`) — one cell per pixel column, two pixel rows per cell. `pixels`
    /// is row-major `width × height`, `None` meaning transparent. The image is
    /// shrunk to fit the columns remaining from the cursor (aspect-preserving,
    /// never enlarged), placed top-left at the cursor, and scrolled like printed
    /// lines if it runs past the bottom; the cursor ends at column 0 of the row
    /// below it (xterm "sixel scrolling"). Shared by the Sixel and Kitty paths.
    pub(crate) fn render_image(&mut self, width: usize, height: usize, pixels: &[Option<u32>]) {
        if width == 0 || height == 0 || pixels.len() < width * height {
            return;
        }
        // Nearest-neighbor source sample for target pixel `(tx, ty)`.
        let sample = |tx: usize, ty: usize, tw: usize, th: usize| -> Option<u32> {
            if ty >= th {
                return None;
            }
            pixels[(ty * height / th) * width + (tx * width / tw)]
        };
        // Combine the two pixels of one cell into a half-block glyph; an unset
        // half takes the default background, both unset leaves the cell alone.
        let half_block = |top: Option<u32>, bottom: Option<u32>, def_bg: u32| {
            let mk = |ch, fg, bg| Cell {
                ch,
                cluster: 0,
                fg,
                bg,
                flags: 0,
                link: 0,
            };
            match (top, bottom) {
                (None, None) => None,
                (Some(t), Some(b)) => Some(mk('\u{2580}', t, b)),
                (Some(t), None) => Some(mk('\u{2580}', t, def_bg)),
                (None, Some(b)) => Some(mk('\u{2584}', b, def_bg)),
            }
        };

        let origin = self.cursor.0;
        let avail = self.cols.saturating_sub(origin).max(1);
        // Fit to the available width (shrink only), preserving aspect.
        let tw = width.min(avail);
        let th = (height * tw / width).max(1);
        let cell_rows = th.div_ceil(2);
        #[cfg(any(test, feature = "gui"))]
        self.store_image(width, height, pixels, origin, tw, cell_rows);

        for cr in 0..cell_rows {
            let y = self.cursor.1;
            for cc in 0..tw {
                let col = origin + cc;
                if col >= self.cols {
                    break;
                }
                let top = sample(cc, cr * 2, tw, th);
                let bottom = sample(cc, cr * 2 + 1, tw, th);
                if let Some(cell) = half_block(top, bottom, self.default_bg) {
                    self.set_cell(col, y, cell);
                }
            }
            self.newline();
        }
        self.carriage_return();
    }

    /// Render a decoded Sixel image (delegates to [`Grid::render_image`]).
    pub(crate) fn render_sixel(&mut self, img: &SixelImage) {
        self.render_image(img.width, img.height, &img.pixels);
    }

    /// Store a placed pixel image for the CPU renderer to overlay over its
    /// reserved half-block cells, anchored by serial (top cell row) so it
    /// scrolls with text. Bounded — the oldest image is dropped past the cap.
    #[cfg(any(test, feature = "gui"))]
    fn store_image(&mut self, pw: usize, ph: usize, pixels: &[Option<u32>], col: usize, cols: usize, rows: usize) {
        const MAX_IMAGES: usize = 8;
        let serial = self.total_scrolled + self.cursor.1;
        self.images.push(GridImage { serial, col, cols, rows, pw, ph, pixels: pixels[..pw * ph].to_vec() });
        if self.images.len() > MAX_IMAGES {
            self.images.remove(0);
        }
    }

    /// Drop images that have scrolled out of the retained scrollback.
    #[cfg(any(test, feature = "gui"))]
    fn evict_scrolled_images(&mut self) {
        let oldest = self.total_scrolled.saturating_sub(self.scrollback.len());
        self.images.retain(|im| im.serial + im.rows > oldest);
    }

    /// Placed pixel images for the CPU renderer to composite over the cells.
    #[cfg(any(test, feature = "gui"))]
    pub(crate) fn images(&self) -> &[GridImage] {
        &self.images
    }

    /// The viewport row of image `im`'s top cell (negative when scrolled partly
    /// above the view).
    #[cfg(any(test, feature = "gui"))]
    pub(crate) fn image_top_row(&self, im: &GridImage) -> isize {
        let off = self.view_offset.min(self.scrollback.len());
        im.serial as isize - self.total_scrolled as isize + off as isize
    }

    /// Scroll the current scrolling region up by one row, blanking the freed
    /// bottom row of the region. Only the region's rows are marked dirty.
    pub fn scroll_up(&mut self) {
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        if bottom <= top || bottom >= self.rows {
            return;
        }
        // Capture the line leaving the top into scrollback, but only for a
        // full-screen scroll on the primary buffer: partial-region scrolls
        // (TUI apps using DECSTBM) and the alternate screen don't form history.
        if top == 0 && bottom == self.rows - 1 && self.primary.is_none() {
            self.scrollback.push_back(Line {
                cells: self.cells[0..self.cols].to_vec(),
                wrapped: self.wrapped[top],
            });
            if self.scrollback.len() > self.scrollback_max {
                self.scrollback.pop_front();
                self.evict_prompt_mark();
                #[cfg(feature = "l13")]
                if let Some(s) = &mut self.command_start {
                    *s = s.saturating_sub(1);
                }
            }
            #[cfg(any(test, feature = "gui"))]
            {
                self.total_scrolled += 1;
                self.evict_scrolled_images();
            }
            // If the user is browsing history, advance the offset in step with
            // the incoming line so the viewed region stays put under new output.
            if self.view_offset > 0 {
                self.view_offset = (self.view_offset + 1).min(self.scrollback.len());
                self.dirty.iter_mut().for_each(|d| *d = true);
            }
        }
        let src = (top + 1) * self.cols;
        let dst = top * self.cols;
        let count = (bottom - top) * self.cols;
        self.cells.copy_within(src..src + count, dst);
        let last = bottom * self.cols;
        let blank = self.erase_cell();
        for c in &mut self.cells[last..last + self.cols] {
            *c = blank;
        }
        self.shift_line_meta(top + 1, top, bottom - top, bottom..bottom + 1);
        for d in &mut self.dirty[top..=bottom] {
            *d = true;
        }
    }

    /// Set the scrolling region to rows `top..=bottom` (0-based, inclusive) and
    /// home the cursor. An invalid range resets the region to the full screen.
    pub(crate) fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let bottom = bottom.min(self.rows.saturating_sub(1));
        if top < bottom {
            self.scroll_top = top;
            self.scroll_bottom = bottom;
        } else {
            self.scroll_top = 0;
            self.scroll_bottom = self.rows.saturating_sub(1);
        }
        self.cursor = self.home_position();
    }

    /// Reset the scrolling region to span the full screen.
    fn reset_scroll_region(&mut self) {
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);
    }

    /// Write `ch` at the cursor with the given [`Pen`], wrapping to the next
    /// line if it would not fit, then advancing the cursor by the glyph's
    /// display width. A double-width glyph also writes a flagged trailing cell.
    pub fn put_char(&mut self, ch: char, pen: Pen) {
        // A non-ASCII scalar may continue the grapheme in the cell to the left
        // (combining mark, ZWJ join, skin tone, variation selector). ASCII is
        // always its own grapheme, so the common path skips the check entirely.
        if !ch.is_ascii() {
            if let Some((bx, by)) = self.left_base()
                && self.continues_grapheme(bx, by, ch)
            {
                self.append_to_glyph(bx, by, ch);
                return;
            }
            // Not a continuation: a zero-width scalar has no cell of its own.
            if char_width(ch) == 0 {
                return;
            }
        }
        let w = char_width(ch); // >= 1 here (zero-width non-continuations dropped above)
        if self.cursor.0 + w > self.cols {
            if self.autowrap {
                // Mark this row as soft-wrapped before leaving it, so the reflow
                // and scrollback know it and its successor are one logical line.
                if self.cursor.1 < self.wrapped.len() {
                    self.wrapped[self.cursor.1] = true;
                }
                self.carriage_return();
                self.newline();
            } else {
                // Autowrap off: keep the glyph in the last cell(s) of this row.
                self.cursor.0 = self.cols.saturating_sub(w);
            }
        }
        // Insert mode (IRM): make room by shifting the rest of the row right.
        if self.insert_mode {
            self.insert_chars(w);
        }
        let (x, y) = self.cursor;
        let link = self.current_link;
        self.set_cell(
            x,
            y,
            Cell {
                ch,
                cluster: 0,
                fg: pen.fg,
                bg: pen.bg,
                flags: pen.attrs,
                link,
            },
        );
        if w == 2 && x + 1 < self.cols {
            // Trailing half: a flagged placeholder the renderer skips. It keeps
            // the pen's colors but only the WIDE_TRAILER layout bit.
            self.set_cell(
                x + 1,
                y,
                Cell {
                    ch: ' ',
                    cluster: 0,
                    fg: pen.fg,
                    bg: pen.bg,
                    flags: WIDE_TRAILER,
                    link,
                },
            );
        }
        self.cursor.0 += w;
    }

    /// The base cell of the grapheme immediately left of the cursor, stepping
    /// back over a wide glyph's trailer to its head. `None` at column 0.
    fn left_base(&self) -> Option<(usize, usize)> {
        let (cx, cy) = self.cursor;
        if cy >= self.rows || cx == 0 {
            return None;
        }
        let left = cx - 1;
        if self.cells[cy * self.cols + left].flags & WIDE_TRAILER != 0 && left >= 1 {
            Some((left - 1, cy)) // land on the wide glyph's head, not its trailer
        } else {
            Some((left, cy))
        }
    }

    /// Whether appending `next` to the glyph at `(x, y)` keeps it a single
    /// grapheme cluster (UAX #29) — i.e. `next` continues that glyph rather than
    /// starting a new one.
    fn continues_grapheme(&self, x: usize, y: usize, next: char) -> bool {
        let mut s = self.glyph_text(x, y);
        s.push(next);
        s.graphemes(true).count() == 1
    }

    /// The full glyph text at `(x, y)`: the base scalar plus any interned
    /// grapheme continuation.
    fn glyph_text(&self, x: usize, y: usize) -> String {
        let cell = self.cells[y * self.cols + x];
        let mut s = String::new();
        s.push(cell.ch);
        if cell.cluster != 0
            && let Some(suffix) = self.clusters.get((cell.cluster - 1) as usize)
        {
            s.push_str(suffix);
        }
        s
    }

    /// Normalize the active selection into an inclusive `(start, end)` pair of
    /// `(col, row)` cells in row-major order, clamped to the grid. `None` when
    /// there is no selection.
    #[cfg(any(test, feature = "gui"))]
    fn selection_bounds(&self) -> Option<((usize, usize), (usize, usize))> {
        let sel = self.selection?;
        let clamp = |(c, r): (usize, usize)| {
            (c.min(self.cols.saturating_sub(1)), r.min(self.rows.saturating_sub(1)))
        };
        let a = clamp(sel.anchor);
        let b = clamp(sel.head);
        // Row-major linear order, so a backward drag still yields start <= end.
        if (a.1, a.0) <= (b.1, b.0) { Some((a, b)) } else { Some((b, a)) }
    }

    /// Whether the cell at `(col, row)` lies within the active selection
    /// (stream/linear order — full intermediate rows are included). Read by the
    /// windowed renderer to invert highlighted cells.
    #[cfg(any(test, feature = "gui"))]
    pub fn is_selected(&self, col: usize, row: usize) -> bool {
        let Some((start, end)) = self.selection_bounds() else {
            return false;
        };
        let lin = |c: usize, r: usize| r * self.cols + c;
        (lin(start.0, start.1)..=lin(end.0, end.1)).contains(&lin(col, row))
    }

    /// The selected text, or `None` when nothing is selected. Lines join with
    /// `\n`; per-line trailing blanks are trimmed, wide-glyph trailers skipped,
    /// and grapheme continuations preserved.
    #[cfg(any(test, feature = "gui"))]
    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_bounds()?;
        let mut out = String::new();
        for row in start.1..=end.1 {
            let c0 = if row == start.1 { start.0 } else { 0 };
            let c1 = if row == end.1 { end.0 } else { self.cols - 1 };
            let mut line = String::new();
            for col in c0..=c1 {
                let cell = self.cells[row * self.cols + col];
                if cell.flags & WIDE_TRAILER != 0 {
                    continue;
                }
                line.push_str(&self.glyph_text(col, row));
            }
            if row != start.1 {
                out.push('\n');
            }
            out.push_str(line.trim_end());
        }
        Some(out)
    }

    /// Append `ch` to the grapheme continuation of the cell at `(x, y)`,
    /// re-interning the grown suffix and marking the row dirty.
    fn append_to_glyph(&mut self, x: usize, y: usize, ch: char) {
        let idx = y * self.cols + x;
        let mut suffix = match self.cells[idx].cluster {
            0 => String::new(),
            id => self
                .clusters
                .get((id - 1) as usize)
                .cloned()
                .unwrap_or_default(),
        };
        suffix.push(ch);
        let id = self.intern_cluster(suffix);
        self.cells[idx].cluster = id;
        self.dirty[y] = true;
    }

    /// Return the id for grapheme continuation `suffix`, interning it on first
    /// use. Ids are `index + 1` so `0` means "no continuation". Returns `0` once
    /// the table is full (the continuation is dropped; the base glyph remains).
    fn intern_cluster(&mut self, suffix: String) -> u16 {
        if let Some(i) = self.clusters.iter().position(|c| *c == suffix) {
            return (i + 1) as u16;
        }
        if self.clusters.len() >= CLUSTER_MAX {
            return 0;
        }
        self.clusters.push(suffix);
        self.clusters.len() as u16
    }

    /// Blank columns `[from, to)` of row `y`, marking it dirty.
    pub(crate) fn clear_row_range(&mut self, y: usize, from: usize, to: usize) {
        if y >= self.rows {
            return;
        }
        let to = to.min(self.cols);
        if from >= to {
            return;
        }
        let base = y * self.cols;
        let blank = self.erase_cell();
        for c in &mut self.cells[base + from..base + to] {
            *c = blank;
        }
        // Erasing through the right margin ends the line here: it no longer
        // wraps into the row below (EL, the tail of ED, ECH at the margin).
        if to >= self.cols {
            self.wrapped[y] = false;
        }
        self.dirty[y] = true;
    }

    /// Blank every cell and home the cursor (used by `CSI 2 J`).
    pub(crate) fn clear_all(&mut self) {
        let blank = self.erase_cell();
        self.cells.fill(blank);
        #[cfg(any(test, feature = "gui"))]
        self.images.clear();
        self.wrapped.iter_mut().for_each(|w| *w = false);
        self.dirty.iter_mut().for_each(|d| *d = true);
        self.cursor = (0, 0);
    }

    /// A blank cell painted in the current default colors — the fill used by
    /// every erase / scroll-clear path, so a default background set via OSC 11
    /// applies to cleared regions, not only to text.
    fn erase_cell(&self) -> Cell {
        let mut c = Cell::blank();
        c.fg = self.default_fg;
        c.bg = self.default_bg;
        c
    }

    /// Update the default foreground/background/cursor colors (OSC 10/11/12).
    /// Mirrors the parser's palette so subsequent erases fill with the new
    /// background and the windowed cursor paints in the new cursor color.
    pub(crate) fn set_default_colors(&mut self, fg: u32, bg: u32, cursor: u32) {
        self.default_fg = fg;
        self.default_bg = bg;
        if self.cursor_color != cursor {
            self.cursor_color = cursor;
            // Repaint the cursor cell's row so a pure OSC 12 change shows.
            let row = self.cursor.1;
            if let Some(d) = self.dirty.get_mut(row) {
                *d = true;
            }
        }
    }

    /// Apply DECSCUSR (`CSI Ps SP q`): set the cursor shape and blink, repainting
    /// the cursor's row so the change shows.
    pub(crate) fn set_cursor_style(&mut self, shape: CursorShape, blink: bool) {
        self.cursor_shape = shape;
        self.cursor_blink = blink;
        if let Some(d) = self.dirty.get_mut(self.cursor.1) {
            *d = true;
        }
    }

    /// Set the power-on default cursor (from config) and apply it now. RIS and
    /// DECSTR restore to this default.
    pub fn set_default_cursor(&mut self, shape: CursorShape, blink: bool) {
        self.default_cursor_shape = shape;
        self.default_cursor_blink = blink;
        self.set_cursor_style(shape, blink);
    }

    /// Override the scrollback line cap (the `scrollback` config key). `0`
    /// disables history. An already-overfull buffer is trimmed immediately.
    pub fn set_scrollback_max(&mut self, max: usize) {
        self.scrollback_max = max;
        while self.scrollback.len() > max {
            self.scrollback.pop_front();
            self.evict_prompt_mark();
            #[cfg(feature = "l13")]
            if let Some(s) = &mut self.command_start {
                *s = s.saturating_sub(1);
            }
        }
        self.view_offset = self.view_offset.min(self.scrollback.len());
    }

    /// Seed the grid's default colors from the configured startup theme and
    /// repaint the (still pristine) screen in them. Called once at startup,
    /// before any child output is parsed — the screen is all blank cells, so
    /// refilling is exact, not lossy.
    pub fn apply_theme(&mut self, theme: &super::Theme) {
        self.set_default_colors(theme.fg, theme.bg, theme.cursor);
        self.cells = vec![self.erase_cell(); self.cols * self.rows];
        self.dirty = vec![true; self.rows];
    }

    /// Live theme switch (config reload): remap every color that resolves to
    /// an `old`-theme entry — across the live screen, scrollback, and the
    /// stashed primary screen — to its `new`-theme counterpart, and adopt the
    /// new defaults. Truecolor/256-cube colors pass through untouched (we
    /// can't know they meant "the theme's red"). Full repaint requested.
    pub fn retheme(&mut self, old: &super::Theme, new: &super::Theme) {
        let map = |c: &mut u32| *c = super::color::remap(*c, old, new);
        for cell in &mut self.cells {
            map(&mut cell.fg);
            map(&mut cell.bg);
        }
        for line in &mut self.scrollback {
            for cell in &mut line.cells {
                map(&mut cell.fg);
                map(&mut cell.bg);
            }
        }
        if let Some(primary) = &mut self.primary {
            for cell in &mut primary.cells {
                map(&mut cell.fg);
                map(&mut cell.bg);
            }
        }
        map(&mut self.default_fg);
        map(&mut self.default_bg);
        self.cursor_color = super::color::remap(self.cursor_color, old, new);
        self.dirty = vec![true; self.rows];
        self.epoch += 1;
    }

    /// Resize the grid to `cols`×`rows`.
    ///
    /// On the primary screen this is a **wrap-aware reflow**: soft-wrapped runs
    /// across scrollback + the live screen are rejoined into logical lines and
    /// re-wrapped to the new width, so narrowing rewraps long lines (pushing the
    /// overflow into history) and widening pulls wrapped continuations — and
    /// lines back out of scrollback — instead of truncating. The cursor, prompt
    /// marks, and per-row line-size attributes ride through the reflow. The saved
    /// (DECSC) cursor is clamped. Every row is marked dirty for a full repaint.
    ///
    /// On the alternate screen the live buffer is merely clipped/extended (its
    /// full-screen app repaints on resize); the *stashed* primary is reflowed so
    /// it is correct when the app exits the alt screen.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        if cols == 0 || rows == 0 || (cols == self.cols && rows == self.rows) {
            return;
        }
        #[cfg(any(test, feature = "gui"))]
        self.images.clear(); // serials/reflow change; drop placed images
        let (old_cols, old_rows) = (self.cols, self.rows);
        let clamp = |(x, y): (usize, usize)| (x.min(cols - 1), y.min(rows - 1));
        let blank = self.erase_cell();
        // Preserve tab stops within the surviving width; default the new columns.
        let mut stops = default_tab_stops(cols);
        let keep = cols.min(old_cols);
        stops[..keep].copy_from_slice(&self.tab_stops[..keep]);
        self.tab_stops = stops;

        // The in-flight command-capture anchor (l13) rides the reflow like a
        // prompt mark, so a mid-command resize preserves the capture.
        #[cfg(feature = "l13")]
        let cmd_start = self.command_start;
        #[cfg(not(feature = "l13"))]
        let cmd_start: Option<usize> = None;
        if self.primary.is_none() {
            let r = reflow_history(
                &self.scrollback,
                &self.cells,
                &self.wrapped,
                &self.line_attrs,
                old_cols,
                old_rows,
                self.cursor,
                &self.prompt_marks,
                cmd_start,
                cols,
                rows,
                blank,
                self.scrollback_max,
            );
            self.scrollback = r.scrollback;
            self.cells = r.cells;
            self.wrapped = r.wrapped;
            self.line_attrs = r.line_attrs;
            self.cursor = r.cursor;
            self.prompt_marks = r.prompt_marks;
            #[cfg(feature = "l13")]
            {
                self.command_start = r.command_start;
            }
            self.saved_cursor = clamp(self.saved_cursor);
        } else {
            let primary = self.primary.take().expect("primary present");
            let r = reflow_history(
                &self.scrollback,
                &primary.cells,
                &primary.wrapped,
                &primary.line_attrs,
                old_cols,
                old_rows,
                primary.cursor,
                &self.prompt_marks,
                cmd_start,
                cols,
                rows,
                blank,
                self.scrollback_max,
            );
            self.scrollback = r.scrollback;
            self.prompt_marks = r.prompt_marks;
            #[cfg(feature = "l13")]
            {
                self.command_start = r.command_start;
            }
            self.primary = Some(SavedScreen {
                cells: r.cells,
                cursor: r.cursor,
                saved_cursor: clamp(primary.saved_cursor),
                mode: primary.mode,
                line_attrs: r.line_attrs,
                wrapped: r.wrapped,
            });
            self.cells = reflow_clip(&self.cells, old_cols, old_rows, cols, rows);
            self.line_attrs = vec![LineAttr::Single; rows];
            self.wrapped = vec![false; rows];
            self.cursor = clamp(self.cursor);
            self.saved_cursor = clamp(self.saved_cursor);
        }

        self.cols = cols;
        self.rows = rows;
        self.dirty = vec![true; rows];
        self.view_offset = 0;
        if let Some(s) = &mut self.status_line {
            s.relayout(cols);
        }
        self.reset_scroll_region();
    }

    /// Switch to the alternate screen, stashing the primary buffer. Only
    /// `?1049` saves the cursor and homes it; `?47`/`?1047` swap the buffer and
    /// leave the cursor where it is. No-op if already on the alternate screen.
    pub(crate) fn enter_alt_screen(&mut self, mode: AltMode) {
        if self.primary.is_some() {
            return;
        }
        #[cfg(any(test, feature = "gui"))]
        self.images.clear();
        let blank = self.erase_cell();
        self.primary = Some(SavedScreen {
            cells: std::mem::replace(&mut self.cells, vec![blank; self.cols * self.rows]),
            cursor: self.cursor,
            saved_cursor: self.saved_cursor,
            mode,
            line_attrs: std::mem::replace(&mut self.line_attrs, vec![LineAttr::Single; self.rows]),
            wrapped: std::mem::replace(&mut self.wrapped, vec![false; self.rows]),
        });
        if mode == AltMode::Dec1049 {
            self.cursor = (0, 0);
        }
        // History isn't browsable under a full-screen app; snap to the live view.
        self.view_offset = 0;
        self.reset_scroll_region();
        self.dirty.iter_mut().for_each(|d| *d = true);
    }

    /// Switch back to the primary screen, restoring its buffer. For `?1049` the
    /// cursor (and DECSC register) saved on entry are restored; `?47`/`?1047`
    /// leave the cursor as the alternate session left it. No-op if not on the
    /// alternate screen.
    pub(crate) fn leave_alt_screen(&mut self) {
        if let Some(saved) = self.primary.take() {
            self.cells = saved.cells;
            self.line_attrs = saved.line_attrs;
            self.wrapped = saved.wrapped;
            if saved.mode == AltMode::Dec1049 {
                self.cursor = (
                    saved.cursor.0.min(self.cols.saturating_sub(1)),
                    saved.cursor.1.min(self.rows.saturating_sub(1)),
                );
                self.saved_cursor = (
                    saved.saved_cursor.0.min(self.cols.saturating_sub(1)),
                    saved.saved_cursor.1.min(self.rows.saturating_sub(1)),
                );
            }
            self.reset_scroll_region();
            self.dirty.iter_mut().for_each(|d| *d = true);
        }
    }

    /// Save the current cursor position (`DECSC` / `CSI s`).
    pub(crate) fn save_cursor(&mut self) {
        self.saved_cursor = self.cursor;
    }

    /// Restore the saved cursor position (`DECRC` / `CSI u`), clamped.
    pub(crate) fn restore_cursor(&mut self) {
        let (x, y) = self.saved_cursor;
        self.set_cursor(x, y);
    }

    /// Delete `n` characters at the cursor, shifting the remainder of the row
    /// left and blanking the freed cells at the right (`DCH`).
    pub(crate) fn delete_chars(&mut self, n: usize) {
        let (x, y) = self.cursor;
        if y >= self.rows || x >= self.cols || n == 0 {
            return;
        }
        let base = y * self.cols;
        let row_end = base + self.cols;
        let from = base + x;
        let n = n.min(self.cols - x);
        self.cells.copy_within(from + n..row_end, from);
        let blank = self.erase_cell();
        for c in &mut self.cells[row_end - n..row_end] {
            *c = blank;
        }
        self.dirty[y] = true;
    }

    /// Insert `n` blank characters at the cursor, shifting the rest of the row
    /// right and dropping cells that fall off the right margin (`ICH`).
    pub(crate) fn insert_chars(&mut self, n: usize) {
        let (x, y) = self.cursor;
        if y >= self.rows || x >= self.cols || n == 0 {
            return;
        }
        let base = y * self.cols;
        let row_end = base + self.cols;
        let from = base + x;
        let n = n.min(self.cols - x);
        self.cells.copy_within(from..row_end - n, from + n);
        let blank = self.erase_cell();
        for c in &mut self.cells[from..from + n] {
            *c = blank;
        }
        self.dirty[y] = true;
    }

    /// Blank `n` characters starting at the cursor without shifting (`ECH`).
    pub(crate) fn erase_chars(&mut self, n: usize) {
        let (x, y) = self.cursor;
        let n = n.min(self.cols.saturating_sub(x));
        self.clear_row_range(y, x, x + n);
    }

    /// Insert `n` blank lines at the cursor row, pushing the rows below it down
    /// within the scrolling region; rows pushed past the region bottom are lost
    /// (`IL`). A no-op when the cursor is outside the scrolling region.
    pub(crate) fn insert_lines(&mut self, n: usize) {
        let cy = self.cursor.1;
        if cy < self.scroll_top || cy > self.scroll_bottom {
            return;
        }
        let n = n.min(self.scroll_bottom + 1 - cy);
        let cols = self.cols;
        // Shift rows [cy, scroll_bottom - n] down by n. copy_within is a memmove,
        // so the forward (overlapping) copy is well-defined.
        let count = (self.scroll_bottom + 1 - cy - n) * cols;
        if count > 0 {
            let src = cy * cols;
            let dst = (cy + n) * cols;
            self.cells.copy_within(src..src + count, dst);
        }
        // Blank the n freed rows at the cursor.
        let blank_end = (cy + n) * cols;
        let blank = self.erase_cell();
        for c in &mut self.cells[cy * cols..blank_end] {
            *c = blank;
        }
        self.shift_line_meta(cy, cy + n, count / cols, cy..cy + n);
        for d in &mut self.dirty[cy..=self.scroll_bottom] {
            *d = true;
        }
    }

    /// Delete `n` lines at the cursor row, pulling the rows below it up within
    /// the scrolling region and blanking the freed rows at the region bottom
    /// (`DL`). A no-op when the cursor is outside the scrolling region.
    pub(crate) fn delete_lines(&mut self, n: usize) {
        let cy = self.cursor.1;
        if cy < self.scroll_top || cy > self.scroll_bottom {
            return;
        }
        let n = n.min(self.scroll_bottom + 1 - cy);
        let cols = self.cols;
        // Shift rows [cy + n, scroll_bottom] up by n.
        let count = (self.scroll_bottom + 1 - cy - n) * cols;
        if count > 0 {
            let src = (cy + n) * cols;
            let dst = cy * cols;
            self.cells.copy_within(src..src + count, dst);
        }
        // Blank the n rows freed at the region bottom.
        let first_blank = (self.scroll_bottom + 1 - n) * cols;
        let region_end = (self.scroll_bottom + 1) * cols;
        let blank = self.erase_cell();
        for c in &mut self.cells[first_blank..region_end] {
            *c = blank;
        }
        self.shift_line_meta(cy + n,
        cy,
        count / cols,
        (self.scroll_bottom + 1 - n)..(self.scroll_bottom + 1),);
        for d in &mut self.dirty[cy..=self.scroll_bottom] {
            *d = true;
        }
    }

    /// Scroll the current scrolling region up by `n` rows (`SU`). Reuses the
    /// single-row [`Grid::scroll_up`] per line, so a full-screen scroll on the
    /// primary buffer captures the displaced lines into scrollback exactly as a
    /// line feed would. `n` is clamped to the region height: scrolling past it
    /// clears the region, and the cap bounds the per-row loop against a hostile
    /// count (e.g. `CSI 9999999999 S`) that would otherwise run for minutes.
    pub(crate) fn scroll_up_n(&mut self, n: usize) {
        let n = n.min(self.scroll_bottom + 1 - self.scroll_top);
        for _ in 0..n {
            self.scroll_up();
        }
    }

    /// Scroll the current scrolling region down by `n` rows (`SD`): shift the
    /// region's rows down and blank the `n` freed rows at the top. Displaced
    /// bottom rows are lost (scrollback is never un-scrolled).
    pub(crate) fn scroll_down_n(&mut self, n: usize) {
        let (top, bottom) = (self.scroll_top, self.scroll_bottom);
        if bottom <= top || bottom >= self.rows {
            return;
        }
        let n = n.min(bottom + 1 - top);
        let cols = self.cols;
        // Shift rows [top, bottom - n] down by n.
        let count = (bottom + 1 - top - n) * cols;
        if count > 0 {
            let src = top * cols;
            let dst = (top + n) * cols;
            self.cells.copy_within(src..src + count, dst);
        }
        // Blank the n freed rows at the region top.
        let blank_end = (top + n) * cols;
        let blank = self.erase_cell();
        for c in &mut self.cells[top * cols..blank_end] {
            *c = blank;
        }
        self.shift_line_meta(top, top + n, count / cols, top..top + n);
        for d in &mut self.dirty[top..=bottom] {
            *d = true;
        }
    }

    /// Move the cursor up one row, scrolling the region down when already at its
    /// top (`RI`, reverse index). The mirror of a line feed at the region bottom.
    pub(crate) fn reverse_index(&mut self) {
        if self.cursor.1 == self.scroll_top {
            self.scroll_down_n(1);
        } else if self.cursor.1 > 0 {
            self.cursor.1 -= 1;
        }
    }

    /// Set the current cursor row's line size attribute (`ESC # 3/4/5/6`).
    pub(crate) fn set_line_attr(&mut self, attr: LineAttr) {
        let y = self.cursor.1;
        if y < self.line_attrs.len() {
            self.line_attrs[y] = attr;
            self.dirty[y] = true;
        }
    }

    /// Mirror a region scroll on the per-row metadata tables (line size + soft
    /// wrap): move `count` rows from `src_row` to begin at `dst_row`, then reset
    /// the rows in `blank` to single-width / unwrapped. Keeps both glued to the
    /// content rows they label as the screen scrolls.
    fn shift_line_meta(
        &mut self,
        src_row: usize,
        dst_row: usize,
        count: usize,
        blank: std::ops::Range<usize>,
    ) {
        if count > 0 {
            self.line_attrs
                .copy_within(src_row..src_row + count, dst_row);
            self.wrapped.copy_within(src_row..src_row + count, dst_row);
        }
        for a in &mut self.line_attrs[blank.clone()] {
            *a = LineAttr::Single;
        }
        for w in &mut self.wrapped[blank] {
            *w = false;
        }
    }

    /// Move the cursor forward `n` tab stops (`HT` / `CHT`), without writing
    /// over the cells it passes. Stops at the right margin when no further tab
    /// stop exists.
    pub(crate) fn tab_forward(&mut self, n: usize) {
        let last = self.cols.saturating_sub(1);
        let mut x = self.cursor.0;
        for _ in 0..n {
            if x >= last {
                x = last;
                break;
            }
            let mut nx = x + 1;
            while nx < last && !self.tab_stops[nx] {
                nx += 1;
            }
            x = nx;
        }
        self.cursor.0 = x.min(last);
    }

    /// Move the cursor back `n` tab stops (`CBT`). Stops at column 0.
    pub(crate) fn tab_backward(&mut self, n: usize) {
        let mut x = self.cursor.0.min(self.cols.saturating_sub(1));
        for _ in 0..n {
            if x == 0 {
                break;
            }
            let mut nx = x - 1;
            while nx > 0 && !self.tab_stops[nx] {
                nx -= 1;
            }
            x = nx;
        }
        self.cursor.0 = x;
    }

    /// Set a tab stop at the current cursor column (`HTS`).
    pub(crate) fn set_tab_stop(&mut self) {
        let x = self.cursor.0;
        if x < self.tab_stops.len() {
            self.tab_stops[x] = true;
        }
    }

    /// Clear the tab stop at the current cursor column (`TBC 0`).
    pub(crate) fn clear_tab_stop(&mut self) {
        let x = self.cursor.0;
        if x < self.tab_stops.len() {
            self.tab_stops[x] = false;
        }
    }

    /// Clear every tab stop (`TBC 3`).
    pub(crate) fn clear_all_tab_stops(&mut self) {
        self.tab_stops.iter_mut().for_each(|s| *s = false);
    }

    /// Full reset (`RIS`): return the grid to its power-on state — blank primary
    /// screen, home cursor, full-screen scroll region, default tab stops,
    /// cleared scrollback, cursor visible, autowrap on. The window title and cwd
    /// are intentionally left alone (a hardware reset doesn't relabel the tab).
    /// The parser separately resets its pen and re-syncs the default colors
    /// (via [`Grid::set_default_colors`]) *before* calling this, so the blank
    /// fill below lands in the configured theme's colors.
    pub(crate) fn reset(&mut self) {
        self.primary = None; // leave the alternate screen if active
        self.cells = vec![self.erase_cell(); self.cols * self.rows];
        #[cfg(any(test, feature = "gui"))]
        self.images.clear();
        self.dirty = vec![true; self.rows];
        self.line_attrs = vec![LineAttr::Single; self.rows];
        self.wrapped = vec![false; self.rows];
        self.cursor = (0, 0);
        self.saved_cursor = (0, 0);
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);
        self.scrollback.clear();
        self.prompt_marks.clear();
        // A hard reset wipes the screen and scrollback the captures point into,
        // so drop the in-flight command mark and any pending completions too —
        // otherwise `command_start` dangles into freed lines (L1).
        #[cfg(feature = "l13")]
        {
            self.command_start = None;
            self.last_command_output = None;
            self.last_exit = None;
            self.finished_commands.clear();
        }
        self.view_offset = 0;
        self.tab_stops = default_tab_stops(self.cols);
        self.cursor_visible = true;
        self.bracketed_paste = false;
        self.mouse_modes = MouseModes::default();
        self.selection = None;
        self.autowrap = true;
        self.origin_mode = false;
        self.insert_mode = false;
        self.cursor_shape = self.default_cursor_shape;
        self.cursor_blink = self.default_cursor_blink;
        // default_fg/bg are intentionally untouched: the parser re-syncs them
        // from its (theme-seeded) palette right after — see RIS handling.
        self.current_link = 0;
    }

    /// Soft reset (`DECSTR`): reset terminal modes without clearing the screen
    /// or moving the active cursor — full-screen scroll region, saved cursor to
    /// home, cursor visible, autowrap on. The parser separately resets its pen.
    pub(crate) fn soft_reset(&mut self) {
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);
        self.saved_cursor = (0, 0);
        self.cursor_visible = true;
        self.bracketed_paste = false;
        self.mouse_modes = MouseModes::default();
        self.selection = None;
        self.autowrap = true;
        self.origin_mode = false;
        self.insert_mode = false;
        self.cursor_shape = self.default_cursor_shape;
        self.cursor_blink = self.default_cursor_blink;
        // default_fg/bg are intentionally untouched: the parser re-syncs them
        // from its (theme-seeded) palette right after — see DECSTR handling.
        self.current_link = 0;
        for a in &mut self.line_attrs {
            *a = LineAttr::Single;
        }
    }

    /// Screen-alignment test (`DECALN`, `ESC # 8`): fill every cell with `E` and
    /// home the cursor. Used to check character positioning.
    pub(crate) fn fill_alignment(&mut self) {
        let mut e = Cell::blank();
        e.ch = 'E';
        self.cells.fill(e);
        for a in &mut self.line_attrs {
            *a = LineAttr::Single;
        }
        self.wrapped.iter_mut().for_each(|w| *w = false);
        self.dirty.iter_mut().for_each(|d| *d = true);
        self.cursor = (0, 0);
    }

    /// Clear all per-row dirty flags. Call after handing a frame to the renderer.
    pub fn clear_dirty(&mut self) {
        self.dirty.iter_mut().for_each(|d| *d = false);
    }

    /// Overlay a status line at the bottom of the screen (L13 `render/set_status`),
    /// laid out at the current width; `fg`/`bg` default to the grid's defaults.
    /// Marks the bottom row dirty so the next frame composites it.
    #[cfg(feature = "l13")]
    pub(crate) fn set_status_line(&mut self, text: String, fg: Option<u32>, bg: Option<u32>) {
        let fg = fg.unwrap_or(self.default_fg);
        let bg = bg.unwrap_or(self.default_bg);
        self.status_line = Some(StatusLine::new(text, fg, bg, self.cols));
        if let Some(d) = self.dirty.last_mut() {
            *d = true;
        }
    }

    /// Remove the status-line overlay (L13 `render/clear_status`), repainting the
    /// bottom row it covered.
    #[cfg(feature = "l13")]
    pub(crate) fn clear_status_line(&mut self) {
        if self.status_line.take().is_some()
            && let Some(d) = self.dirty.last_mut()
        {
            *d = true;
        }
    }

    /// The status-line overlay cells (length `cols`) to composite over the bottom
    /// row, or `None` when no overlay is set or the alternate screen is active
    /// (its full-screen app owns the whole grid).
    pub(crate) fn status_row(&self) -> Option<&[Cell]> {
        if self.primary.is_some() {
            return None;
        }
        self.status_line.as_ref().map(|s| s.cells.as_slice())
    }

    /// Begin capturing a command's output (OSC 133;C): anchor the output start at
    /// the current cursor line, captured at the matching `D`.
    #[cfg(feature = "l13")]
    pub(crate) fn command_output_begin(&mut self) {
        self.command_start = Some(self.scrollback.len() + self.cursor.1);
    }

    /// A command finished (OSC 133;D): record its exit status and, if its output
    /// start was marked (C), capture the text from there to the cursor as the
    /// `terminal://command` resource.
    #[cfg(feature = "l13")]
    pub(crate) fn command_finished(&mut self, exit: Option<i32>) {
        self.last_exit = exit;
        // Assign unconditionally: a `D` with no preceding `C` (no output-start
        // mark) must clear any earlier command's capture rather than leave it
        // stale on the `terminal://command` resource (M1).
        self.last_command_output = self
            .command_start
            .take()
            .map(|start| self.capture_command_output(start));
        // Queue this completion so every `D` is reported, even when several land
        // in one byte batch (M2) — the captured output is snapshotted per entry.
        self.finished_commands
            .push_back((exit, self.last_command_output.clone()));
    }

    /// Join the cell rows in the absolute line range `[start, cursor line)` into
    /// text, one row per line, trailing blanks trimmed per row.
    #[cfg(feature = "l13")]
    fn capture_command_output(&self, start: usize) -> String {
        let history = self.scrollback.len();
        let end = history + self.cursor.1;
        let start = start.min(end);
        let mut out = String::new();
        for i in start..end {
            if i != start {
                out.push('\n');
            }
            let cells = if i < history {
                &self.scrollback[i].cells[..]
            } else {
                let s = (i - history) * self.cols;
                &self.cells[s..s + self.cols]
            };
            out.push_str(&row_text(cells, &self.clusters));
        }
        out
    }

    /// The exit code of the last finished command, or `None` if none has
    /// finished (or the shell reported no code).
    #[cfg(feature = "l13")]
    pub(crate) fn last_command_exit(&self) -> Option<i32> {
        self.last_exit
    }

    /// The output text of the last finished command (OSC 133;C..D), if captured.
    #[cfg(feature = "l13")]
    pub(crate) fn last_command_output(&self) -> Option<&str> {
        self.last_command_output.as_deref()
    }

    // ── Nexus headless-introspection accessors (RFC 0003) ───────────────────
    // Public, dependency-free reads of the grid for the `Vt` facade. The
    // exit/output accessors degrade to `None` when built without `l13` (their
    // backing fields are l13-gated), so the facade compiles in any feature set.

    /// The visible screen as text: one line per row, with trailing blank rows
    /// dropped. The building block for `terminal://screen` reads.
    #[must_use]
    pub fn screen_text(&self) -> String {
        let mut lines: Vec<String> = self
            .cells
            .chunks(self.cols)
            .map(|row| row_text(row, &self.clusters))
            .collect();
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines.join("\n")
    }

    /// The most recent `max` scrollback lines as text (oldest first).
    #[must_use]
    pub fn scrollback_text(&self, max: usize) -> String {
        let skip = self.scrollback.len().saturating_sub(max);
        self.scrollback
            .iter()
            .skip(skip)
            .map(|line| row_text(&line.cells, &self.clusters))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Exit code of the last finished command (OSC 133;D), or `None`.
    #[must_use]
    pub fn last_exit_code(&self) -> Option<i32> {
        #[cfg(feature = "l13")]
        {
            self.last_exit
        }
        #[cfg(not(feature = "l13"))]
        {
            None
        }
    }

    /// Output text of the last finished command (OSC 133;C..D), if captured.
    #[must_use]
    pub fn last_output(&self) -> Option<&str> {
        #[cfg(feature = "l13")]
        {
            self.last_command_output.as_deref()
        }
        #[cfg(not(feature = "l13"))]
        {
            None
        }
    }

    /// Pop the oldest pending command completion (OSC 133;D) as its exit code
    /// (`None` when the shell omitted it) and captured output. A host drains
    /// this in a loop after feeding a batch of bytes, emitting one
    /// command-finished signal per completion — so back-to-back completions in
    /// a single batch are each reported rather than coalesced. Always `None`
    /// without `l13`.
    pub fn pop_finished_command(&mut self) -> Option<(Option<i32>, Option<String>)> {
        #[cfg(feature = "l13")]
        {
            self.finished_commands.pop_front()
        }
        #[cfg(not(feature = "l13"))]
        {
            None
        }
    }

    /// Build the `terminal://dimensions` change notification if a client is
    /// subscribed, for the runtime driver to write to the child after a resize
    /// (which happens outside the parser's `advance`, so it has no `responses` in
    /// hand). Returns `None` when nobody is subscribed — no wasted frame.
    #[cfg(feature = "l13")]
    pub(crate) fn resize_notification(&self) -> Option<Vec<u8>> {
        let mut buf = Vec::new();
        super::channel::notify_resource_changed(self, super::channel::RES_DIMENSIONS, &mut buf);
        (!buf.is_empty()).then_some(buf)
    }

    /// Snapshot only the rows currently marked dirty, cloning their cells into
    /// a [`DirtyFrame`]. This is the high-locality handoff the renderer consumes.
    pub fn snapshot_dirty(&self) -> DirtyFrame {
        let rows = self
            .dirty
            .iter()
            .enumerate()
            .filter(|&(_, d)| *d)
            .map(|(y, _)| {
                let start = y * self.cols;
                let cells = match (y + 1 == self.rows).then(|| self.status_row()).flatten() {
                    Some(s) => s.to_vec(),
                    None => self.cells[start..start + self.cols].to_vec(),
                };
                (y, cells)
            })
            .collect();
        DirtyFrame {
            cursor: self.cursor,
            rows,
            links: self.links.clone(),
            clusters: self.clusters.clone(),
            line_attrs: self.line_attrs.clone(),
        }
    }

    /// Move the viewport up into history by up to `n` lines, clamped to the
    /// available scrollback. No-op on the alternate screen (no history there).
    /// Returns `true` if the view actually moved.
    pub fn scroll_view_up(&mut self, n: usize) -> bool {
        if self.primary.is_some() {
            return false;
        }
        let target = (self.view_offset + n).min(self.scrollback.len());
        self.set_view_offset(target)
    }

    /// Move the viewport back down toward the live bottom by up to `n` lines.
    /// Returns `true` if the view actually moved.
    pub fn scroll_view_down(&mut self, n: usize) -> bool {
        let target = self.view_offset.saturating_sub(n);
        self.set_view_offset(target)
    }

    /// Snap the viewport back to the live bottom. Returns `true` if it moved.
    pub fn reset_view(&mut self) -> bool {
        self.set_view_offset(0)
    }

    /// Borrow physical row `abs` (0 = oldest scrollback line) as `(cells, wrapped)`.
    #[cfg(any(test, feature = "gui"))]
    fn phys_row(&self, abs: usize) -> (&[Cell], bool) {
        let h = self.scrollback.len();
        if abs < h {
            (&self.scrollback[abs].cells, self.scrollback[abs].wrapped)
        } else {
            let y = abs - h;
            (&self.cells[y * self.cols..(y + 1) * self.cols], self.wrapped[y])
        }
    }

    /// Scroll the viewport so absolute row `abs` is visible, about a third down.
    #[cfg(any(test, feature = "gui"))]
    fn scroll_to_abs(&mut self, abs: usize) {
        let h = self.scrollback.len();
        let want = h as isize + (self.rows / 3) as isize - abs as isize;
        let off = want.clamp(0, h as isize) as usize;
        self.set_view_offset(off);
    }

    /// Search the scrollback + live screen for `query`, case-insensitively
    /// (ASCII), joining soft-wrapped rows into logical lines so a match can cross
    /// a wrap. Stores matches for highlighting and next/prev, scrolls the first
    /// into view, and returns the count. An empty query clears the search.
    #[cfg(any(test, feature = "gui"))]
    pub fn search(&mut self, query: &str) -> usize {
        self.search = None;
        let q: Vec<char> = query.chars().map(|c| c.to_ascii_lowercase()).collect();
        if q.is_empty() {
            self.dirty.iter_mut().for_each(|d| *d = true);
            return 0;
        }
        let total = self.scrollback.len() + self.rows;
        let mut st = Search::default();
        let mut text: Vec<char> = Vec::new();
        let mut at: Vec<(usize, usize)> = Vec::new();
        for abs in 0..total {
            let (cells, wrapped) = self.phys_row(abs);
            for (col, cell) in cells.iter().enumerate() {
                if cell.flags & WIDE_TRAILER != 0 {
                    continue;
                }
                text.push(cell.ch.to_ascii_lowercase());
                at.push((abs, col));
            }
            if wrapped {
                continue;
            }
            find_matches(&text, &at, &q, &mut st);
            text.clear();
            at.clear();
            if st.anchors.len() >= SEARCH_MAX {
                break;
            }
        }
        find_matches(&text, &at, &q, &mut st);
        let n = st.anchors.len();
        if n > 0 {
            let anchor = st.anchors[0].0;
            self.search = Some(st);
            self.scroll_to_abs(anchor);
        }
        self.dirty.iter_mut().for_each(|d| *d = true);
        n
    }

    /// Step to the next (`forward`) or previous match, scrolling it into view.
    #[cfg(any(test, feature = "gui"))]
    pub fn search_jump(&mut self, forward: bool) -> bool {
        let abs = {
            let Some(s) = &mut self.search else { return false };
            if s.anchors.is_empty() {
                return false;
            }
            let n = s.anchors.len();
            s.current = if forward { (s.current + 1) % n } else { (s.current + n - 1) % n };
            s.anchors[s.current].0
        };
        self.scroll_to_abs(abs);
        self.dirty.iter_mut().for_each(|d| *d = true);
        true
    }

    /// Highlight state for viewport cell `(col, vr)`: `None` (no match),
    /// `Some(false)` (a match), `Some(true)` (the active match).
    #[cfg(any(test, feature = "gui"))]
    pub fn search_highlight(&self, col: usize, vr: usize) -> Option<bool> {
        let s = self.search.as_ref()?;
        let off = self.view_offset.min(self.scrollback.len());
        let abs = self.scrollback.len() - off + vr;
        for &(start, end, mi) in s.rows.get(&abs)? {
            if col >= start && col < end {
                return Some(mi == s.current);
            }
        }
        None
    }

    /// `(current_match_1based, total)` while searching, else `None`.
    #[cfg(any(test, feature = "gui"))]
    pub fn search_status(&self) -> Option<(usize, usize)> {
        let s = self.search.as_ref()?;
        Some((s.current + 1, s.anchors.len()))
    }

    /// Clear any active search (highlights + matches).
    #[cfg(any(test, feature = "gui"))]
    pub fn clear_search(&mut self) {
        if self.search.take().is_some() {
            self.dirty.iter_mut().for_each(|d| *d = true);
        }
    }

    /// Set the scrollback view offset, marking every row dirty so the renderer
    /// repaints the whole viewport. Returns `true` if the offset changed.
    fn set_view_offset(&mut self, offset: usize) -> bool {
        if offset == self.view_offset {
            return false;
        }
        self.view_offset = offset;
        self.dirty.iter_mut().for_each(|d| *d = true);
        true
    }

    /// Record a shell prompt start (OSC 133;A) at the current cursor row, for
    /// prompt-to-prompt scrollback navigation. No-op on the alternate screen,
    /// which has no history. Marks are logical line indices (0 = oldest retained
    /// scrollback line), kept sorted and deduplicated.
    pub(crate) fn mark_prompt(&mut self) {
        if self.primary.is_some() {
            return;
        }
        let line = self.scrollback.len() + self.cursor.1;
        if let Err(pos) = self.prompt_marks.binary_search(&line) {
            self.prompt_marks.insert(pos, line);
            if self.prompt_marks.len() > PROMPT_MARKS_MAX {
                self.prompt_marks.remove(0);
            }
        }
    }

    /// One scrollback line was evicted from the front: every logical index drops
    /// by one, and a mark on the evicted line (index 0) is discarded.
    fn evict_prompt_mark(&mut self) {
        self.prompt_marks.retain(|&l| l != 0);
        for l in &mut self.prompt_marks {
            *l -= 1;
        }
    }

    /// Scroll the viewport up to the nearest prompt mark above the current top
    /// visible line (OSC 133 navigation). Returns `true` if it moved.
    pub fn scroll_to_prev_prompt(&mut self) -> bool {
        if self.primary.is_some() {
            return false;
        }
        let h = self.scrollback.len();
        let top_visible = h - self.view_offset.min(h);
        match self
            .prompt_marks
            .iter()
            .copied()
            .filter(|&l| l < top_visible)
            .max()
        {
            Some(l) => self.set_view_offset(h - l),
            None => false,
        }
    }

    /// Scroll the viewport down to the nearest prompt mark below the current top
    /// visible line, snapping to the live bottom when the next mark is on the
    /// live screen or there is none. Returns `true` if it moved.
    pub fn scroll_to_next_prompt(&mut self) -> bool {
        if self.primary.is_some() {
            return false;
        }
        let h = self.scrollback.len();
        let top_visible = h - self.view_offset.min(h);
        match self
            .prompt_marks
            .iter()
            .copied()
            .filter(|&l| l > top_visible)
            .min()
        {
            Some(l) if l < h => self.set_view_offset(h - l),
            _ => self.reset_view(),
        }
    }

    /// The cell visible at viewport `(col, row)`, compositing scrollback history
    /// above the live grid according to [`Grid::view_offset`]: the top `off`
    /// rows show the tail of history, the rest the live grid shifted down by
    /// `off`. A blank cell fills positions past a short history line's width.
    /// Used by the windowed renderers so a scrolled-up view paints history.
    #[cfg(any(test, feature = "gui"))]
    pub fn viewport_cell(&self, col: usize, row: usize) -> Cell {
        let off = self.view_offset.min(self.scrollback.len());
        if row < off {
            let line = &self.scrollback[self.scrollback.len() - off + row].cells;
            line.get(col).copied().unwrap_or_else(Cell::blank)
        } else {
            self.cells[(row - off) * self.cols + col]
        }
    }

    /// The OSC 8 hyperlink URI covering viewport `(col, row)`, if any. Mirrors
    /// [`Grid::viewport_cell`] so links in scrolled-back history resolve too;
    /// powers Ctrl+click in the windowed front-end.
    #[cfg(any(test, feature = "gui"))]
    pub fn link_at(&self, col: usize, row: usize) -> Option<&str> {
        if col >= self.cols || row >= self.rows {
            return None;
        }
        let id = self.viewport_cell(col, row).link as usize;
        self.links.get(id.checked_sub(1)?).map(String::as_str)
    }

    /// Snapshot the entire visible viewport, compositing scrollback history
    /// above the live grid according to [`Grid::view_offset`]. Every row is
    /// included. History lines are padded/truncated to the current width.
    /// Used by the renderer whenever the view is scrolled up.
    pub fn snapshot_viewport(&self) -> DirtyFrame {
        let history = self.scrollback.len();
        let off = self.view_offset.min(history);
        let mut rows = Vec::with_capacity(self.rows);
        let mut attrs = Vec::with_capacity(self.rows);
        for y in 0..self.rows {
            let status = if y + 1 == self.rows {
                self.status_row()
            } else {
                None
            };
            let cells = if let Some(s) = status {
                s.to_vec()
            } else if y < off {
                // Top `off` viewport rows show the tail of history.
                let line = &self.scrollback[history - off + y].cells;
                let mut row = vec![Cell::blank(); self.cols];
                let n = line.len().min(self.cols);
                row[..n].copy_from_slice(&line[..n]);
                row
            } else {
                // The rest shows the live grid, shifted down by `off`.
                let gy = y - off;
                let start = gy * self.cols;
                self.cells[start..start + self.cols].to_vec()
            };
            rows.push((y, cells));
            attrs.push(if status.is_some() || y < off {
                LineAttr::Single // history + status-overlay rows are single width
            } else {
                self.line_attrs[y - off]
            });
        }
        DirtyFrame {
            cursor: self.cursor,
            rows,
            links: self.links.clone(),
            clusters: self.clusters.clone(),
            line_attrs: attrs,
        }
    }

    /// Drain bytes queued for the host terminal (forwarded OSC 52 clipboard
    /// requests). Empty when there's nothing to send.
    pub fn take_host_out(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.host_out)
    }

    /// Set the active hyperlink (OSC 8). `None` (or an empty URI) closes the
    /// link; a URI is interned and its id stamped onto subsequently written
    /// cells.
    pub(crate) fn set_link(&mut self, uri: Option<&str>) {
        self.current_link = match uri {
            None | Some("") => 0,
            Some(u) => self.intern_link(u),
        };
    }

    /// Queue a desktop notification (OSC 9/777) for the windowed front-end to
    /// raise. Bounded so a flood can't grow the queue without limit between
    /// drains; the TUI relays the OSC to the host instead.
    pub(crate) fn push_notification(&mut self, title: String, body: String) {
        const MAX: usize = 32;
        if self.notifications.len() < MAX {
            self.notifications.push((title, body));
        }
    }

    /// Return the id for `uri`, interning it on first use. Ids are `index + 1`
    /// so `0` can mean "no link". Returns `0` once the table is full.
    fn intern_link(&mut self, uri: &str) -> u16 {
        if let Some(i) = self.links.iter().position(|l| l == uri) {
            return (i + 1) as u16;
        }
        if self.links.len() >= LINK_MAX {
            return 0;
        }
        self.links.push(uri.to_string());
        self.links.len() as u16
    }
}

/// A snapshot of the dirty rows of a [`Grid`], plus the cursor position so the
/// renderer can place the hardware cursor.
pub struct DirtyFrame {
    /// Cursor position `(col, row)` at snapshot time.
    pub cursor: (usize, usize),
    /// Dirty rows as `(row_index, cells)` pairs.
    pub rows: Vec<(usize, Vec<Cell>)>,
    /// Interned hyperlink URIs (OSC 8); a [`Cell::link`] of `n` indexes
    /// `links[n - 1]`. Cloned so the renderer can resolve links without the lock.
    pub links: Vec<String>,
    /// Interned grapheme continuations (see [`Grid::clusters`]); a
    /// [`Cell::cluster`] of `n` indexes `clusters[n - 1]`. Cloned so the renderer
    /// can resolve glyphs without holding the grid lock.
    pub clusters: Vec<String>,
    /// Per-row line size attributes (DECDWL/DECDHL), indexed by each row's `y`
    /// in [`rows`](Self::rows). Cloned so the renderer can relay them to the host.
    pub line_attrs: Vec<LineAttr>,
}

/// Reconstruct a row of cells as text: base glyph plus any interned grapheme
/// continuation, skipping wide-glyph trailers, with trailing blanks trimmed.
/// Shared by the MCP screen/scrollback/command readers.
#[cfg(feature = "l13")]
pub(crate) fn row_text(cells: &[Cell], clusters: &[String]) -> String {
    let mut s = String::new();
    for cell in cells {
        if cell.flags & WIDE_TRAILER != 0 {
            continue;
        }
        s.push(cell.ch);
        if cell.cluster != 0
            && let Some(suffix) = clusters.get((cell.cluster - 1) as usize)
        {
            s.push_str(suffix);
        }
    }
    s.trim_end().to_string()
}
