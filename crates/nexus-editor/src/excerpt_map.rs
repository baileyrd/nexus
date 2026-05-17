//! BL-141 Phase 2 Approach B — position-mapping primitive.
//!
//! A multibuffer Excerpt block carries an `(source_relpath, line_start,
//! line_end)` range plus a `content` snapshot built by
//! [`core_plugin::slice_lines`] — the source's lines `[line_start..line_end]`
//! joined with `'\n'` (terminator-agnostic). Approach A treats the
//! snapshot as opaque text: edits round-trip via `UpdateBlockContent`
//! and `save` splices the new snapshot back into the source file.
//!
//! Approach B routes per-keystroke `InsertText` / `DeleteText` directly
//! to the source file's own `Session`. To do that we need a pure
//! translation between **byte offsets in the excerpt's content** and
//! **byte offsets in the source's full text**. That translation is
//! line-by-line, not raw-byte:
//!
//! - The excerpt's content uses `'\n'` separators (1 byte each), but
//!   the source may use `'\r\n'` (2 bytes) or `'\n'` (1 byte).
//! - Within each line the bodies are identical — `str::lines()` strips
//!   the terminator but doesn't change the line content.
//! - Therefore byte K **within** line N of the excerpt maps to byte K
//!   **within** line `(excerpt_line_start - 1 + N)` of the source.
//!
//! [`LineIndex`] precomputes the source's per-line byte offsets so
//! later translations are O(log n) on the line count, not O(source
//! bytes).
//!
//! These primitives are intentionally Session-agnostic: they consume
//! `&str` + line numbers and return `Option<usize>`. Step 2 of
//! Approach B wires them into `apply_transaction`'s text-op path.

#![allow(dead_code)] // Step 1 primitives; step 2 wires them up.

/// Byte offsets of every line start in a source string. Built once
/// per source text; reused across many position translations against
/// that source.
///
/// `line_starts.len()` equals the source's line count as reported by
/// `str::lines()` — i.e. a trailing newline does **not** add a
/// phantom empty line (matching `str::lines()`'s contract). A truly
/// empty source has an empty `line_starts`, so [`LineIndex::line_start`]
/// returns `None` for every line.
#[derive(Clone, Debug)]
pub struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    /// Build the index. O(source bytes).
    pub fn new(source: &str) -> Self {
        let mut line_starts = Vec::new();
        if source.is_empty() {
            return Self { line_starts };
        }
        line_starts.push(0);
        let bytes = source.as_bytes();
        let len = bytes.len();
        let mut i = 0;
        while i < len {
            if bytes[i] == b'\n' {
                let next = i + 1;
                // `str::lines()` does not produce a phantom empty line
                // when the source ends with '\n'; mirror that — only
                // record a line start when there's actual content
                // (or further newlines) after the terminator.
                if next < len {
                    line_starts.push(next);
                }
            }
            i += 1;
        }
        Self { line_starts }
    }

    /// Byte offset of the first byte on line `line` (1-based, inclusive).
    /// Returns `None` when `line > total_lines()` or `line == 0`.
    pub fn line_start(&self, line: u32) -> Option<usize> {
        if line == 0 {
            return None;
        }
        let idx = (line - 1) as usize;
        self.line_starts.get(idx).copied()
    }

    /// Total line count per `str::lines()`. May be zero (empty source).
    pub fn total_lines(&self) -> u32 {
        u32::try_from(self.line_starts.len()).unwrap_or(u32::MAX)
    }

    /// Inclusive byte range of line `line` **excluding** its terminator.
    /// Returns `None` when the line doesn't exist. Use this to compute
    /// the byte length of a single source line.
    pub fn line_content_end(&self, line: u32, source: &str) -> Option<usize> {
        let start = self.line_start(line)?;
        // The line ends at the next '\n' minus optional '\r', or EOF.
        let bytes = source.as_bytes();
        let mut i = start;
        while i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
        // Strip a trailing '\r' (CRLF).
        if i > start && bytes[i.saturating_sub(1)] == b'\r' {
            Some(i - 1)
        } else {
            Some(i)
        }
    }
}

/// Result of [`excerpt_to_source`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePos {
    /// 1-based source line number this position falls on.
    pub line: u32,
    /// Byte offset within the source line's **body** (terminator
    /// excluded). Range `[0, line_body_length]`.
    pub intra_line_byte: usize,
    /// Absolute byte offset in the full source string.
    pub source_byte: usize,
}

/// Translate a byte offset within an Excerpt's `content` (lines joined
/// by `'\n'`) into the corresponding source-file byte offset.
///
/// - `excerpt_content`: the synthetic block's current content, as
///   constructed by `core_plugin::slice_lines` (lines joined with
///   `'\n'`, no trailing newline).
/// - `excerpt_line_start`: 1-based source line the excerpt begins at.
/// - `source_line_index`: a [`LineIndex`] built from the source's
///   **current** text.
/// - `excerpt_byte_offset`: the position to translate.
///
/// Returns `None` when:
///
/// - `excerpt_byte_offset` exceeds `excerpt_content.len()`.
/// - The excerpt's line range starts past the source's EOF.
/// - A line internal to the excerpt no longer exists in the source
///   (e.g. external truncation since the excerpt was captured).
///
/// The line bodies are not re-validated against the source — Approach
/// B's external-edit subscription (step 3) is responsible for refreshing
/// the snapshot when sources change. This primitive trusts its inputs.
pub fn excerpt_to_source(
    excerpt_content: &str,
    excerpt_line_start: u32,
    source_line_index: &LineIndex,
    excerpt_byte_offset: usize,
) -> Option<SourcePos> {
    if excerpt_byte_offset > excerpt_content.len() {
        return None;
    }
    if excerpt_line_start == 0 {
        return None;
    }
    let bytes = excerpt_content.as_bytes();
    let mut line_idx: u32 = 0;
    let mut line_start_in_excerpt: usize = 0;
    let mut i: usize = 0;
    // Walk the excerpt one byte at a time, tracking which line we're
    // on. We stop once `i == excerpt_byte_offset` — the answer is the
    // current line + (i - line_start_in_excerpt).
    while i < excerpt_byte_offset {
        if bytes[i] == b'\n' {
            line_idx = line_idx.saturating_add(1);
            line_start_in_excerpt = i + 1;
        }
        i += 1;
    }
    let intra_line = excerpt_byte_offset - line_start_in_excerpt;
    let source_line = excerpt_line_start.checked_add(line_idx)?;
    let source_line_start = source_line_index.line_start(source_line)?;
    Some(SourcePos {
        line: source_line,
        intra_line_byte: intra_line,
        source_byte: source_line_start + intra_line,
    })
}

/// Translate a source byte offset back into the corresponding offset
/// in the excerpt's content. Returns `None` when the source position
/// falls outside the excerpt's covered range.
///
/// `excerpt_line_end` is the 1-based **inclusive** last line of the
/// excerpt — matches [`crate::block::BlockType::Excerpt::line_end`].
pub fn source_to_excerpt(
    excerpt_content: &str,
    excerpt_line_start: u32,
    excerpt_line_end: u32,
    source: &str,
    source_line_index: &LineIndex,
    source_byte_offset: usize,
) -> Option<usize> {
    if excerpt_line_start == 0 || excerpt_line_start > excerpt_line_end {
        return None;
    }
    if source_byte_offset > source.len() {
        return None;
    }
    // Find which source line `source_byte_offset` falls on. Binary
    // search the line_starts array — the largest `line_starts[i]` that
    // is `<= source_byte_offset`.
    let target_line: u32 = {
        let starts = &source_line_index.line_starts;
        if starts.is_empty() {
            return None;
        }
        let mut lo: usize = 0;
        let mut hi: usize = starts.len();
        while lo + 1 < hi {
            let mid = usize::midpoint(lo, hi);
            if starts[mid] <= source_byte_offset {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        // lo is the 0-based line index; +1 for 1-based.
        u32::try_from(lo).ok()?.checked_add(1)?
    };
    if target_line < excerpt_line_start || target_line > excerpt_line_end {
        return None;
    }
    // `source_byte_offset` may fall on a CR of a CRLF terminator. We
    // pin it to the end of the line body (so the inverse maps cleanly
    // through `excerpt_to_source`).
    let line_body_end = source_line_index.line_content_end(target_line, source)?;
    let line_start = source_line_index.line_start(target_line)?;
    let effective = source_byte_offset.min(line_body_end);
    let intra_line = effective.saturating_sub(line_start);

    // Now walk the excerpt forward to the start of `target_line` and
    // add `intra_line`. The excerpt line index is
    // `target_line - excerpt_line_start` (0-based).
    let target_excerpt_line: u32 = target_line - excerpt_line_start;
    let bytes = excerpt_content.as_bytes();
    let mut line_idx: u32 = 0;
    let mut i: usize = 0;
    while line_idx < target_excerpt_line && i < bytes.len() {
        if bytes[i] == b'\n' {
            line_idx += 1;
        }
        i += 1;
    }
    if line_idx < target_excerpt_line {
        // Excerpt's stored content is shorter than its declared range —
        // e.g. the source has been truncated and the snapshot stayed
        // longer than the source. Refuse rather than guess.
        return None;
    }
    Some(i + intra_line)
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── LineIndex ────────────────────────────────────────────────────────────

    #[test]
    fn line_index_empty_source_has_zero_lines() {
        let idx = LineIndex::new("");
        assert_eq!(idx.total_lines(), 0);
        assert_eq!(idx.line_start(1), None);
    }

    #[test]
    fn line_index_single_line_no_terminator() {
        let idx = LineIndex::new("hello");
        assert_eq!(idx.total_lines(), 1);
        assert_eq!(idx.line_start(1), Some(0));
        assert_eq!(idx.line_start(2), None);
        assert_eq!(idx.line_content_end(1, "hello"), Some(5));
    }

    #[test]
    fn line_index_single_line_trailing_newline_no_phantom_line() {
        // str::lines() reports 1 line for "hello\n" — matches Vec's
        // behaviour and mirrors slice_lines.
        let idx = LineIndex::new("hello\n");
        assert_eq!(idx.total_lines(), 1);
        assert_eq!(idx.line_start(1), Some(0));
        assert_eq!(idx.line_content_end(1, "hello\n"), Some(5));
    }

    #[test]
    fn line_index_multi_line_lf() {
        let src = "a\nbb\nccc";
        let idx = LineIndex::new(src);
        assert_eq!(idx.total_lines(), 3);
        assert_eq!(idx.line_start(1), Some(0));
        assert_eq!(idx.line_start(2), Some(2));
        assert_eq!(idx.line_start(3), Some(5));
        assert_eq!(idx.line_content_end(1, src), Some(1));
        assert_eq!(idx.line_content_end(2, src), Some(4));
        assert_eq!(idx.line_content_end(3, src), Some(8));
    }

    #[test]
    fn line_index_multi_line_crlf() {
        // "a\r\nbb\r\nccc" — 11 bytes, 3 lines.
        let src = "a\r\nbb\r\nccc";
        let idx = LineIndex::new(src);
        assert_eq!(idx.total_lines(), 3);
        assert_eq!(idx.line_start(1), Some(0));
        assert_eq!(idx.line_start(2), Some(3));
        assert_eq!(idx.line_start(3), Some(7));
        // Line body end strips the '\r' (one byte before the '\n').
        assert_eq!(idx.line_content_end(1, src), Some(1));
        assert_eq!(idx.line_content_end(2, src), Some(5));
        // Line 3 has no terminator; body ends at EOF.
        assert_eq!(idx.line_content_end(3, src), Some(10));
    }

    #[test]
    fn line_index_line_zero_is_invalid() {
        let idx = LineIndex::new("hello");
        assert_eq!(idx.line_start(0), None);
    }

    // ── excerpt_to_source ────────────────────────────────────────────────────

    fn idx(src: &str) -> LineIndex {
        LineIndex::new(src)
    }

    #[test]
    fn excerpt_to_source_single_line_at_start() {
        // Source: 3 lines, excerpt is just line 1.
        let src = "alpha\nbeta\ngamma";
        let excerpt = "alpha";
        let li = idx(src);
        // Offset 0 → source byte 0.
        let p = excerpt_to_source(excerpt, 1, &li, 0).unwrap();
        assert_eq!(p, SourcePos { line: 1, intra_line_byte: 0, source_byte: 0 });
        // Offset 3 → source byte 3 (mid-line).
        let p = excerpt_to_source(excerpt, 1, &li, 3).unwrap();
        assert_eq!(p.source_byte, 3);
        // Offset 5 (end of line) → source byte 5.
        let p = excerpt_to_source(excerpt, 1, &li, 5).unwrap();
        assert_eq!(p.source_byte, 5);
    }

    #[test]
    fn excerpt_to_source_mid_file_single_line() {
        let src = "alpha\nbeta\ngamma";
        // Excerpt = line 2 ("beta"), starting at byte 6.
        let excerpt = "beta";
        let li = idx(src);
        let p = excerpt_to_source(excerpt, 2, &li, 0).unwrap();
        assert_eq!(p.source_byte, 6);
        let p = excerpt_to_source(excerpt, 2, &li, 4).unwrap();
        assert_eq!(p.source_byte, 10);
    }

    #[test]
    fn excerpt_to_source_multi_line_lands_on_correct_source_line() {
        let src = "alpha\nbeta\ngamma\ndelta";
        // Excerpt = lines 2..=3 = "beta\ngamma" (10 bytes).
        let excerpt = "beta\ngamma";
        let li = idx(src);
        // Offset 0 (start of 'beta') → source byte 6.
        assert_eq!(excerpt_to_source(excerpt, 2, &li, 0).unwrap().source_byte, 6);
        // Offset 4 (the '\n' inside the excerpt) → end of source line 2 = byte 10.
        // (The '\n' in the excerpt represents the terminator byte after
        //  line 2's content. We map it to the line-body-end position.)
        let p = excerpt_to_source(excerpt, 2, &li, 4).unwrap();
        assert_eq!(p.line, 2);
        assert_eq!(p.intra_line_byte, 4);
        // Offset 5 (start of 'gamma') → source byte 11.
        let p = excerpt_to_source(excerpt, 2, &li, 5).unwrap();
        assert_eq!(p.source_byte, 11);
        assert_eq!(p.line, 3);
        assert_eq!(p.intra_line_byte, 0);
        // End of excerpt (10) → end of 'gamma' = byte 16.
        let p = excerpt_to_source(excerpt, 2, &li, 10).unwrap();
        assert_eq!(p.source_byte, 16);
    }

    #[test]
    fn excerpt_to_source_crlf_source_maps_through_lf_separator() {
        // Source uses CRLF; excerpt uses LF (per slice_lines).
        let src = "alpha\r\nbeta\r\ngamma";
        let excerpt = "alpha\nbeta";
        let li = idx(src);
        // Excerpt offset 0 → source byte 0.
        assert_eq!(excerpt_to_source(excerpt, 1, &li, 0).unwrap().source_byte, 0);
        // Excerpt offset 5 (the '\n' in the excerpt) → byte 5 = source's '\r'.
        // We map it to intra_line_byte 5 on line 1, which is the line-body-end.
        let p = excerpt_to_source(excerpt, 1, &li, 5).unwrap();
        assert_eq!(p.line, 1);
        assert_eq!(p.intra_line_byte, 5);
        // Excerpt offset 6 ('b' of beta) → source byte 7 (line 2 starts at 7).
        let p = excerpt_to_source(excerpt, 1, &li, 6).unwrap();
        assert_eq!(p.source_byte, 7);
        assert_eq!(p.line, 2);
    }

    #[test]
    fn excerpt_to_source_returns_none_when_offset_past_end() {
        let src = "alpha\nbeta";
        let excerpt = "alpha";
        let li = idx(src);
        assert!(excerpt_to_source(excerpt, 1, &li, 99).is_none());
    }

    #[test]
    fn excerpt_to_source_returns_none_when_line_outside_source() {
        let src = "alpha";
        let excerpt = "ghost";
        let li = idx(src);
        // Excerpt declares line_start=5 but source has only 1 line.
        assert!(excerpt_to_source(excerpt, 5, &li, 0).is_none());
    }

    #[test]
    fn excerpt_to_source_line_zero_invalid() {
        let src = "alpha";
        let excerpt = "alpha";
        let li = idx(src);
        assert!(excerpt_to_source(excerpt, 0, &li, 0).is_none());
    }

    // ── source_to_excerpt ────────────────────────────────────────────────────

    #[test]
    fn source_to_excerpt_round_trip_lf() {
        let src = "alpha\nbeta\ngamma\ndelta";
        let excerpt = "beta\ngamma";
        let li = idx(src);
        for excerpt_off in 0..=excerpt.len() {
            let p = excerpt_to_source(excerpt, 2, &li, excerpt_off).unwrap();
            let back =
                source_to_excerpt(excerpt, 2, 3, src, &li, p.source_byte).unwrap();
            assert_eq!(back, excerpt_off, "round-trip at excerpt offset {excerpt_off}");
        }
    }

    #[test]
    fn source_to_excerpt_returns_none_outside_covered_range() {
        let src = "alpha\nbeta\ngamma\ndelta";
        let excerpt = "beta\ngamma"; // lines 2..=3
        let li = idx(src);
        // Source offset 0 = line 1 → outside.
        assert!(source_to_excerpt(excerpt, 2, 3, src, &li, 0).is_none());
        // Source offset 17 = line 4 ("delta") → outside.
        assert!(source_to_excerpt(excerpt, 2, 3, src, &li, 17).is_none());
    }

    #[test]
    fn source_to_excerpt_handles_crlf_terminator_byte() {
        let src = "alpha\r\nbeta\r\ngamma";
        let excerpt = "alpha\nbeta";
        let li = idx(src);
        // Source byte 6 is the '\r' of CRLF after "alpha".
        // We pin it to line-body-end (byte 5) which round-trips to
        // excerpt offset 5 (the '\n' in the excerpt).
        let back = source_to_excerpt(excerpt, 1, 2, src, &li, 6).unwrap();
        assert_eq!(back, 5);
        // Source byte 7 = start of "beta" → excerpt offset 6.
        let back = source_to_excerpt(excerpt, 1, 2, src, &li, 7).unwrap();
        assert_eq!(back, 6);
    }

    #[test]
    fn source_to_excerpt_invalid_range_returns_none() {
        let src = "alpha\nbeta";
        let excerpt = "alpha";
        let li = idx(src);
        // line_end < line_start.
        assert!(source_to_excerpt(excerpt, 2, 1, src, &li, 0).is_none());
        // line_start == 0.
        assert!(source_to_excerpt(excerpt, 0, 1, src, &li, 0).is_none());
    }

    #[test]
    fn source_to_excerpt_returns_none_when_offset_past_source_end() {
        let src = "alpha";
        let excerpt = "alpha";
        let li = idx(src);
        assert!(source_to_excerpt(excerpt, 1, 1, src, &li, 99).is_none());
    }

    #[test]
    fn source_to_excerpt_refuses_when_snapshot_shorter_than_declared_range() {
        // Excerpt declares lines 1..=3 but its content has only 1 line.
        // Could happen if the source was truncated externally and the
        // snapshot is now inconsistent with `line_end`. We refuse rather
        // than guess.
        let src = "a\nb\nc";
        let excerpt = "a";
        let li = idx(src);
        // Source offset 2 = line 2; outside the excerpt's snapshot.
        assert!(source_to_excerpt(excerpt, 1, 3, src, &li, 2).is_none());
    }
}
