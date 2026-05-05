//! `alacritty_terminal` grid wrapper for nexus-terminal (ADR 0027).
//!
//! [`TermGrid`] owns a `Term<NexusEventListener>` and a VTE `Processor`.
//! Raw PTY bytes are fed in via [`TermGrid::process_bytes`]; the full
//! rendered screen is read back via [`TermGrid::read_screen`] (structured
//! cells, used by the gpui renderer) or [`TermGrid::read_screen_text`]
//! (plain text, used by `nexus-agent` for context injection).
//!
//! # Thread safety
//! `TermGrid` is `!Send` (alacritty's `Term` is not `Sync`). The session
//! manager holds it under its existing `Mutex<SessionManager>`, so callers
//! already serialise access.

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::Config;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor, StdSyncHandler};
use alacritty_terminal::Term;
use serde::{Deserialize, Serialize};

// ── IPC response types ────────────────────────────────────────────────────────

/// Colour of a single terminal cell, mapped from alacritty's `Color` enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum CellColor {
    /// The terminal's default foreground or background colour.
    Default,
    /// A named ANSI colour (value is the `NamedColor` discriminant).
    Named(u8),
    /// An indexed colour from the 256-colour palette.
    Indexed(u8),
    /// A 24-bit RGB colour (r, g, b).
    Rgb(u8, u8, u8),
}

/// Compact flags byte exposed over IPC. Bit positions:
/// - 0x01 = bold
/// - 0x02 = italic
/// - 0x04 = underline (any style)
/// - 0x08 = strikeout
/// - 0x10 = dim
/// - 0x20 = inverse
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenCell {
    /// UTF-8 text content of the cell (typically one grapheme cluster).
    pub text: String,
    /// Foreground colour.
    pub fg: CellColor,
    /// Background colour.
    pub bg: CellColor,
    /// Packed attribute flags (bold, italic, underline, etc.).
    pub flags: u8,
}

/// One row of cells in the visible terminal screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenRow {
    /// Ordered cells from column 0 to the last column.
    pub cells: Vec<ScreenCell>,
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn color_to_cell_color(c: Color) -> CellColor {
    match c {
        Color::Named(n) => CellColor::Named(n as u8),
        Color::Indexed(i) => CellColor::Indexed(i),
        Color::Spec(rgb) => CellColor::Rgb(rgb.r, rgb.g, rgb.b),
    }
}

fn encode_flags(f: Flags) -> u8 {
    let mut out: u8 = 0;
    if f.contains(Flags::BOLD)    { out |= 0x01; }
    if f.contains(Flags::ITALIC)  { out |= 0x02; }
    if f.intersects(Flags::ALL_UNDERLINES) { out |= 0x04; }
    if f.contains(Flags::STRIKEOUT) { out |= 0x08; }
    if f.contains(Flags::DIM)    { out |= 0x10; }
    if f.contains(Flags::INVERSE) { out |= 0x20; }
    out
}

// ── NexusEventListener ────────────────────────────────────────────────────────

/// Minimal event listener. Discards events that require a live PTY write-back
/// (device attribute queries, colour requests) during the transition period;
/// those will be handled when the gpui shell owns the PTY directly in Phase 2.
pub struct NexusEventListener;

impl EventListener for NexusEventListener {
    fn send_event(&self, event: Event) {
        match event {
            // Log title changes — useful for debugging shell integration.
            Event::Title(t) => tracing::debug!("terminal title: {t}"),
            Event::Bell      => tracing::trace!("terminal bell"),
            // PtyWrite: the terminal wants to reply to a device query.
            // During Phase 2 the caller should forward these to the PTY writer.
            // For now they are intentionally dropped.
            Event::PtyWrite(s) => tracing::trace!("terminal pty write (dropped): {s:?}"),
            _ => {}
        }
    }
}

// ── TermDimensions ────────────────────────────────────────────────────────────

struct TermDimensions {
    cols:  usize,
    lines: usize,
}

impl Dimensions for TermDimensions {
    fn total_lines(&self) -> usize { self.lines }
    fn screen_lines(&self) -> usize { self.lines }
    fn columns(&self)      -> usize { self.cols }
}

// ── TermGrid ──────────────────────────────────────────────────────────────────

/// Wraps an `alacritty_terminal::Term` + VTE processor for one PTY session.
pub struct TermGrid {
    term:      Term<NexusEventListener>,
    processor: Processor<StdSyncHandler>,
    cols:      usize,
    lines:     usize,
}

impl TermGrid {
    /// Create a new `TermGrid` with the given dimensions.
    pub fn new(cols: usize, lines: usize) -> Self {
        let config = Config {
            scrolling_history: 10_000,
            ..Config::default()
        };
        let dims = TermDimensions { cols, lines };
        let term = Term::new(config, &dims, NexusEventListener);
        Self { term, processor: Processor::new(), cols, lines }
    }

    /// Feed raw PTY bytes into the VTE state machine.
    pub fn process_bytes(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
    }

    /// Resize the terminal grid. Should be called whenever the PTY is resized.
    pub fn resize(&mut self, cols: usize, lines: usize) {
        if self.cols == cols && self.lines == lines {
            return;
        }
        self.cols  = cols;
        self.lines = lines;
        self.term.resize(TermDimensions { cols, lines });
    }

    /// Return the current visible screen as structured cell data.
    ///
    /// Each row contains exactly `columns()` cells; trailing space cells are
    /// preserved so the caller can compute character positions correctly.
    pub fn read_screen(&self) -> Vec<ScreenRow> {
        let grid  = self.term.grid();
        let nrows = grid.screen_lines();
        let ncols = grid.columns();

        (0..nrows)
            .map(|l| {
                let cells = (0..ncols)
                    .map(|c| {
                        let cell = &grid[Line(l as i32)][Column(c)];
                        let is_fg_default = matches!(
                            cell.fg,
                            Color::Named(NamedColor::Foreground)
                        );
                        let is_bg_default = matches!(
                            cell.bg,
                            Color::Named(NamedColor::Background)
                        );
                        ScreenCell {
                            text:  cell.c.to_string(),
                            fg:    if is_fg_default { CellColor::Default } else { color_to_cell_color(cell.fg) },
                            bg:    if is_bg_default { CellColor::Default } else { color_to_cell_color(cell.bg) },
                            flags: encode_flags(cell.flags),
                        }
                    })
                    .collect();
                ScreenRow { cells }
            })
            .collect()
    }

    /// Return the current visible screen as plain text.
    ///
    /// Each row is right-trimmed of spaces and terminated with `\n`.
    pub fn read_screen_text(&self) -> String {
        let grid  = self.term.grid();
        let nrows = grid.screen_lines();
        let ncols = grid.columns();
        let mut out = String::with_capacity(nrows * (ncols + 1));

        for l in 0..nrows {
            let row_start = out.len();
            for c in 0..ncols {
                out.push(grid[Line(l as i32)][Column(c)].c);
            }
            // Right-trim trailing spaces.
            while out.len() > row_start && out.ends_with(' ') {
                out.pop();
            }
            out.push('\n');
        }
        out
    }

    /// Current terminal dimensions (cols × lines).
    pub fn dimensions(&self) -> (usize, usize) {
        (self.cols, self.lines)
    }
}
