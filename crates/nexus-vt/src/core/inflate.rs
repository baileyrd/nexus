//! DEFLATE (RFC 1951) and zlib (RFC 1950) decompression — no crates.
//!
//! A compact, allocation-light inflater used to decode the zlib-wrapped pixel
//! data inside PNG `IDAT` chunks (and Kitty's `o=z` compressed payloads). The
//! Huffman decoder follows the canonical "puff.c" walk: accumulate one bit at a
//! time and compare the running code against the first code of each length.

/// LSB-first bit reader over a byte slice.
struct BitReader<'a> {
    data: &'a [u8],
    byte: usize,
    bit: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader {
            data,
            byte: 0,
            bit: 0,
        }
    }

    /// Read a single bit (LSB-first within each byte). `None` past end of input.
    fn bit(&mut self) -> Option<u32> {
        let byte = *self.data.get(self.byte)?;
        let v = (byte >> self.bit) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.byte += 1;
        }
        Some(v as u32)
    }

    /// Read `n` bits as a little-endian integer.
    fn bits(&mut self, n: u32) -> Option<u32> {
        let mut v = 0u32;
        for i in 0..n {
            v |= self.bit()? << i;
        }
        Some(v)
    }

    /// Advance to the next byte boundary (for stored blocks).
    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.byte += 1;
        }
    }
}

/// A canonical Huffman table built from a list of code lengths.
struct Huffman {
    /// `counts[l]` = number of symbols with code length `l` (1..=15).
    counts: [u16; 16],
    /// Symbols ordered by (length, symbol) — the canonical decode order.
    symbols: Vec<u16>,
}

impl Huffman {
    fn build(lengths: &[u16]) -> Huffman {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[l as usize] += 1;
        }
        counts[0] = 0; // length-0 symbols don't participate
        let mut offsets = [0u16; 16];
        for l in 1..16 {
            offsets[l] = offsets[l - 1] + counts[l - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offsets[l as usize] as usize] = sym as u16;
                offsets[l as usize] += 1;
            }
        }
        Huffman { counts, symbols }
    }

    /// Decode one symbol by walking code lengths 1..=15.
    fn decode(&self, br: &mut BitReader) -> Option<u16> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..16 {
            code |= br.bit()? as i32;
            let count = self.counts[len] as i32;
            if code - first < count {
                return self.symbols.get((index + (code - first)) as usize).copied();
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        None
    }
}

// Length codes 257..=285: base length and number of extra bits.
const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
// Distance codes 0..=29: base distance and number of extra bits.
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
// Order in which code-length code lengths are stored in a dynamic block.
const CL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Decompress a zlib stream (RFC 1950): 2-byte header, DEFLATE body, trailing
/// Adler-32 (not verified). Returns `None` on malformed input; output is capped
/// at `max_out`.
pub(crate) fn zlib_decompress(data: &[u8], max_out: usize) -> Option<Vec<u8>> {
    if data.len() < 2 {
        return None;
    }
    let (cmf, flg) = (data[0], data[1]);
    if !(cmf as u16 * 256 + flg as u16).is_multiple_of(31) || cmf & 0x0f != 8 {
        return None; // bad check bits or not the DEFLATE method
    }
    let mut start = 2;
    if flg & 0x20 != 0 {
        start += 4; // FDICT: skip the 4-byte preset-dictionary id
    }
    inflate(data.get(start..)?, max_out)
}

/// Raw DEFLATE (RFC 1951). Output is capped at `max_out`.
pub(crate) fn inflate(data: &[u8], max_out: usize) -> Option<Vec<u8>> {
    let mut br = BitReader::new(data);
    let mut out: Vec<u8> = Vec::new();

    loop {
        let bfinal = br.bit()?;
        match br.bits(2)? {
            0 => inflate_stored(&mut br, &mut out, max_out)?,
            1 => inflate_block(&mut br, &mut out, &fixed_litlen(), &fixed_dist(), max_out)?,
            2 => {
                let (litlen, dist) = read_dynamic_tables(&mut br)?;
                inflate_block(&mut br, &mut out, &litlen, &dist, max_out)?;
            }
            _ => return None, // reserved block type
        }
        if bfinal == 1 {
            return Some(out);
        }
        if out.len() >= max_out {
            return Some(out);
        }
    }
}

/// A stored (uncompressed) block: `LEN`/`NLEN` then `LEN` literal bytes.
fn inflate_stored(br: &mut BitReader, out: &mut Vec<u8>, max_out: usize) -> Option<()> {
    br.align();
    let len = br.bits(16)? as usize;
    let nlen = br.bits(16)?;
    if nlen != (!len as u16) as u32 {
        return None; // LEN / one's-complement mismatch
    }
    for _ in 0..len {
        if out.len() >= max_out {
            break;
        }
        out.push(br.bits(8)? as u8);
    }
    Some(())
}

/// Decode a Huffman block (fixed or dynamic) into `out`.
fn inflate_block(
    br: &mut BitReader,
    out: &mut Vec<u8>,
    litlen: &Huffman,
    dist: &Huffman,
    max_out: usize,
) -> Option<()> {
    loop {
        let sym = litlen.decode(br)?;
        match sym {
            0..=255 => {
                out.push(sym as u8);
            }
            256 => return Some(()), // end of block
            257..=285 => {
                let i = (sym - 257) as usize;
                let length = LEN_BASE[i] as usize + br.bits(LEN_EXTRA[i])? as usize;
                let dsym = dist.decode(br)? as usize;
                if dsym >= DIST_BASE.len() {
                    return None;
                }
                let distance = DIST_BASE[dsym] as usize + br.bits(DIST_EXTRA[dsym])? as usize;
                if distance == 0 || distance > out.len() {
                    return None; // reference points before the output start
                }
                let start = out.len() - distance;
                for k in 0..length {
                    let byte = out[start + k];
                    out.push(byte);
                }
            }
            _ => return None, // 286/287 are invalid
        }
        if out.len() >= max_out {
            return Some(()); // truncate rather than grow without bound
        }
    }
}

/// Read the dynamic block's code-length, literal/length, and distance tables.
fn read_dynamic_tables(br: &mut BitReader) -> Option<(Huffman, Huffman)> {
    let hlit = br.bits(5)? as usize + 257;
    let hdist = br.bits(5)? as usize + 1;
    let hclen = br.bits(4)? as usize + 4;

    // Code-length code lengths, in their shuffled storage order.
    let mut cl_lengths = [0u16; 19];
    for &order in CL_ORDER.iter().take(hclen) {
        cl_lengths[order] = br.bits(3)? as u16;
    }
    let cl_huffman = Huffman::build(&cl_lengths);

    // The literal/length and distance code lengths share one run-length stream.
    let total = hlit + hdist;
    let mut lengths: Vec<u16> = Vec::with_capacity(total);
    while lengths.len() < total {
        match cl_huffman.decode(br)? {
            sym @ 0..=15 => lengths.push(sym),
            16 => {
                // Repeat the previous length 3..=6 times.
                let prev = *lengths.last()?;
                let n = 3 + br.bits(2)? as usize;
                lengths.extend(std::iter::repeat_n(prev, n));
            }
            17 => {
                let n = 3 + br.bits(3)? as usize;
                lengths.extend(std::iter::repeat_n(0, n));
            }
            18 => {
                let n = 11 + br.bits(7)? as usize;
                lengths.extend(std::iter::repeat_n(0, n));
            }
            _ => return None,
        }
    }
    if lengths.len() > total {
        return None; // a repeat overran the table
    }
    let litlen = Huffman::build(&lengths[..hlit]);
    let dist = Huffman::build(&lengths[hlit..]);
    Some((litlen, dist))
}

/// The fixed literal/length Huffman table (RFC 1951 §3.2.6).
fn fixed_litlen() -> Huffman {
    let mut lengths = [0u16; 288];
    for (i, l) in lengths.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    Huffman::build(&lengths)
}

/// The fixed distance Huffman table: 30 codes, all length 5.
fn fixed_dist() -> Huffman {
    Huffman::build(&[5u16; 30])
}
