//! Minimal baseline JPEG decoder (no crates) for inline images (iTerm2 OSC
//! 1337, and any other path handed a JFIF/EXIF stream). Handles the common case:
//! Huffman-coded baseline sequential DCT (SOF0), 8-bit precision, 1 component
//! (grayscale) or 3 components (YCbCr) with arbitrary 4:4:4 / 4:2:2 / 4:2:0
//! chroma subsampling, optional restart intervals.
//!
//! Progressive (SOF2), arithmetic coding, 12-bit, and CMYK/YCCK (4-component)
//! streams decode to `None` rather than guessing — the caller falls back to not
//! displaying, mirroring [`super::png`].

/// A decoded image as tightly-packed RGBA8 (`rgba.len() == width * height * 4`);
/// JPEG is opaque so every alpha byte is 255. Shaped like [`super::png::Image`].
pub(crate) struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// Pixel-count cap (4M px) so a malformed SOF can't request a vast allocation.
const MAX_PIXELS: usize = 4 * 1024 * 1024;

/// JPEG zig-zag order: maps a coefficient's position in the entropy stream to its
/// row-major index in the natural 8x8 block.
#[rustfmt::skip]
const ZIGZAG: [usize; 64] = [
     0,  1,  8, 16,  9,  2,  3, 10,
    17, 24, 32, 25, 18, 11,  4,  5,
    12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13,  6,  7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36,
    29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46,
    53, 60, 61, 54, 47, 55, 62, 63,
];

/// A canonical Huffman table decoded per JPEG Annex C/F: `bits[l]` counts codes
/// of length `l` (1..=16); `values` lists the symbols in code order. `mincode`/
/// `maxcode`/`valptr` accelerate per-symbol decode (Annex F.2.2.3).
#[derive(Default, Clone)]
struct Huff {
    values: Vec<u8>,
    mincode: [i32; 17],
    maxcode: [i32; 17], // -1 when no code of that length
    valptr: [i32; 17],
}

impl Huff {
    fn build(counts: &[u8; 16], values: Vec<u8>) -> Self {
        // Assign canonical codes in increasing length, then derive the
        // min/max/valptr lookup per length.
        let mut h = Huff {
            values,
            mincode: [0; 17],
            maxcode: [-1; 17],
            valptr: [0; 17],
        };
        let mut code = 0i32;
        let mut k = 0usize;
        for l in 1..=16usize {
            let n = counts[l - 1] as usize;
            if n > 0 {
                h.valptr[l] = k as i32;
                h.mincode[l] = code;
                code += n as i32;
                h.maxcode[l] = code - 1;
                k += n;
            }
            code <<= 1;
        }
        h
    }

    fn decode(&self, br: &mut BitReader) -> Option<u8> {
        let mut code = 0i32;
        for l in 1..=16usize {
            code = (code << 1) | br.bit() as i32;
            if self.maxcode[l] >= 0 && code <= self.maxcode[l] {
                let idx = (self.valptr[l] + (code - self.mincode[l])) as usize;
                return self.values.get(idx).copied();
            }
        }
        None
    }
}

/// MSB-first bit reader over the entropy-coded segment, unstuffing `FF 00` and
/// stopping at any real marker (which it leaves in place for restart resync).
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u8,
    cnt: u8,
    eos: bool,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        BitReader { data, pos, buf: 0, cnt: 0, eos: false }
    }

    /// Next bit (0 past end of segment / at a marker — callers bound reads by the
    /// Huffman tables and coefficient counts, so padding bits are never used).
    fn bit(&mut self) -> u8 {
        if self.cnt == 0 {
            if self.eos || self.pos >= self.data.len() {
                self.eos = true;
                return 0;
            }
            let b = self.data[self.pos];
            if b == 0xFF {
                let next = self.data.get(self.pos + 1).copied().unwrap_or(0xD9);
                if next == 0x00 {
                    self.pos += 2; // unstuff: a literal 0xFF data byte
                } else {
                    self.eos = true; // a marker; leave it for resync
                    return 0;
                }
            } else {
                self.pos += 1;
            }
            self.buf = b;
            self.cnt = 8;
        }
        self.cnt -= 1;
        (self.buf >> self.cnt) & 1
    }

    /// Read `n` bits as an unsigned value.
    fn receive(&mut self, n: u8) -> i32 {
        let mut v = 0i32;
        for _ in 0..n {
            v = (v << 1) | self.bit() as i32;
        }
        v
    }

    /// Byte-align and consume the next restart marker (`FF D0..D7`), readying the
    /// reader for the following interval.
    fn restart(&mut self) {
        self.cnt = 0;
        self.eos = false;
        while self.pos + 1 < self.data.len() {
            if self.data[self.pos] == 0xFF {
                let n = self.data[self.pos + 1];
                if (0xD0..=0xD7).contains(&n) {
                    self.pos += 2;
                    return;
                }
                if n == 0xFF {
                    self.pos += 1;
                    continue;
                }
            }
            self.pos += 1;
        }
    }
}

/// `extend` (JPEG F.2.2.1): sign-extend an `n`-bit magnitude `v` decoded from the
/// stream into the signed coefficient it represents.
fn extend(v: i32, n: u8) -> i32 {
    if n == 0 || v >= (1 << (n - 1)) {
        v
    } else {
        v - (1 << n) + 1
    }
}

struct Component {
    id: u8,
    h: usize, // horizontal sampling factor
    v: usize, // vertical sampling factor
    tq: usize, // quant table index
    td: usize, // DC Huffman table index (set at SOS)
    ta: usize, // AC Huffman table index (set at SOS)
    pred: i32, // running DC predictor
    /// Decoded samples at component resolution, padded to whole MCUs.
    plane: Vec<u8>,
    cw: usize, // plane width in samples
}

/// Decode a baseline JPEG into RGBA8, or `None` if malformed or an unsupported
/// variant.
pub(crate) fn decode(data: &[u8]) -> Option<Image> {
    if data.len() < 2 || data[0] != 0xFF || data[1] != 0xD8 {
        return None; // not SOI
    }
    let mut qt: [[u16; 64]; 4] = [[0; 64]; 4];
    let mut dc: [Huff; 4] = Default::default();
    let mut ac: [Huff; 4] = Default::default();
    let mut comps: Vec<Component> = Vec::new();
    let (mut width, mut height) = (0usize, 0usize);
    let mut restart_interval = 0usize;

    let mut i = 2;
    loop {
        // Markers are `FF xx`; fill bytes (extra FFs) are skipped.
        if i + 1 >= data.len() {
            return None;
        }
        if data[i] != 0xFF {
            return None;
        }
        let mut marker = data[i + 1];
        i += 2;
        while marker == 0xFF && i < data.len() {
            marker = data[i];
            i += 1;
        }
        match marker {
            0xD9 => return None, // EOI before any scan
            // Standalone markers carry no payload.
            0x01 | 0xD0..=0xD7 => continue,
            _ => {}
        }
        if i + 2 > data.len() {
            return None;
        }
        let len = ((data[i] as usize) << 8) | data[i + 1] as usize;
        if len < 2 || i + len > data.len() {
            return None;
        }
        let seg = &data[i + 2..i + len];
        match marker {
            0xC0 | 0xC1 => parse_sof(seg, &mut comps, &mut width, &mut height)?, // baseline / ext. sequential
            0xC2 | 0xC3 | 0xC5..=0xCF => return None, // progressive / arithmetic / unsupported
            0xC4 => parse_dht(seg, &mut dc, &mut ac)?,
            0xDB => parse_dqt(seg, &mut qt)?,
            0xDD if seg.len() >= 2 => {
                restart_interval = ((seg[0] as usize) << 8) | seg[1] as usize;
            }
            0xDA => {
                // Start of scan: bind Huffman tables to components, then decode
                // the entropy data that immediately follows this header.
                parse_sos(seg, &mut comps)?;
                let entropy_start = i + len;
                return decode_scan(
                    data,
                    entropy_start,
                    &mut comps,
                    &qt,
                    &dc,
                    &ac,
                    width,
                    height,
                    restart_interval,
                );
            }
            _ => {} // APPn, COM, DNL, etc. — skip
        }
        i += len;
    }
}

fn parse_sof(
    seg: &[u8],
    comps: &mut Vec<Component>,
    width: &mut usize,
    height: &mut usize,
) -> Option<()> {
    if seg.len() < 6 || seg[0] != 8 {
        return None; // only 8-bit precision
    }
    *height = ((seg[1] as usize) << 8) | seg[2] as usize;
    *width = ((seg[3] as usize) << 8) | seg[4] as usize;
    let nc = seg[5] as usize;
    if *width == 0 || *height == 0 || !(nc == 1 || nc == 3) {
        return None;
    }
    if width.checked_mul(*height)? > MAX_PIXELS {
        return None;
    }
    if seg.len() < 6 + nc * 3 {
        return None;
    }
    for c in 0..nc {
        let o = 6 + c * 3;
        let (h, v) = ((seg[o + 1] >> 4) as usize, (seg[o + 1] & 0x0F) as usize);
        if h == 0 || v == 0 || h > 4 || v > 4 {
            return None;
        }
        comps.push(Component {
            id: seg[o],
            h,
            v,
            tq: (seg[o + 2] & 3) as usize,
            td: 0,
            ta: 0,
            pred: 0,
            plane: Vec::new(),
            cw: 0,
        });
    }
    Some(())
}

fn parse_dqt(mut seg: &[u8], qt: &mut [[u16; 64]; 4]) -> Option<()> {
    // One DQT segment may hold several tables back to back.
    while !seg.is_empty() {
        let pq = seg[0] >> 4;
        let tq = (seg[0] & 0x0F) as usize;
        if tq > 3 {
            return None;
        }
        seg = &seg[1..];
        if pq == 0 {
            if seg.len() < 64 {
                return None;
            }
            for k in 0..64 {
                qt[tq][k] = seg[k] as u16;
            }
            seg = &seg[64..];
        } else if pq == 1 {
            if seg.len() < 128 {
                return None;
            }
            for k in 0..64 {
                qt[tq][k] = ((seg[2 * k] as u16) << 8) | seg[2 * k + 1] as u16;
            }
            seg = &seg[128..];
        } else {
            return None;
        }
    }
    Some(())
}

fn parse_dht(mut seg: &[u8], dc: &mut [Huff; 4], ac: &mut [Huff; 4]) -> Option<()> {
    // One DHT segment may hold several tables back to back.
    while !seg.is_empty() {
        if seg.len() < 17 {
            return None;
        }
        let tc = seg[0] >> 4; // 0 = DC, 1 = AC
        let th = (seg[0] & 0x0F) as usize;
        if th > 3 || tc > 1 {
            return None;
        }
        let mut counts = [0u8; 16];
        counts.copy_from_slice(&seg[1..17]);
        let total: usize = counts.iter().map(|&c| c as usize).sum();
        if seg.len() < 17 + total {
            return None;
        }
        let values = seg[17..17 + total].to_vec();
        let table = Huff::build(&counts, values);
        if tc == 0 {
            dc[th] = table;
        } else {
            ac[th] = table;
        }
        seg = &seg[17 + total..];
    }
    Some(())
}

fn parse_sos(seg: &[u8], comps: &mut [Component]) -> Option<()> {
    if seg.is_empty() {
        return None;
    }
    let ns = seg[0] as usize;
    if ns == 0 || seg.len() < 1 + ns * 2 + 3 {
        return None;
    }
    for s in 0..ns {
        let cs = seg[1 + s * 2];
        let tdta = seg[2 + s * 2];
        let comp = comps.iter_mut().find(|c| c.id == cs)?;
        comp.td = (tdta >> 4) as usize;
        comp.ta = (tdta & 0x0F) as usize;
    }
    // Trailing Ss/Se/AhAl: baseline requires 0/63/0, but we don't enforce it.
    Some(())
}

#[allow(clippy::too_many_arguments)]
fn decode_scan(
    data: &[u8],
    entropy_start: usize,
    comps: &mut [Component],
    qt: &[[u16; 64]; 4],
    dc: &[Huff; 4],
    ac: &[Huff; 4],
    width: usize,
    height: usize,
    restart_interval: usize,
) -> Option<Image> {
    let hmax = comps.iter().map(|c| c.h).max()?;
    let vmax = comps.iter().map(|c| c.v).max()?;
    let mcu_w = hmax * 8;
    let mcu_h = vmax * 8;
    let mcus_x = width.div_ceil(mcu_w);
    let mcus_y = height.div_ceil(mcu_h);

    // Allocate each component's sample plane, padded to whole MCUs.
    for c in comps.iter_mut() {
        c.cw = mcus_x * c.h * 8;
        let ch = mcus_y * c.v * 8;
        c.plane = vec![0u8; c.cw.checked_mul(ch)?];
        c.pred = 0;
    }

    let cosines = idct_matrix();

    let mut br = BitReader::new(data, entropy_start);
    let mut mcu = 0usize;
    let mut block_coeffs = [0i32; 64];
    for my in 0..mcus_y {
        for mx in 0..mcus_x {
            if restart_interval > 0 && mcu > 0 && mcu.is_multiple_of(restart_interval) {
                br.restart();
                for c in comps.iter_mut() {
                    c.pred = 0;
                }
            }
            for comp in comps.iter_mut() {
                let (ch, cv, ctq, ctd, cta, ccw) =
                    (comp.h, comp.v, comp.tq, comp.td, comp.ta, comp.cw);
                for by in 0..cv {
                    for bx in 0..ch {
                        block_coeffs.fill(0);
                        decode_block(
                            &mut br,
                            &dc[ctd],
                            &ac[cta],
                            &qt[ctq],
                            &mut comp.pred,
                            &mut block_coeffs,
                        )?;
                        // IDCT straight into the plane at this block's origin.
                        let px0 = (mx * ch + bx) * 8;
                        let py0 = (my * cv + by) * 8;
                        idct_to_plane(&block_coeffs, &cosines, &mut comp.plane, ccw, px0, py0);
                    }
                }
            }
            mcu += 1;
        }
    }

    Some(to_rgba(comps, width, height, hmax, vmax))
}

/// Decode one 8x8 block: DC difference (predicted) then run-length AC, writing
/// dequantized coefficients into `out` in natural (row-major) order.
fn decode_block(
    br: &mut BitReader,
    dc: &Huff,
    ac: &Huff,
    qt: &[u16; 64],
    pred: &mut i32,
    out: &mut [i32; 64],
) -> Option<()> {
    let t = dc.decode(br)?;
    let diff = if t == 0 { 0 } else { extend(br.receive(t), t) };
    *pred += diff;
    out[0] = *pred * qt[0] as i32;

    let mut k = 1usize;
    while k < 64 {
        let rs = ac.decode(br)?;
        let r = (rs >> 4) as usize;
        let s = rs & 0x0F;
        if s == 0 {
            if r == 15 {
                k += 16; // ZRL: skip 16 zeros
                continue;
            }
            break; // EOB
        }
        k += r;
        if k >= 64 {
            break;
        }
        let coeff = extend(br.receive(s), s);
        out[ZIGZAG[k]] = coeff * qt[k] as i32;
        k += 1;
    }
    Some(())
}

/// Separable inverse DCT of a natural-order coefficient block, writing clamped,
/// level-shifted samples into `plane` at `(px0, py0)`.
fn idct_to_plane(
    block: &[i32; 64],
    m: &[[f32; 8]; 8],
    plane: &mut [u8],
    plane_w: usize,
    px0: usize,
    py0: usize,
) {
    // Pass 1: rows. tmp[y][x] = sum_u m[u][x] * F[y][u].
    let mut tmp = [0f32; 64];
    for y in 0..8 {
        let row = &block[y * 8..y * 8 + 8];
        for (x, t) in tmp[y * 8..y * 8 + 8].iter_mut().enumerate() {
            let mut s = 0f32;
            for u in 0..8 {
                s += m[u][x] * row[u] as f32;
            }
            *t = s;
        }
    }
    // Pass 2: columns, with level shift (+128) and clamp to 0..=255.
    for x in 0..8 {
        for y in 0..8 {
            let mut s = 0f32;
            for v in 0..8 {
                s += m[v][y] * tmp[v * 8 + x];
            }
            let p = (s.round() as i32 + 128).clamp(0, 255) as u8;
            plane[(py0 + y) * plane_w + (px0 + x)] = p;
        }
    }
}

/// Precompute the separable IDCT basis `m[u][x] = 0.5 * c(u) * cos((2x+1)u*pi/16)`
/// (the 1/sqrt2 DC factor and the overall 1/4 scale folded in across two passes).
fn idct_matrix() -> [[f32; 8]; 8] {
    let mut m = [[0f32; 8]; 8];
    let pi = std::f32::consts::PI;
    for (u, row) in m.iter_mut().enumerate() {
        let cu = if u == 0 { std::f32::consts::FRAC_1_SQRT_2 } else { 1.0 };
        for (x, val) in row.iter_mut().enumerate() {
            *val = 0.5 * cu * ((2 * x + 1) as f32 * u as f32 * pi / 16.0).cos();
        }
    }
    m
}

/// Upsample each component to full resolution (nearest-neighbor) and convert
/// YCbCr -> RGB (or replicate luma for grayscale) into packed RGBA8.
fn to_rgba(comps: &[Component], width: usize, height: usize, hmax: usize, vmax: usize) -> Image {
    let mut rgba = vec![0u8; width * height * 4];
    let sample = |c: &Component, x: usize, y: usize| -> u8 {
        let sx = x * c.h / hmax;
        let sy = y * c.v / vmax;
        c.plane[sy * c.cw + sx]
    };
    for y in 0..height {
        for x in 0..width {
            let o = (y * width + x) * 4;
            let (r, g, b) = if comps.len() == 1 {
                let l = sample(&comps[0], x, y);
                (l, l, l)
            } else {
                let yc = sample(&comps[0], x, y) as f32;
                let cb = sample(&comps[1], x, y) as f32 - 128.0;
                let cr = sample(&comps[2], x, y) as f32 - 128.0;
                let r = yc + 1.402 * cr;
                let g = yc - 0.344136 * cb - 0.714136 * cr;
                let b = yc + 1.772 * cb;
                (
                    r.round().clamp(0.0, 255.0) as u8,
                    g.round().clamp(0.0, 255.0) as u8,
                    b.round().clamp(0.0, 255.0) as u8,
                )
            };
            rgba[o] = r;
            rgba[o + 1] = g;
            rgba[o + 2] = b;
            rgba[o + 3] = 255;
        }
    }
    Image { width, height, rgba }
}
