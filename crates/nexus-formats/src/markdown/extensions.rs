//! Nexus-specific markdown extension detectors.
//!
//! - Inline `#tags`
//! - Callout / admonition blocks (`[!TYPE]`)
//! - Block-reference anchors (` ^id` suffix)
//! - Math spans (`$...$` inline and `$$...$$` block)

// ── Public types ──────────────────────────────────────────────────────────────

/// Where a tag was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagSource {
    /// Listed in YAML frontmatter `tags:` key.
    Frontmatter,
    /// Found inline as `#tag` in body text.
    Inline,
}

/// A tag reference (with or without the leading `#`).
#[derive(Debug, Clone)]
pub struct Tag {
    /// Tag name without the `#` prefix.
    pub name: String,
    /// Where the tag came from.
    pub source: TagSource,
}

/// A math expression span.
#[derive(Debug, Clone)]
pub struct MathSpan {
    /// `true` for `$$block$$`, `false` for `$inline$`.
    pub display: bool,
    /// LaTeX source content (without delimiters).
    pub content: String,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Scan `text` for inline `#tag` patterns and append results to `tags`.
///
/// A tag starts with `#` that is at position 0 or immediately after whitespace,
/// followed by at least one `[a-zA-Z0-9_/-]` character.
pub fn extract_inline_tags(text: &str, tags: &mut Vec<Tag>) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '#' {
            let preceded_by_ws = i == 0 || chars[i - 1].is_whitespace();
            if preceded_by_ws && i + 1 < len && is_tag_char(chars[i + 1]) {
                let start = i + 1;
                let mut end = start;
                while end < len && is_tag_char(chars[end]) {
                    end += 1;
                }
                let name: String = chars[start..end].iter().collect();
                if !tags
                    .iter()
                    .any(|t| t.name == name && t.source == TagSource::Inline)
                {
                    tags.push(Tag {
                        name,
                        source: TagSource::Inline,
                    });
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
}

/// Detect a callout / admonition prefix in blockquote text.
///
/// If `text` starts with `[!TYPE]` (TYPE is ASCII alphabetic), returns
/// `("callout", Some(lowercase_type), remainder)`.
/// Otherwise returns `("blockquote", None, original_text)`.
#[must_use]
pub fn detect_callout(text: &str) -> (&'static str, Option<String>, String) {
    let trimmed = text.trim_start();
    if let Some(rest) = trimmed.strip_prefix("[!") {
        if let Some(close) = rest.find(']') {
            let callout_type_raw = &rest[..close];
            if !callout_type_raw.is_empty()
                && callout_type_raw.chars().all(|c| c.is_ascii_alphabetic())
            {
                let after = rest[close + 1..].trim().to_string();
                return (
                    "callout",
                    Some(callout_type_raw.to_ascii_lowercase()),
                    after,
                );
            }
        }
    }
    ("blockquote", None, text.to_string())
}

/// Detect a trailing block-reference anchor (` ^some-id`) at the end of text.
///
/// Returns `(cleaned_text, Some(id))` when found, or `(original, None)`.
/// The id must be `[a-zA-Z0-9_-]+`.
#[must_use]
pub fn extract_block_ref(content: &str) -> (String, Option<String>) {
    if let Some(pos) = content.rfind(" ^") {
        let candidate = &content[pos + 2..];
        if !candidate.is_empty()
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return (content[..pos].to_string(), Some(candidate.to_string()));
        }
    }
    (content.to_string(), None)
}

/// Extract all math spans from `text`.
///
/// Block math `$$...$$` is scanned first; remaining text is searched for
/// inline `$...$`. A `$` is only treated as an inline-math delimiter when:
/// - The character immediately after the opening `$` is not a space.
/// - The character immediately before the closing `$` is not a space.
#[must_use]
pub fn extract_math_spans(text: &str) -> Vec<MathSpan> {
    let mut spans = Vec::new();
    // Track which ranges are already consumed to avoid double-matching.
    let mut consumed: Vec<(usize, usize)> = Vec::new();

    // ── Block math $$...$$
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i + 1 < len {
        if bytes[i] == b'$' && bytes[i + 1] == b'$' {
            let content_start = i + 2;
            if let Some(rel) = text[content_start..].find("$$") {
                let end = content_start + rel;
                let content = text[content_start..end].to_string();
                consumed.push((i, end + 2));
                spans.push(MathSpan {
                    display: true,
                    content,
                });
                i = end + 2;
                continue;
            }
        }
        i += 1;
    }

    // ── Inline math $...$
    let mut i = 0;
    while i < len {
        if bytes[i] == b'$' {
            // Not already inside a block-math span.
            if consumed.iter().any(|&(s, e)| i >= s && i < e) {
                i += 1;
                continue;
            }
            // Character after `$` must not be a space.
            if i + 1 < len && bytes[i + 1] != b' ' {
                let content_start = i + 1;
                // Find closing `$` that is not preceded by a space.
                if let Some(rel) = text[content_start..].find('$') {
                    let close_pos = content_start + rel;
                    if close_pos > content_start
                        && bytes[close_pos - 1] != b' '
                        // Ensure the closing `$` is not the start of `$$`.
                        && !(close_pos + 1 < len && bytes[close_pos + 1] == b'$')
                    {
                        let content = text[content_start..close_pos].to_string();
                        spans.push(MathSpan {
                            display: false,
                            content,
                        });
                        i = close_pos + 1;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }

    spans
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn is_tag_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '/' || c == '-'
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Tags ──────────────────────────────────────────────────────────────

    #[test]
    fn inline_tags_basic() {
        let mut tags = Vec::new();
        extract_inline_tags("Hello #rust and #programming", &mut tags);
        assert!(tags.iter().any(|t| t.name == "rust"));
        assert!(tags.iter().any(|t| t.name == "programming"));
    }

    #[test]
    fn inline_tag_at_start_of_string() {
        let mut tags = Vec::new();
        extract_inline_tags("#nexus is great", &mut tags);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "nexus");
    }

    #[test]
    fn hash_in_url_not_a_tag() {
        // The `#` in `https://example.com#anchor` is preceded by a non-ws char.
        let mut tags = Vec::new();
        extract_inline_tags("Visit https://example.com#anchor today", &mut tags);
        assert!(tags.is_empty());
    }

    #[test]
    fn no_duplicate_inline_tags() {
        let mut tags = Vec::new();
        extract_inline_tags("#rust #rust #rust", &mut tags);
        assert_eq!(tags.len(), 1);
    }

    // ── Callouts ─────────────────────────────────────────────────────────

    #[test]
    fn callout_with_title() {
        let (btype, ctype, _) = detect_callout("[!warning] Be careful");
        assert_eq!(btype, "callout");
        assert_eq!(ctype.as_deref(), Some("warning"));
    }

    #[test]
    fn callout_case_insensitive_lowercased() {
        let (_, ctype, _) = detect_callout("[!WARNING]");
        assert_eq!(ctype.as_deref(), Some("warning"));
    }

    #[test]
    fn regular_blockquote_not_callout() {
        let (btype, ctype, _) = detect_callout("Just a regular quote");
        assert_eq!(btype, "blockquote");
        assert!(ctype.is_none());
    }

    // ── Block refs ────────────────────────────────────────────────────────

    #[test]
    fn block_ref_at_end() {
        let (text, id) = extract_block_ref("Hello world ^abc123");
        assert_eq!(text, "Hello world");
        assert_eq!(id.as_deref(), Some("abc123"));
    }

    #[test]
    fn block_ref_only_at_end() {
        // `^mid` in the middle is NOT a block ref.
        let (text, id) = extract_block_ref("Hello ^mid world");
        assert!(id.is_none());
        assert!(text.contains("^mid"));
    }

    #[test]
    fn no_block_ref() {
        let (text, id) = extract_block_ref("No anchor here");
        assert!(id.is_none());
        assert_eq!(text, "No anchor here");
    }

    // ── Math spans ────────────────────────────────────────────────────────

    #[test]
    fn inline_math() {
        let spans = extract_math_spans("The formula $E=mc^2$ is famous.");
        let inline: Vec<_> = spans.iter().filter(|s| !s.display).collect();
        assert!(!inline.is_empty());
        assert_eq!(inline[0].content, "E=mc^2");
    }

    #[test]
    fn block_math() {
        let spans = extract_math_spans("$$\\int_0^\\infty e^{-x}dx = 1$$");
        let display: Vec<_> = spans.iter().filter(|s| s.display).collect();
        assert!(!display.is_empty());
        assert!(display[0].content.contains("int"));
    }

    #[test]
    fn dollar_with_space_after_not_math() {
        // "$5 dollars" should not trigger math.
        let spans = extract_math_spans("I have $5 dollars.");
        let inline: Vec<_> = spans.iter().filter(|s| !s.display).collect();
        assert!(inline.is_empty());
    }

    #[test]
    fn no_math_returns_empty() {
        let spans = extract_math_spans("No math here at all.");
        assert!(spans.is_empty());
    }
}
