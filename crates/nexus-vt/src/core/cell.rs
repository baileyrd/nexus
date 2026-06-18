//! Cell model and character-width classification (L05).
//!
//! The [`Cell`] is the atom of the screen buffer — a base glyph plus any trailing
//! grapheme continuation (combining marks, ZWJ joins, …) and truecolor
//! attributes — and [`char_width`] classifies a code point's display width per
//! Unicode UAX #11.

use unicode_width::UnicodeWidthChar;

/// Default foreground color (white) used on reset and for blank cells.
pub const DEFAULT_FG: u32 = 0xFFFFFF;
/// Default background color (black) used on reset and for blank cells.
pub const DEFAULT_BG: u32 = 0x000000;

/// [`Cell::flags`] bit marking the trailing (second) cell of a double-width
/// character. The renderer skips these so the wide glyph occupies two columns.
///
/// This is a *layout* flag (bit 0); the `ATTR_*` rendition bits live above it,
/// so a cell's `flags` is `WIDE_TRAILER | <pen attributes>`.
pub const WIDE_TRAILER: u16 = 1 << 0;

// SGR text-attribute bits, stored in [`Cell::flags`] and carried by [`Pen`].
// They occupy bits 1.. so they never collide with [`WIDE_TRAILER`].
/// SGR 1 — bold / increased intensity.
pub const ATTR_BOLD: u16 = 1 << 1;
/// SGR 2 — dim / decreased intensity.
pub const ATTR_DIM: u16 = 1 << 2;
/// SGR 3 — italic.
pub const ATTR_ITALIC: u16 = 1 << 3;
/// SGR 4 — underline.
pub const ATTR_UNDERLINE: u16 = 1 << 4;
/// SGR 5 — blink.
pub const ATTR_BLINK: u16 = 1 << 5;
/// SGR 7 — reverse video (swap fg/bg).
pub const ATTR_REVERSE: u16 = 1 << 6;
/// SGR 8 — concealed / hidden.
pub const ATTR_HIDDEN: u16 = 1 << 7;
/// SGR 9 — crossed-out / strikethrough.
pub const ATTR_STRIKE: u16 = 1 << 8;

/// Mask of every rendition attribute bit (everything except [`WIDE_TRAILER`]).
pub const ATTR_MASK: u16 = ATTR_BOLD
    | ATTR_DIM
    | ATTR_ITALIC
    | ATTR_UNDERLINE
    | ATTR_BLINK
    | ATTR_REVERSE
    | ATTR_HIDDEN
    | ATTR_STRIKE;

/// The current SGR graphic rendition — foreground/background color and the set
/// of active text attributes — that the parser stamps onto each glyph it
/// writes. A single source of truth for "how the next character looks".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pen {
    /// Foreground color as `0xRRGGBB`.
    pub fg: u32,
    /// Background color as `0xRRGGBB`.
    pub bg: u32,
    /// Active text-attribute bits (`ATTR_*`).
    pub attrs: u16,
}

impl Default for Pen {
    /// The reset pen: default colors and no attributes.
    fn default() -> Self {
        Pen {
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
            attrs: 0,
        }
    }
}

/// Display width of `ch` in terminal cells: `0` for zero-width (combining
/// marks, joiners, variation selectors, …), `2` for wide East Asian / emoji
/// code points, and `1` otherwise.
///
/// Backed by the [`unicode-width`] crate, which implements the full Unicode
/// East Asian Width (UAX #11) and emoji-presentation property tables. Control
/// characters (for which the crate reports no width) collapse to `0`; the
/// parser handles C0/C1 controls before they ever reach this function.
///
/// [`unicode-width`]: https://docs.rs/unicode-width
pub fn char_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}

/// A single character cell: its base glyph, an optional grapheme-cluster id for
/// glyphs that span more than one code point (combining marks, ZWJ emoji
/// sequences, …), and truecolor attributes. Kept `Copy` so the grid can shift
/// cells with `copy_within`.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Cell {
    /// The base (first) scalar of the grapheme. Used for width classification
    /// and as the whole glyph when [`cluster`](Self::cluster) is `0`.
    pub ch: char,
    /// Grapheme-continuation id: `0` for a lone `ch`, else an index+1 into
    /// `Grid::clusters`, whose string is the continuation appended after `ch`
    /// (combining marks, ZWJ-joined scalars, variation selectors, …).
    pub cluster: u16,
    /// Foreground color as `0xRRGGBB`.
    pub fg: u32,
    /// Background color as `0xRRGGBB`.
    pub bg: u32,
    /// Attribute bitset (bold, italic, …). Reserved for future use.
    pub flags: u16,
    /// Hyperlink id (OSC 8): `0` for none, else an index+1 into `Grid::links`.
    pub link: u16,
}

impl Cell {
    /// Construct a blank cell (space glyph, default colors).
    pub(crate) fn blank() -> Self {
        Cell {
            ch: ' ',
            cluster: 0,
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
            flags: 0,
            link: 0,
        }
    }
}
