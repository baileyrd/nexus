//! Byte-level output ring buffer (PRD-09 §3).
//!
//! # What this is
//!
//! A fixed-capacity, FIFO buffer of raw output bytes captured from a PTY
//! master. Writes past `capacity` evict the oldest bytes. Consumers can
//! snapshot the whole window, search for byte patterns, or drain bytes
//! into their own output sink.
//!
//! # What this is not (yet)
//!
//! PRD-09 §3.1–§3.4 prescribe a richer model: per-line records with
//! timestamps, ANSI-stripped text for search, per-process deduplication
//! of spinner lines, and a global 500 MB cap across all sessions
//! enforced by a monitor task. This Phase B cut ships the byte-level
//! core those features compose on top of; nothing above has to redesign
//! the storage once they land.
//!
//! # Design notes
//!
//! - **Storage:** `VecDeque<u8>` — a contiguous ring with O(1) push/pop
//!   at both ends. Cheap eviction via `pop_front`; cheap iteration via
//!   the two-slice view. `VecDeque` preallocates to its initial capacity,
//!   so a 10 MiB buffer lands as one 10 MiB heap allocation at
//!   construction time and never grows.
//! - **Thread safety:** the buffer itself is `Send` but not `Sync`.
//!   Callers serialise access (typically by owning the buffer inside
//!   the same `&mut self` scope as their `Session`). If a future
//!   reader-thread model lands, wrap in `Arc<Mutex<OutputBuffer>>`.
//! - **Raw bytes only:** ANSI codes pass through untouched. Search is
//!   byte-level so it sees escape sequences — for ANSI-stripped text
//!   search, layer a separate line-indexed view on top (§3.2 / §3.3).
//!
//! # Capacity defaults
//!
//! PRD-09 §3.1 specifies 10 MB per session as the default. The
//! [`OutputBuffer::DEFAULT_CAPACITY`] constant encodes that; callers
//! can override via [`OutputBuffer::with_capacity`] for long-running
//! builds with heavier output.

use std::collections::VecDeque;

/// FIFO byte buffer with fixed capacity — oldest bytes are dropped when
/// a write would exceed `capacity`.
#[derive(Debug)]
pub struct OutputBuffer {
    buf: VecDeque<u8>,
    capacity: usize,
    /// Running total of bytes that have been evicted over this buffer's
    /// lifetime. Useful for diagnostics and for the future memory-
    /// pressure event (PRD-09 §3.4 `ProcessOutputDropped`).
    dropped: u64,
}

impl OutputBuffer {
    /// PRD-09 §3.1 default: 10 MB.
    pub const DEFAULT_CAPACITY: usize = 10 * 1024 * 1024;

    /// Create a buffer with the PRD default capacity.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    /// Create a buffer with an explicit byte capacity. Zero-capacity
    /// buffers silently drop every byte — tests and sentinel cases can
    /// rely on this, but real use should pass a meaningful value.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(capacity),
            capacity,
            dropped: 0,
        }
    }

    /// Append `bytes` to the buffer. Any bytes that would exceed the
    /// configured capacity cause the corresponding number of oldest
    /// bytes to be dropped from the front, and the drop count is
    /// incremented.
    pub fn push(&mut self, bytes: &[u8]) {
        if self.capacity == 0 {
            self.dropped = self.dropped.saturating_add(bytes.len() as u64);
            return;
        }

        // If the incoming slice alone exceeds capacity, drop the whole
        // existing buffer and keep only the tail of the new slice.
        if bytes.len() >= self.capacity {
            let keep = &bytes[bytes.len() - self.capacity..];
            let dropped_now = (self.buf.len() + (bytes.len() - keep.len())) as u64;
            self.dropped = self.dropped.saturating_add(dropped_now);
            self.buf.clear();
            self.buf.extend(keep);
            return;
        }

        // Normal path: make room, then append.
        let available = self.capacity - self.buf.len();
        if bytes.len() > available {
            let need_drop = bytes.len() - available;
            self.evict(need_drop);
        }
        self.buf.extend(bytes);
    }

    /// Drop the oldest `n` bytes — used internally by `push` on overflow
    /// but also exposed for callers that want to proactively trim (e.g.
    /// the future memory-pressure monitor shaving 10 % off under load).
    pub fn evict(&mut self, n: usize) {
        let n = n.min(self.buf.len());
        self.dropped = self.dropped.saturating_add(n as u64);
        for _ in 0..n {
            self.buf.pop_front();
        }
    }

    /// Copy the current buffer contents out as a fresh `Vec<u8>`,
    /// oldest-first.
    #[must_use]
    pub fn snapshot(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
    }

    /// Return the two contiguous slices of the underlying ring (head,
    /// tail) — concatenating them gives the FIFO-ordered contents. Use
    /// this when you want to stream bytes out without allocating a fresh
    /// `Vec`. Either slice may be empty.
    #[must_use]
    pub fn slices(&self) -> (&[u8], &[u8]) {
        self.buf.as_slices()
    }

    /// Current byte count — always in `[0, capacity()]`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Configured capacity in bytes.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Cumulative bytes evicted over this buffer's lifetime.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Whether the buffer currently holds zero bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Drop every byte in the buffer and reset the drop counter.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.dropped = 0;
    }

    /// Return `true` if `needle` appears in the current buffer, searching
    /// across the ring boundary. Runs in O(len). Small helper for the
    /// §3.3 exact-match path; regex search belongs in a higher layer
    /// that keeps a line-indexed view.
    #[must_use]
    pub fn contains(&self, needle: &[u8]) -> bool {
        self.find(needle).is_some()
    }

    /// Return the offset (oldest-first) of `needle` in the buffer, or
    /// `None` if absent. Empty needles return `Some(0)` matching the
    /// std `slice::windows` convention.
    #[must_use]
    pub fn find(&self, needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return Some(0);
        }
        if needle.len() > self.buf.len() {
            return None;
        }
        // Materialise for simplicity — for 10 MB buffers this is a one-
        // off 10 MB copy and a linear scan, well under the §3.3 100 ms
        // target. A future line-indexed view will sidestep the copy.
        let snap = self.snapshot();
        snap.windows(needle.len()).position(|w| w == needle)
    }
}

impl Default for OutputBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_empty_at_default_capacity() {
        let b = OutputBuffer::new();
        assert_eq!(b.capacity(), OutputBuffer::DEFAULT_CAPACITY);
        assert_eq!(b.len(), 0);
        assert!(b.is_empty());
        assert_eq!(b.dropped(), 0);
    }

    #[test]
    fn push_under_capacity_retains_all() {
        let mut b = OutputBuffer::with_capacity(32);
        b.push(b"hello");
        b.push(b"world");
        assert_eq!(b.snapshot(), b"helloworld");
        assert_eq!(b.dropped(), 0);
    }

    #[test]
    fn push_at_exact_capacity_fits() {
        let mut b = OutputBuffer::with_capacity(5);
        b.push(b"hello");
        assert_eq!(b.snapshot(), b"hello");
        assert_eq!(b.dropped(), 0);
    }

    #[test]
    fn push_one_byte_past_capacity_evicts_front() {
        let mut b = OutputBuffer::with_capacity(5);
        b.push(b"hello");
        b.push(b"!");
        assert_eq!(b.snapshot(), b"ello!");
        assert_eq!(b.dropped(), 1);
    }

    #[test]
    fn push_slice_larger_than_capacity_keeps_only_tail() {
        let mut b = OutputBuffer::with_capacity(4);
        b.push(b"abcdefghij");
        assert_eq!(b.snapshot(), b"ghij");
        // `abcdef` (6) from the new slice plus the (empty) existing
        // buffer were dropped.
        assert_eq!(b.dropped(), 6);
    }

    #[test]
    fn multiple_overlapping_pushes_maintain_fifo_order() {
        let mut b = OutputBuffer::with_capacity(6);
        b.push(b"ab");
        b.push(b"cd");
        b.push(b"ef"); // fills
        b.push(b"gh"); // evicts "ab"
        assert_eq!(b.snapshot(), b"cdefgh");
        assert_eq!(b.dropped(), 2);
    }

    #[test]
    fn zero_capacity_drops_everything() {
        let mut b = OutputBuffer::with_capacity(0);
        b.push(b"anything");
        assert!(b.is_empty());
        assert_eq!(b.dropped(), 8);
    }

    #[test]
    fn evict_trims_from_front() {
        let mut b = OutputBuffer::with_capacity(10);
        b.push(b"0123456789");
        b.evict(3);
        assert_eq!(b.snapshot(), b"3456789");
        assert_eq!(b.dropped(), 3);
    }

    #[test]
    fn evict_clamps_to_len() {
        let mut b = OutputBuffer::with_capacity(10);
        b.push(b"abc");
        b.evict(100);
        assert!(b.is_empty());
        assert_eq!(b.dropped(), 3);
    }

    #[test]
    fn clear_resets_buffer_and_drop_counter() {
        let mut b = OutputBuffer::with_capacity(4);
        b.push(b"abcdef"); // forces eviction
        assert!(b.dropped() > 0);
        b.clear();
        assert!(b.is_empty());
        assert_eq!(b.dropped(), 0);
    }

    #[test]
    fn find_and_contains_work_across_ring_boundary() {
        // Set up a buffer whose contents wrap across the underlying
        // VecDeque's internal split so `as_slices()` returns two parts.
        let mut b = OutputBuffer::with_capacity(8);
        b.push(b"AAAAAAAA"); // fills
        b.push(b"BCD"); // evicts 3, leaves head mid-array
                        // After: "AAAAABCD" logically, but underlying slices are split.
        assert!(b.contains(b"AAAAA"));
        assert!(b.contains(b"ABCD"));
        assert_eq!(b.find(b"BCD"), Some(5));
        assert!(!b.contains(b"ZZZ"));
    }

    #[test]
    fn find_empty_needle_returns_zero() {
        let b = OutputBuffer::with_capacity(4);
        assert_eq!(b.find(b""), Some(0));
    }

    #[test]
    fn find_needle_longer_than_buffer_returns_none() {
        let mut b = OutputBuffer::with_capacity(4);
        b.push(b"ab");
        assert_eq!(b.find(b"abcdef"), None);
    }

    #[test]
    fn slices_view_matches_snapshot() {
        let mut b = OutputBuffer::with_capacity(8);
        b.push(b"AAAAAAAA");
        b.push(b"BCD"); // forces ring wrap
        let (head, tail) = b.slices();
        let mut combined = Vec::new();
        combined.extend_from_slice(head);
        combined.extend_from_slice(tail);
        assert_eq!(combined, b.snapshot());
    }
}
