//! Server-side terminal grid backed by `vte` (PRD-09 §3 addendum).
//!
//! # What this adds
//!
//! The existing [`crate::OutputBuffer`] + [`crate::LineBuffer`] pair keeps raw
//! bytes and ANSI-stripped text. Neither gives Rust code a picture of the
//! *rendered screen* — cursor position, which characters are currently visible,
//! or what attributes they carry. That picture lives only in xterm.js.
//!
//! [`TerminalGrid`] fixes this: it drives a [`vte::Parser`] over the same byte
//! stream that goes to [`crate::OutputBuffer`], maintaining a 2-D array of
//! [`RawCell`]s that mirrors the terminal's visible content. The grid can then
//! be snapshotted by AI agents and pattern matchers without a JS round-trip.
//!
//! # VTE coverage
//!
//! The [`vte::Perform`] implementation on [`GridState`] handles:
//!
//! - **Printable characters** — written at the cursor, with auto-wrap.
//! - **Control characters** — CR, LF, VT, FF, BS, HT.
//! - **CSI** — cursor movement (A/B/C/D/E/F/G/H/f/d), erase (J/K/X),
//!   line operations (L/M), character operations (P/@), scroll (S/T),
//!   scroll region (r), SGR attributes (m), and private modes (?47/1047/1049
//!   for the alternate screen).
//! - **ESC** — reverse index (M), save/restore cursor (7/8).
//! - **OSC 133** — semantic shell integration zones (A/B/C/D).
//!
//! Sequences not listed here are silently ignored; the grid never panics on
//! unknown input.

use serde::{Deserialize, Serialize};
use vte::{Params, Perform};

// ── Color types ───────────────────────────────────────────────────────────────

/// Terminal color value carried by a [`RawCell`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TermColor {
    /// Terminal default (inherits from the theme).
    #[default]
    Default,
    /// One of the 16 named ANSI colors (0-7 normal, 8-15 bright).
    Ansi(u8),
    /// xterm 256-color palette index.
    Indexed(u8),
    /// 24-bit RGB.
    Rgb(u8, u8, u8),
}

// ── Cell attributes ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
struct CellAttrs {
    bold: bool,
    italic: bool,
    underline: bool,
    fg: TermColor,
    bg: TermColor,
}

// ── Grid cells ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct RawCell {
    c: char,
    attrs: CellAttrs,
}

impl Default for RawCell {
    fn default() -> Self {
        Self { c: ' ', attrs: CellAttrs::default() }
    }
}

// ── Shell integration (OSC 133) ───────────────────────────────────────────────

/// OSC 133 semantic zone kind (shell integration markers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoneKind {
    /// `133;A` — prompt start.
    PromptStart,
    /// `133;B` — command / user-input start.
    CommandStart,
    /// `133;C` — command output start.
    OutputStart,
    /// `133;D` — command output end (optionally carries exit status).
    OutputEnd,
}

/// A single OSC 133 semantic zone marker emitted by the shell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellZone {
    /// What kind of zone boundary this is.
    pub kind: ZoneKind,
    /// Zero-indexed row where the marker was emitted.
    pub row: usize,
    /// Zero-indexed column where the marker was emitted.
    pub col: usize,
    /// Exit status from `OSC 133;D;<exit_code>` — present only for
    /// [`ZoneKind::OutputEnd`] when the shell included one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
}

// ── Screen snapshot ───────────────────────────────────────────────────────────

/// Point-in-time snapshot of the terminal's visible screen. Returned by
/// [`TerminalGrid::snapshot`] and delivered over the IPC boundary by
/// `com.nexus.terminal` handler 18 (`read_screen`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenSnapshot {
    /// Total visible rows.
    pub rows: usize,
    /// Total visible columns.
    pub cols: usize,
    /// Zero-indexed cursor row.
    pub cursor_row: usize,
    /// Zero-indexed cursor column.
    pub cursor_col: usize,
    /// Plain-text content of each visible row. Trailing whitespace is trimmed;
    /// entirely blank rows appear as empty strings. The outer Vec has exactly
    /// [`Self::rows`] elements.
    pub text_rows: Vec<String>,
    /// All OSC 133 zone markers accumulated since the session was created.
    /// The list is append-only; callers that want only new zones should track
    /// their own offset into it between calls.
    pub zones: Vec<ShellZone>,
}

// ── Inner VTE state ───────────────────────────────────────────────────────────

struct GridState {
    rows: usize,
    cols: usize,
    primary: Vec<Vec<RawCell>>,
    alternate: Vec<Vec<RawCell>>,
    alt_active: bool,
    cursor_row: usize,
    cursor_col: usize,
    /// When true the cursor is "past the last column" — the next printable
    /// character triggers an auto-wrap before being written. This mirrors the
    /// VT100 "pending wrap" flag so that writing exactly `cols` chars does not
    /// immediately advance the line (the cursor stays in the last column until
    /// the next char arrives).
    pending_wrap: bool,
    cur_attrs: CellAttrs,
    /// Inclusive scroll region bounds (0-indexed rows).
    scroll_top: usize,
    scroll_bottom: usize,
    zones: Vec<ShellZone>,
    saved_cursor: (usize, usize),
    saved_attrs: CellAttrs,
}

impl GridState {
    fn new(rows: usize, cols: usize) -> Self {
        let blank_row = || vec![RawCell::default(); cols];
        let scroll_bottom = rows.saturating_sub(1);
        Self {
            rows,
            cols,
            primary: (0..rows).map(|_| blank_row()).collect(),
            alternate: (0..rows).map(|_| blank_row()).collect(),
            alt_active: false,
            cursor_row: 0,
            cursor_col: 0,
            pending_wrap: false,
            cur_attrs: CellAttrs::default(),
            scroll_top: 0,
            scroll_bottom,
            zones: Vec::new(),
            saved_cursor: (0, 0),
            saved_attrs: CellAttrs::default(),
        }
    }

    fn grid_mut(&mut self) -> &mut Vec<Vec<RawCell>> {
        if self.alt_active { &mut self.alternate } else { &mut self.primary }
    }

    fn grid_ref(&self) -> &[Vec<RawCell>] {
        if self.alt_active { &self.alternate } else { &self.primary }
    }

    fn snapshot(&self) -> ScreenSnapshot {
        let text_rows = self.grid_ref().iter().map(|row| {
            let s: String = row.iter().map(|c| c.c).collect();
            s.trim_end().to_string()
        }).collect();
        ScreenSnapshot {
            rows: self.rows,
            cols: self.cols,
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
            text_rows,
            zones: self.zones.clone(),
        }
    }

    fn resize(&mut self, rows: usize, cols: usize) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let blank_row = |c| vec![RawCell::default(); c];
        // Rebuild primary: preserve existing content where it fits.
        let mut new_primary = vec![blank_row(cols); rows];
        for (r, row) in self.primary.iter().enumerate().take(rows) {
            for (c, cell) in row.iter().enumerate().take(cols) {
                new_primary[r][c] = cell.clone();
            }
        }
        self.primary = new_primary;
        // Alternate screen content is ephemeral — just resize it blank.
        self.alternate = vec![blank_row(cols); rows];
        self.rows = rows;
        self.cols = cols;
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
        self.scroll_top = 0;
        self.scroll_bottom = rows.saturating_sub(1);
        self.pending_wrap = false;
    }

    // ── Printable character ───────────────────────────────────────────────────

    fn put_char(&mut self, c: char) {
        if self.pending_wrap {
            self.cursor_col = 0;
            if self.cursor_row == self.scroll_bottom {
                self.scroll_up(1);
            } else {
                self.cursor_row = (self.cursor_row + 1).min(self.rows.saturating_sub(1));
            }
            self.pending_wrap = false;
        }
        let row = self.cursor_row.min(self.rows.saturating_sub(1));
        let col = self.cursor_col.min(self.cols.saturating_sub(1));
        let attrs = self.cur_attrs;
        let grid = self.grid_mut();
        if row < grid.len() && col < grid[row].len() {
            grid[row][col] = RawCell { c, attrs };
        }
        if self.cursor_col + 1 >= self.cols {
            self.pending_wrap = true;
        } else {
            self.cursor_col += 1;
        }
    }

    // ── Scroll operations ─────────────────────────────────────────────────────

    /// Scroll the active scroll region up by `n` lines. The top lines
    /// disappear; blank lines appear at the bottom.
    fn scroll_up(&mut self, n: usize) {
        let top = self.scroll_top;
        let bottom = self.scroll_bottom.min(self.rows.saturating_sub(1));
        if top > bottom || n == 0 { return; }
        let region_h = bottom - top + 1;
        let n = n.min(region_h);
        let grid = self.grid_mut();
        let slice = &mut grid[top..=bottom];
        slice.rotate_left(n);
        let len = slice.len();
        for row in &mut slice[len - n..] {
            row.iter_mut().for_each(|c| *c = RawCell::default());
        }
    }

    /// Scroll the active scroll region down by `n` lines. The bottom lines
    /// disappear; blank lines appear at the top.
    fn scroll_down(&mut self, n: usize) {
        let top = self.scroll_top;
        let bottom = self.scroll_bottom.min(self.rows.saturating_sub(1));
        if top > bottom || n == 0 { return; }
        let region_h = bottom - top + 1;
        let n = n.min(region_h);
        let grid = self.grid_mut();
        let slice = &mut grid[top..=bottom];
        slice.rotate_right(n);
        for row in &mut slice[..n] {
            row.iter_mut().for_each(|c| *c = RawCell::default());
        }
    }

    // ── Cursor operations ─────────────────────────────────────────────────────

    fn cursor_up(&mut self, n: usize) {
        self.pending_wrap = false;
        self.cursor_row = self.cursor_row.saturating_sub(n);
    }

    fn cursor_down(&mut self, n: usize) {
        self.pending_wrap = false;
        self.cursor_row = (self.cursor_row + n).min(self.rows.saturating_sub(1));
    }

    fn cursor_forward(&mut self, n: usize) {
        self.pending_wrap = false;
        self.cursor_col = (self.cursor_col + n).min(self.cols.saturating_sub(1));
    }

    fn cursor_back(&mut self, n: usize) {
        self.pending_wrap = false;
        self.cursor_col = self.cursor_col.saturating_sub(n);
    }

    fn cursor_set(&mut self, row: usize, col: usize) {
        self.pending_wrap = false;
        self.cursor_row = row.min(self.rows.saturating_sub(1));
        self.cursor_col = col.min(self.cols.saturating_sub(1));
    }

    // ── Erase operations ──────────────────────────────────────────────────────

    fn erase_display(&mut self, n: u16) {
        let cr = self.cursor_row.min(self.rows.saturating_sub(1));
        let cc = self.cursor_col.min(self.cols.saturating_sub(1));
        let rows = self.rows;
        let cols = self.cols;
        let grid = self.grid_mut();
        match n {
            0 => {
                // Cursor to end of screen.
                for c in cc..cols {
                    grid[cr][c] = RawCell::default();
                }
                for r in (cr + 1)..rows {
                    grid[r].iter_mut().for_each(|c| *c = RawCell::default());
                }
            }
            1 => {
                // Start of screen to cursor.
                for r in 0..cr {
                    grid[r].iter_mut().for_each(|c| *c = RawCell::default());
                }
                for c in 0..=cc {
                    grid[cr][c] = RawCell::default();
                }
            }
            2 | 3 => {
                // Entire screen (3 also clears scrollback, which we don't
                // maintain, so 2 and 3 are identical here).
                for r in 0..rows {
                    grid[r].iter_mut().for_each(|c| *c = RawCell::default());
                }
            }
            _ => {}
        }
    }

    fn erase_line(&mut self, n: u16) {
        let cr = self.cursor_row.min(self.rows.saturating_sub(1));
        let cc = self.cursor_col.min(self.cols.saturating_sub(1));
        let cols = self.cols;
        let grid = self.grid_mut();
        if cr >= grid.len() { return; }
        match n {
            0 => {
                for c in cc..cols { grid[cr][c] = RawCell::default(); }
            }
            1 => {
                for c in 0..=cc { grid[cr][c] = RawCell::default(); }
            }
            2 => {
                grid[cr].iter_mut().for_each(|c| *c = RawCell::default());
            }
            _ => {}
        }
    }

    fn erase_chars(&mut self, n: usize) {
        let row = self.cursor_row.min(self.rows.saturating_sub(1));
        let col = self.cursor_col;
        if col >= self.cols { return; }
        let n = n.min(self.cols - col);
        let grid = self.grid_mut();
        if row >= grid.len() { return; }
        for c in &mut grid[row][col..(col + n)] {
            *c = RawCell::default();
        }
    }

    // ── Line / character insertion / deletion ─────────────────────────────────

    fn insert_lines(&mut self, n: usize) {
        let row = self.cursor_row;
        if row < self.scroll_top || row > self.scroll_bottom { return; }
        let bottom = self.scroll_bottom.min(self.rows.saturating_sub(1));
        let region_h = bottom + 1 - row;
        let n = n.min(region_h);
        let grid = self.grid_mut();
        let slice = &mut grid[row..=bottom];
        slice.rotate_right(n);
        for r in &mut slice[..n] {
            r.iter_mut().for_each(|c| *c = RawCell::default());
        }
        self.cursor_col = 0;
        self.pending_wrap = false;
    }

    fn delete_lines(&mut self, n: usize) {
        let row = self.cursor_row;
        if row < self.scroll_top || row > self.scroll_bottom { return; }
        let bottom = self.scroll_bottom.min(self.rows.saturating_sub(1));
        let region_h = bottom + 1 - row;
        let n = n.min(region_h);
        let grid = self.grid_mut();
        let slice = &mut grid[row..=bottom];
        slice.rotate_left(n);
        let len = slice.len();
        for r in &mut slice[len - n..] {
            r.iter_mut().for_each(|c| *c = RawCell::default());
        }
        self.cursor_col = 0;
        self.pending_wrap = false;
    }

    fn delete_chars(&mut self, n: usize) {
        let row = self.cursor_row.min(self.rows.saturating_sub(1));
        let col = self.cursor_col;
        if col >= self.cols { return; }
        let n = n.min(self.cols - col);
        let grid = self.grid_mut();
        if row >= grid.len() { return; }
        let row_slice = &mut grid[row][col..];
        row_slice.rotate_left(n);
        let len = row_slice.len();
        for c in &mut row_slice[len - n..] {
            *c = RawCell::default();
        }
    }

    fn insert_chars(&mut self, n: usize) {
        let row = self.cursor_row.min(self.rows.saturating_sub(1));
        let col = self.cursor_col;
        if col >= self.cols { return; }
        let n = n.min(self.cols - col);
        let grid = self.grid_mut();
        if row >= grid.len() { return; }
        let row_slice = &mut grid[row][col..];
        row_slice.rotate_right(n);
        for c in &mut row_slice[..n] {
            *c = RawCell::default();
        }
    }

    // ── SGR attribute handling ────────────────────────────────────────────────

    fn handle_sgr(&mut self, params: &Params) {
        let mut iter = params.iter().peekable();
        // Empty params list means CSI m → reset all.
        if iter.peek().is_none() {
            self.cur_attrs = CellAttrs::default();
            return;
        }
        while let Some(p) = iter.next() {
            let code = p.first().copied().unwrap_or(0);
            match code {
                0 => self.cur_attrs = CellAttrs::default(),
                1 => self.cur_attrs.bold = true,
                3 => self.cur_attrs.italic = true,
                4 => self.cur_attrs.underline = true,
                22 => self.cur_attrs.bold = false,
                23 => self.cur_attrs.italic = false,
                24 => self.cur_attrs.underline = false,
                30..=37 => self.cur_attrs.fg = TermColor::Ansi(code as u8 - 30),
                38 => {
                    if p.len() >= 3 && p[1] == 5 {
                        // Colon syntax: 38:5:n
                        self.cur_attrs.fg = TermColor::Indexed(p[2] as u8);
                    } else if p.len() >= 5 && p[1] == 2 {
                        // Colon syntax: 38:2:r:g:b
                        self.cur_attrs.fg = TermColor::Rgb(p[2] as u8, p[3] as u8, p[4] as u8);
                    } else if let Some(sub) = iter.next() {
                        // Semicolon syntax
                        match sub.first().copied().unwrap_or(0) {
                            5 => {
                                let idx = iter.next()
                                    .and_then(|p| p.first().copied())
                                    .unwrap_or(0);
                                self.cur_attrs.fg = TermColor::Indexed(idx as u8);
                            }
                            2 => {
                                let r = iter.next().and_then(|p| p.first().copied()).unwrap_or(0);
                                let g = iter.next().and_then(|p| p.first().copied()).unwrap_or(0);
                                let b = iter.next().and_then(|p| p.first().copied()).unwrap_or(0);
                                self.cur_attrs.fg = TermColor::Rgb(r as u8, g as u8, b as u8);
                            }
                            _ => {}
                        }
                    }
                }
                39 => self.cur_attrs.fg = TermColor::Default,
                40..=47 => self.cur_attrs.bg = TermColor::Ansi(code as u8 - 40),
                48 => {
                    if p.len() >= 3 && p[1] == 5 {
                        self.cur_attrs.bg = TermColor::Indexed(p[2] as u8);
                    } else if p.len() >= 5 && p[1] == 2 {
                        self.cur_attrs.bg = TermColor::Rgb(p[2] as u8, p[3] as u8, p[4] as u8);
                    } else if let Some(sub) = iter.next() {
                        match sub.first().copied().unwrap_or(0) {
                            5 => {
                                let idx = iter.next()
                                    .and_then(|p| p.first().copied())
                                    .unwrap_or(0);
                                self.cur_attrs.bg = TermColor::Indexed(idx as u8);
                            }
                            2 => {
                                let r = iter.next().and_then(|p| p.first().copied()).unwrap_or(0);
                                let g = iter.next().and_then(|p| p.first().copied()).unwrap_or(0);
                                let b = iter.next().and_then(|p| p.first().copied()).unwrap_or(0);
                                self.cur_attrs.bg = TermColor::Rgb(r as u8, g as u8, b as u8);
                            }
                            _ => {}
                        }
                    }
                }
                49 => self.cur_attrs.bg = TermColor::Default,
                90..=97 => self.cur_attrs.fg = TermColor::Ansi(code as u8 - 90 + 8),
                100..=107 => self.cur_attrs.bg = TermColor::Ansi(code as u8 - 100 + 8),
                _ => {} // ignore unknown / unimplemented attributes
            }
        }
    }
}

// ── vte::Perform implementation ───────────────────────────────────────────────

impl Perform for GridState {
    fn print(&mut self, c: char) {
        self.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x08 => {
                // BS
                self.pending_wrap = false;
                if self.cursor_col > 0 { self.cursor_col -= 1; }
            }
            0x09 => {
                // HT — advance to the next tab stop (every 8 columns)
                self.pending_wrap = false;
                let next_tab = (self.cursor_col / 8 + 1) * 8;
                self.cursor_col = next_tab.min(self.cols.saturating_sub(1));
            }
            0x0A | 0x0B | 0x0C => {
                // LF / VT / FF
                self.pending_wrap = false;
                if self.cursor_row == self.scroll_bottom {
                    self.scroll_up(1);
                } else {
                    self.cursor_row = (self.cursor_row + 1).min(self.rows.saturating_sub(1));
                }
            }
            0x0D => {
                // CR
                self.pending_wrap = false;
                self.cursor_col = 0;
            }
            _ => {} // BEL and everything else — ignore
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        // Helper: n-th param value, treating 0 as `default` (for cursor cmds
        // where 0 means "1" per ECMA-48). Uses 0-indexed position in params.
        let p = |params: &Params, idx: usize, default: usize| -> usize {
            params.iter().nth(idx)
                .and_then(|p| p.first().copied())
                .map(|v| if v == 0 { default } else { v as usize })
                .unwrap_or(default)
        };
        // n-th param as raw value (0 stays 0 — for modes/erase selectors).
        let raw = |params: &Params, idx: usize| -> u16 {
            params.iter().nth(idx)
                .and_then(|p| p.first().copied())
                .unwrap_or(0)
        };

        let priv_mode = intermediates.first() == Some(&b'?');

        match (action, priv_mode) {
            ('A', false) => self.cursor_up(p(params, 0, 1)),
            ('B', false) => self.cursor_down(p(params, 0, 1)),
            ('C', false) => self.cursor_forward(p(params, 0, 1)),
            ('D', false) => self.cursor_back(p(params, 0, 1)),
            ('E', false) => {
                self.cursor_down(p(params, 0, 1));
                self.cursor_col = 0;
            }
            ('F', false) => {
                self.cursor_up(p(params, 0, 1));
                self.cursor_col = 0;
            }
            ('G', false) => {
                self.pending_wrap = false;
                let col = p(params, 0, 1).saturating_sub(1); // 1-indexed → 0
                self.cursor_col = col.min(self.cols.saturating_sub(1));
            }
            ('H' | 'f', false) => {
                let row = p(params, 0, 1).saturating_sub(1);
                let col = p(params, 1, 1).saturating_sub(1);
                self.cursor_set(row, col);
            }
            ('J', false) => self.erase_display(raw(params, 0)),
            ('K', false) => self.erase_line(raw(params, 0)),
            ('L', false) => self.insert_lines(p(params, 0, 1)),
            ('M', false) => self.delete_lines(p(params, 0, 1)),
            ('P', false) => self.delete_chars(p(params, 0, 1)),
            ('S', false) => self.scroll_up(p(params, 0, 1)),
            ('T', false) => self.scroll_down(p(params, 0, 1)),
            ('X', false) => self.erase_chars(p(params, 0, 1)),
            ('@', false) => self.insert_chars(p(params, 0, 1)),
            ('d', false) => {
                self.pending_wrap = false;
                let row = p(params, 0, 1).saturating_sub(1);
                self.cursor_row = row.min(self.rows.saturating_sub(1));
            }
            ('m', false) => self.handle_sgr(params),
            ('r', false) => {
                let top = p(params, 0, 1).saturating_sub(1);
                let bottom = p(params, 1, self.rows).saturating_sub(1);
                if top < bottom && bottom < self.rows {
                    self.scroll_top = top;
                    self.scroll_bottom = bottom;
                }
            }
            ('h', true) | ('l', true) => {
                let enable = action == 'h';
                for param_slice in params.iter() {
                    let mode = param_slice.first().copied().unwrap_or(0);
                    match mode {
                        // Alternate screen buffers
                        47 | 1047 | 1049 => {
                            if enable != self.alt_active {
                                if enable {
                                    // Clear the alternate screen when entering it.
                                    for row in &mut self.alternate {
                                        row.iter_mut().for_each(|c| *c = RawCell::default());
                                    }
                                    if mode == 1049 {
                                        self.saved_cursor = (self.cursor_row, self.cursor_col);
                                        self.saved_attrs = self.cur_attrs;
                                        // 1049 homes the cursor on entry
                                        self.cursor_row = 0;
                                        self.cursor_col = 0;
                                        self.pending_wrap = false;
                                    }
                                } else if mode == 1049 {
                                    // Restore cursor when leaving alt screen.
                                    (self.cursor_row, self.cursor_col) = self.saved_cursor;
                                    self.cur_attrs = self.saved_attrs;
                                }
                                self.alt_active = enable;
                            }
                        }
                        _ => {} // other private modes (cursor visibility, etc.) — ignore
                    }
                }
            }
            _ => {} // ignore unrecognised sequences
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // Only handle OSC 133 (shell integration). Everything else (window
        // titles, iTerm2 images, etc.) is irrelevant to the grid state.
        if params.len() < 2 { return; }
        if params[0] != b"133" { return; }
        let kind = match params.get(1).copied() {
            Some(b"A") => ZoneKind::PromptStart,
            Some(b"B") => ZoneKind::CommandStart,
            Some(b"C") => ZoneKind::OutputStart,
            Some(b"D") => ZoneKind::OutputEnd,
            _ => return,
        };
        let exit_code = if kind == ZoneKind::OutputEnd {
            params.get(2)
                .and_then(|p| std::str::from_utf8(p).ok())
                .and_then(|s| s.parse::<u32>().ok())
        } else {
            None
        };
        self.zones.push(ShellZone {
            kind,
            row: self.cursor_row,
            col: self.cursor_col,
            exit_code,
        });
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'M' => {
                // Reverse index: scroll down if at top of scroll region,
                // otherwise just move the cursor up one row.
                if self.cursor_row == self.scroll_top {
                    self.scroll_down(1);
                } else {
                    self.cursor_row = self.cursor_row.saturating_sub(1);
                }
            }
            b'7' => {
                // Save cursor + attributes (DECSC).
                self.saved_cursor = (self.cursor_row, self.cursor_col);
                self.saved_attrs = self.cur_attrs;
            }
            b'8' => {
                // Restore cursor + attributes (DECRC).
                (self.cursor_row, self.cursor_col) = self.saved_cursor;
                self.cur_attrs = self.saved_attrs;
                self.pending_wrap = false;
            }
            _ => {}
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Server-side terminal grid. Feed raw PTY bytes via [`Self::feed`]; call
/// [`Self::snapshot`] to read the current visible screen contents.
///
/// This type is owned by [`crate::manager::SessionManager`] alongside the
/// existing [`crate::OutputBuffer`] and [`crate::LineBuffer`]. All three are
/// fed the same byte stream in a single [`crate::manager::SessionManager::drain`]
/// call.
pub struct TerminalGrid {
    state: GridState,
    parser: vte::Parser,
}

impl TerminalGrid {
    /// Create a new grid with the given initial dimensions.
    ///
    /// Both dimensions are clamped to `1 × 1` so a zero size is never stored.
    /// The default PTY size used in [`crate::session::SessionConfig`] is
    /// 80 × 24, matching what most CLIs and curses applications assume.
    #[must_use]
    pub fn new(rows: u16, cols: u16) -> Self {
        let rows = (rows as usize).max(1);
        let cols = (cols as usize).max(1);
        Self {
            state: GridState::new(rows, cols),
            parser: vte::Parser::new(),
        }
    }

    /// Feed `bytes` from the PTY into the VTE parser, updating the grid.
    ///
    /// Every call to [`crate::manager::SessionManager::drain`] routes the same
    /// raw bytes here. The grid's visible state after this call accurately
    /// reflects what the terminal would render on a real screen.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.state, bytes);
    }

    /// Update the grid dimensions, preserving existing content where it fits.
    ///
    /// Called by [`crate::manager::SessionManager::resize`] to keep the grid
    /// in sync with the PTY's reported window size. The alternate screen is
    /// cleared on resize.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.state.resize(rows as usize, cols as usize);
    }

    /// Take a point-in-time snapshot of the visible screen.
    ///
    /// Clones the grid cells into a [`ScreenSnapshot`] that can be serialized
    /// over the IPC boundary. For a 24 × 80 grid this is a ~2 KB allocation;
    /// callers should not call this in a tight loop.
    #[must_use]
    pub fn snapshot(&self) -> ScreenSnapshot {
        self.state.snapshot()
    }
}

impl std::fmt::Debug for TerminalGrid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalGrid")
            .field("rows", &self.state.rows)
            .field("cols", &self.state.cols)
            .field("cursor", &(self.state.cursor_row, self.state.cursor_col))
            .finish_non_exhaustive()
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: u16, cols: u16) -> TerminalGrid {
        TerminalGrid::new(rows, cols)
    }

    #[test]
    fn plain_text_appears_in_text_rows() {
        let mut g = grid(24, 80);
        g.feed(b"hello");
        let snap = g.snapshot();
        assert_eq!(snap.text_rows[0], "hello");
        assert_eq!(snap.cursor_row, 0);
        assert_eq!(snap.cursor_col, 5);
    }

    #[test]
    fn cr_lf_advances_cursor_to_next_row() {
        let mut g = grid(24, 80);
        g.feed(b"line1\r\nline2");
        let snap = g.snapshot();
        assert_eq!(snap.text_rows[0], "line1");
        assert_eq!(snap.text_rows[1], "line2");
        assert_eq!(snap.cursor_row, 1);
    }

    #[test]
    fn lf_only_advances_without_cr() {
        let mut g = grid(24, 80);
        g.feed(b"AB\nCD");
        let snap = g.snapshot();
        // LF only moves row, not column — CD starts at col 2 (same as AB end).
        assert_eq!(snap.text_rows[0], "AB");
        assert_eq!(&snap.text_rows[1][2..4], "CD");
    }

    #[test]
    fn ansi_sgr_codes_do_not_appear_in_text_rows() {
        let mut g = grid(24, 80);
        g.feed(b"\x1b[31mred\x1b[0m plain");
        let snap = g.snapshot();
        assert_eq!(snap.text_rows[0], "red plain");
    }

    #[test]
    fn cursor_position_csi_h_positions_chars() {
        let mut g = grid(24, 80);
        // Write "AB", move to (1,1) = row 0 col 0, write "C"
        g.feed(b"AB\x1b[1;1HC"); // CSI 1;1 H = cursor row 1 col 1 (1-indexed)
        let snap = g.snapshot();
        // Row 0: "C" was written at col 0, then "B" remains at col 1
        assert_eq!(&snap.text_rows[0][..2], "CB");
    }

    #[test]
    fn csi_g_cursor_horizontal_absolute() {
        let mut g = grid(24, 80);
        g.feed(b"hello");
        g.feed(b"\x1b[3G"); // cursor to column 3 (1-indexed)
        g.feed(b"X");
        let snap = g.snapshot();
        // "heXlo" — col 2 (0-indexed) replaced
        assert_eq!(&snap.text_rows[0][..5], "heXlo");
    }

    #[test]
    fn erase_to_end_of_line_csi_k() {
        let mut g = grid(24, 80);
        g.feed(b"hello world");
        g.feed(b"\x1b[6G"); // cursor to column 6 (1-indexed = col 5, 0-indexed)
        g.feed(b"\x1b[K"); // erase from cursor to end of line
        let snap = g.snapshot();
        assert_eq!(snap.text_rows[0], "hello"); // " world" erased
    }

    #[test]
    fn erase_entire_line_csi_2k() {
        let mut g = grid(24, 80);
        g.feed(b"hello\r\nworld");
        g.feed(b"\x1b[1;1H"); // back to row 1
        g.feed(b"\x1b[2K");   // erase entire line
        let snap = g.snapshot();
        assert_eq!(snap.text_rows[0], "");
        assert_eq!(snap.text_rows[1], "world");
    }

    #[test]
    fn erase_display_csi_2j_clears_screen() {
        let mut g = grid(24, 80);
        g.feed(b"line1\r\nline2\r\nline3");
        g.feed(b"\x1b[2J"); // erase entire screen
        let snap = g.snapshot();
        for row in &snap.text_rows {
            assert!(row.is_empty(), "expected blank, got: {row:?}");
        }
    }

    #[test]
    fn scroll_up_by_lf_past_bottom() {
        let mut g = grid(3, 10); // 3-row grid
        // 4 LF-terminated lines → first line scrolls off
        g.feed(b"line1\r\nline2\r\nline3\r\nline4");
        let snap = g.snapshot();
        assert_eq!(snap.text_rows[0], "line2");
        assert_eq!(snap.text_rows[1], "line3");
        assert_eq!(snap.text_rows[2], "line4");
    }

    #[test]
    fn csi_scroll_up_s_moves_content() {
        let mut g = grid(4, 10);
        g.feed(b"A\r\nB\r\nC\r\nD");
        g.feed(b"\x1b[2S"); // scroll up 2
        let snap = g.snapshot();
        assert_eq!(snap.text_rows[0], "C");
        assert_eq!(snap.text_rows[1], "D");
        assert_eq!(snap.text_rows[2], ""); // blank
        assert_eq!(snap.text_rows[3], ""); // blank
    }

    #[test]
    fn insert_delete_lines() {
        let mut g = grid(4, 10);
        g.feed(b"A\r\nB\r\nC\r\nD");
        // Move to row 1 (B) and insert a blank line
        g.feed(b"\x1b[2;1H\x1b[L"); // cursor row 2, insert 1 line
        let snap = g.snapshot();
        assert_eq!(snap.text_rows[0], "A");
        assert_eq!(snap.text_rows[1], ""); // inserted blank
        assert_eq!(snap.text_rows[2], "B");
        assert_eq!(snap.text_rows[3], "C"); // D was pushed off
    }

    #[test]
    fn delete_chars_csi_p() {
        let mut g = grid(24, 20);
        g.feed(b"hello world");
        g.feed(b"\x1b[1;6H"); // cursor to col 6 (after "hello ")
        g.feed(b"\x1b[6P");   // delete 6 chars ("world" + one trailing space)
        let snap = g.snapshot();
        assert_eq!(snap.text_rows[0], "hello");
    }

    #[test]
    fn backspace_moves_cursor_left() {
        let mut g = grid(24, 80);
        g.feed(b"abc\x08X"); // write "abc", BS, write "X"
        let snap = g.snapshot();
        assert_eq!(&snap.text_rows[0][..3], "abX");
    }

    #[test]
    fn tab_advances_to_next_tab_stop() {
        let mut g = grid(24, 80);
        g.feed(b"A\tB"); // 'A' at col 0, tab → col 8, 'B' at col 8
        let snap = g.snapshot();
        assert_eq!(snap.text_rows[0].len(), 9); // "A" + 7 spaces + "B"
        assert_eq!(&snap.text_rows[0][0..1], "A");
        assert_eq!(&snap.text_rows[0][8..9], "B");
    }

    #[test]
    fn reverse_index_esc_m_scrolls_when_at_top() {
        let mut g = grid(3, 10);
        g.feed(b"A\r\nB\r\nC");
        g.feed(b"\x1b[1;1H"); // cursor to top-left
        g.feed(b"\x1bM");     // reverse index at top → scroll down (content moves down)
        let snap = g.snapshot();
        assert_eq!(snap.text_rows[0], ""); // blank inserted at top
        assert_eq!(snap.text_rows[1], "A");
        assert_eq!(snap.text_rows[2], "B"); // C was pushed off
    }

    #[test]
    fn save_restore_cursor_esc_7_8() {
        let mut g = grid(24, 80);
        g.feed(b"hello");
        g.feed(b"\x1b7");     // save cursor (row 0, col 5)
        g.feed(b"\r\nworld"); // move somewhere else
        g.feed(b"\x1b8");     // restore cursor
        let snap = g.snapshot();
        assert_eq!(snap.cursor_row, 0);
        assert_eq!(snap.cursor_col, 5);
    }

    #[test]
    fn alternate_screen_csi_1049_clears_on_entry() {
        let mut g = grid(24, 80);
        g.feed(b"primary content");
        g.feed(b"\x1b[?1049h"); // switch to alt screen (clears it)
        g.feed(b"alt content");
        let snap = g.snapshot();
        assert_eq!(snap.text_rows[0], "alt content");

        g.feed(b"\x1b[?1049l"); // restore primary
        let snap = g.snapshot();
        assert_eq!(snap.text_rows[0], "primary content");
    }

    #[test]
    fn osc_133_zones_are_recorded() {
        let mut g = grid(24, 80);
        g.feed(b"\x1b]133;A\x07"); // prompt start
        g.feed(b"$ ");
        g.feed(b"\x1b]133;B\x07"); // command start
        g.feed(b"ls\r\n");
        g.feed(b"\x1b]133;C\x07"); // output start
        g.feed(b"file.txt\r\n");
        g.feed(b"\x1b]133;D;0\x07"); // output end, exit code 0
        let snap = g.snapshot();
        assert_eq!(snap.zones.len(), 4);
        assert_eq!(snap.zones[0].kind, ZoneKind::PromptStart);
        assert_eq!(snap.zones[1].kind, ZoneKind::CommandStart);
        assert_eq!(snap.zones[2].kind, ZoneKind::OutputStart);
        assert_eq!(snap.zones[3].kind, ZoneKind::OutputEnd);
        assert_eq!(snap.zones[3].exit_code, Some(0));
    }

    #[test]
    fn osc_133_zones_accumulate_across_multiple_feeds() {
        let mut g = grid(24, 80);
        g.feed(b"\x1b]133;A\x07");
        g.feed(b"\x1b]133;B\x07");
        let snap1 = g.snapshot();
        assert_eq!(snap1.zones.len(), 2);
        // Subsequent snapshot sees the same accumulated list.
        let snap2 = g.snapshot();
        assert_eq!(snap2.zones.len(), 2);
    }

    #[test]
    fn resize_preserves_existing_content() {
        let mut g = grid(10, 40);
        g.feed(b"hello world");
        g.resize(10, 80); // widen
        let snap = g.snapshot();
        assert_eq!(snap.cols, 80);
        assert_eq!(snap.text_rows[0], "hello world");
    }

    #[test]
    fn resize_clamps_cursor_to_new_bounds() {
        let mut g = grid(24, 80);
        g.feed(b"\x1b[24;80H"); // cursor to bottom-right
        let snap = g.snapshot();
        assert_eq!(snap.cursor_row, 23);
        assert_eq!(snap.cursor_col, 79);
        g.resize(10, 40);
        let snap = g.snapshot();
        assert!(snap.cursor_row < 10);
        assert!(snap.cursor_col < 40);
    }

    #[test]
    fn rows_and_cols_match_construction() {
        let g = grid(24, 80);
        let snap = g.snapshot();
        assert_eq!(snap.rows, 24);
        assert_eq!(snap.cols, 80);
        assert_eq!(snap.text_rows.len(), 24);
    }

    #[test]
    fn pending_wrap_does_not_duplicate_last_column() {
        let mut g = grid(3, 5);
        g.feed(b"ABCDE"); // fills cols 0-4, pending_wrap set
        g.feed(b"F");     // auto-wrap: next row, col 0
        let snap = g.snapshot();
        assert_eq!(snap.text_rows[0], "ABCDE");
        assert_eq!(&snap.text_rows[1][..1], "F");
    }

    #[test]
    fn scroll_region_csi_r_limits_scroll_area() {
        let mut g = grid(5, 10);
        g.feed(b"A\r\nB\r\nC\r\nD\r\nE");
        // Set scroll region to rows 2-4 (1-indexed), i.e. rows 1-3 (0-indexed)
        g.feed(b"\x1b[2;4r");
        // Move cursor to bottom of scroll region (row 4 / index 3) and LF
        g.feed(b"\x1b[4;1H\n");
        let snap = g.snapshot();
        // Row 0 (A) is outside the scroll region → unchanged
        assert_eq!(snap.text_rows[0], "A");
        // Row 4 (E) is outside → unchanged
        assert_eq!(snap.text_rows[4], "E");
        // Rows 1-3 scrolled up; C moved to row 1, D to row 2, row 3 blank
        assert_eq!(snap.text_rows[1], "C");
        assert_eq!(snap.text_rows[2], "D");
        assert_eq!(snap.text_rows[3], ""); // blank inserted
    }
}
