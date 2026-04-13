//! Wikilink and embed scanner.
//!
//! Scans markdown text for `[[wikilinks]]`, `[[target|display]]` variants,
//! `![[embeds]]`, and heading/block-ref fragments (`#heading`, `#^block-id`).

// ── Public types ──────────────────────────────────────────────────────────────

/// The kind of an in-document link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkType {
    /// `[[target]]` or `[[target|display]]`
    Wikilink,
    /// `![[target]]` — inline embed
    Embed,
    /// Standard `CommonMark` `[text](url)` — populated by the full parser
    Markdown,
}

/// A wikilink or embed reference found in document text.
#[derive(Debug, Clone)]
pub struct WikiLink {
    /// How the link is represented in the source.
    pub link_type: LinkType,
    /// Resolved target (path component before `#`). Empty for bare `[[]]`.
    pub target: String,
    /// Display text after `|`, if any.
    pub display: Option<String>,
    /// Fragment after `#` (heading or `^block-id`).
    pub fragment: Option<String>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Scan `text` for `[[wikilinks]]` and `![[embeds]]`.
///
/// This is a manual byte-level scan so that the preceding `!` for embeds can
/// be detected without regex overhead.
#[must_use]
pub fn scan(text: &str) -> Vec<WikiLink> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut links = Vec::new();
    let mut i = 0;

    while i + 1 < len {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let is_embed = i > 0 && bytes[i - 1] == b'!';

            let start = i + 2;
            if let Some(rel) = text[start..].find("]]") {
                let inner = &text[start..start + rel];
                let link = parse_inner(inner, is_embed);
                links.push(link);
                i = start + rel + 2; // skip past ']]'
                continue;
            }
        }
        i += 1;
    }

    links
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn parse_inner(inner: &str, is_embed: bool) -> WikiLink {
    if is_embed {
        let (target, fragment) = split_fragment(inner);
        return WikiLink {
            link_type: LinkType::Embed,
            target,
            display: None,
            fragment,
        };
    }

    // Check for display text separator `|`.
    if let Some(pipe) = inner.find('|') {
        let target_part = &inner[..pipe];
        let display_text = inner[pipe + 1..].to_string();
        let (target, fragment) = split_fragment(target_part);
        WikiLink {
            link_type: LinkType::Wikilink,
            target,
            display: Some(display_text),
            fragment,
        }
    } else {
        let (target, fragment) = split_fragment(inner);
        WikiLink {
            link_type: LinkType::Wikilink,
            target,
            display: None,
            fragment,
        }
    }
}

/// Split `target#fragment` into `(target, Some(fragment))` or `(original, None)`.
fn split_fragment(s: &str) -> (String, Option<String>) {
    if let Some(pos) = s.find('#') {
        (s[..pos].to_string(), Some(s[pos + 1..].to_string()))
    } else {
        (s.to_string(), None)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_wikilink() {
        let links = scan("See [[other note]]");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].link_type, LinkType::Wikilink);
        assert_eq!(links[0].target, "other note");
        assert!(links[0].display.is_none());
        assert!(links[0].fragment.is_none());
    }

    #[test]
    fn wikilink_with_display_text() {
        let links = scan("See [[path/to/note|display text]]");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "path/to/note");
        assert_eq!(links[0].display.as_deref(), Some("display text"));
    }

    #[test]
    fn wikilink_with_heading_fragment() {
        let links = scan("[[note#Section Title]]");
        assert_eq!(links[0].target, "note");
        assert_eq!(links[0].fragment.as_deref(), Some("Section Title"));
    }

    #[test]
    fn wikilink_with_block_ref_fragment() {
        let links = scan("[[note#^abc123]]");
        assert_eq!(links[0].target, "note");
        assert_eq!(links[0].fragment.as_deref(), Some("^abc123"));
    }

    #[test]
    fn wikilink_with_fragment_and_display() {
        let links = scan("[[note#Heading|display]]");
        assert_eq!(links[0].target, "note");
        assert_eq!(links[0].fragment.as_deref(), Some("Heading"));
        assert_eq!(links[0].display.as_deref(), Some("display"));
    }

    #[test]
    fn embed_link() {
        let links = scan("![[embedded-note]]");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].link_type, LinkType::Embed);
        assert_eq!(links[0].target, "embedded-note");
    }

    #[test]
    fn embed_with_fragment() {
        let links = scan("![[note#Section]]");
        assert_eq!(links[0].link_type, LinkType::Embed);
        assert_eq!(links[0].target, "note");
        assert_eq!(links[0].fragment.as_deref(), Some("Section"));
    }

    #[test]
    fn no_wikilinks_returns_empty() {
        let links = scan("Just a plain paragraph with no links.");
        assert!(links.is_empty());
    }

    #[test]
    fn multiple_links_in_text() {
        let links = scan("See [[a]] and [[b|B name]] also ![[c]]");
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].target, "a");
        assert_eq!(links[1].target, "b");
        assert_eq!(links[1].display.as_deref(), Some("B name"));
        assert_eq!(links[2].link_type, LinkType::Embed);
        assert_eq!(links[2].target, "c");
    }

    #[test]
    fn unclosed_bracket_ignored() {
        let links = scan("[[unclosed");
        assert!(links.is_empty());
    }
}
