//! URL auto-detection + localhost→loopback resolution (PRD-09 §6).
//!
//! # Scope
//!
//! Takes an ANSI-stripped text line (typically a [`crate::Line::text_only`])
//! and returns every URL match with:
//!
//! - the byte range inside the line where the URL appears,
//! - the raw URL text as printed,
//! - a resolved URL suitable for opening — `localhost:NNN` is rewritten
//!   to `http://127.0.0.1:NNN` because many browsers refuse plain
//!   `localhost:` strings and because the dev-server rewrite is what
//!   users actually want (§6.2).
//!
//! # What this is not
//!
//! - **No storage**: detection is an on-demand query. The PRD explicitly
//!   calls out "Run URL detection on output lines incrementally (not
//!   entire buffer at once)", so callers detect when they want to
//!   render clickable links rather than eagerly caching on every push.
//! - **No UI/click plumbing**: that lives in the frontend and is wired
//!   up via the normal contribution-registry URI handler surface
//!   (PRD-04 `protocol_handlers` + PRD-07 §7.4).
//!
//! # Patterns
//!
//! Three regex families cover the realistic cases:
//!
//! - `https?://…` — the common one.
//! - `file://…` — absolute file URIs (editors, compilers, linters).
//! - `localhost:PORT…` / `127.0.0.1:PORT…` — bare host:port without
//!   scheme, which dev servers (vite, next dev, webpack-dev-server)
//!   print unmodified. We prepend `http://` and rewrite `localhost`
//!   to `127.0.0.1` on resolve.
//!
//! Trailing punctuation (`.`, `,`, `;`, `!`, `?`, `)`, `]`, `}`) is
//! stripped because it's almost always English sentence punctuation, not
//! part of the URL. Balanced parens inside the URL (e.g. Wikipedia URLs
//! with `(disambiguation)`) are not handled — keep the rule simple for
//! v1 and ship a richer parser when the matcher becomes a bottleneck.

use std::sync::OnceLock;

use regex_lite::Regex;

/// Category of a URL match — useful for callers that want to style or
/// filter e.g. local links differently from external ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlKind {
    /// Absolute `http://` or `https://` URL.
    HttpHttps,
    /// `file://` URL — typically editor jump-to-line style references.
    File,
    /// Bare `localhost:PORT` or `127.0.0.1:PORT` (no scheme). Resolved
    /// with a synthesised `http://` prefix.
    Localhost,
}

/// A single URL occurrence inside a text line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlMatch {
    /// Byte offset where the URL starts inside the scanned text.
    pub start: usize,
    /// Byte offset one past the end of the URL inside the scanned text.
    pub end: usize,
    /// URL as it appears in the source text (no normalisation).
    pub raw: String,
    /// URL ready to pass to a browser / OS-level opener.
    /// `localhost:NNN…` is rewritten to `http://127.0.0.1:NNN…`.
    pub resolved: String,
    /// Which family matched.
    pub kind: UrlKind,
}

impl UrlMatch {
    /// Byte length of the URL in the source text.
    #[must_use]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Whether the URL has zero length. Should never be true for
    /// matches returned by [`detect_urls`]; present for contract symmetry.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Trailing ASCII bytes that are almost always sentence punctuation
/// rather than part of a URL. Kept as a byte slice because URLs are
/// ASCII-only at this boundary.
const TRAILING_PUNCTUATION: &[u8] = b".,;:!?)]}>";

/// Rewrite a raw URL into the form a browser / `open` command accepts.
///
/// - `localhost:PORT…` becomes `http://127.0.0.1:PORT…` (§6.2).
/// - `127.0.0.1:PORT…` without scheme gets the `http://` prefix.
/// - Everything else is returned unchanged.
#[must_use]
pub fn resolve_url(raw: &str) -> String {
    if raw.starts_with("http://") || raw.starts_with("https://") || raw.starts_with("file://") {
        return raw.to_string();
    }
    if let Some(rest) = raw.strip_prefix("localhost:") {
        return format!("http://127.0.0.1:{rest}");
    }
    if raw.starts_with("127.0.0.1:") {
        return format!("http://{raw}");
    }
    raw.to_string()
}

/// Scan `text` for URLs and return every match, sorted by start offset.
///
/// Multiple occurrences in the same line all surface. Overlapping matches
/// (e.g. `https://localhost:3000` matches both `https?://` and the
/// `localhost:PORT` patterns) are resolved by preferring the HTTP match
/// — it's strictly more specific and carries the correct scheme.
#[must_use]
pub fn detect_urls(text: &str) -> Vec<UrlMatch> {
    let mut hits: Vec<UrlMatch> = Vec::new();

    for m in http_regex().find_iter(text) {
        hits.push(make_match(text, m.start(), m.end(), UrlKind::HttpHttps));
    }
    for m in file_regex().find_iter(text) {
        hits.push(make_match(text, m.start(), m.end(), UrlKind::File));
    }
    // Suppress `localhost:` hits that overlap an already-captured HTTP
    // match (`https://localhost:3000` should surface as one HttpHttps
    // entry, not as HTTP + Localhost).
    for m in localhost_regex().find_iter(text) {
        let overlaps = hits
            .iter()
            .any(|existing| ranges_overlap(existing.start, existing.end, m.start(), m.end()));
        if !overlaps {
            hits.push(make_match(text, m.start(), m.end(), UrlKind::Localhost));
        }
    }

    hits.sort_by_key(|u| u.start);
    hits
}

fn ranges_overlap(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> bool {
    a_start < b_end && b_start < a_end
}

fn make_match(text: &str, start: usize, end: usize, kind: UrlKind) -> UrlMatch {
    // Strip trailing punctuation. Walk bytes from the end because the
    // regex captured an ASCII-only greedy span.
    let mut trimmed_end = end;
    let bytes = text.as_bytes();
    while trimmed_end > start {
        let last = bytes[trimmed_end - 1];
        if TRAILING_PUNCTUATION.contains(&last) {
            trimmed_end -= 1;
        } else {
            break;
        }
    }
    let raw = text[start..trimmed_end].to_string();
    let resolved = resolve_url(&raw);
    UrlMatch {
        start,
        end: trimmed_end,
        raw,
        resolved,
        kind,
    }
}

// Lazy-compiled regex singletons. `regex-lite` doesn't support backrefs
// or fancy features but is ASCII-fast and has no Unicode-table weight,
// which is exactly what we want here.

fn http_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // No backslash-brace class inside the char class — regex-lite
        // doesn't coerce `{}` like PCRE. Enumerate the terminators.
        Regex::new(r#"https?://[^\s()\[\]<>"']+"#).expect("http regex compiles")
    })
}

fn file_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"file://[^\s()\[\]<>"']+"#).expect("file regex compiles"))
}

fn localhost_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Word boundary up front so we don't match in the middle of
        // words like "nolocalhost:1234". Port is required — without it
        // `localhost` alone is too noisy.
        Regex::new(r#"\b(?:localhost|127\.0\.0\.1):\d+[^\s()\[\]<>"']*"#)
            .expect("localhost regex compiles")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_produces_no_matches() {
        assert!(detect_urls("just some words, no urls here").is_empty());
    }

    #[test]
    fn single_https_url_is_detected() {
        let hits = detect_urls("see https://example.com for details");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].raw, "https://example.com");
        assert_eq!(hits[0].kind, UrlKind::HttpHttps);
        assert_eq!(hits[0].resolved, "https://example.com");
    }

    #[test]
    fn trailing_sentence_punctuation_is_stripped() {
        let hits = detect_urls("go to https://example.com.");
        assert_eq!(hits[0].raw, "https://example.com");
    }

    #[test]
    fn trailing_paren_and_comma_stripped() {
        let hits = detect_urls("see (https://example.com), etc");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].raw, "https://example.com");
    }

    #[test]
    fn multiple_urls_surface_in_order() {
        let hits = detect_urls("first https://a.example then https://b.example end");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].raw, "https://a.example");
        assert_eq!(hits[1].raw, "https://b.example");
        assert!(hits[0].start < hits[1].start);
    }

    #[test]
    fn localhost_bare_is_detected_and_resolved_to_loopback() {
        let hits = detect_urls("Server listening at localhost:3000");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].raw, "localhost:3000");
        assert_eq!(hits[0].kind, UrlKind::Localhost);
        assert_eq!(hits[0].resolved, "http://127.0.0.1:3000");
    }

    #[test]
    fn localhost_with_path_keeps_path_in_resolved() {
        let hits = detect_urls("dashboard at localhost:8080/admin");
        assert_eq!(hits[0].resolved, "http://127.0.0.1:8080/admin");
    }

    #[test]
    fn loopback_ip_without_scheme_gets_http_prefix() {
        let hits = detect_urls("hit 127.0.0.1:5173 for the dev server");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, UrlKind::Localhost);
        assert_eq!(hits[0].resolved, "http://127.0.0.1:5173");
    }

    #[test]
    fn http_localhost_does_not_double_match() {
        // `https://localhost:3000` would match both http and localhost
        // regexes. Make sure only one match surfaces.
        let hits = detect_urls("serving https://localhost:3000/api");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].raw, "https://localhost:3000/api");
        assert_eq!(hits[0].kind, UrlKind::HttpHttps);
        // resolve leaves an HTTPS URL alone — we only rewrite bare
        // localhost:, not already-schemed URLs.
        assert_eq!(hits[0].resolved, "https://localhost:3000/api");
    }

    #[test]
    fn file_uri_is_detected() {
        let hits = detect_urls("open file:///home/user/app.js:42 to fix");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, UrlKind::File);
        assert_eq!(hits[0].raw, "file:///home/user/app.js:42");
        assert_eq!(hits[0].resolved, "file:///home/user/app.js:42");
    }

    #[test]
    fn url_with_query_string_and_fragment_survives() {
        let hits = detect_urls("go https://example.com/path?x=1&y=2#frag now");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].raw, "https://example.com/path?x=1&y=2#frag");
    }

    #[test]
    fn angle_bracketed_url_body_is_captured_bracket_stripped() {
        let hits = detect_urls("email <https://example.com>");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].raw, "https://example.com");
    }

    #[test]
    fn localhost_inside_word_is_not_matched() {
        // Word-boundary prefix should reject "nolocalhost:1234".
        assert!(detect_urls("check nolocalhost:1234 for noise").is_empty());
    }

    #[test]
    fn bare_localhost_without_port_is_not_matched() {
        // Without a port this is too noisy; `localhost` alone in prose
        // shouldn't surface as a URL.
        assert!(detect_urls("bind localhost, then serve").is_empty());
    }

    #[test]
    fn resolve_leaves_absolute_urls_unchanged() {
        assert_eq!(resolve_url("https://example.com"), "https://example.com");
        assert_eq!(resolve_url("http://example.com"), "http://example.com");
        assert_eq!(resolve_url("file:///etc/hosts"), "file:///etc/hosts");
    }

    #[test]
    fn resolve_rewrites_bare_localhost_and_loopback() {
        assert_eq!(
            resolve_url("localhost:3000/api"),
            "http://127.0.0.1:3000/api"
        );
        assert_eq!(resolve_url("127.0.0.1:8080"), "http://127.0.0.1:8080");
    }

    #[test]
    fn url_match_len_and_is_empty_work() {
        let m = UrlMatch {
            start: 5,
            end: 20,
            raw: "https://abc.com".into(),
            resolved: "https://abc.com".into(),
            kind: UrlKind::HttpHttps,
        };
        assert_eq!(m.len(), 15);
        assert!(!m.is_empty());
    }
}
