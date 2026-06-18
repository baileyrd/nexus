//! Minimal Sixel decoder (L07 graphics) — no external crates.
//!
//! Decodes the payload of a Sixel DCS (`DCS <params> q <data> ST`) into an RGB
//! pixel buffer. The grid renders that buffer as truecolor half-block glyphs
//! (`▀`/`▄`) — one cell per pixel column, two pixel rows per cell — so images
//! display in any host terminal even though rusty_term has no framebuffer of its
//! own. Resolution is therefore cell-grained, not pixel-perfect; that's inherent
//! to a passthrough terminal.
//!
//! Format recap: the data is a stream of horizontal *bands* six pixels tall. A
//! data byte `?`..`~` encodes six vertical pixels (`bit 0` = top) in the current
//! color at the current column, then advances the column. `#` selects or defines
//! a color register, `!Pn` repeats the next data byte `Pn` times, `$` returns to
//! the left of the current band (to overlay another color), `-` starts the next
//! band, and `"` carries raster attributes (ignored — we size from the pixels).

/// A decoded Sixel image: row-major pixels, `None` for never-painted
/// (transparent) cells.
pub(crate) struct SixelImage {
    pub width: usize,
    pub height: usize,
    /// `0xRRGGBB` per pixel, `None` where transparent. `len() == width * height`.
    pub pixels: Vec<Option<u32>>,
}

impl SixelImage {
    fn empty() -> Self {
        SixelImage {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        }
    }
}

/// Per-axis dimension cap and total-pixel cap, bounding memory against an
/// oversized or hostile image (the buffer is one-shot and downsampled away).
const MAX_DIM: usize = 2000;
const MAX_PIXELS: usize = 4 * 1024 * 1024;

/// Decode a Sixel data payload (the bytes after the `q`) into an image.
pub(crate) fn decode(data: &[u8]) -> SixelImage {
    let mut dec = Decoder::new();
    dec.run(data);
    dec.finish()
}

struct Decoder {
    palette: [u32; 256],
    color: usize,
    x: usize,
    band: usize,
    /// Ragged row-major paint buffer; `rows[y][x]` is the pixel at `(x, y)`.
    rows: Vec<Vec<Option<u32>>>,
    /// Exclusive max column painted, i.e. the final image width.
    max_x: usize,
}

impl Decoder {
    fn new() -> Self {
        Decoder {
            palette: default_palette(),
            color: 0,
            x: 0,
            band: 0,
            rows: Vec::new(),
            max_x: 0,
        }
    }

    fn run(&mut self, data: &[u8]) {
        let mut i = 0;
        let mut repeat = 1usize;
        while i < data.len() {
            match data[i] {
                // Sixel data byte: six vertical pixels at the current column.
                0x3f..=0x7e => {
                    let bits = (data[i] - 0x3f) as u32;
                    let color = self.palette[self.color];
                    let y0 = self.band * 6;
                    for _ in 0..repeat.max(1) {
                        if bits != 0 {
                            for bit in 0..6 {
                                if bits & (1 << bit) != 0 {
                                    self.set_pixel(self.x, y0 + bit, color);
                                }
                            }
                        }
                        self.x += 1;
                    }
                    repeat = 1;
                    i += 1;
                }
                // `!Pn` — repeat the next data byte Pn times. Clamped to MAX_DIM:
                // the column ceiling bounds useful output, and an unclamped count
                // (parse_num saturates to usize::MAX) would otherwise spin the
                // inner loop indefinitely on hostile input.
                b'!' => {
                    i += 1;
                    repeat = parse_num(data, &mut i).clamp(1, MAX_DIM);
                }
                // `#Pc` selects a register; `#Pc;Pu;Px;Py;Pz` defines one.
                b'#' => {
                    i += 1;
                    let pc = parse_num(data, &mut i).min(255);
                    if data.get(i) == Some(&b';') {
                        let mut p = [0usize; 4];
                        for slot in p.iter_mut() {
                            if data.get(i) == Some(&b';') {
                                i += 1;
                                *slot = parse_num(data, &mut i);
                            } else {
                                break;
                            }
                        }
                        self.palette[pc] = convert_color(p[0], p[1], p[2], p[3]);
                    }
                    self.color = pc;
                }
                // `$` — graphics carriage return (back to the band's left edge).
                b'$' => {
                    self.x = 0;
                    i += 1;
                }
                // `-` — graphics new line (advance to the next band).
                b'-' => {
                    self.band += 1;
                    self.x = 0;
                    i += 1;
                }
                // `"Pan;Pad;Ph;Pv` raster attributes — skip; we size from pixels.
                b'"' => {
                    i += 1;
                    for _ in 0..4 {
                        parse_num(data, &mut i);
                        if data.get(i) == Some(&b';') {
                            i += 1;
                        }
                    }
                }
                // Whitespace (newlines in wrapped sixels) and anything else: skip.
                _ => i += 1,
            }
        }
    }

    fn set_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x >= MAX_DIM || y >= MAX_DIM {
            return;
        }
        if y >= self.rows.len() {
            self.rows.resize(y + 1, Vec::new());
        }
        let row = &mut self.rows[y];
        if x >= row.len() {
            row.resize(x + 1, None);
        }
        row[x] = Some(color);
        self.max_x = self.max_x.max(x + 1);
    }

    fn finish(self) -> SixelImage {
        let width = self.max_x;
        if width == 0 || self.rows.is_empty() {
            return SixelImage::empty();
        }
        let mut height = self.rows.len();
        if width * height > MAX_PIXELS {
            height = MAX_PIXELS / width;
        }
        let mut pixels = vec![None; width * height];
        for (y, row) in self.rows.iter().enumerate().take(height) {
            for (x, &p) in row.iter().enumerate().take(width) {
                pixels[y * width + x] = p;
            }
        }
        SixelImage {
            width,
            height,
            pixels,
        }
    }
}

/// Read a run of ASCII digits into a number, advancing `i`. Saturates rather
/// than overflowing on absurd input.
fn parse_num(data: &[u8], i: &mut usize) -> usize {
    let mut n = 0usize;
    while let Some(&b) = data.get(*i) {
        if !b.is_ascii_digit() {
            break;
        }
        n = n.saturating_mul(10).saturating_add((b - b'0') as usize);
        *i += 1;
    }
    n
}

/// Convert a Sixel color definition to `0xRRGGBB`. `Pu == 1` is HLS; `Pu == 2`
/// (and anything else, defensively) is RGB with components in 0–100 percent.
fn convert_color(pu: usize, px: usize, py: usize, pz: usize) -> u32 {
    if pu == 1 {
        hls_to_rgb(px, py, pz)
    } else {
        let scale = |v: usize| ((v.min(100) * 255 + 50) / 100) as u32;
        (scale(px) << 16) | (scale(py) << 8) | scale(pz)
    }
}

/// DEC HLS (hue 0–360 with 0° = blue, lightness/saturation 0–100) to `0xRRGGBB`.
/// The +120° rotation maps DEC's blue-origin hue onto the standard red-origin
/// HSL wheel.
fn hls_to_rgb(h: usize, l: usize, s: usize) -> u32 {
    let h = ((h % 360) as f64 + 120.0) % 360.0 / 360.0;
    let l = (l.min(100) as f64) / 100.0;
    let s = (s.min(100) as f64) / 100.0;
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let r = hue_to_channel(p, q, h + 1.0 / 3.0);
    let g = hue_to_channel(p, q, h);
    let b = hue_to_channel(p, q, h - 1.0 / 3.0);
    let to8 = |c: f64| (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    (to8(r) << 16) | (to8(g) << 8) | to8(b)
}

fn hue_to_channel(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

/// The VT340 default 16-color palette (registers 0–15), in `0xRRGGBB`; the rest
/// start black. Images that rely on defaults are rare — most redefine colors.
fn default_palette() -> [u32; 256] {
    // Source values are DEC percentages; converted here to 8-bit.
    const PCT: [(usize, usize, usize); 16] = [
        (0, 0, 0),
        (20, 20, 80),
        (80, 13, 13),
        (20, 80, 20),
        (80, 20, 80),
        (20, 80, 80),
        (80, 80, 20),
        (53, 53, 53),
        (26, 26, 26),
        (33, 33, 60),
        (60, 26, 26),
        (33, 60, 33),
        (60, 33, 60),
        (33, 60, 60),
        (60, 60, 33),
        (80, 80, 80),
    ];
    let mut p = [0u32; 256];
    for (i, &(r, g, b)) in PCT.iter().enumerate() {
        p[i] = convert_color(2, r, g, b);
    }
    p
}
