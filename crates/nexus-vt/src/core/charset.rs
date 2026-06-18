//! Character-set designation and translation (L05/L06).
//!
//! VT100 terminals designate a character set into one of the G0–G3 slots
//! (`ESC ( <F>` … `ESC + <F>`) and invoke one into GL with the SI/SO shift
//! controls. The only non-ASCII set still in common use is the DEC Special
//! Graphics and Line Drawing set, which ncurses-era TUIs select to draw box
//! borders: with it active in GL, the ASCII letters `j k l m n q t u v w x` map
//! to `┘ ┐ ┌ └ ┼ ─ ├ ┤ ┴ ┬ │`. Without this translation those borders render as
//! stray letters — the gap this module closes.

/// A character set that can be designated into a G0–G3 slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Charset {
    /// US-ASCII (the `B` designator) — the identity mapping. Also the resting
    /// state of every slot and what UK (`A`) plus the other national sets we
    /// don't model collapse to (their handful of substitutions aren't worth it).
    #[default]
    Ascii,
    /// DEC Special Graphics and Line Drawing (the `0` designator).
    DecSpecialGraphics,
}

impl Charset {
    /// Resolve a designation byte (the final byte of `ESC ( <F>`, etc.) to the
    /// charset it selects.
    pub(crate) fn from_designator(b: u8) -> Charset {
        match b {
            b'0' => Charset::DecSpecialGraphics,
            // `B` (ASCII), `A` (UK), and every other designator we don't model
            // fold to the identity mapping.
            _ => Charset::Ascii,
        }
    }

    /// Translate a printable GL byte (`0x20..=0x7e`) through this charset.
    pub(crate) fn map(self, b: u8) -> char {
        match self {
            Charset::Ascii => b as char,
            Charset::DecSpecialGraphics => dec_special_graphics(b),
        }
    }
}

/// Map a printable byte through the DEC Special Graphics set. Only `0x60..=0x7e`
/// differ from ASCII; every other byte (digits, punctuation, space) passes
/// through unchanged, matching xterm/VT100.
fn dec_special_graphics(b: u8) -> char {
    match b {
        0x60 => '◆',
        0x61 => '▒',
        0x62 => '\u{2409}', // HT symbol
        0x63 => '\u{240c}', // FF symbol
        0x64 => '\u{240d}', // CR symbol
        0x65 => '\u{240a}', // LF symbol
        0x66 => '°',
        0x67 => '±',
        0x68 => '\u{2424}', // NL symbol
        0x69 => '\u{240b}', // VT symbol
        0x6a => '┘',
        0x6b => '┐',
        0x6c => '┌',
        0x6d => '└',
        0x6e => '┼',
        0x6f => '⎺',
        0x70 => '⎻',
        0x71 => '─',
        0x72 => '⎼',
        0x73 => '⎽',
        0x74 => '├',
        0x75 => '┤',
        0x76 => '┴',
        0x77 => '┬',
        0x78 => '│',
        0x79 => '≤',
        0x7a => '≥',
        0x7b => 'π',
        0x7c => '≠',
        0x7d => '£',
        0x7e => '·',
        _ => b as char,
    }
}
