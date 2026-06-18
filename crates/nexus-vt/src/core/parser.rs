//! The escape-sequence parser (L06): an incremental state machine that turns a
//! shell's output byte stream into [`Grid`] mutations.
//!
//! It implements a pragmatic subset of the VT100/ECMA-48 escape repertoire
//! (SGR colors, cursor positioning, erase line/display, scrolling region,
//! alternate screen, device-attribute replies) and tracks the current SGR
//! colors across `advance` calls. OSC strings are handed to [`osc::dispatch`];
//! DCS/APC/PM/SOS strings are consumed opaquely so they never leak as text.

use super::cell::{
    ATTR_BLINK, ATTR_BOLD, ATTR_DIM, ATTR_HIDDEN, ATTR_ITALIC, ATTR_REVERSE, ATTR_STRIKE,
    ATTR_UNDERLINE, Pen,
};
#[cfg(feature = "l13")]
use super::channel;
use super::charset::Charset;
use super::color::Palette;
use super::grid::{CursorShape, Grid, LineAttr, alt_mode};
use super::kitty;
use super::osc;
use super::sixel;

/// States of the escape-sequence recognizer.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ParserState {
    /// Printable / control bytes go straight to the grid.
    Ground,
    /// Saw `ESC`; awaiting the sequence introducer.
    Esc,
    /// Inside a `CSI` (`ESC [`) sequence; accumulating parameter bytes.
    Csi,
    /// Mid-way through a multibyte UTF-8 code point; awaiting continuation bytes.
    Utf8,
    /// Inside an `OSC` (`ESC ]`) string; bytes are consumed until a `BEL` or
    /// `ST` terminator. We don't act on OSC payloads (titles, cwd reports), we
    /// just keep them from leaking onto the screen.
    Osc,
    /// Saw `ESC` while inside an OSC string; awaiting the `\` of an `ST`
    /// (`ESC \`) terminator.
    OscEsc,
    /// A charset-designation escape (`ESC ( B`, etc.); consume one more byte.
    EscCharset,
    /// An `ESC #` sequence; the next byte selects the function (`8` = DECALN).
    EscHash,
    /// Inside a SOS (`ESC X`) or PM (`ESC ^`) string. The body is consumed
    /// opaquely until `ST` so it never leaks as printed text.
    StrSink,
    /// Saw `ESC` while inside a [`ParserState::StrSink`] string; awaiting the
    /// `\` of an `ST` terminator.
    StrSinkEsc,
    /// Inside a DCS (`ESC P`) string; the body is buffered into `dcs_buffer` so
    /// a Sixel image (`<params> q <data>`) can be decoded at `ST`. Other DCS
    /// types are recognized and discarded.
    Dcs,
    /// Saw `ESC` while inside a [`ParserState::Dcs`] string; awaiting `ST`.
    DcsEsc,
    /// Inside an APC (`ESC _`) string; buffered into `apc_buffer` so a Kitty
    /// graphics command (`G…`) can be decoded at `ST`. Other APC strings are
    /// recognized and discarded.
    Apc,
    /// Saw `ESC` while inside a [`ParserState::Apc`] string; awaiting `ST`.
    ApcEsc,
}

/// Incremental parser that turns a shell's output byte stream into [`Grid`]
/// mutations. Tracks the current SGR colors across `advance` calls.
pub struct AnsiParser {
    state: ParserState,
    /// Current SGR graphic rendition (colors + attributes) stamped onto each
    /// glyph written to the grid.
    pen: Pen,
    param_buffer: String,
    /// Set when a CSI sequence carries a private marker (`?`, `<`, `=`, `>`).
    /// Such sequences (e.g. DEC private mode set/reset) are consumed but not
    /// acted upon.
    csi_private: bool,
    /// The actual private-marker byte (`?`/`<`/`=`/`>`) of the CSI in flight, or
    /// `0` if none. Lets the dispatcher tell DA2 (`CSI > c`) from DEC private
    /// modes (`CSI ? … h/l`). Reset at the start of each CSI.
    csi_marker: u8,
    /// The last intermediate byte (`0x20..=0x2f`) seen in the CSI in flight, or
    /// `0` if none. Distinguishes e.g. DECSTR (`CSI ! p`) from a bare `CSI p`.
    /// Reset at the start of each CSI.
    csi_intermediate: u8,
    /// Code point accumulated so far while in [`ParserState::Utf8`].
    utf8_acc: u32,
    /// Number of UTF-8 continuation bytes still expected.
    utf8_remaining: usize,
    /// Bytes the parser owes the host in reply to a query (DA1/DA2/DSR). The
    /// driver drains these via [`AnsiParser::take_responses`] after each
    /// `advance` and writes them back to the PTY master, where the child reads
    /// them as terminal input.
    responses: Vec<u8>,
    /// Raw bytes of the OSC string currently being collected (between `ESC ]`
    /// and its `BEL`/`ST` terminator). Decoded as UTF-8 at dispatch. Capped at
    /// [`OSC_MAX`] so a pathological unterminated OSC can't grow without bound.
    osc_buffer: Vec<u8>,
    /// Raw bytes of the DCS string currently being collected (between `ESC P`
    /// and its `ST`), used to decode Sixel images. Capped at [`DCS_MAX`].
    dcs_buffer: Vec<u8>,
    /// Raw bytes of the APC string currently being collected (between `ESC _`
    /// and its `ST`), used to decode Kitty graphics. Capped at [`DCS_MAX`].
    apc_buffer: Vec<u8>,
    /// In-flight Kitty graphics transmission, accumulated across APC chunks.
    kitty: kitty::Transmission,
    /// The most recently printed graphic character, for `REP` (`CSI b`) to
    /// repeat. Cleared by C0 controls (CR/LF/BS/HT) so `REP` only repeats a
    /// character still adjacent to the cursor, matching xterm.
    last_char: Option<char>,
    /// The four G0–G3 character-set slots, designated by `ESC ( ) * +`. G0 is
    /// active in GL initially; SI/SO switch GL between G0 and G1.
    charsets: [Charset; 4],
    /// Active GL slot: `0` (G0, selected by SI) or `1` (G1, selected by SO).
    /// The printable-byte path maps through `charsets[gl]`.
    gl: usize,
    /// The G-slot a pending `ESC ( ) * +` designation writes to, captured when
    /// the intermediate byte is seen so the final byte lands in the right slot.
    charset_slot: usize,
    /// The live color table: 256 indexed colors plus the dynamic default
    /// fg/bg/cursor. SGR color selectors resolve through it; OSC 4/10/11/12
    /// mutate it.
    palette: Palette,
}

/// Upper bound on the bytes buffered for a single OSC string. Real titles and
/// cwd URIs are far shorter; past this we keep consuming the string but stop
/// storing it.
const OSC_MAX: usize = 4096;

/// Upper bound on an OSC string carrying an iTerm2 inline image
/// (`OSC 1337 ; File=...:<base64>`), which is far larger than a title. Matched on
/// the `1337;File=` prefix so ordinary OSC strings keep the tight [`OSC_MAX`] cap.
const OSC_IMAGE_MAX: usize = 8 * 1024 * 1024;

/// Upper bound on the bytes buffered for a single DCS string. Sixel images can
/// be large but are bounded here; past this we keep consuming but stop storing.
const DCS_MAX: usize = 4 * 1024 * 1024;

impl Default for AnsiParser {
    fn default() -> Self {
        Self::new()
    }
}

impl AnsiParser {
    /// Create a parser in the ground state with default colors.
    pub fn new() -> Self {
        Self::with_theme(super::color::Theme::default())
    }

    /// Create a parser in the ground state whose palette — and every later
    /// reset of it — is seeded from `theme`. The initial pen uses the theme's
    /// default colors, so text drawn before any SGR lands in the right colors.
    pub fn with_theme(theme: super::color::Theme) -> Self {
        let palette = Palette::with_theme(theme);
        let pen = Pen {
            fg: palette.fg,
            bg: palette.bg,
            attrs: 0,
        };
        Self {
            state: ParserState::Ground,
            pen,
            param_buffer: String::new(),
            csi_private: false,
            csi_marker: 0,
            csi_intermediate: 0,
            utf8_acc: 0,
            utf8_remaining: 0,
            responses: Vec::new(),
            osc_buffer: Vec::new(),
            dcs_buffer: Vec::new(),
            apc_buffer: Vec::new(),
            kitty: kitty::Transmission::default(),
            last_char: None,
            charsets: [Charset::Ascii; 4],
            gl: 0,
            charset_slot: 0,
            palette,
        }
    }

    /// Drain the bytes the parser owes the host in reply to queries (DA1/DA2/
    /// DSR). Returns an empty vector when there is nothing to send. The driver
    /// calls this after `advance` and writes the result to the PTY master.
    pub fn take_responses(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.responses)
    }

    /// Live theme switch (config reload). Returns the previous seed theme so
    /// the caller can remap the grid with [`Grid::retheme`].
    ///
    /// The live palette's dynamic state rides through: entries still at their
    /// old built-in value follow the new theme, while colors the child set
    /// itself (OSC 4/10/11/12) are kept — the child's choices outrank ours,
    /// exactly as they would have had the new theme been set at startup. The
    /// pen is remapped the same way, so text typed next stays coherent.
    pub fn retheme(&mut self, new: super::color::Theme) -> super::color::Theme {
        let old = self.palette.seed();
        self.pen.fg = super::color::remap(self.pen.fg, &old, &new);
        self.pen.bg = super::color::remap(self.pen.bg, &old, &new);
        self.palette.retheme(&old, &new);
        old
    }

    /// Feed a chunk of bytes, applying their effects to `g`. Parser state
    /// persists across calls, so escape sequences may straddle chunk boundaries.
    pub fn advance(&mut self, g: &mut Grid, bytes: &[u8]) {
        for &b in bytes {
            match self.state {
                ParserState::Ground => self.ground_byte(b, g),
                ParserState::Utf8 => {
                    if (0x80..=0xbf).contains(&b) {
                        // Valid continuation byte: fold in its 6 payload bits.
                        self.utf8_acc = (self.utf8_acc << 6) | (b as u32 & 0x3f);
                        self.utf8_remaining -= 1;
                        if self.utf8_remaining == 0 {
                            let ch = char::from_u32(self.utf8_acc).unwrap_or('\u{FFFD}');
                            g.put_char(ch, self.pen);
                            self.last_char = Some(ch);
                            self.state = ParserState::Ground;
                        }
                    } else {
                        // Truncated sequence: emit a replacement char, then
                        // reprocess this byte from the ground state.
                        g.put_char('\u{FFFD}', self.pen);
                        self.state = ParserState::Ground;
                        self.ground_byte(b, g);
                    }
                }
                ParserState::Esc => match b {
                    b'[' => {
                        self.param_buffer.clear();
                        self.csi_private = false;
                        self.csi_marker = 0;
                        self.csi_intermediate = 0;
                        self.state = ParserState::Csi;
                    }
                    b']' => {
                        self.osc_buffer.clear();
                        self.state = ParserState::Osc;
                    }
                    // DECSC / DECRC: save and restore the cursor.
                    b'7' => {
                        g.save_cursor();
                        self.state = ParserState::Ground;
                    }
                    b'8' => {
                        g.restore_cursor();
                        self.state = ParserState::Ground;
                    }
                    // IND — index: down one line, scrolling at the region bottom.
                    b'D' => {
                        g.newline();
                        self.state = ParserState::Ground;
                    }
                    // NEL — next line: carriage return followed by an index.
                    b'E' => {
                        g.carriage_return();
                        g.newline();
                        self.state = ParserState::Ground;
                    }
                    // RI — reverse index: up one line, scrolling at the region top.
                    b'M' => {
                        g.reverse_index();
                        self.state = ParserState::Ground;
                    }
                    // HTS — set a tab stop at the current column.
                    b'H' => {
                        g.set_tab_stop();
                        self.state = ParserState::Ground;
                    }
                    // RIS — reset to initial state (full reset).
                    b'c' => {
                        self.palette.reset();
                        self.reset_sgr();
                        // Sync the themed defaults *before* the grid refills
                        // its cells, so the blank screen lands in theme colors.
                        g.set_default_colors(self.palette.fg, self.palette.bg, self.palette.cursor);
                        g.reset();
                        self.charsets = [Charset::Ascii; 4];
                        self.gl = 0;
                        self.last_char = None;
                        self.state = ParserState::Ground;
                    }
                    // ESC # — the next byte selects the function (8 = DECALN).
                    b'#' => self.state = ParserState::EscHash,
                    // Charset designation: `ESC ( ) * +` choose the G0–G3 slot
                    // the next (final) byte designates into.
                    b'(' => {
                        self.charset_slot = 0;
                        self.state = ParserState::EscCharset;
                    }
                    b')' => {
                        self.charset_slot = 1;
                        self.state = ParserState::EscCharset;
                    }
                    b'*' => {
                        self.charset_slot = 2;
                        self.state = ParserState::EscCharset;
                    }
                    b'+' => {
                        self.charset_slot = 3;
                        self.state = ParserState::EscCharset;
                    }
                    // DCS (`ESC P`) is buffered so a Sixel image can be decoded
                    // at ST (see `finish_dcs`); other DCS types are discarded.
                    // SOS (`X`), PM (`^`), and APC (`_`, e.g. Kitty graphics) are
                    // consumed opaquely so they don't leak as printed text.
                    b'P' => {
                        self.dcs_buffer.clear();
                        self.state = ParserState::Dcs;
                    }
                    b'_' => {
                        self.apc_buffer.clear();
                        self.state = ParserState::Apc;
                    }
                    b'X' | b'^' => self.state = ParserState::StrSink,
                    // Any other ESC X sequence is a single byte we don't model;
                    // consuming b returns us to ground without leaking it.
                    _ => self.state = ParserState::Ground,
                },
                ParserState::Csi => match b {
                    // Parameter bytes.
                    b'0'..=b'9' | b';' => self.param_buffer.push(b as char),
                    // Private markers (`<`, `=`, `>`, `?`): flag, remember which,
                    // and keep collecting.
                    0x3c..=0x3f => {
                        self.csi_private = true;
                        self.csi_marker = b;
                    }
                    // Intermediate bytes (space..`/`): remembered (the last one
                    // is enough to tell DECSTR `CSI ! p` from a bare `CSI p`).
                    0x20..=0x2f => self.csi_intermediate = b,
                    // Final byte: dispatch and reset. Private sequences (with a
                    // `?`/`<`/`=`/`>` marker) go to their own handler.
                    0x40..=0x7e => {
                        if self.csi_private {
                            self.handle_private_csi(b, g);
                        } else {
                            self.handle_csi(b, g);
                        }
                        self.state = ParserState::Ground;
                        self.param_buffer.clear();
                        self.csi_private = false;
                    }
                    // CAN / SUB cancel the sequence.
                    0x18 | 0x1a => {
                        self.state = ParserState::Ground;
                        self.param_buffer.clear();
                        self.csi_private = false;
                    }
                    // ESC starts a fresh escape sequence.
                    0x1b => {
                        self.state = ParserState::Esc;
                        self.param_buffer.clear();
                        self.csi_private = false;
                    }
                    // Other C0 controls execute in place; the CSI continues
                    // (VT500 parser semantics) so its parameters are preserved.
                    0x00..=0x17 | 0x19 | 0x1c..=0x1f => {
                        self.ground_byte(b, g);
                        self.state = ParserState::Csi;
                    }
                    // DEL is ignored inside a CSI; any other byte aborts.
                    _ => {
                        if b != 0x7f {
                            self.state = ParserState::Ground;
                            self.param_buffer.clear();
                            self.csi_private = false;
                        }
                    }
                },
                ParserState::Osc => match b {
                    0x07 => {
                        // BEL terminator: act on the collected string.
                        self.finish_osc(g);
                        self.state = ParserState::Ground;
                    }
                    0x1b => self.state = ParserState::OscEsc, // possible ST (ESC \)
                    _ => {
                        // Accumulate the payload byte, bounded. iTerm2 inline
                        // images (`1337;File=`) get a much larger cap; the cheap
                        // length check short-circuits for every ordinary OSC.
                        if self.osc_buffer.len() < OSC_MAX
                            || (self.osc_buffer.len() < OSC_IMAGE_MAX
                                && self.osc_buffer.starts_with(b"1337;File="))
                        {
                            self.osc_buffer.push(b);
                        }
                    }
                },
                ParserState::OscEsc => {
                    // Whether or not this is the `\` of an ST, the OSC string is
                    // over; act on it and return to ground.
                    self.finish_osc(g);
                    self.state = ParserState::Ground;
                }
                ParserState::EscCharset => {
                    // The final byte designates a charset into the slot chosen
                    // by the `( ) * +` intermediate; `0` is DEC line-drawing.
                    self.charsets[self.charset_slot] = Charset::from_designator(b);
                    self.state = ParserState::Ground;
                }
                ParserState::EscHash => {
                    // `ESC # 8` is DECALN (screen-alignment fill); `ESC # 3/4/5/6`
                    // set the current line's size (DECDHL top/bottom, DECDWL,
                    // DECSWL). Anything else is consumed without effect.
                    match b {
                        b'8' => g.fill_alignment(),
                        b'3' => g.set_line_attr(LineAttr::DoubleTop),
                        b'4' => g.set_line_attr(LineAttr::DoubleBottom),
                        b'5' => g.set_line_attr(LineAttr::Single),
                        b'6' => g.set_line_attr(LineAttr::DoubleWidth),
                        _ => {}
                    }
                    self.state = ParserState::Ground;
                }
                ParserState::StrSink => match b {
                    0x1b => self.state = ParserState::StrSinkEsc, // possible ST (ESC \)
                    0x18 | 0x1a => self.state = ParserState::Ground, // CAN / SUB abort
                    _ => {}                                       // consume body byte
                },
                ParserState::StrSinkEsc => {
                    // Whether or not this is the `\` of an ST, the string is
                    // over; drop the byte and return to ground.
                    self.state = ParserState::Ground;
                }
                ParserState::Dcs => match b {
                    0x1b => self.state = ParserState::DcsEsc, // possible ST (ESC \)
                    0x18 | 0x1a => {
                        // CAN / SUB abort the string with no action.
                        self.dcs_buffer.clear();
                        self.state = ParserState::Ground;
                    }
                    _ => {
                        if self.dcs_buffer.len() < DCS_MAX {
                            self.dcs_buffer.push(b);
                        }
                    }
                },
                ParserState::DcsEsc => {
                    // The DCS string is over (ST or otherwise); act on it.
                    self.finish_dcs(g);
                    self.state = ParserState::Ground;
                }
                ParserState::Apc => match b {
                    0x1b => self.state = ParserState::ApcEsc, // possible ST (ESC \)
                    0x18 | 0x1a => {
                        // CAN / SUB abort the string with no action.
                        self.apc_buffer.clear();
                        self.state = ParserState::Ground;
                    }
                    _ => {
                        if self.apc_buffer.len() < DCS_MAX {
                            self.apc_buffer.push(b);
                        }
                    }
                },
                ParserState::ApcEsc => {
                    // The APC string is over (ST or otherwise); act on it.
                    self.finish_apc(g);
                    self.state = ParserState::Ground;
                }
            }
        }
    }

    /// Finalize a collected DCS string. If it's a Sixel (`<params> q <data>`),
    /// decode it and render it into the grid as half-block cells; other DCS
    /// types are ignored. Clears the buffer either way.
    fn finish_dcs(&mut self, g: &mut Grid) {
        // XTGETTCAP (`DCS + q <hex>;... ST`): answer terminfo capability queries.
        // The `+` intermediate distinguishes it from Sixel (`<params> q <data>`).
        if self.dcs_buffer.starts_with(b"+q") {
            self.answer_xtgettcap();
            self.dcs_buffer.clear();
            return;
        }
        let mut i = 0;
        // Skip the Sixel parameter bytes (digits and `;`) to the final byte,
        // which for Sixel is `q`.
        while i < self.dcs_buffer.len()
            && (self.dcs_buffer[i].is_ascii_digit() || self.dcs_buffer[i] == b';')
        {
            i += 1;
        }
        if self.dcs_buffer.get(i) == Some(&b'q') {
            let img = sixel::decode(&self.dcs_buffer[i + 1..]);
            g.render_sixel(&img);
        }
        self.dcs_buffer.clear();
    }

    /// Answer an XTGETTCAP query (`DCS + q <hex>;... ST`). For each `;`-separated
    /// hex-encoded capability name, reply `DCS 1 + r <name>=<value> ST` for a
    /// string/number cap, `DCS 1 + r <name> ST` for a boolean, or `DCS 0 + r
    /// <name> ST` for one we don't advertise. The requested name is echoed back
    /// verbatim; values are hex-encoded. The advertised set mirrors the shipped
    /// `extra/rusty_term.terminfo` (xterm-256color core + the `Tc` truecolor flag).
    fn answer_xtgettcap(&mut self) {
        // `starts_with(b"+q")` guarantees at least two bytes, so `[2..]` is safe.
        for name in self.dcs_buffer[2..].split(|&b| b == b';') {
            if name.is_empty() {
                continue;
            }
            match hex_decode(name).as_deref().and_then(lookup_cap) {
                Some(Cap::Bool) => {
                    self.responses.extend_from_slice(b"\x1bP1+r");
                    self.responses.extend_from_slice(name);
                    self.responses.extend_from_slice(b"\x1b\\");
                }
                Some(Cap::Str(val)) => {
                    self.responses.extend_from_slice(b"\x1bP1+r");
                    self.responses.extend_from_slice(name);
                    self.responses.push(b'=');
                    push_hex(&mut self.responses, val.as_bytes());
                    self.responses.extend_from_slice(b"\x1b\\");
                }
                None => {
                    self.responses.extend_from_slice(b"\x1bP0+r");
                    self.responses.extend_from_slice(name);
                    self.responses.extend_from_slice(b"\x1b\\");
                }
            }
        }
    }

    /// Finalize a collected APC string. Kitty graphics commands begin with `G`;
    /// hand those to the Kitty decoder (which may render an image and queue a
    /// response). Other APC strings are ignored. Clears the buffer either way.
    fn finish_apc(&mut self, g: &mut Grid) {
        if self.apc_buffer.first() == Some(&b'G') {
            kitty::feed(&mut self.kitty, &self.apc_buffer, g, &mut self.responses);
        }
        self.apc_buffer.clear();
    }

    /// Act on a completed OSC string. The L13 structured channel claims its
    /// private OSC code (`OSC 5379 ; …`); everything else goes to [`osc::dispatch`].
    fn finish_osc(&mut self, g: &mut Grid) {
        #[cfg(feature = "l13")]
        if let Some(payload) = self.osc_buffer.strip_prefix(channel::OSC_PREFIX) {
            channel::handle(payload, g, &mut self.responses);
            return;
        }
        osc::dispatch(
            &self.osc_buffer,
            g,
            &mut self.palette,
            &mut self.responses,
            &mut self.pen,
        );
    }

    /// Handle a single byte while in the ground state: C0 controls, printable
    /// ASCII, and the lead byte of a UTF-8 code point (which transitions into
    /// [`ParserState::Utf8`]).
    fn ground_byte(&mut self, b: u8, g: &mut Grid) {
        match b {
            0x1b => self.state = ParserState::Esc,
            0x08 => {
                g.cursor.0 = g.cursor.0.saturating_sub(1); // backspace
                self.last_char = None;
            }
            b'\n' => {
                g.carriage_return();
                g.newline();
                self.last_char = None;
            }
            b'\r' => {
                g.carriage_return();
                self.last_char = None;
            }
            b'\t' => {
                // Advance to the next tab stop (non-destructive), clamped at the
                // right margin so we never wrap/scroll on a tab.
                g.tab_forward(1);
                self.last_char = None;
            }
            // SO / SI — locking shifts selecting G1 / G0 into GL. ncurses
            // toggles these around line-drawing runs (`smacs` / `rmacs`).
            0x0e => self.gl = 1,
            0x0f => self.gl = 0,
            0x20..=0x7e => {
                let ch = self.charsets[self.gl].map(b);
                g.put_char(ch, self.pen);
                self.last_char = Some(ch);
            }
            // UTF-8 lead bytes: stash the payload bits and how many continuation
            // bytes to expect. (0xC0/0xC1 are always overlong, hence excluded.)
            0xc2..=0xdf => {
                self.utf8_acc = (b as u32) & 0x1f;
                self.utf8_remaining = 1;
                self.state = ParserState::Utf8;
            }
            0xe0..=0xef => {
                self.utf8_acc = (b as u32) & 0x0f;
                self.utf8_remaining = 2;
                self.state = ParserState::Utf8;
            }
            0xf0..=0xf4 => {
                self.utf8_acc = (b as u32) & 0x07;
                self.utf8_remaining = 3;
                self.state = ParserState::Utf8;
            }
            // Stray continuation or otherwise invalid lead byte.
            0x80..=0xbf | 0xc0..=0xc1 | 0xf5..=0xff => {
                g.put_char('\u{FFFD}', self.pen);
            }
            // Other C0 controls are ignored.
            _ => {}
        }
    }

    /// Handle a private CSI sequence (one carrying a `?`/`<`/`=`/`>` marker).
    ///
    /// Alternate-screen DEC modes are acted upon internally. Input-generating
    /// modes (mouse, focus, bracketed paste, and the Kitty keyboard / xterm
    /// modifyOtherKeys protocols) are *relayed* to the host terminal so it
    /// produces the corresponding input — see [`is_host_input_mode`].
    /// Everything else (cursor visibility `25`, autowrap `7`, …) is consumed and
    /// ignored so it never leaks as text.
    fn handle_private_csi(&mut self, cmd: u8, g: &mut Grid) {
        // DA2 (Secondary Device Attributes): `CSI > c`. Reply with a terminal
        // type (0), a firmware "version", and a ROM cartridge field (0) — the
        // values are conventional; programs care that an answer arrives.
        if self.csi_marker == b'>' && cmd == b'c' {
            self.responses.extend_from_slice(b"\x1b[>0;1;0c");
            return;
        }
        // XTVERSION (`CSI > q`): report terminal name + version in xterm's
        // `DCS > | <name>(<version>) ST` form. Feature-detection probes read it.
        if self.csi_marker == b'>' && cmd == b'q' {
            self.responses.extend_from_slice(
                concat!("\x1bP>|rusty_term(", env!("CARGO_PKG_VERSION"), ")\x1b\\").as_bytes(),
            );
            return;
        }
        // DA3 (Tertiary Device Attributes): `CSI = c`. Reply with the xterm-style
        // `DCS ! | <unit id> ST`; the id is a conventional all-zero site code.
        if self.csi_marker == b'=' && cmd == b'c' {
            self.responses.extend_from_slice(b"\x1bP!|00000000\x1b\\");
            return;
        }
        // Kitty keyboard protocol (`CSI > flags u` push, `= flags ; mode u` set,
        // `< n u` pop, `? u` query) and the xterm key-modifier resources /
        // modifyOtherKeys (`CSI > Pp ; Pv m`) are input-generating: relay them
        // verbatim so a capable host performs the enhanced key encoding and
        // answers the query — the same delegation as mouse/paste. Native
        // encoding isn't possible here; we receive the host's already-encoded
        // bytes, not key-press events.
        let kitty_keyboard = cmd == b'u' && matches!(self.csi_marker, b'>' | b'<' | b'=' | b'?');
        let modify_keys = cmd == b'm' && self.csi_marker == b'>';
        if kitty_keyboard || modify_keys {
            g.host_out.push(0x1b);
            g.host_out.push(b'[');
            g.host_out.push(self.csi_marker);
            g.host_out.extend_from_slice(self.param_buffer.as_bytes());
            g.host_out.push(cmd);
            return;
        }
        // Only DEC-private (`?`) set/reset (`h`/`l`) sequences are actionable
        // here; other private forms are consumed without effect.
        if self.csi_marker != b'?' || (cmd != b'h' && cmd != b'l') {
            return;
        }
        let set = cmd == b'h';
        let params = self.parse_params();
        for param in params.iter().flatten().copied() {
            if let Some(mode) = alt_mode(param) {
                // 47 / 1047 / 1049 select the alternate screen buffer; the mode
                // governs cursor save/restore (only 1049). The leave path uses
                // the mode stashed on entry, so the reset value doesn't matter.
                if set {
                    g.enter_alt_screen(mode);
                } else {
                    g.leave_alt_screen();
                }
            } else if is_host_input_mode(param) {
                // Relay verbatim to the host terminal so it starts/stops
                // generating mouse/focus/paste input, which flows through our
                // stdin back to the child. Queued on host_out, drained by the
                // renderer each frame (same channel as OSC 52 clipboard).
                g.host_out.extend_from_slice(b"\x1b[?");
                g.host_out.extend_from_slice(param.to_string().as_bytes());
                g.host_out.push(if set { b'h' } else { b'l' });
                // Also record state the windowed front-end needs: it has no
                // host to relay to and must wrap pastes / encode mouse events
                // itself. (TUI mode relays above and never reads these.)
                match param {
                    2004 => g.bracketed_paste = set,
                    1000 | 1002 | 1003 => {
                        g.mouse_modes.base = if set {
                            param
                        } else if g.mouse_modes.base == param {
                            0
                        } else {
                            g.mouse_modes.base
                        };
                    }
                    1005 | 1006 | 1015 | 1016 => {
                        let bit: u8 = match param {
                            1005 => 1,
                            1006 => 2,
                            1015 => 4,
                            _ => 8, // 1016
                        };
                        if set {
                            g.mouse_modes.extended |= bit;
                        } else {
                            g.mouse_modes.extended &= !bit;
                        }
                    }
                    _ => {}
                }
            } else if param == 25 {
                // DECTCEM — text cursor enable. The renderer reads this to show
                // or hide the host cursor; not relayed (we own the host cursor).
                g.cursor_visible = set;
            } else if param == 7 {
                // DECAWM — autowrap mode.
                g.autowrap = set;
            } else if param == 6 {
                // DECOM — origin mode; toggling it homes the cursor.
                g.set_origin_mode(set);
            }
        }
    }

    /// Parse `param_buffer` into positional parameters. An empty slot (e.g. the
    /// leading field of `CSI ;5H`) becomes `None` so callers can apply the
    /// per-command default in the correct position, per ECMA-48 §5.4.2.
    fn parse_params(&self) -> Vec<Option<usize>> {
        if self.param_buffer.is_empty() {
            return Vec::new();
        }
        self.param_buffer
            .split(';')
            .map(|s| if s.is_empty() { None } else { s.parse().ok() })
            .collect()
    }

    /// Dispatch a completed CSI sequence given its final command byte.
    fn handle_csi(&mut self, cmd: u8, g: &mut Grid) {
        let params = self.parse_params();
        // Positional parameter `i` with `default` applied to an absent/empty slot.
        let p = |i: usize, default: usize| params.get(i).copied().flatten().unwrap_or(default);
        // Most cursor-motion commands take a single count that defaults to (and
        // treats 0 as) 1.
        let count = p(0, 1).max(1);

        match cmd {
            b'm' => {
                // SGR: an empty list resets; otherwise an empty slot means 0.
                let sgr: Vec<usize> = params.iter().map(|o| o.unwrap_or(0)).collect();
                self.apply_sgr(&sgr);
            }
            b'H' | b'f' => {
                // CSI row ; col H — both 1-based; default to 1. Origin-aware.
                g.set_cursor_abs(p(1, 1).saturating_sub(1), p(0, 1).saturating_sub(1));
            }
            b'A' => g.cursor_up(count),   // CUU (margin-aware)
            b'B' => g.cursor_down(count), // CUD (margin-aware)
            b'C' => g.set_cursor(g.cursor.0.saturating_add(count), g.cursor.1), // CUF
            b'D' => g.set_cursor(g.cursor.0.saturating_sub(count), g.cursor.1), // CUB
            b'E' => g.set_cursor(0, g.cursor.1.saturating_add(count)), // CNL
            b'F' => g.set_cursor(0, g.cursor.1.saturating_sub(count)), // CPL
            b'G' => g.set_cursor(p(0, 1).saturating_sub(1), g.cursor.1), // CHA
            b'd' => g.set_cursor_abs(g.cursor.0, p(0, 1).saturating_sub(1)), // VPA (origin-aware)
            b'b' => {
                // REP — repeat the last printed graphic character `count` times.
                // Clamp to the addressable capacity (screen + scrollback): a
                // larger count only overwrites cells it has already filled, while
                // an unclamped value (parsed up to `usize`) would spin under the
                // held grid lock and hang the terminal on hostile input.
                if let Some(ch) = self.last_char {
                    let cap = g.rows.saturating_add(g.scrollback_max).saturating_mul(g.cols);
                    for _ in 0..count.min(cap) {
                        g.put_char(ch, self.pen);
                    }
                }
            }
            b'@' => g.insert_chars(count), // ICH
            b'I' => g.tab_forward(count),  // CHT
            b'Z' => g.tab_backward(count), // CBT
            b'g' => match p(0, 0) {
                0 => g.clear_tab_stop(),      // TBC 0 — clear stop at cursor
                3 => g.clear_all_tab_stops(), // TBC 3 — clear all stops
                _ => {}
            },
            b'P' => g.delete_chars(count), // DCH
            b'X' => g.erase_chars(count),  // ECH
            b'L' => g.insert_lines(count), // IL
            b'M' => g.delete_lines(count), // DL
            b'S' => g.scroll_up_n(count),  // SU
            b'T' => {
                // SD — scroll down. The multi-parameter `CSI Ps;Ps;Ps;Ps;Ps T`
                // form is xterm highlight-mouse-tracking, which we don't model;
                // only the single-parameter form is SD.
                if params.len() <= 1 {
                    g.scroll_down_n(count);
                }
            }
            b's' => g.save_cursor(),    // SCP
            b'u' => g.restore_cursor(), // RCP
            b'r' => {
                // DECSTBM — set top/bottom scrolling margins (1-based).
                let top = p(0, 1).saturating_sub(1);
                let bottom = p(1, g.rows).saturating_sub(1);
                g.set_scroll_region(top, bottom);
            }
            b'J' => {
                // ED — erase in display.
                let (cx, cy) = g.cursor;
                match p(0, 0) {
                    0 => {
                        // Cursor to end of screen.
                        g.clear_row_range(cy, cx, g.cols);
                        for y in (cy + 1)..g.rows {
                            g.clear_row_range(y, 0, g.cols);
                        }
                    }
                    1 => {
                        // Start of screen to cursor (inclusive).
                        for y in 0..cy {
                            g.clear_row_range(y, 0, g.cols);
                        }
                        g.clear_row_range(cy, 0, cx + 1);
                    }
                    2 | 3 => g.clear_all(),
                    _ => {}
                }
            }
            b'K' => {
                // EL — erase in line.
                let (cx, cy) = g.cursor;
                match p(0, 0) {
                    0 => g.clear_row_range(cy, cx, g.cols),
                    1 => g.clear_row_range(cy, 0, cx + 1),
                    2 => g.clear_row_range(cy, 0, g.cols),
                    _ => {}
                }
            }
            b'h' | b'l' => {
                // ANSI (non-private) mode set/reset. Only IRM (mode 4 —
                // insert/replace) is modeled; other ANSI modes are ignored.
                let set = cmd == b'h';
                if params.iter().flatten().any(|&m| m == 4) {
                    g.insert_mode = set;
                }
            }
            b'p' if self.csi_intermediate == b'!' => {
                // DECSTR — soft terminal reset (`CSI ! p`).
                self.palette.reset();
                self.reset_sgr();
                g.set_default_colors(self.palette.fg, self.palette.bg, self.palette.cursor);
                g.soft_reset();
                self.last_char = None;
                self.charsets = [Charset::Ascii; 4];
                self.gl = 0;
            }
            b'q' if self.csi_intermediate == b' ' => {
                // DECSCUSR — set cursor style (`CSI Ps SP q`). Odd params blink,
                // even are steady: 0/1 block, 2 block, 3/4 underline, 5/6 bar.
                let (shape, blink) = match p(0, 0) {
                    0 | 1 => (CursorShape::Block, true),
                    2 => (CursorShape::Block, false),
                    3 => (CursorShape::Underline, true),
                    4 => (CursorShape::Underline, false),
                    5 => (CursorShape::Bar, true),
                    6 => (CursorShape::Bar, false),
                    _ => return, // unknown style: leave the cursor unchanged
                };
                g.set_cursor_style(shape, blink);
                // Relay to the host (TUI), which owns its own cursor; the windowed
                // front-end reads the grid state and renders the shape directly.
                g.host_out.extend_from_slice(b"\x1b[");
                g.host_out.extend_from_slice(self.param_buffer.as_bytes());
                g.host_out.extend_from_slice(b" q");
            }
            b'c' => {
                // DA1 (Primary Device Attributes). Only the default/`0` form is
                // a query; reply that we're a VT100 with Advanced Video Option,
                // a level apps widely accept. A program that sent this would
                // otherwise block waiting for the answer.
                if p(0, 0) == 0 {
                    self.responses.extend_from_slice(b"\x1b[?1;2c");
                }
            }
            b'n' => match p(0, 0) {
                // DSR — Device Status Report.
                5 => self.responses.extend_from_slice(b"\x1b[0n"), // terminal OK
                6 => {
                    // CPR — report the cursor position, 1-based row;col.
                    let (cx, cy) = g.cursor;
                    self.responses
                        .extend_from_slice(format!("\x1b[{};{}R", cy + 1, cx + 1).as_bytes());
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// Reset SGR state to the default pen (default colors, no attributes).
    fn reset_sgr(&mut self) {
        self.pen = Pen {
            fg: self.palette.fg,
            bg: self.palette.bg,
            attrs: 0,
        };
    }

    /// Apply an SGR (`CSI … m`) parameter list. Supports reset, the text
    /// attributes (bold/dim/italic/underline/blink/reverse/hidden/strike and
    /// their resets), the 16-color palette (normal + bright), and the extended
    /// `38/48;5;n` (256-color) and `38/48;2;r;g;b` (truecolor) forms. An empty
    /// list means reset.
    fn apply_sgr(&mut self, params: &[usize]) {
        if params.is_empty() {
            self.reset_sgr();
            return;
        }
        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => self.reset_sgr(),
                // Set text attributes.
                1 => self.pen.attrs |= ATTR_BOLD,
                2 => self.pen.attrs |= ATTR_DIM,
                3 => self.pen.attrs |= ATTR_ITALIC,
                4 => self.pen.attrs |= ATTR_UNDERLINE,
                // 5 (slow) and 6 (rapid) blink both map to our single blink bit.
                5 | 6 => self.pen.attrs |= ATTR_BLINK,
                7 => self.pen.attrs |= ATTR_REVERSE,
                8 => self.pen.attrs |= ATTR_HIDDEN,
                9 => self.pen.attrs |= ATTR_STRIKE,
                // Reset text attributes. 22 clears both bold and dim.
                22 => self.pen.attrs &= !(ATTR_BOLD | ATTR_DIM),
                23 => self.pen.attrs &= !ATTR_ITALIC,
                24 => self.pen.attrs &= !ATTR_UNDERLINE,
                25 => self.pen.attrs &= !ATTR_BLINK,
                27 => self.pen.attrs &= !ATTR_REVERSE,
                28 => self.pen.attrs &= !ATTR_HIDDEN,
                29 => self.pen.attrs &= !ATTR_STRIKE,
                // Colors.
                30..=37 => self.pen.fg = self.palette.index(params[i] - 30),
                38 => {
                    if let Some((color, consumed)) = self.palette.extended(&params[i + 1..]) {
                        self.pen.fg = color;
                        i += consumed;
                    }
                }
                39 => self.pen.fg = self.palette.fg,
                40..=47 => self.pen.bg = self.palette.index(params[i] - 40),
                48 => {
                    if let Some((color, consumed)) = self.palette.extended(&params[i + 1..]) {
                        self.pen.bg = color;
                        i += consumed;
                    }
                }
                49 => self.pen.bg = self.palette.bg,
                90..=97 => self.pen.fg = self.palette.index(8 + (params[i] - 90)),
                100..=107 => self.pen.bg = self.palette.index(8 + (params[i] - 100)),
                _ => {}
            }
            i += 1;
        }
    }
}

/// Whether a DEC private mode controls *host-terminal input generation* —
/// cursor-key encoding, mouse tracking, focus reporting, or bracketed paste —
/// and so must be relayed to the host rather than handled internally or ignored.
///
/// - `1` — DECCKM cursor-keys mode (arrows send `SS3` vs `CSI`); relaying it
///   keeps the host's arrow encoding in step with what the child expects
/// - `1000`/`1002`/`1003` — X11 mouse: click, button-event (drag), any-event
/// - `1004` — focus in/out reporting
/// - `1005`/`1006`/`1015`/`1016` — extended mouse coordinate encodings
/// - `2004` — bracketed paste
fn is_host_input_mode(param: usize) -> bool {
    matches!(
        param,
        1 | 1000 | 1002 | 1003 | 1004 | 1005 | 1006 | 1015 | 1016 | 2004
    )
}

/// A terminfo capability rusty_term advertises via XTGETTCAP: a boolean flag
/// (present, no value) or a string/number whose value is reported.
#[derive(Clone, Copy)]
enum Cap {
    Bool,
    Str(&'static str),
}

/// Look up a capability name we advertise, mirroring `extra/rusty_term.terminfo`
/// (xterm-256color core + `Tc`): 256 colors, the `Tc` truecolor flag, and the
/// terminal name. Anything else is unknown (the caller replies `DCS 0 + r`).
fn lookup_cap(name: &[u8]) -> Option<Cap> {
    match name {
        b"Co" | b"colors" => Some(Cap::Str("256")),
        b"Tc" => Some(Cap::Bool),
        b"TN" | b"name" => Some(Cap::Str("rusty_term")),
        _ => None,
    }
}

/// Decode an even-length ASCII-hex slice into bytes; `None` on odd length or a
/// non-hex digit.
fn hex_decode(hex: &[u8]) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let nibble = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    };
    let mut out = Vec::with_capacity(hex.len() / 2);
    for pair in hex.chunks_exact(2) {
        out.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
    }
    Some(out)
}

/// Append `data` to `out` as lowercase ASCII hex (two digits per byte).
fn push_hex(out: &mut Vec<u8>, data: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for &b in data {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 0xf) as usize]);
    }
}
