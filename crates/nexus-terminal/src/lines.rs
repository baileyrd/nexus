//! Line-indexed view of PTY output (PRD-09 §3.2 / §3.3).
//!
//! # Role
//!
//! [`OutputBuffer`] captures raw bytes for playback and exact-byte
//! search. [`LineBuffer`] sits alongside it and ingests the same stream
//! but emits structured, ANSI-stripped lines with timestamps and an
//! adjacent-duplicate counter. This is the surface the UI's search
//! panel reads, the future URL detector scans, and the AI context
//! assembler pulls from.
//!
//! Storing lines is cheap (plain `String`s) and kept bounded by
//! [`LineBuffer::max_lines`] — 10 000 by default, matching the PRD
//! §3.3 "100k-line buffer" upper bound while keeping a smaller
//! default for typical sessions.
//!
//! # Duplicate compression
//!
//! PRD §3.1 calls for deduplicating repeated identical lines — spinner
//! and progress-bar output otherwise dominates the ring. [`LineBuffer`]
//! implements the simplest variant: if the newest line's `text_only`
//! exactly matches the previous line's `text_only`, the counter on the
//! existing line is incremented and no new record is inserted. Only
//! *adjacent* repeats collapse; a dedup pass further back in the ring
//! is out of scope for this phase.
//!
//! # What this is NOT
//!
//! - No full terminal state emulation. Cursor moves, erase, scroll
//!   regions are stripped by [`crate::ansi::strip_ansi`] and not
//!   replayed — see the ANSI module docs for why.
//! - No per-line timestamps beyond a wall-clock `SystemTime::now()`
//!   taken at push time. Tests pin the timestamp in their fixtures
//!   to avoid flaking.
//! - No regex cache across searches. PRD §3.3 specifies "compile regex
//!   once" but across multiple calls to [`LineBuffer::find_regex`] we
//!   recompile each time. That's the caller's call to cache if needed.

use std::collections::VecDeque;
use std::time::SystemTime;

use crate::ansi::strip_ansi;

/// One output line in the structured view.
#[derive(Debug, Clone)]
pub struct Line {
    /// Wall-clock time of first ingestion. Does not update on dedup.
    pub timestamp: SystemTime,
    /// Raw bytes as received from the PTY — includes ANSI sequences
    /// and the trailing `\n` if present. Preserved so callers can
    /// re-play the output at render time.
    pub raw: Vec<u8>,
    /// ANSI-stripped text, with the trailing `\n` removed. This is the
    /// field search and display read from.
    pub text_only: String,
    /// Number of consecutive occurrences that collapsed into this
    /// line. Starts at 1 and increments on each matching adjacent
    /// push.
    pub repeats: u32,
}

impl Line {
    fn new(raw: Vec<u8>, text_only: String) -> Self {
        Self {
            timestamp: SystemTime::now(),
            raw,
            text_only,
            repeats: 1,
        }
    }
}

/// Ring buffer of structured [`Line`]s capped at `max_lines`. Oldest
/// lines drop when the cap is reached.
#[derive(Debug)]
pub struct LineBuffer {
    lines: VecDeque<Line>,
    max_lines: usize,
    /// Bytes pushed into [`Self::push`] that did not yet end with a
    /// newline. Held here until a subsequent push completes the line
    /// (or [`Self::flush_pending`] is called explicitly at shutdown).
    pending: Vec<u8>,
    /// Cumulative count of lines evicted from the front over this
    /// buffer's lifetime. Feeds the future §3.4 pressure event.
    dropped: u64,
}

impl LineBuffer {
    /// PRD-inspired default: 10 000 lines — large enough for most
    /// interactive sessions, small enough that a snapshot fits in
    /// memory easily.
    pub const DEFAULT_MAX_LINES: usize = 10_000;

    /// Create a line buffer with the default cap.
    #[must_use]
    pub fn new() -> Self {
        Self::with_max_lines(Self::DEFAULT_MAX_LINES)
    }

    /// Create with an explicit line cap.
    #[must_use]
    pub fn with_max_lines(max_lines: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(max_lines.min(1024)),
            max_lines,
            pending: Vec::new(),
            dropped: 0,
        }
    }

    /// Ingest `bytes` from the PTY. Splits on `\n`; complete lines are
    /// ANSI-stripped and pushed. Any trailing bytes without a newline
    /// are held in the pending buffer until a future push completes
    /// them — this matches terminal reality where output arrives in
    /// fragments.
    pub fn push(&mut self, bytes: &[u8]) {
        if self.max_lines == 0 {
            // Line-less buffer — drop everything, count nothing.
            return;
        }
        let mut start = 0;
        for (idx, &byte) in bytes.iter().enumerate() {
            if byte == b'\n' {
                // The first complete line in this push stitches onto
                // any partial bytes we were holding from a previous
                // call — essential for the realistic case where a PTY
                // read splits mid-line. Subsequent newlines consume
                // only the current slice because pending is now drained.
                if self.pending.is_empty() {
                    let slice = &bytes[start..=idx];
                    self.ingest_complete_line(slice);
                } else {
                    let mut stitched =
                        Vec::with_capacity(self.pending.len() + (idx - start) + 1);
                    stitched.extend_from_slice(&self.pending);
                    stitched.extend_from_slice(&bytes[start..=idx]);
                    self.pending.clear();
                    self.ingest_complete_line(&stitched);
                }
                start = idx + 1;
            }
        }
        if start < bytes.len() {
            self.pending.extend_from_slice(&bytes[start..]);
        }
    }

    /// Emit whatever is in the pending buffer as a line, even if it
    /// lacks a trailing newline. Used when the child exits and the
    /// last line should be preserved.
    pub fn flush_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let raw = std::mem::take(&mut self.pending);
        let text_only = strip_ansi(&raw);
        self.insert_line(Line::new(raw, text_only));
    }

    /// Number of structured lines currently held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether the buffer holds zero lines and has no pending partial.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.pending.is_empty()
    }

    /// Maximum number of lines the buffer will retain.
    #[must_use]
    pub fn max_lines(&self) -> usize {
        self.max_lines
    }

    /// Bytes currently accumulated toward the next complete line.
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Cumulative lines evicted from the front over this buffer's
    /// lifetime.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Iterate over stored lines, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &Line> {
        self.lines.iter()
    }

    /// Clear every line and forget the pending partial.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.pending.clear();
        self.dropped = 0;
    }

    /// Return every line whose `text_only` contains `query`, oldest
    /// first. `query` is a plain substring — for regex use
    /// [`Self::find_regex`].
    #[must_use]
    pub fn find(&self, query: &str) -> Vec<&Line> {
        self.lines
            .iter()
            .filter(|l| l.text_only.contains(query))
            .collect()
    }

    /// Return every line whose `text_only` matches `pattern`, or `None`
    /// if the pattern fails to compile.
    #[must_use]
    pub fn find_regex(&self, pattern: &str) -> Option<Vec<&Line>> {
        let re = regex_lite::Regex::new(pattern).ok()?;
        Some(
            self.lines
                .iter()
                .filter(|l| re.is_match(&l.text_only))
                .collect(),
        )
    }

    fn ingest_complete_line(&mut self, slice: &[u8]) {
        let raw: Vec<u8> = slice.to_vec();
        // Strip trailing newline (and an adjacent carriage return, for
        // CRLF endings) from `text_only` — we keep the raw bytes for
        // playback but the search / display form drops the line
        // terminator.
        let mut text_bytes = slice;
        if text_bytes.last() == Some(&b'\n') {
            text_bytes = &text_bytes[..text_bytes.len() - 1];
        }
        if text_bytes.last() == Some(&b'\r') {
            text_bytes = &text_bytes[..text_bytes.len() - 1];
        }
        let text_only = strip_ansi(text_bytes);
        self.insert_line(Line::new(raw, text_only));
    }

    fn insert_line(&mut self, line: Line) {
        // Dedup adjacent exact repeats (§3.1 spinner/progress-bar case).
        if let Some(last) = self.lines.back_mut() {
            if last.text_only == line.text_only {
                last.repeats = last.repeats.saturating_add(1);
                return;
            }
        }
        while self.lines.len() >= self.max_lines {
            self.lines.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.lines.push_back(line);
    }
}

impl Default for LineBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_emits_one_line_per_newline() {
        let mut b = LineBuffer::new();
        b.push(b"alpha\nbeta\ngamma\n");
        assert_eq!(b.len(), 3);
        let texts: Vec<_> = b.iter().map(|l| l.text_only.clone()).collect();
        assert_eq!(texts, vec!["alpha", "beta", "gamma"]);
        assert_eq!(b.pending_len(), 0);
    }

    #[test]
    fn partial_line_buffered_until_newline_arrives() {
        let mut b = LineBuffer::new();
        b.push(b"hello");
        assert_eq!(b.len(), 0);
        assert_eq!(b.pending_len(), 5);
        b.push(b" world\n");
        assert_eq!(b.len(), 1);
        assert_eq!(b.iter().next().unwrap().text_only, "hello world");
        assert_eq!(b.pending_len(), 0);
    }

    #[test]
    fn flush_pending_emits_partial_line_as_record() {
        let mut b = LineBuffer::new();
        b.push(b"no-newline");
        assert_eq!(b.len(), 0);
        b.flush_pending();
        assert_eq!(b.len(), 1);
        assert_eq!(b.iter().next().unwrap().text_only, "no-newline");
        assert!(b.pending_len() == 0);
    }

    #[test]
    fn ansi_sequences_stripped_from_text_only() {
        let mut b = LineBuffer::new();
        b.push(b"\x1b[31mERROR\x1b[0m: disk full\n");
        let line = b.iter().next().unwrap();
        assert_eq!(line.text_only, "ERROR: disk full");
        // Raw bytes keep the ANSI so render-time playback is faithful.
        assert!(line.raw.starts_with(b"\x1b[31m"));
    }

    #[test]
    fn crlf_line_ending_is_preserved_in_raw_but_stripped_from_text() {
        let mut b = LineBuffer::new();
        b.push(b"msg\r\n");
        let line = b.iter().next().unwrap();
        assert_eq!(line.text_only, "msg");
        assert_eq!(line.raw, b"msg\r\n");
    }

    #[test]
    fn adjacent_duplicates_collapse_into_repeat_counter() {
        let mut b = LineBuffer::new();
        b.push(b"tick\ntick\ntick\ndifferent\ntick\n");
        assert_eq!(b.len(), 3);
        let lines: Vec<_> = b.iter().collect();
        assert_eq!(lines[0].text_only, "tick");
        assert_eq!(lines[0].repeats, 3);
        assert_eq!(lines[1].text_only, "different");
        assert_eq!(lines[1].repeats, 1);
        assert_eq!(lines[2].text_only, "tick");
        assert_eq!(lines[2].repeats, 1);
    }

    #[test]
    fn cap_evicts_oldest_lines() {
        let mut b = LineBuffer::with_max_lines(3);
        b.push(b"a\nb\nc\nd\ne\n");
        let texts: Vec<_> = b.iter().map(|l| l.text_only.clone()).collect();
        assert_eq!(texts, vec!["c", "d", "e"]);
        assert_eq!(b.dropped(), 2);
    }

    #[test]
    fn find_returns_matching_lines_in_order() {
        let mut b = LineBuffer::new();
        b.push(b"GET /health 200\n");
        b.push(b"POST /api/items 201\n");
        b.push(b"GET /metrics 200\n");
        let hits = b.find("GET");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].text_only, "GET /health 200");
        assert_eq!(hits[1].text_only, "GET /metrics 200");
    }

    #[test]
    fn find_regex_returns_matches_or_none_on_bad_pattern() {
        let mut b = LineBuffer::new();
        b.push(b"err: disk\nwarn: slow\nerr: net\n");
        let hits = b.find_regex(r"^err:").expect("valid regex");
        assert_eq!(hits.len(), 2);
        assert!(b.find_regex(r"(").is_none(), "bad regex should be None");
    }

    #[test]
    fn zero_cap_drops_everything_but_accepts_calls() {
        let mut b = LineBuffer::with_max_lines(0);
        b.push(b"a\nb\nc\n");
        assert_eq!(b.len(), 0);
        assert!(b.is_empty());
    }

    #[test]
    fn clear_resets_lines_pending_and_drop_counter() {
        let mut b = LineBuffer::with_max_lines(2);
        b.push(b"a\nb\nc\nd\nleftover");
        assert!(b.dropped() > 0);
        assert!(b.pending_len() > 0);
        b.clear();
        assert!(b.is_empty());
        assert_eq!(b.dropped(), 0);
    }
}
