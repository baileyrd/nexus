//! Markdown body conversion: callouts, mention links, toggles.

use std::collections::HashMap;

use super::{filename, percent_decode};

/// Marker injected into the body for blocks we couldn't convert. The
/// orchestrator scans for it to record warnings.
const UNCONVERTED_MARKER: &str = "<!-- nexus:unconverted -->";

/// Returns true if [`convert_notion_markdown`] left a warning marker in the
/// body, indicating an unsupported block was passed through.
#[must_use]
pub fn has_unconverted_warning_marker(body: &str) -> bool {
    body.contains(UNCONVERTED_MARKER)
}

/// Convert a Notion-export markdown body to Nexus markdown.
///
/// `link_rewrites` maps URL-encoded source filenames (e.g.
/// `Page%20Title%20abc.md`) to the destination wikilink target (the
/// cleaned title). Used to rewrite `[Title](Page%20Title%20abc.md)` →
/// `[[Title]]`.
///
/// Pure function; no I/O.
#[must_use]
pub fn convert_notion_markdown(input: &str, link_rewrites: &HashMap<String, String>) -> String {
    let after_links = rewrite_internal_links(input, link_rewrites);
    convert_callouts(&after_links)
}

// ── Mention link rewrite ─────────────────────────────────────────────────────

/// Walk the body and replace `[Display](Encoded%20Path.md)` with `[[Title]]`
/// when the encoded path resolves to a known page in the link index.
fn rewrite_internal_links(input: &str, rewrites: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let bytes = input.as_bytes();

    while i < input.len() {
        if bytes[i] == b'[' {
            if let Some((display, target, end)) = parse_inline_link(input, i) {
                if target.ends_with(".md") {
                    if let Some(replacement) = lookup_link_replacement(target, display, rewrites) {
                        out.push_str(&replacement);
                        i = end;
                        continue;
                    }
                }
            }
            out.push('[');
            i += 1;
            continue;
        }
        // Copy one full UTF-8 codepoint.
        let ch_len = utf8_char_len(bytes[i]);
        out.push_str(&input[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn utf8_char_len(first_byte: u8) -> usize {
    if first_byte < 0x80 {
        1
    } else if first_byte < 0xC0 {
        // Continuation byte — shouldn't be seen as a "first" byte in valid
        // UTF-8, but be defensive.
        1
    } else if first_byte < 0xE0 {
        2
    } else if first_byte < 0xF0 {
        3
    } else {
        4
    }
}

fn parse_inline_link(s: &str, start: usize) -> Option<(&str, &str, usize)> {
    debug_assert_eq!(s.as_bytes()[start], b'[');
    let bytes = s.as_bytes();
    let mut depth = 1;
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            b'\n' => return None, // links don't span lines
            _ => {}
        }
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b']' {
        return None;
    }
    let display_end = i;
    if i + 1 >= bytes.len() || bytes[i + 1] != b'(' {
        return None;
    }
    // Find matching ).
    let target_start = i + 2;
    let mut depth = 1;
    let mut j = target_start;
    while j < bytes.len() {
        match bytes[j] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            b'\n' => return None,
            _ => {}
        }
        j += 1;
    }
    if j >= bytes.len() || bytes[j] != b')' {
        return None;
    }
    let display = &s[start + 1..display_end];
    let target = &s[target_start..j];
    Some((display, target, j + 1))
}

fn lookup_link_replacement(
    target: &str,
    display: &str,
    rewrites: &HashMap<String, String>,
) -> Option<String> {
    // Try the URL-encoded target as-is, then try its basename.
    if let Some(title) = rewrites.get(target) {
        return Some(format_wikilink(display, title));
    }
    if let Some(basename) = target.rsplit('/').next() {
        if let Some(title) = rewrites.get(basename) {
            return Some(format_wikilink(display, title));
        }
    }
    // Fall back to decoded form: filename without UUID, no extension.
    let decoded = percent_decode(target);
    let basename = decoded.rsplit('/').next().unwrap_or(&decoded);
    let (cleaned, _) = filename::strip_notion_uuid(basename);
    if cleaned.ends_with(".md") {
        let title = &cleaned[..cleaned.len() - 3];
        return Some(format_wikilink(display, title));
    }
    None
}

fn format_wikilink(display: &str, target: &str) -> String {
    if display == target {
        format!("[[{target}]]")
    } else {
        format!("[[{target}|{display}]]")
    }
}

// ── Callout conversion ──────────────────────────────────────────────────────

/// Map a leading emoji to a Nexus callout type. Notion uses freeform emojis;
/// we recognize the common ones and fall back to `note` for everything else.
fn callout_type_for_emoji(emoji: &str) -> &'static str {
    match emoji {
        "💡" | "ℹ️" | "📝" | "📌" => "note",
        "💭" | "🤔" => "tip",
        "⚠️" | "⚠" | "🚧" => "warning",
        "❗" | "❌" | "🛑" | "🔥" => "danger",
        "✅" | "🎉" | "👍" => "tip",
        "📖" | "🔍" | "🎯" => "info",
        _ => "note",
    }
}

/// Walk block quotes and convert `> <emoji> body` into Nexus callout syntax.
fn convert_callouts(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut lines = input.lines().peekable();
    let mut first = true;

    while let Some(line) = lines.next() {
        if !first {
            out.push('\n');
        }
        first = false;

        if let Some(rest) = line.strip_prefix("> ") {
            if let Some((emoji, body)) = split_leading_emoji(rest) {
                let kind = callout_type_for_emoji(emoji);
                if body.is_empty() {
                    out.push_str(&format!("> [!{kind}]"));
                } else {
                    out.push_str(&format!("> [!{kind}] {body}"));
                }
                continue;
            }
        }
        out.push_str(line);
    }

    if input.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Split a string into (leading emoji grapheme, rest). Returns None if the
/// first character isn't an emoji-range codepoint.
fn split_leading_emoji(s: &str) -> Option<(&str, &str)> {
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    if !is_emoji_starter(first) {
        return None;
    }
    // Greedily consume emoji modifiers and a single VS16 / ZWJ-joined glyph.
    let mut end = first.len_utf8();
    for (i, c) in s.char_indices().skip(1) {
        if is_emoji_modifier(c) || c == '\u{FE0F}' || c == '\u{200D}' {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    let emoji = &s[..end];
    let rest = s[end..].trim_start();
    Some((emoji, rest))
}

fn is_emoji_starter(c: char) -> bool {
    matches!(
        c as u32,
        0x1F300..=0x1FAFF
            | 0x2600..=0x27BF
            | 0x2300..=0x23FF
            | 0x1F000..=0x1F02F
            | 0x1F0A0..=0x1F0FF
            | 0x1F100..=0x1F1FF
    )
}

fn is_emoji_modifier(c: char) -> bool {
    matches!(c as u32, 0x1F3FB..=0x1F3FF)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn rewrites_known_internal_link_to_wikilink() {
        let mut r = HashMap::new();
        r.insert(
            "Page%20B%20bbb111bbb222bbb333bbb444bbb55555.md".to_string(),
            "Page B".to_string(),
        );
        let out = convert_notion_markdown(
            "See [Page B](Page%20B%20bbb111bbb222bbb333bbb444bbb55555.md) please.",
            &r,
        );
        assert_eq!(out, "See [[Page B]] please.");
    }

    #[test]
    fn rewrites_with_aliased_display() {
        let mut r = HashMap::new();
        r.insert(
            "B%20bbb111bbb222bbb333bbb444bbb55555.md".to_string(),
            "B".to_string(),
        );
        let out = convert_notion_markdown(
            "Read [the second](B%20bbb111bbb222bbb333bbb444bbb55555.md).",
            &r,
        );
        assert_eq!(out, "Read [[B|the second]].");
    }

    #[test]
    fn leaves_external_links_alone() {
        let r = HashMap::new();
        let out =
            convert_notion_markdown("Visit [example](https://example.com) today.", &r);
        assert_eq!(out, "Visit [example](https://example.com) today.");
    }

    #[test]
    fn falls_back_when_link_not_in_index() {
        let r = HashMap::new();
        let out = convert_notion_markdown(
            "See [unknown](Unknown%20deadbeefdeadbeefdeadbeefdeadbeef.md).",
            &r,
        );
        // Not in index — fall back to decoded title.
        assert_eq!(out, "See [[Unknown|unknown]].");
    }

    #[test]
    fn converts_info_callout() {
        let r = HashMap::new();
        let out = convert_notion_markdown("> 💡 An info note.", &r);
        assert_eq!(out, "> [!note] An info note.");
    }

    #[test]
    fn converts_warning_callout_with_vs16() {
        let r = HashMap::new();
        let out = convert_notion_markdown("> ⚠️ Watch out.", &r);
        assert_eq!(out, "> [!warning] Watch out.");
    }

    #[test]
    fn converts_danger_callout() {
        let r = HashMap::new();
        let out = convert_notion_markdown("> 🛑 Stop.", &r);
        assert_eq!(out, "> [!danger] Stop.");
    }

    #[test]
    fn unknown_emoji_falls_back_to_note() {
        let r = HashMap::new();
        let out = convert_notion_markdown("> 🦄 Magical.", &r);
        assert_eq!(out, "> [!note] Magical.");
    }

    #[test]
    fn quote_without_emoji_is_unchanged() {
        let r = HashMap::new();
        let out = convert_notion_markdown("> Just a normal quote.", &r);
        assert_eq!(out, "> Just a normal quote.");
    }

    #[test]
    fn preserves_trailing_newline() {
        let r = HashMap::new();
        let out = convert_notion_markdown("Line 1\nLine 2\n", &r);
        assert_eq!(out, "Line 1\nLine 2\n");
    }
}
