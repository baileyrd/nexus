//! Color palette and SGR color resolution (L06/L08).
//!
//! Holds the live [`Palette`] — the 256 indexed colors plus the dynamic default
//! foreground/background/cursor — and the helpers that resolve SGR color
//! selectors and parse/format the X11 color specs carried by OSC 4/10/11/12.

use super::cell::{DEFAULT_BG, DEFAULT_FG};

/// Startup color theme: the default foreground/background/cursor plus the
/// 16-color ANSI palette, as configured by the user. The "built-in values"
/// every reset path (RIS, DECSTR, OSC 104/110/111/112) restores — so a
/// configured theme survives a `reset`, exactly as the hardware defaults
/// would. Indexed colors 16-255 always come from the fixed xterm cube/ramp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Default foreground (SGR 39).
    pub fg: u32,
    /// Default background (SGR 49) — also the erase-fill color.
    pub bg: u32,
    /// Cursor color (OSC 12 round-trips).
    pub cursor: u32,
    /// ANSI palette indices 0-15 (normal + bright).
    pub palette16: [u32; 16],
}

impl Default for Theme {
    /// The classic built-ins: white on black, xterm 16-color palette.
    fn default() -> Self {
        Theme {
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
            cursor: DEFAULT_FG,
            palette16: PALETTE_16,
        }
    }
}

/// Standard 16-color ANSI palette (indices 0-7 normal, 8-15 bright), in
/// `0xRRGGBB` form. Roughly matches the classic xterm palette. The seed for the
/// low 16 entries of a fresh [`Palette`].
pub(crate) const PALETTE_16: [u32; 16] = [
    0x000000, 0x800000, 0x008000, 0x808000, 0x000080, 0x800080, 0x008080, 0xC0C0C0, 0x808080,
    0xFF0000, 0x00FF00, 0xFFFF00, 0x0000FF, 0xFF00FF, 0x00FFFF, 0xFFFFFF,
];

/// A live, mutable color table: the 256 indexed colors plus the dynamic default
/// foreground, background, and cursor colors. Initialized to the built-in xterm
/// values and mutated by OSC 4 (palette) and OSC 10/11/12 (defaults). SGR color
/// selectors resolve through it, so a palette change recolors text written
/// afterwards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Palette {
    /// The 256-color table; `colors[n]` is the `0xRRGGBB` of index `n`.
    colors: [u32; 256],
    /// Default foreground (SGR 39, OSC 10).
    pub(crate) fg: u32,
    /// Default background (SGR 49, OSC 11) — also the erase-fill color.
    pub(crate) bg: u32,
    /// Cursor color (OSC 12). Stored for query round-trips; the renderer owns
    /// the host cursor, so it is not painted yet.
    pub(crate) cursor: u32,
    /// The configured startup theme: what every reset restores.
    seed: Theme,
}

impl Default for Palette {
    fn default() -> Self {
        Self::new()
    }
}

impl Palette {
    /// A palette seeded with the built-in xterm 256-color table and default
    /// colors.
    pub(crate) fn new() -> Self {
        Self::with_theme(Theme::default())
    }

    /// A palette seeded from `theme`: its fg/bg/cursor and 16-color palette,
    /// with indices 16-255 from the fixed xterm table. The theme is retained
    /// as the target of every subsequent reset.
    pub(crate) fn with_theme(theme: Theme) -> Self {
        let mut colors = [0u32; 256];
        for (i, slot) in colors.iter_mut().enumerate() {
            *slot = theme_index(&theme, i);
        }
        Palette {
            colors,
            fg: theme.fg,
            bg: theme.bg,
            cursor: theme.cursor,
            seed: theme,
        }
    }

    /// Reset the indexed table *and* the default colors to the built-in values
    /// (RIS / DECSTR).
    pub(crate) fn reset(&mut self) {
        *self = Palette::with_theme(self.seed);
    }

    /// Reset every indexed color (or, if `indices` is non-empty, only those) to
    /// its built-in value, leaving the default fg/bg/cursor untouched (OSC 104).
    pub(crate) fn reset_colors(&mut self, indices: &[usize]) {
        if indices.is_empty() {
            for (i, slot) in self.colors.iter_mut().enumerate() {
                *slot = theme_index(&self.seed, i);
            }
        } else {
            for &n in indices {
                if n < self.colors.len() {
                    self.colors[n] = theme_index(&self.seed, n);
                }
            }
        }
    }

    /// Reset only the default foreground to its built-in value (OSC 110).
    pub(crate) fn reset_fg(&mut self) {
        self.fg = self.seed.fg;
    }

    /// Reset only the default background to its built-in value (OSC 111).
    pub(crate) fn reset_bg(&mut self) {
        self.bg = self.seed.bg;
    }

    /// Reset only the cursor color to its built-in value (OSC 112).
    pub(crate) fn reset_cursor(&mut self) {
        self.cursor = self.seed.cursor;
    }

    /// The configured startup theme this palette reseeds from on every reset.
    pub(crate) fn seed(&self) -> Theme {
        self.seed
    }

    /// Live theme switch: adopt `new` as the seed and migrate the dynamic
    /// state. Entries still at their `old` built-in value follow the new
    /// theme; entries the child changed (OSC 4/10/11/12) are preserved.
    pub(crate) fn retheme(&mut self, old: &Theme, new: &Theme) {
        for i in 0..16 {
            if self.colors[i] == old.palette16[i] {
                self.colors[i] = new.palette16[i];
            }
        }
        if self.fg == old.fg {
            self.fg = new.fg;
        }
        if self.bg == old.bg {
            self.bg = new.bg;
        }
        if self.cursor == old.cursor {
            self.cursor = new.cursor;
        }
        self.seed = *new;
    }

    /// The `0xRRGGBB` of palette index `n`. Indices ≥ 256 (never produced by
    /// our SGR paths) fall back to the default foreground.
    pub(crate) fn index(&self, n: usize) -> u32 {
        self.colors.get(n).copied().unwrap_or(DEFAULT_FG)
    }

    /// Set palette index `n` (OSC 4). Out-of-range indices are ignored.
    pub(crate) fn set_index(&mut self, n: usize, rgb: u32) {
        if let Some(slot) = self.colors.get_mut(n) {
            *slot = rgb;
        }
    }

    /// Resolve the tail of an extended SGR color selector (the part after
    /// `38`/`48`). Returns the resolved `0xRRGGBB` color and how many additional
    /// params were consumed, or `None` if the form is unrecognized.
    ///
    /// * `5; n`        → 256-color palette index `n` (2 params consumed)
    /// * `2; r; g; b`  → truecolor (4 params consumed)
    pub(crate) fn extended(&self, rest: &[usize]) -> Option<(u32, usize)> {
        match rest.first().copied() {
            Some(5) => {
                let n = *rest.get(1)?;
                Some((self.index(n), 2))
            }
            Some(2) => {
                let r = (*rest.get(1)? & 0xFF) as u32;
                let g = (*rest.get(2)? & 0xFF) as u32;
                let b = (*rest.get(3)? & 0xFF) as u32;
                Some(((r << 16) | (g << 8) | b, 4))
            }
            _ => None,
        }
    }
}

/// The "built-in" value of palette index `n` under `theme`: 0-15 come from the
/// theme's ANSI palette, 16-255 from the fixed xterm cube/ramp.
fn theme_index(theme: &Theme, n: usize) -> u32 {
    match n {
        0..=15 => theme.palette16[n],
        _ => xterm_256_to_rgb(n),
    }
}

/// Remap a resolved color from `old`'s theme to `new`'s: a color that exactly
/// matches the old default fg/bg/cursor or one of the old 16 ANSI entries
/// becomes the new theme's counterpart; anything else (truecolor, 256-cube)
/// passes through. Cells store resolved RGB rather than palette indices, so
/// this exact-match mapping is how a live config reload recolors content that
/// is already on screen. First match wins when old entries collide.
pub(crate) fn remap(c: u32, old: &Theme, new: &Theme) -> u32 {
    if c == old.fg {
        return new.fg;
    }
    if c == old.bg {
        return new.bg;
    }
    if c == old.cursor {
        return new.cursor;
    }
    for i in 0..16 {
        if c == old.palette16[i] {
            return new.palette16[i];
        }
    }
    c
}

/// Convert an xterm 256-color index to `0xRRGGBB`.
///
/// 0-15 map to the base palette, 16-231 form a 6×6×6 color cube, and 232-255
/// are a 24-step grayscale ramp.
fn xterm_256_to_rgb(n: usize) -> u32 {
    match n {
        0..=15 => PALETTE_16[n],
        16..=231 => {
            let n = n - 16;
            let steps = [0u32, 95, 135, 175, 215, 255];
            let r = steps[(n / 36) % 6];
            let g = steps[(n / 6) % 6];
            let b = steps[n % 6];
            (r << 16) | (g << 8) | b
        }
        232..=255 => {
            let level = 8 + (n - 232) as u32 * 10;
            (level << 16) | (level << 8) | level
        }
        _ => DEFAULT_FG,
    }
}

/// Parse an X11 color spec, as carried by OSC 4/10/11/12, into `0xRRGGBB`.
///
/// Accepts the two forms apps actually emit:
/// * `rgb:R/G/B` — 1–4 hex digits per channel, scaled to 8 bits (the canonical
///   xterm form, usually `rgb:RR/GG/BB` or `rgb:RRRR/GGGG/BBBB`).
/// * `#RGB`, `#RRGGBB`, `#RRRGGGBBB`, `#RRRRGGGGBBBB` — equal-width hex triples.
///
/// Returns `None` for anything else (e.g. named colors), which callers ignore.
pub(crate) fn parse_color_spec(spec: &str) -> Option<u32> {
    if let Some(rest) = spec.strip_prefix("rgb:") {
        let mut it = rest.split('/');
        let r = scale_hex_channel(it.next()?)?;
        let g = scale_hex_channel(it.next()?)?;
        let b = scale_hex_channel(it.next()?)?;
        if it.next().is_some() {
            return None;
        }
        Some((r << 16) | (g << 8) | b)
    } else if let Some(hex) = spec.strip_prefix('#') {
        if hex.is_empty() || hex.len() % 3 != 0 {
            return None;
        }
        let w = hex.len() / 3;
        let r = scale_hex_channel(hex.get(0..w)?)?;
        let g = scale_hex_channel(hex.get(w..2 * w)?)?;
        let b = scale_hex_channel(hex.get(2 * w..3 * w)?)?;
        Some((r << 16) | (g << 8) | b)
    } else {
        None
    }
}

/// Parse one hex channel of 1–4 digits and scale it to 8 bits, so `f`→0xFF,
/// `ff`→0xFF, `ffff`→0xFF (each width's max maps to full intensity).
fn scale_hex_channel(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 4 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(s, 16).ok()?;
    let max = (1u32 << (4 * s.len() as u32)) - 1; // 0xF, 0xFF, 0xFFF, 0xFFFF
    Some((value * 255 + max / 2) / max)
}

/// Format a `0xRRGGBB` as the `rgb:RRRR/GGGG/BBBB` 16-bit spec xterm uses in OSC
/// color query replies (each 8-bit channel is doubled).
pub(crate) fn format_color_spec(rgb: u32) -> String {
    let (r, g, b) = ((rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF);
    format!("rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}")
}
