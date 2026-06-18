//! Minimal PNG decoder (no crates) for the common Kitty graphics case: 8-bit,
//! non-interlaced images in grayscale / RGB / palette / gray+alpha / RGBA. The
//! zlib-compressed `IDAT` stream is inflated by [`super::inflate`]; scanlines
//! are then reversed through the five PNG filters and expanded to RGBA8.
//!
//! Unsupported variants (bit depths other than 8, interlaced images) decode to
//! `None` rather than guessing — the caller falls back to not displaying.

use super::inflate;

/// A decoded image as tightly-packed RGBA8 (`rgba.len() == width * height * 4`).
pub(crate) struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
/// Pixel-count cap (16 MiB of RGBA) so a malformed `IHDR` can't request a vast
/// allocation.
const MAX_PIXELS: usize = 4 * 1024 * 1024;

/// Decode a PNG into RGBA8, or `None` if malformed or an unsupported variant.
pub(crate) fn decode(data: &[u8]) -> Option<Image> {
    if data.len() < 8 || data[..8] != SIGNATURE {
        return None;
    }

    let mut width = 0usize;
    let mut height = 0usize;
    let mut depth = 0u8;
    let mut color = 0u8;
    let mut interlace = 0u8;
    let mut have_ihdr = false;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();

    // Walk chunks: 4-byte big-endian length, 4-byte type, data, 4-byte CRC.
    let mut i = 8;
    while i + 8 <= data.len() {
        let len = u32::from_be_bytes(data[i..i + 4].try_into().ok()?) as usize;
        let kind = &data[i + 4..i + 8];
        let body = i + 8;
        let end = body.checked_add(len)?;
        if end + 4 > data.len() {
            break; // truncated chunk (need body + CRC)
        }
        let chunk = &data[body..end];
        match kind {
            b"IHDR" => {
                if chunk.len() < 13 {
                    return None;
                }
                width = u32::from_be_bytes(chunk[0..4].try_into().ok()?) as usize;
                height = u32::from_be_bytes(chunk[4..8].try_into().ok()?) as usize;
                depth = chunk[8];
                color = chunk[9];
                interlace = chunk[12];
                have_ihdr = true;
            }
            b"PLTE" => {
                for px in chunk.chunks_exact(3) {
                    palette.push([px[0], px[1], px[2]]);
                }
            }
            b"tRNS" => trns = chunk.to_vec(),
            b"IDAT" => idat.extend_from_slice(chunk),
            b"IEND" => break,
            _ => {} // ancillary chunks we don't need
        }
        i = end + 4;
    }

    if !have_ihdr || width == 0 || height == 0 || depth != 8 || interlace != 0 {
        return None;
    }
    if width.checked_mul(height)? > MAX_PIXELS {
        return None;
    }
    let channels = match color {
        0 => 1, // grayscale
        2 => 3, // RGB
        3 => 1, // palette index
        4 => 2, // grayscale + alpha
        6 => 4, // RGBA
        _ => return None,
    };

    let stride = width.checked_mul(channels)?;
    let expected = stride.checked_add(1)?.checked_mul(height)?; // +1 filter byte / row
    let raw = inflate::zlib_decompress(&idat, expected + 64)?;
    if raw.len() < expected {
        return None;
    }

    let mut prev = vec![0u8; stride];
    let mut cur = vec![0u8; stride];
    let mut rgba = Vec::with_capacity(width * height * 4);
    let mut pos = 0;
    for _ in 0..height {
        let filter = raw[pos];
        pos += 1;
        cur.copy_from_slice(&raw[pos..pos + stride]);
        pos += stride;
        unfilter(filter, &mut cur, &prev, channels)?;
        for x in 0..width {
            let o = x * channels;
            let (r, g, b, a) = match color {
                0 => (cur[o], cur[o], cur[o], 255),
                2 => (cur[o], cur[o + 1], cur[o + 2], 255),
                3 => {
                    let idx = cur[o] as usize;
                    let [r, g, b] = *palette.get(idx)?;
                    (r, g, b, *trns.get(idx).unwrap_or(&255))
                }
                4 => (cur[o], cur[o], cur[o], cur[o + 1]),
                _ => (cur[o], cur[o + 1], cur[o + 2], cur[o + 3]),
            };
            rgba.extend_from_slice(&[r, g, b, a]);
        }
        std::mem::swap(&mut prev, &mut cur);
    }

    Some(Image {
        width,
        height,
        rgba,
    })
}

/// Reverse one PNG scanline filter in place. `bpp` is bytes-per-pixel (the
/// filter's left-neighbor stride); `prev` is the already-reconstructed row above.
fn unfilter(filter: u8, cur: &mut [u8], prev: &[u8], bpp: usize) -> Option<()> {
    let n = cur.len();
    match filter {
        0 => {} // None
        1 => {
            // Sub: add the byte `bpp` to the left.
            for x in bpp..n {
                cur[x] = cur[x].wrapping_add(cur[x - bpp]);
            }
        }
        2 => {
            // Up: add the byte above.
            for x in 0..n {
                cur[x] = cur[x].wrapping_add(prev[x]);
            }
        }
        3 => {
            // Average: add floor((left + above) / 2).
            for x in 0..n {
                let left = if x >= bpp { cur[x - bpp] as u16 } else { 0 };
                let avg = ((left + prev[x] as u16) / 2) as u8;
                cur[x] = cur[x].wrapping_add(avg);
            }
        }
        4 => {
            // Paeth predictor.
            for x in 0..n {
                let a = if x >= bpp { cur[x - bpp] } else { 0 };
                let b = prev[x];
                let c = if x >= bpp { prev[x - bpp] } else { 0 };
                cur[x] = cur[x].wrapping_add(paeth(a, b, c));
            }
        }
        _ => return None,
    }
    Some(())
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ai, bi, ci) = (a as i32, b as i32, c as i32);
    let p = ai + bi - ci;
    let (pa, pb, pc) = ((p - ai).abs(), (p - bi).abs(), (p - ci).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}
