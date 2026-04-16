//! Inline annotation extraction (parse) and serialization (render).
//!
//! "Inline" content is the text that sits inside a block — paragraphs,
//! headings, list items, quotes, callouts. It is represented as a
//! plain-text `String` plus a vector of [`Annotation`]s that carry
//! byte ranges and formatting metadata.
//!
//! Annotations split into two rendering styles:
//!
//! - **Wrapping** annotations are emitted with surrounding markers on
//!   serialize (`**bold**`, `[text](url)`, etc.). Their range covers
//!   the visible content between the markers.
//! - **Pass-through** annotations mark ranges of text that already
//!   contain their source syntax (`[[target]]`, `$x$`). Serialization
//!   emits the text unchanged; the annotation exists for the UI to
//!   render differently.

use comrak::nodes::{AstNode, NodeValue};

use crate::annotation::{Annotation, AnnotationType};

// ── Extraction (parse) ────────────────────────────────────────────────────────

/// Walk the inline children of `parent` and build a plain-text buffer +
/// annotation list.
///
/// Wikilink and inline-math scans run as a post-pass in
/// [`attach_post_pass_annotations`], so their ranges compose with
/// comrak-derived ranges.
pub fn collect_inline<'a>(parent: &'a AstNode<'a>) -> (String, Vec<Annotation>) {
    let mut buf = String::new();
    let mut anns = Vec::new();
    for child in parent.children() {
        visit_inline(child, &mut buf, &mut anns);
    }
    attach_post_pass_annotations(&buf, &mut anns);
    (buf, anns)
}

fn visit_inline<'a>(node: &'a AstNode<'a>, buf: &mut String, anns: &mut Vec<Annotation>) {
    let start = buf.len();
    let value = node.data.borrow().value.clone();
    match value {
        NodeValue::Text(s) => buf.push_str(&s),
        NodeValue::SoftBreak | NodeValue::LineBreak => buf.push(' '),
        NodeValue::Code(c) => {
            // Preserve the source form so roundtrip is literal.
            let backticks = "`".repeat(c.num_backticks.max(1));
            buf.push_str(&backticks);
            buf.push_str(&c.literal);
            buf.push_str(&backticks);
            anns.push(Annotation {
                start,
                end: buf.len(),
                ty: AnnotationType::Code,
            });
        }
        NodeValue::Emph => {
            for child in node.children() {
                visit_inline(child, buf, anns);
            }
            anns.push(Annotation {
                start,
                end: buf.len(),
                ty: AnnotationType::Italic,
            });
        }
        NodeValue::Strong => {
            for child in node.children() {
                visit_inline(child, buf, anns);
            }
            anns.push(Annotation {
                start,
                end: buf.len(),
                ty: AnnotationType::Bold,
            });
        }
        NodeValue::Strikethrough => {
            for child in node.children() {
                visit_inline(child, buf, anns);
            }
            anns.push(Annotation {
                start,
                end: buf.len(),
                ty: AnnotationType::Strikethrough,
            });
        }
        NodeValue::Link(l) => {
            let url = l.url.clone();
            let title = if l.title.is_empty() {
                None
            } else {
                Some(l.title.clone())
            };
            if url.is_empty() {
                // Broken / unresolved reference — fall back to plain
                // text so `[[wikilink]]` forms survive comrak's
                // shortcut-ref handling.
                for child in node.children() {
                    visit_inline(child, buf, anns);
                }
            } else {
                for child in node.children() {
                    visit_inline(child, buf, anns);
                }
                anns.push(Annotation {
                    start,
                    end: buf.len(),
                    ty: AnnotationType::Link { url, title },
                });
            }
        }
        NodeValue::Image(l) => {
            // Inline image: render as `![alt](src)` literal to keep
            // roundtrip deterministic.
            buf.push_str("![");
            for child in node.children() {
                visit_inline(child, buf, anns);
            }
            buf.push_str("](");
            buf.push_str(&l.url);
            buf.push(')');
        }
        NodeValue::HtmlInline(s) => buf.push_str(&s),
        // Unknown inline nodes: best-effort recurse.
        _ => {
            for child in node.children() {
                visit_inline(child, buf, anns);
            }
        }
    }
}

/// Scan the assembled plain-text buffer for wikilink and inline-math
/// occurrences and append pass-through annotations.
///
/// Exposed at module scope so `parse` can re-run just the post-scan
/// after promoting a paragraph into an `Embed` block.
pub fn attach_post_pass_annotations(buf: &str, anns: &mut Vec<Annotation>) {
    scan_wikilinks(buf, anns);
    scan_inline_math(buf, anns);
}

fn scan_wikilinks(buf: &str, anns: &mut Vec<Annotation>) {
    let bytes = buf.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i + 1 < len {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            let content_start = i + 2;
            if let Some(rel) = buf[content_start..].find("]]") {
                let content_end = content_start + rel;
                let inner = &buf[content_start..content_end];
                let (path, display, _fragment) = parse_wikilink_inner(inner);
                anns.push(Annotation {
                    start: i,
                    end: content_end + 2,
                    ty: AnnotationType::Wikilink {
                        path,
                        display_text: display,
                        is_resolved: false,
                    },
                });
                i = content_end + 2;
                continue;
            }
        }
        i += 1;
    }
}

/// Parse the inside of `[[...]]` into `(target, display, fragment)`.
fn parse_wikilink_inner(inner: &str) -> (String, Option<String>, Option<String>) {
    let (target_part, display) = if let Some(pipe) = inner.find('|') {
        (&inner[..pipe], Some(inner[pipe + 1..].to_string()))
    } else {
        (inner, None)
    };
    if let Some(hash) = target_part.find('#') {
        (
            target_part[..hash].to_string(),
            display,
            Some(target_part[hash + 1..].to_string()),
        )
    } else {
        (target_part.to_string(), display, None)
    }
}

fn scan_inline_math(buf: &str, anns: &mut Vec<Annotation>) {
    let bytes = buf.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        // Skip block-math `$$...$$` occurrences; those are handled at
        // the block level, not as inline annotations.
        if i + 1 < len && bytes[i] == b'$' && bytes[i + 1] == b'$' {
            // Advance past the block-math closing `$$`.
            if let Some(rel) = buf[i + 2..].find("$$") {
                i = i + 2 + rel + 2;
                continue;
            }
            break;
        }
        if bytes[i] == b'$' && i + 1 < len && bytes[i + 1] != b' ' {
            let content_start = i + 1;
            if let Some(rel) = buf[content_start..].find('$') {
                let close_pos = content_start + rel;
                if close_pos > content_start
                    && bytes[close_pos - 1] != b' '
                    && !(close_pos + 1 < len && bytes[close_pos + 1] == b'$')
                {
                    let formula = buf[content_start..close_pos].to_string();
                    anns.push(Annotation {
                        start: i,
                        end: close_pos + 1,
                        ty: AnnotationType::MathInline { formula },
                    });
                    i = close_pos + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
}

// ── Serialization ─────────────────────────────────────────────────────────────

/// Render `content` + `annotations` back to markdown.
///
/// Pass-through annotations (`Wikilink`, `MathInline`, `Mention`,
/// `BlockRef`, `Custom`) do not emit extra syntax — their ranges
/// already contain the source form in `content`. Wrapping annotations
/// (`Bold`, `Italic`, `Strikethrough`, `Code`, `Link`) emit matching
/// open/close markers.
#[must_use]
pub fn serialize_inline(content: &str, annotations: &[Annotation]) -> String {
    // Build event list: (position, is_open, stable_index, annotation).
    // `stable_index` breaks ties so sort is deterministic.
    let mut events: Vec<(usize, bool, usize, &Annotation)> =
        Vec::with_capacity(annotations.len() * 2);
    for (idx, ann) in annotations.iter().enumerate() {
        if !is_wrapping(&ann.ty) {
            continue;
        }
        events.push((ann.start, true, idx, ann));
        events.push((ann.end, false, idx, ann));
    }
    // Sort: by position ascending; at the same position, closes come
    // before opens (so `**a**b` → `**a**b`, not `**ab**`); within the
    // same kind, stable by insertion index.
    events.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1)) // false(close)=0 before true(open)=1
            .then_with(|| a.2.cmp(&b.2))
    });

    let mut out = String::with_capacity(content.len() + annotations.len() * 4);
    let mut cursor = 0;
    for (pos, is_open, _, ann) in events {
        if pos > cursor {
            out.push_str(&content[cursor..pos]);
            cursor = pos;
        } else if pos < cursor {
            // Annotation range starts before current cursor — this
            // can happen when two wrapping annotations share a
            // boundary. Just emit the marker.
        }
        out.push_str(&marker_for(&ann.ty, is_open));
    }
    if cursor < content.len() {
        out.push_str(&content[cursor..]);
    }
    out
}

fn is_wrapping(ty: &AnnotationType) -> bool {
    matches!(
        ty,
        AnnotationType::Bold
            | AnnotationType::Italic
            | AnnotationType::Strikethrough
            | AnnotationType::Code
            | AnnotationType::Link { .. }
    )
}

fn marker_for(ty: &AnnotationType, is_open: bool) -> String {
    match ty {
        AnnotationType::Bold => "**".into(),
        AnnotationType::Italic => "*".into(),
        AnnotationType::Strikethrough => "~~".into(),
        AnnotationType::Link { url, .. } => {
            if is_open {
                "[".into()
            } else {
                format!("]({url})")
            }
        }
        // Code keeps its backticks in content; pass-through annotations
        // never reach this path (guarded by `is_wrapping`).
        _ => String::new(),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use comrak::{parse_document, Arena, Options};

    fn extract(md: &str) -> (String, Vec<Annotation>) {
        let arena = Arena::new();
        let mut opts = Options::default();
        opts.extension.strikethrough = true;
        opts.extension.table = true;
        opts.extension.autolink = true;
        opts.extension.tasklist = true;
        let root = parse_document(&arena, md, &opts);
        // Descend into the first Paragraph child.
        for child in root.children() {
            if matches!(child.data.borrow().value, NodeValue::Paragraph) {
                let mut buf = String::new();
                let mut anns = Vec::new();
                for inline in child.children() {
                    visit_inline(inline, &mut buf, &mut anns);
                }
                attach_post_pass_annotations(&buf, &mut anns);
                return (buf, anns);
            }
        }
        (String::new(), Vec::new())
    }

    #[test]
    fn extract_bold() {
        let (buf, anns) = extract("**hello**\n");
        assert_eq!(buf, "hello");
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].ty, AnnotationType::Bold);
    }

    #[test]
    fn extract_italic() {
        let (_, anns) = extract("*hi*\n");
        assert!(anns.iter().any(|a| a.ty == AnnotationType::Italic));
    }

    #[test]
    fn extract_strikethrough() {
        let (_, anns) = extract("~~gone~~\n");
        assert!(anns.iter().any(|a| a.ty == AnnotationType::Strikethrough));
    }

    #[test]
    fn extract_inline_code_keeps_backticks_in_content() {
        let (buf, anns) = extract("a `code` b\n");
        assert!(buf.contains("`code`"));
        assert!(anns.iter().any(|a| a.ty == AnnotationType::Code));
    }

    #[test]
    fn extract_link() {
        let (buf, anns) = extract("[text](https://x)\n");
        assert_eq!(buf, "text");
        let link = anns
            .iter()
            .find(|a| matches!(a.ty, AnnotationType::Link { .. }))
            .unwrap();
        match &link.ty {
            AnnotationType::Link { url, .. } => assert_eq!(url, "https://x"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn extract_wikilink() {
        let (buf, anns) = extract("See [[target|display]]\n");
        assert!(buf.contains("[[target|display]]"));
        let wl = anns
            .iter()
            .find(|a| matches!(a.ty, AnnotationType::Wikilink { .. }))
            .unwrap();
        match &wl.ty {
            AnnotationType::Wikilink {
                path, display_text, ..
            } => {
                assert_eq!(path, "target");
                assert_eq!(display_text.as_deref(), Some("display"));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn extract_inline_math() {
        let (buf, anns) = extract("$E=mc^2$ is famous\n");
        assert!(buf.starts_with("$E=mc^2$"));
        let m = anns
            .iter()
            .find(|a| matches!(a.ty, AnnotationType::MathInline { .. }))
            .unwrap();
        match &m.ty {
            AnnotationType::MathInline { formula } => assert_eq!(formula, "E=mc^2"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn dollar_with_space_is_not_math() {
        let (_, anns) = extract("I have $5 dollars\n");
        assert!(!anns
            .iter()
            .any(|a| matches!(a.ty, AnnotationType::MathInline { .. })));
    }

    // ── Serialization ──

    #[test]
    fn serialize_bold_wraps() {
        let anns = vec![Annotation {
            start: 0,
            end: 5,
            ty: AnnotationType::Bold,
        }];
        assert_eq!(serialize_inline("hello", &anns), "**hello**");
    }

    #[test]
    fn serialize_italic_wraps() {
        let anns = vec![Annotation {
            start: 0,
            end: 2,
            ty: AnnotationType::Italic,
        }];
        assert_eq!(serialize_inline("hi", &anns), "*hi*");
    }

    #[test]
    fn serialize_strikethrough_wraps() {
        let anns = vec![Annotation {
            start: 0,
            end: 4,
            ty: AnnotationType::Strikethrough,
        }];
        assert_eq!(serialize_inline("gone", &anns), "~~gone~~");
    }

    #[test]
    fn serialize_link_wraps_with_url() {
        let anns = vec![Annotation {
            start: 0,
            end: 4,
            ty: AnnotationType::Link {
                url: "https://x".into(),
                title: None,
            },
        }];
        assert_eq!(serialize_inline("text", &anns), "[text](https://x)");
    }

    #[test]
    fn serialize_wikilink_passes_through() {
        let anns = vec![Annotation {
            start: 0,
            end: 10,
            ty: AnnotationType::Wikilink {
                path: "foo".into(),
                display_text: None,
                is_resolved: false,
            },
        }];
        assert_eq!(serialize_inline("[[foo]]xyz", &anns), "[[foo]]xyz");
    }

    #[test]
    fn serialize_inline_math_passes_through() {
        let content = "The value $x^2$ matters";
        let anns = vec![Annotation {
            start: 10,
            end: 15,
            ty: AnnotationType::MathInline {
                formula: "x^2".into(),
            },
        }];
        assert_eq!(serialize_inline(content, &anns), content);
    }

    #[test]
    fn serialize_plain_text_with_no_annotations() {
        assert_eq!(serialize_inline("plain", &[]), "plain");
    }

    #[test]
    fn serialize_adjacent_bold_italic() {
        let content = "helloworld";
        let anns = vec![
            Annotation {
                start: 0,
                end: 5,
                ty: AnnotationType::Bold,
            },
            Annotation {
                start: 5,
                end: 10,
                ty: AnnotationType::Italic,
            },
        ];
        assert_eq!(serialize_inline(content, &anns), "**hello***world*");
    }

    #[test]
    fn roundtrip_bold() {
        let src = "**bold text**";
        let arena = Arena::new();
        let root = parse_document(&arena, src, &Options::default());
        let para = root
            .children()
            .find(|c| matches!(c.data.borrow().value, NodeValue::Paragraph))
            .unwrap();
        let (buf, anns) = {
            let mut b = String::new();
            let mut a = Vec::new();
            for inline in para.children() {
                visit_inline(inline, &mut b, &mut a);
            }
            attach_post_pass_annotations(&b, &mut a);
            (b, a)
        };
        let rendered = serialize_inline(&buf, &anns);
        assert_eq!(rendered, "**bold text**");
    }
}
