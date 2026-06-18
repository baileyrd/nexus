//! nexus-vt — a headless VT (terminal) engine for Nexus.
//!
//! This is an in-tree port of the GUI-free core of
//! [`baileyrd/rusty_term`](https://github.com/baileyrd/rusty_term): a
//! VT100/ECMA-48 escape-sequence parser ([`AnsiParser`]) driving a screen
//! [`Grid`] with scrollback, an alternate screen, a scrolling region, OSC
//! handling (title / cwd / hyperlinks / colors), and OSC 133 semantic-prompt
//! command/exit-code tracking. It models terminal *state* — what is on the
//! screen — server-side, so the Rust backend, CLI, TUI, and agents can read it
//! (RFC 0003 Track B). It is **headless**: no windowing, no renderer, and no
//! in-band side channel (see [`core::channel`]). Its only dependencies are
//! `unicode-width` and `unicode-segmentation`.
//!
//! The [`Vt`] facade is the high-level entry point: feed it raw PTY bytes with
//! [`Vt::advance`] and read structured state back ([`Vt::screen_text`],
//! [`Vt::last_exit`], …). The lower-level [`Grid`] and [`AnsiParser`] are
//! re-exported for callers that need direct access.
//!
//! See `ATTRIBUTION.md` for what was ported and what was deliberately left out
//! (the `winit`/`wgpu` GUI, the PTY backend + tokio runtime, and the L13 OSC
//! JSON-RPC transport).

mod core;

pub use core::{
    AnsiParser, CursorShape, DirtyFrame, Grid, LineAttr, Theme, SCROLLBACK_MAX, ATTR_BLINK,
    ATTR_BOLD, ATTR_DIM, ATTR_HIDDEN, ATTR_ITALIC, ATTR_MASK, ATTR_REVERSE, ATTR_STRIKE,
    ATTR_UNDERLINE, WIDE_TRAILER,
};

/// A headless terminal: a [`Grid`] plus the [`AnsiParser`] that drives it.
///
/// Feed it raw PTY output with [`advance`](Self::advance); read the modelled
/// screen, scrollback, cursor, working directory, title, and last-command
/// exit/output back through the accessors. This is the agent-/CLI-/TUI-facing
/// view of a terminal session — a parallel, structured model alongside whatever
/// interactive renderer (e.g. xterm.js) the frontend uses.
pub struct Vt {
    grid: Grid,
    parser: AnsiParser,
}

impl Vt {
    /// Create a `cols`×`rows` terminal (both must be non-zero for a usable
    /// grid; the underlying [`Grid`] clamps degenerate sizes).
    #[must_use]
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            grid: Grid::new(cols, rows),
            parser: AnsiParser::new(),
        }
    }

    /// Feed raw bytes read from the PTY through the parser into the grid.
    pub fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.grid, bytes);
    }

    /// Resize the terminal, reflowing scrollback to the new width.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.grid.resize(cols, rows);
    }

    /// The current visible screen as text (one line per row, trailing blank
    /// rows dropped).
    #[must_use]
    pub fn screen_text(&self) -> String {
        self.grid.screen_text()
    }

    /// The most recent `n` scrollback lines as text (oldest first).
    #[must_use]
    pub fn scrollback_text(&self, n: usize) -> String {
        self.grid.scrollback_text(n)
    }

    /// Cursor position as `(col, row)`, both zero-based.
    #[must_use]
    pub fn cursor(&self) -> (usize, usize) {
        self.grid.cursor
    }

    /// Terminal size as `(cols, rows)`.
    #[must_use]
    pub fn size(&self) -> (usize, usize) {
        (self.grid.cols, self.grid.rows)
    }

    /// Working directory last reported by the child via OSC 7 (empty until set).
    #[must_use]
    pub fn cwd(&self) -> &str {
        &self.grid.cwd
    }

    /// Window title last set by the child via OSC 0/2 (empty until set).
    #[must_use]
    pub fn title(&self) -> &str {
        &self.grid.title
    }

    /// Exit code of the last finished command (OSC 133;D), or `None`.
    #[must_use]
    pub fn last_exit(&self) -> Option<i32> {
        self.grid.last_exit_code()
    }

    /// Output text of the last finished command (between OSC 133;C and ;D), if
    /// captured.
    #[must_use]
    pub fn last_command_output(&self) -> Option<&str> {
        self.grid.last_output()
    }

    /// If a command finished (OSC 133;D) since the last call, return its exit
    /// code (`None` when the shell omitted it) and captured output, clearing the
    /// pending flag. Returns `None` when no command finished since last drained.
    ///
    /// Call this after each [`advance`](Self::advance) to emit exactly one
    /// command-finished signal per completion.
    pub fn take_finished_command(&mut self) -> Option<(Option<i32>, Option<String>)> {
        if self.grid.take_command_finished() {
            Some((self.grid.last_exit_code(), self.grid.last_output().map(str::to_owned)))
        } else {
            None
        }
    }

    /// Direct access to the underlying grid (for callers needing cells, dirty
    /// rows, or other modelled state beyond the text accessors).
    #[must_use]
    pub fn grid(&self) -> &Grid {
        &self.grid
    }
}

#[cfg(test)]
mod facade_tests {
    use super::*;

    #[test]
    fn prints_and_reads_back_a_line() {
        let mut vt = Vt::new(20, 5);
        vt.advance(b"hello world");
        assert!(vt.screen_text().contains("hello world"));
        assert_eq!(vt.cursor().1, 0);
    }

    #[test]
    fn osc_133_captures_exit_code_and_output() {
        let mut vt = Vt::new(40, 10);
        // Prompt start, command-output begin, some output, then finished;0.
        vt.advance(b"\x1b]133;A\x07ls\x1b]133;C\x07hello\n\x1b]133;D;0\x07");
        assert_eq!(vt.last_exit(), Some(0));
        assert!(
            vt.last_command_output().unwrap_or_default().contains("hello"),
            "captured output was {:?}",
            vt.last_command_output()
        );
    }

    #[test]
    fn osc_133_nonzero_exit() {
        let mut vt = Vt::new(40, 10);
        vt.advance(b"\x1b]133;C\x07boom\n\x1b]133;D;7\x07");
        assert_eq!(vt.last_exit(), Some(7));
    }

    #[test]
    fn take_finished_command_drains_once_per_completion() {
        let mut vt = Vt::new(40, 10);
        // No completion yet.
        assert_eq!(vt.take_finished_command(), None);

        vt.advance(b"\x1b]133;C\x07out\n\x1b]133;D;2\x07");
        let finished = vt.take_finished_command();
        assert!(matches!(finished, Some((Some(2), Some(ref o))) if o.contains("out")));
        // Draining is one-shot: a second call sees nothing new.
        assert_eq!(vt.take_finished_command(), None);

        // A second command finishes and is drained independently.
        vt.advance(b"\x1b]133;C\x07\x1b]133;D;0\x07");
        assert!(matches!(vt.take_finished_command(), Some((Some(0), _))));
    }

    #[test]
    fn osc_0_sets_title_and_osc_7_sets_cwd() {
        let mut vt = Vt::new(40, 10);
        vt.advance(b"\x1b]0;my-title\x07");
        vt.advance(b"\x1b]7;file:///home/user/work\x07");
        assert_eq!(vt.title(), "my-title");
        assert_eq!(vt.cwd(), "file:///home/user/work");
    }

    #[test]
    fn scrollback_accumulates_past_the_screen() {
        let mut vt = Vt::new(10, 2);
        for i in 0..6 {
            vt.advance(format!("line{i}\r\n").as_bytes());
        }
        // With a 2-row screen, earlier lines must have scrolled into history.
        let sb = vt.scrollback_text(100);
        assert!(sb.contains("line0"), "scrollback was {sb:?}");
    }
}
