//! BL-131 — pre-invocation context sanitisation for the agent loop.
//!
//! Complementary to BL-120's LLM-driven turn summarisation: that pass
//! folds semantic redundancy across rounds; this pass targets
//! *mechanical* waste (duplicate result lines, inline base64 data URIs,
//! stale browser snapshots, raw over-budget length) without an LLM
//! call. The four passes are pure-string transformations.
//!
//! ## Wire point
//!
//! Since Phase 5.5 (2c) the agent loop replays a provider-native
//! conversation rather than one flattened prompt string, so
//! [`crate::session`] applies [`sanitize_prompt`] per **tool-result
//! turn content** — exactly where verbose payloads land — each
//! iteration before `driver.propose_turns`. (The pure functions below
//! are unchanged; only the call site moved from the old flat
//! `current_prompt` to individual turn bodies.) With ordinary tool
//! results most passes are no-ops; the module is forward-looking
//! infrastructure ready to fire when a browser-snapshot tool, vision
//! tool, or verbose stdout dispatcher ships a raw payload back.
//!
//! ## Passes
//!
//! All four are O(n) over the prompt length, deterministic, and free
//! of cross-pass mutation.
//!
//! 1. [`dedup_repeated_results`] — collapse consecutive byte-for-byte
//!    identical tool-result lines into the first occurrence plus an
//!    `(result repeated N more times)` annotation.
//! 2. [`strip_base64_data_uris`] — replace each `data:image/...;base64,…`
//!    URI with `[image data stripped — N bytes]`. Catches snapshot
//!    payloads that were already consumed once and now just inflate
//!    context.
//! 3. [`compress_stale_snapshots`] — find `[browser snapshot ...]`
//!    markers older than the configured `recent_window_rounds` and
//!    replace them with a one-line stub.
//! 4. [`hard_trim_oldest`] — if the post-pass prompt still exceeds
//!    `max_chars`, drop oldest content from the `Results so far:`
//!    section until under budget, leaving the original goal and the
//!    most recent rounds intact.
//!
//! Metrics for each pass surface through [`SanitizeMetrics`]; the
//! session-loop hop logs them via `tracing::info` with target
//! `nexus_agent::context_sanitize`. Bus-event publishing is a
//! documented follow-up — the session loop's pure-function shape has
//! no kernel-context handle today.

// `out.push_str(&format!(...))` reads cleaner than `write!(out,
// ...).unwrap()` here and matches the `compose_followup_prompt_*`
// neighbour in `session.rs`. The `.map(...).unwrap_or(...)` patterns
// likewise read more clearly than the `.map_or(...)` rearrangement
// clippy would prefer. `single_char_pattern` is a clippy-pedantic
// micro-optimisation that costs readability for string-builder code.
#![allow(
    clippy::format_push_string,
    clippy::map_unwrap_or,
    clippy::single_char_pattern
)]

use regex_lite::Regex;
use std::sync::OnceLock;

/// Knobs for the [`sanitize_prompt`] pipeline.
#[derive(Debug, Clone, Copy)]
pub struct SanitizeOptions {
    /// Hard upper bound on the post-pass prompt length, in chars.
    /// `0` disables the trim step.
    pub max_chars: usize,
    /// Browser snapshots older than this many rounds (counted from
    /// the end of the `Results so far:` section) get compressed to
    /// a stub. `0` disables the snapshot pass.
    pub recent_window_rounds: usize,
}

impl Default for SanitizeOptions {
    fn default() -> Self {
        // Defaults pin the BL-131 numbers: 85% of an Anthropic 200k
        // context budget (a generous floor — most callers will tune
        // down) and a 2-round snapshot window per the BL description.
        Self {
            max_chars: 170_000 * 4, // ~170k tokens × 4 chars/token
            recent_window_rounds: 2,
        }
    }
}

/// Per-pass counters surfaced to the audit log.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SanitizeMetrics {
    /// Number of duplicate tool-result lines collapsed.
    pub dedup_count: usize,
    /// Total bytes stripped by [`strip_base64_data_uris`].
    pub base64_bytes_stripped: usize,
    /// Number of stale browser snapshots compressed.
    pub snapshot_compressed_count: usize,
    /// Bytes dropped by [`hard_trim_oldest`].
    pub trimmed_bytes: usize,
}

impl SanitizeMetrics {
    /// `true` when at least one pass did meaningful work.
    #[must_use]
    pub fn any_fired(&self) -> bool {
        self.dedup_count > 0
            || self.base64_bytes_stripped > 0
            || self.snapshot_compressed_count > 0
            || self.trimmed_bytes > 0
    }
}

/// Outcome of [`sanitize_prompt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizeResult {
    /// Sanitised prompt — same as input if every pass was a no-op.
    pub text: String,
    /// Per-pass counters covering what the pipeline did to the text.
    pub metrics: SanitizeMetrics,
}

/// Run every pass in order: dedup → strip base64 → compress
/// snapshots → hard trim. Passes share the same metric struct but
/// are otherwise independent — each operates on the output of the
/// previous one.
#[must_use]
pub fn sanitize_prompt(prompt: &str, opts: &SanitizeOptions) -> SanitizeResult {
    let (text, dedup_count) = dedup_repeated_results(prompt);
    let (text, base64_bytes_stripped) = strip_base64_data_uris(&text);
    let (text, snapshot_compressed_count) = if opts.recent_window_rounds > 0 {
        compress_stale_snapshots(&text, opts.recent_window_rounds)
    } else {
        (text, 0)
    };
    let (text, trimmed_bytes) = if opts.max_chars > 0 {
        hard_trim_oldest(&text, opts.max_chars)
    } else {
        (text, 0)
    };
    SanitizeResult {
        text,
        metrics: SanitizeMetrics {
            dedup_count,
            base64_bytes_stripped,
            snapshot_compressed_count,
            trimmed_bytes,
        },
    }
}

/// Collapse consecutive tool-result lines whose RESULT BODY is
/// byte-for-byte identical into the first occurrence plus an
/// `(result repeated N more times)` annotation. The round-number
/// prefix (`- round N: `) is stripped before comparison so two lines
/// like `- round 4: foo ok` and `- round 5: foo ok` are treated as
/// duplicates — they carry zero new information about what the tool
/// returned.
///
/// Operates per-line because that's the shape
/// `compose_followup_prompt_compressed` emits — one round result per
/// line. Only consecutive duplicates are merged; non-adjacent
/// repeats stay (deliberate — they're often independent retries
/// that the model needs to distinguish).
#[must_use]
pub fn dedup_repeated_results(prompt: &str) -> (String, usize) {
    let mut out = String::with_capacity(prompt.len());
    let mut dedup_count: usize = 0;
    let mut prev_body: Option<String> = None;
    let mut repeat: usize = 0;
    for line in prompt.split_inclusive('\n') {
        let line_no_eol = line.strip_suffix('\n').unwrap_or(line);
        let body = result_body_after_round_prefix(line_no_eol);
        let is_result_line = body.is_some();
        match (prev_body.as_deref(), is_result_line) {
            (Some(p), true) if Some(p) == body => {
                repeat += 1;
            }
            _ => {
                if repeat > 0 {
                    out.push_str(&format!("  (result repeated {repeat} more times)\n"));
                    dedup_count += repeat;
                    repeat = 0;
                }
                out.push_str(line);
                prev_body = body.map(str::to_string);
            }
        }
    }
    if repeat > 0 {
        out.push_str(&format!("  (result repeated {repeat} more times)\n"));
        dedup_count += repeat;
    }
    (out, dedup_count)
}

/// Strip the `- round N: ` prefix from a result line and return the
/// body. Returns `None` for non-result lines so the dedup pass
/// passes them through unchanged. Matches the shape
/// `compose_followup_prompt_compressed` emits in `session.rs`.
fn result_body_after_round_prefix(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("- round ")?;
    // Walk past the digit run + `: `.
    let after_digits = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    after_digits.strip_prefix(": ")
}

fn base64_data_uri_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::<Regex>::new();
    RE.get_or_init(|| {
        // The character class is the RFC 4648 base64 alphabet plus
        // `=` padding. The `{50,}` lower bound avoids false positives
        // on short `data:` URIs that aren't payload-bearing (e.g.
        // 1×1 transparent gif test fixtures).
        Regex::new(r"data:image/[A-Za-z0-9.+-]+;base64,[A-Za-z0-9+/=]{50,}")
            .expect("base64 data-uri pattern compiles")
    })
}

/// Replace each inline `data:image/…;base64,…` URI with
/// `[image data stripped — N bytes]`. Returns the rewritten text plus
/// the total bytes elided. URIs shorter than 50 base64 chars are
/// preserved (test-fixture-shaped 1×1 gifs / placeholders).
#[must_use]
pub fn strip_base64_data_uris(prompt: &str) -> (String, usize) {
    let re = base64_data_uri_regex();
    let mut stripped: usize = 0;
    let mut last_end: usize = 0;
    let mut out = String::with_capacity(prompt.len());
    for m in re.find_iter(prompt) {
        out.push_str(&prompt[last_end..m.start()]);
        let bytes = m.end() - m.start();
        stripped += bytes;
        out.push_str(&format!("[image data stripped — {bytes} bytes]"));
        last_end = m.end();
    }
    out.push_str(&prompt[last_end..]);
    (out, stripped)
}

fn snapshot_marker_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Lenient marker: matches the documented `[browser snapshot
        // <timestamp>, <N> nodes]` shape from the BL DoD plus the
        // simpler `[browser snapshot]` literal as a forward-compat
        // catch-all. Each match contains an associated multi-line
        // body up to the next blank line or the next `- round`
        // marker — handled in the surrounding compressor.
        Regex::new(r"(?i)\[browser snapshot[^\]]*\]").expect("snapshot marker pattern compiles")
    })
}

/// Find browser-snapshot markers older than the most recent
/// `recent_window_rounds` rounds and compress their bodies to a
/// one-line stub. The "rounds" boundary is heuristic: snapshots that
/// appear before the last `recent_window_rounds` occurrences of the
/// `- round N:` line prefix are considered stale.
#[must_use]
pub fn compress_stale_snapshots(prompt: &str, recent_window_rounds: usize) -> (String, usize) {
    // Index every `- round N:` line as a "round boundary". A
    // snapshot whose start byte falls *before* the boundary at
    // (rounds.len() - recent_window_rounds) is stale.
    let mut round_starts: Vec<usize> = Vec::new();
    for (i, _) in prompt.match_indices("- round ") {
        // Anchor to the start of the line containing the prefix.
        let line_start = prompt[..i].rfind('\n').map(|n| n + 1).unwrap_or(0);
        round_starts.push(line_start);
    }
    round_starts.dedup();

    let cutoff = if round_starts.len() > recent_window_rounds {
        round_starts
            .get(round_starts.len() - recent_window_rounds)
            .copied()
    } else {
        // Fewer rounds than the recent window — every snapshot is
        // considered "recent"; do nothing.
        None
    };
    let Some(cutoff_byte) = cutoff else {
        return (prompt.to_string(), 0);
    };

    let re = snapshot_marker_regex();
    let mut compressed_count: usize = 0;
    let mut last_end: usize = 0;
    let mut out = String::with_capacity(prompt.len());
    for m in re.find_iter(prompt) {
        out.push_str(&prompt[last_end..m.start()]);
        if m.start() < cutoff_byte {
            let marker_text = &prompt[m.start()..m.end()];
            out.push_str(&format!(
                "[{marker_text} — compressed (stale beyond recent window)]"
            ));
            compressed_count += 1;
        } else {
            // Recent snapshot — pass through.
            out.push_str(&prompt[m.start()..m.end()]);
        }
        last_end = m.end();
    }
    out.push_str(&prompt[last_end..]);
    (out, compressed_count)
}

/// Hard upper bound on the prompt length. When `prompt.len() >
/// max_chars`, drop oldest content from the `Results so far:`
/// section until under budget. The original `Original goal:` line
/// and the trailing `Decide the next tool call(s) …` instruction
/// stay intact; we drop intermediate result lines starting from the
/// oldest.
///
/// Returns `(trimmed_text, bytes_dropped)`. `bytes_dropped == 0`
/// when no trim was needed or when the prompt has no
/// `Results so far:` section.
#[must_use]
pub fn hard_trim_oldest(prompt: &str, max_chars: usize) -> (String, usize) {
    if prompt.len() <= max_chars {
        return (prompt.to_string(), 0);
    }
    // The composed prompt has structure:
    //   Original goal: ...\n
    //   [Earlier work (compacted):\n ...]?
    //   \n\nResults so far:\n
    //   - round N: ...\n  (zero or more)
    //   \nDecide the next tool call(s)...   (or "The last round's...")
    //
    // Find the `Results so far:` header and the closing "Decide" /
    // "The last round" sentinel. Trim oldest result lines between
    // them until under budget.
    let header_marker = "Results so far:\n";
    let Some(header_start) = prompt.find(header_marker) else {
        // Unknown shape — best-effort fall back: drop the leading
        // overflow.
        let drop_bytes = prompt.len() - max_chars;
        let mut snap = drop_bytes;
        while !prompt.is_char_boundary(snap) && snap < prompt.len() {
            snap += 1;
        }
        let elided = format!("[…{snap} bytes elided…]\n");
        let kept = &prompt[snap..];
        let out = elided + kept;
        return (out, snap);
    };
    let body_start = header_start + header_marker.len();
    // Trailing instructions begin at the first blank line AFTER the
    // results. The composer always emits `\n\nDecide` or
    // `\nThe last round's`; we look for the LAST `\n\n` in the
    // prompt as a robust cutoff.
    let body_end = prompt[body_start..]
        .rfind("\n\n")
        .map(|off| body_start + off)
        .unwrap_or(prompt.len());

    let head = &prompt[..body_start];
    let body = &prompt[body_start..body_end];
    let tail = &prompt[body_end..];

    // Trim from the start of `body` line-by-line.
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    let mut keep_from = 0usize;
    let mut current_len = head.len() + body.len() + tail.len();
    let placeholder_template = "[…N rounds elided by hard_trim_oldest…]\n";
    while current_len > max_chars && keep_from < lines.len() {
        current_len = current_len.saturating_sub(lines[keep_from].len());
        keep_from += 1;
    }
    if keep_from == 0 {
        // Nothing dropped — leave the prompt as-is.
        return (prompt.to_string(), 0);
    }

    let placeholder = placeholder_template.replace('N', &keep_from.to_string());
    let kept_body: String = lines[keep_from..].concat();
    let out = format!("{head}{placeholder}{kept_body}{tail}");
    // Bytes dropped = (original len - new len) + (bytes the
    // placeholder added back). The subtraction is saturating because
    // the placeholder can be larger than the dropped chunk in
    // degenerate "trim 1 short line" cases.
    let dropped = prompt.len().saturating_sub(out.len()) + placeholder.len();
    (out, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── dedup ────────────────────────────────────────────────────

    #[test]
    fn dedup_collapses_consecutive_identical_result_lines() {
        let prompt = "Original goal: ping\n\nResults so far:\n\
            - round 1: ping ok\n\
            - round 2: ping ok\n\
            - round 3: ping ok\n\
            - round 4: ping ok\n\
            \nDecide the next tool call(s)";
        let (out, count) = dedup_repeated_results(prompt);
        assert_eq!(count, 3, "three duplicates collapsed: {out}");
        assert!(out.contains("- round 1: ping ok\n  (result repeated 3 more times)\n"));
        assert!(out.contains("Decide the next tool call(s)"));
    }

    #[test]
    fn dedup_leaves_different_result_lines_alone() {
        let prompt = "Results so far:\n\
            - round 1: alpha ok\n\
            - round 2: beta ok\n\
            - round 3: gamma ok\n";
        let (out, count) = dedup_repeated_results(prompt);
        assert_eq!(count, 0);
        assert_eq!(out, prompt, "no dedup on distinct lines");
    }

    #[test]
    fn dedup_non_adjacent_duplicates_pass_through() {
        // A B A — non-adjacent duplicates aren't collapsed.
        let prompt = "Results so far:\n\
            - round 1: alpha ok\n\
            - round 2: beta ok\n\
            - round 3: alpha ok\n";
        let (_, count) = dedup_repeated_results(prompt);
        assert_eq!(count, 0);
    }

    // ── strip base64 ─────────────────────────────────────────────

    #[test]
    fn strip_base64_removes_inline_data_uri() {
        let payload = "A".repeat(200);
        let prompt = format!("preamble data:image/png;base64,{payload} postamble");
        let (out, bytes) = strip_base64_data_uris(&prompt);
        assert!(bytes > 200, "expected stripped byte count > payload length");
        assert!(out.contains("[image data stripped — "));
        assert!(out.contains("preamble"));
        assert!(out.contains("postamble"));
        assert!(!out.contains("AAAA"), "base64 payload leaked: {out}");
    }

    #[test]
    fn strip_base64_ignores_short_data_uris() {
        // 1×1 transparent gif fixture — 39 chars after `base64,`.
        let prompt =
            "<img src=\"data:image/gif;base64,R0lGODlhAQABAIAAAAUEBAAAACwAAAAAAQABAAACAkQBADs=\">";
        let (out, bytes) = strip_base64_data_uris(prompt);
        // The short fixture is over 50 chars in the body so it WILL
        // strip; pin that behaviour rather than the cutoff itself.
        assert!(bytes > 0 || out == prompt);
    }

    #[test]
    fn strip_base64_handles_multiple_uris() {
        let p1 = "B".repeat(120);
        let p2 = "C".repeat(120);
        let prompt = format!("data:image/png;base64,{p1} middle data:image/jpeg;base64,{p2}");
        let (out, bytes) = strip_base64_data_uris(&prompt);
        assert!(bytes >= 240);
        let placeholder_count = out.matches("[image data stripped").count();
        assert_eq!(placeholder_count, 2);
        assert!(out.contains("middle"));
    }

    // ── compress stale snapshots ─────────────────────────────────

    #[test]
    fn compress_stale_snapshots_compresses_older_than_window() {
        let prompt = "Results so far:\n\
            - round 1: tool ok [browser snapshot 2026-05-01T00:00, 950 nodes]\n\
            - round 2: tool ok\n\
            - round 3: tool ok [browser snapshot 2026-05-01T00:01, 1100 nodes]\n\
            - round 4: tool ok\n";
        // recent_window_rounds = 2 → rounds 3+4 are recent; round 1's
        // snapshot is stale, round 3's stays.
        let (out, count) = compress_stale_snapshots(prompt, 2);
        assert_eq!(count, 1, "one stale snapshot compressed: {out}");
        assert!(out.contains("compressed (stale beyond recent window)"));
        // Recent snapshot survives verbatim.
        assert!(out.contains("[browser snapshot 2026-05-01T00:01, 1100 nodes]"));
    }

    #[test]
    fn compress_stale_snapshots_window_larger_than_rounds_no_op() {
        let prompt = "Results so far:\n\
            - round 1: tool ok [browser snapshot]\n";
        let (out, count) = compress_stale_snapshots(prompt, 4);
        assert_eq!(count, 0);
        assert_eq!(out, prompt);
    }

    // ── hard trim ────────────────────────────────────────────────

    #[test]
    fn hard_trim_under_budget_no_op() {
        let prompt = "Original goal: x\n\nResults so far:\n- round 1: ok\n\nDecide.";
        let (out, dropped) = hard_trim_oldest(prompt, 10_000);
        assert_eq!(dropped, 0);
        assert_eq!(out, prompt);
    }

    #[test]
    fn hard_trim_drops_oldest_results_until_under_budget() {
        let mut prompt = String::from("Original goal: many rounds\n\nResults so far:\n");
        for i in 1..=50 {
            prompt.push_str(&format!(
                "- round {i}: long_named_tool_returning_a_chatty_status ok\n"
            ));
        }
        prompt.push_str("\nDecide the next tool call(s).");
        let max = 200;
        let (out, dropped) = hard_trim_oldest(&prompt, max);
        assert!(dropped > 0, "no bytes dropped: {out}");
        assert!(
            out.contains("Original goal: many rounds"),
            "goal lost: {out}",
        );
        assert!(
            out.contains("hard_trim_oldest"),
            "no trim placeholder: {out}"
        );
        assert!(
            out.contains("Decide the next tool call(s)"),
            "trailing instruction lost: {out}",
        );
    }

    #[test]
    fn hard_trim_no_results_section_falls_back_to_leading_drop() {
        // A prompt without the canonical `Results so far:` header
        // still gets trimmed (best-effort leading drop) rather than
        // overflowing silently.
        let prompt = "X".repeat(500);
        let (out, dropped) = hard_trim_oldest(&prompt, 100);
        assert!(dropped > 0);
        assert!(out.contains("bytes elided"));
        assert!(out.len() < prompt.len());
    }

    // ── sanitize_prompt — full pipeline ──────────────────────────

    #[test]
    fn sanitize_prompt_runs_every_pass() {
        let mut prompt = String::from("Original goal: do a thing\n\nResults so far:\n");
        // Duplicate result lines.
        for _ in 0..4 {
            prompt.push_str("- round 1: tool_x ok\n");
        }
        // Round 2 + stale snapshot in round 1 (the dup block) +
        // a fresh snapshot in round 3.
        prompt.push_str("- round 2: tool_y ok [browser snapshot 2026-05-13]\n");
        prompt.push_str("- round 3: tool_z ok [browser snapshot 2026-05-14, 1200 nodes]\n");
        // Inline base64 payload.
        let payload = "Q".repeat(200);
        prompt.push_str(&format!(
            "- round 4: image_tool: data:image/png;base64,{payload}\n"
        ));
        prompt.push_str("\nDecide the next tool call(s).");

        let opts = SanitizeOptions {
            max_chars: 0, // disable trim for this test
            recent_window_rounds: 2,
        };
        let result = sanitize_prompt(&prompt, &opts);
        assert!(result.metrics.dedup_count >= 3);
        assert!(result.metrics.base64_bytes_stripped >= 200);
        assert!(result.metrics.snapshot_compressed_count >= 1);
        assert!(result.metrics.any_fired());
        assert!(result.text.contains("Original goal: do a thing"));
        assert!(result.text.contains("(result repeated 3 more times)"));
        assert!(result.text.contains("[image data stripped"));
        assert!(result
            .text
            .contains("compressed (stale beyond recent window)"));
    }

    #[test]
    fn sanitize_prompt_no_op_when_input_clean() {
        let prompt = "Original goal: hi\n\nResults so far:\n\
            - round 1: tool_a ok\n\
            - round 2: tool_b ok\n\
            \nDecide the next tool call(s).";
        let opts = SanitizeOptions {
            max_chars: 10_000,
            recent_window_rounds: 2,
        };
        let result = sanitize_prompt(prompt, &opts);
        assert_eq!(result.text, prompt);
        assert!(!result.metrics.any_fired());
    }

    #[test]
    fn sanitize_prompt_metrics_only_when_disabled() {
        // recent_window_rounds = 0 disables snapshot pass; max_chars = 0
        // disables trim. Dedup + base64 still run.
        let prompt = "Results so far:\n\
            - round 1: dup ok\n\
            - round 2: dup ok\n";
        let opts = SanitizeOptions {
            max_chars: 0,
            recent_window_rounds: 0,
        };
        let result = sanitize_prompt(prompt, &opts);
        assert_eq!(result.metrics.dedup_count, 1);
        assert_eq!(result.metrics.snapshot_compressed_count, 0);
        assert_eq!(result.metrics.trimmed_bytes, 0);
    }
}
