//! Markdown/MDX parser pipeline.
//!
//! Parses a markdown file into structured blocks, links, tags, and frontmatter.

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, Options, parse_document};
use sha2::{Digest, Sha256};

use crate::StorageError;
use crate::tasks::ParsedTask;

// ── Public types ──────────────────────────────────────────────────────────────

/// Result of parsing a single markdown/MDX file.
#[derive(Debug, Clone)]
pub struct ParsedFile {
    /// SHA-256 hex digest of the raw bytes.
    pub content_hash: String,
    /// YAML frontmatter properties.
    pub frontmatter: Vec<Property>,
    /// Top-level content blocks.
    pub blocks: Vec<ParsedBlock>,
    /// All links found in the document.
    pub links: Vec<ParsedLink>,
    /// All tags found in the document.
    pub tags: Vec<ParsedTag>,
    /// Task items extracted from checkbox lists.
    pub tasks: Vec<ParsedTask>,
}

/// A single YAML frontmatter property.
#[derive(Debug, Clone)]
pub struct Property {
    /// The YAML key.
    pub key: String,
    /// JSON-serialized value.
    pub value: String,
    /// Type hint: "string", "number", "list", or "object".
    pub property_type: Option<String>,
}

/// A single content block extracted from the AST.
#[derive(Debug, Clone)]
pub struct ParsedBlock {
    /// Kind of block: "heading", "paragraph", "codeblock", "list", "table",
    /// "callout", or "blockquote".
    pub block_type: String,
    /// Heading level 1-6; `None` for non-headings.
    pub level: Option<i32>,
    /// Plain-text content.
    pub content: String,
    /// Raw markdown source (currently not populated).
    pub raw_markdown: Option<String>,
    /// 1-based start line in the source.
    pub start_line: u32,
    /// 1-based end line in the source.
    pub end_line: u32,
    /// Block reference anchor ID (e.g. `"abc123"` from trailing ` ^abc123`).
    pub block_ref_id: Option<String>,
    /// Callout type for callout blocks (e.g. `"warning"`, `"tip"`).
    pub callout_type: Option<String>,
}

/// A link found in the document.
#[derive(Debug, Clone)]
pub struct ParsedLink {
    /// Display text for the link.
    pub link_text: String,
    /// Target path or URL, if available.
    pub target_path: Option<String>,
    /// Kind of link: "wikilink", "markdown", or "embed".
    pub link_type: String,
    /// Heading or block-ref fragment (e.g. `"Heading"` from `[[note#Heading]]`).
    pub fragment: Option<String>,
}

/// A tag found in the document.
#[derive(Debug, Clone)]
pub struct ParsedTag {
    /// Tag name without the `#` prefix.
    pub name: String,
    /// Where the tag came from: "frontmatter" or "inline".
    pub source: String,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Compute the SHA-256 hex digest of `content`.
#[must_use]
pub fn content_hash(content: &[u8]) -> String {
    Sha256::digest(content)
        .iter()
        .fold(String::with_capacity(64), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

/// Parse a markdown/MDX string into a [`ParsedFile`].
///
/// Extracts YAML frontmatter, parses the body with comrak, and returns
/// structured blocks, links, and tags.
///
/// # Errors
///
/// Returns [`StorageError::ParseError`] if YAML frontmatter is malformed.
#[allow(clippy::too_many_lines)]
pub fn parse_markdown(content: &str) -> Result<ParsedFile, StorageError> {
    let hash = content_hash(content.as_bytes());

    let (frontmatter, fm_tags, body) = extract_frontmatter(content)?;

    let arena = Arena::new();
    let mut opts = Options::default();
    opts.extension.strikethrough = true;
    opts.extension.table = true;
    opts.extension.autolink = true;
    opts.extension.tasklist = true;

    let root = parse_document(&arena, &body, &opts);

    let mut blocks = Vec::new();
    let mut links = Vec::new();
    let mut tags = fm_tags;
    let mut tasks = Vec::new();

    // Walk top-level children of the document root.
    for child in root.children() {
        let ast = child.data.borrow();
        let sp = &ast.sourcepos;
        let start_line = u32::try_from(sp.start.line).unwrap_or(0);
        let end_line = u32::try_from(sp.end.line).unwrap_or(0);

        match &ast.value {
            NodeValue::Heading(h) => {
                let raw_text = collect_text(child);
                let (text, block_ref_id) = extract_block_ref(&raw_text);
                extract_wikilinks_and_embeds(&text, &mut links);
                extract_inline_tags(&text, &mut tags);
                blocks.push(ParsedBlock {
                    block_type: "heading".to_string(),
                    level: Some(i32::from(h.level)),
                    content: text,
                    raw_markdown: None,
                    start_line,
                    end_line,
                    block_ref_id,
                    callout_type: None,
                });
            }
            NodeValue::Paragraph => {
                let raw_text = collect_text(child);
                let (text, block_ref_id) = extract_block_ref(&raw_text);
                extract_wikilinks_and_embeds(&text, &mut links);
                extract_markdown_links(child, &mut links);
                extract_inline_tags(&text, &mut tags);
                blocks.push(ParsedBlock {
                    block_type: "paragraph".to_string(),
                    level: None,
                    content: text,
                    raw_markdown: None,
                    start_line,
                    end_line,
                    block_ref_id,
                    callout_type: None,
                });
            }
            NodeValue::CodeBlock(cb) => {
                blocks.push(ParsedBlock {
                    block_type: "codeblock".to_string(),
                    level: None,
                    content: cb.literal.clone(),
                    raw_markdown: None,
                    start_line,
                    end_line,
                    block_ref_id: None,
                    callout_type: None,
                });
            }
            NodeValue::List(_) => {
                extract_tasks(child, &mut tasks);
                let raw_text = collect_text(child);
                let (text, block_ref_id) = extract_block_ref(&raw_text);
                extract_wikilinks_and_embeds(&text, &mut links);
                extract_markdown_links(child, &mut links);
                extract_inline_tags(&text, &mut tags);
                blocks.push(ParsedBlock {
                    block_type: "list".to_string(),
                    level: None,
                    content: text,
                    raw_markdown: None,
                    start_line,
                    end_line,
                    block_ref_id,
                    callout_type: None,
                });
            }
            NodeValue::Table(_) => {
                let text = collect_text(child);
                blocks.push(ParsedBlock {
                    block_type: "table".to_string(),
                    level: None,
                    content: text,
                    raw_markdown: None,
                    start_line,
                    end_line,
                    block_ref_id: None,
                    callout_type: None,
                });
            }
            NodeValue::BlockQuote => {
                let raw_text = collect_text(child);
                let (block_type, callout_type, content_after_callout) =
                    detect_callout(&raw_text);
                let (content, block_ref_id) = extract_block_ref(&content_after_callout);
                extract_wikilinks_and_embeds(&content, &mut links);
                extract_inline_tags(&content, &mut tags);
                blocks.push(ParsedBlock {
                    block_type,
                    level: None,
                    content,
                    raw_markdown: None,
                    start_line,
                    end_line,
                    block_ref_id,
                    callout_type,
                });
            }
            _ => {}
        }
    }

    // Deduplicate tags by (name, source).
    tags.dedup_by(|a, b| a.name == b.name && a.source == b.source);

    Ok(ParsedFile {
        content_hash: hash,
        frontmatter,
        blocks,
        links,
        tags,
        tasks,
    })
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Recursively collect all text from `Text` and `Code` inline nodes.
fn collect_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut buf = String::new();
    collect_text_into(node, &mut buf);
    buf
}

fn collect_text_into<'a>(node: &'a AstNode<'a>, buf: &mut String) {
    match &node.data.borrow().value {
        NodeValue::Text(t) => buf.push_str(t),
        NodeValue::Code(c) => buf.push_str(&c.literal),
        NodeValue::SoftBreak | NodeValue::LineBreak => buf.push(' '),
        _ => {}
    }
    for child in node.children() {
        collect_text_into(child, buf);
    }
}

/// Extract YAML frontmatter, returning (properties, tags, `body_without_frontmatter`).
fn extract_frontmatter(
    content: &str,
) -> Result<(Vec<Property>, Vec<ParsedTag>, String), StorageError> {
    if !content.starts_with("---\n") {
        return Ok((Vec::new(), Vec::new(), content.to_string()));
    }

    // Find closing delimiter: "\n---\n" or "\n---" at end of string.
    let after_open = &content[4..]; // skip "---\n"
    let close_pattern = "\n---";
    let Some(close_pos) = after_open.find(close_pattern) else {
        // Unterminated frontmatter — treat as no frontmatter.
        return Ok((Vec::new(), Vec::new(), content.to_string()));
    };

    let yaml_src = &after_open[..close_pos];
    // Body starts after "\n---" plus optional newline.
    let after_close = &after_open[close_pos + close_pattern.len()..];
    let body = if let Some(stripped) = after_close.strip_prefix('\n') {
        stripped.to_string()
    } else {
        after_close.to_string()
    };

    // Parse YAML.
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(yaml_src).map_err(|e| StorageError::ParseError {
            file: "<frontmatter>".to_string(),
            error: e.to_string(),
        })?;

    let mut properties = Vec::new();
    let mut tags: Vec<ParsedTag> = Vec::new();

    if let serde_yaml::Value::Mapping(map) = &yaml {
        for (k, v) in map {
            let key = match k {
                serde_yaml::Value::String(s) => s.clone(),
                other => format!("{other:?}"),
            };

            // Extract tags from the `tags` key.
            if key == "tags" {
                if let serde_yaml::Value::Sequence(seq) = v {
                    for item in seq {
                        if let serde_yaml::Value::String(tag) = item {
                            tags.push(ParsedTag {
                                name: tag.clone(),
                                source: "frontmatter".to_string(),
                            });
                        }
                    }
                }
            }

            let (json_val, type_hint) = yaml_to_json_and_type(v);
            properties.push(Property {
                key,
                value: json_val,
                property_type: Some(type_hint),
            });
        }
    }

    Ok((properties, tags, body))
}

/// Convert a `serde_yaml::Value` to a JSON string and a type hint.
fn yaml_to_json_and_type(value: &serde_yaml::Value) -> (String, String) {
    let json = yaml_value_to_json(value);
    let type_hint = match value {
        serde_yaml::Value::Number(_) => "number",
        serde_yaml::Value::Sequence(_) => "list",
        serde_yaml::Value::Mapping(_) => "object",
        serde_yaml::Value::String(_)
        | serde_yaml::Value::Bool(_)
        | serde_yaml::Value::Null
        | serde_yaml::Value::Tagged(_) => "string",
    };
    (
        serde_json::to_string(&json).unwrap_or_else(|_| "null".to_string()),
        type_hint.to_string(),
    )
}

/// Convert a `serde_yaml::Value` to a `serde_json::Value`.
fn yaml_value_to_json(value: &serde_yaml::Value) -> serde_json::Value {
    match value {
        serde_yaml::Value::Null => serde_json::Value::Null,
        serde_yaml::Value::Bool(b) => serde_json::Value::Bool(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map_or(serde_json::Value::Null, serde_json::Value::Number)
            } else {
                serde_json::Value::Null
            }
        }
        serde_yaml::Value::String(s) => serde_json::Value::String(s.clone()),
        serde_yaml::Value::Sequence(seq) => {
            serde_json::Value::Array(seq.iter().map(yaml_value_to_json).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        serde_yaml::Value::String(s) => s.clone(),
                        other => format!("{other:?}"),
                    };
                    (key, yaml_value_to_json(v))
                })
                .collect();
            serde_json::Value::Object(obj)
        }
        serde_yaml::Value::Tagged(t) => yaml_value_to_json(&t.value),
    }
}

/// Scan `text` for `[[wikilinks]]` and `![[embeds]]`, appending to `links`.
///
/// Handles fragments: `[[note#Heading]]` and `[[note#Heading|display]]`.
fn extract_wikilinks_and_embeds(text: &str, links: &mut Vec<ParsedLink>) {
    // Use a manual scan so we can check for preceding '!'.
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            // Check for embed (preceding '!').
            let is_embed = i > 0 && bytes[i - 1] == b'!';

            // Find closing ']]'.
            let start = i + 2;
            if let Some(rel) = text[start..].find("]]") {
                let inner = &text[start..start + rel];
                if is_embed {
                    let (target, fragment) = split_fragment(inner);
                    let target_path = if target.is_empty() {
                        inner.to_string()
                    } else {
                        target
                    };
                    links.push(ParsedLink {
                        link_text: inner.to_string(),
                        target_path: Some(target_path),
                        link_type: "embed".to_string(),
                        fragment,
                    });
                } else if let Some(pipe) = inner.find('|') {
                    let target_with_frag = &inner[..pipe];
                    let display = inner[pipe + 1..].to_string();
                    let (target, fragment) = split_fragment(target_with_frag);
                    links.push(ParsedLink {
                        link_text: display,
                        target_path: Some(target),
                        link_type: "wikilink".to_string(),
                        fragment,
                    });
                } else {
                    let (target, fragment) = split_fragment(inner);
                    if fragment.is_some() {
                        // Has a fragment: target is the part before `#`.
                        links.push(ParsedLink {
                            link_text: inner.to_string(),
                            target_path: Some(target),
                            link_type: "wikilink".to_string(),
                            fragment,
                        });
                    } else {
                        links.push(ParsedLink {
                            link_text: inner.to_string(),
                            target_path: None,
                            link_type: "wikilink".to_string(),
                            fragment: None,
                        });
                    }
                }
                i = start + rel + 2; // move past ']]'
                continue;
            }
        }
        i += 1;
    }
}

/// Walk AST nodes to extract `NodeValue::Link` entries.
fn extract_markdown_links<'a>(node: &'a AstNode<'a>, links: &mut Vec<ParsedLink>) {
    for descendant in node.descendants() {
        if let NodeValue::Link(lnk) = &descendant.data.borrow().value {
            let text = collect_text(descendant);
            let url = lnk.url.clone();
            links.push(ParsedLink {
                link_text: text,
                target_path: if url.is_empty() { None } else { Some(url) },
                link_type: "markdown".to_string(),
                fragment: None,
            });
        }
    }
}

/// Scan `text` for inline `#tag` patterns and append to `tags`.
///
/// A tag is `#` followed by `[a-zA-Z0-9_/-]+` that appears at the start of
/// the string or immediately after whitespace.
fn extract_inline_tags(text: &str, tags: &mut Vec<ParsedTag>) {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        if chars[i] == '#' {
            // Must be at start or preceded by whitespace.
            let preceded_by_ws = i == 0 || chars[i - 1].is_whitespace();
            if preceded_by_ws && i + 1 < len && is_tag_char(chars[i + 1]) {
                // Collect the tag name.
                let start = i + 1;
                let mut end = start;
                while end < len && is_tag_char(chars[end]) {
                    end += 1;
                }
                let name: String = chars[start..end].iter().collect();
                if !tags.iter().any(|t| t.name == name && t.source == "inline") {
                    tags.push(ParsedTag {
                        name,
                        source: "inline".to_string(),
                    });
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
}

/// Returns `true` if `c` is valid inside a tag name.
fn is_tag_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '/' || c == '-'
}

/// Walk list-item children of a list node, extracting task items.
fn extract_tasks<'a>(list_node: &'a AstNode<'a>, tasks: &mut Vec<ParsedTask>) {
    for item in list_node.children() {
        let ast = item.data.borrow();
        if let NodeValue::TaskItem(nti) = &ast.value {
            let text = collect_text(item).trim().to_string();
            let line = u32::try_from(ast.sourcepos.start.line).unwrap_or(0);
            tasks.push(ParsedTask {
                content: text,
                completed: nti.symbol.is_some(),
                line_number: line,
            });
        }
    }
}

/// Detect whether a blockquote is a callout.
///
/// If `text` starts with `[!TYPE]` where TYPE is ASCII alphabetic, returns
/// `("callout", Some(lowercase_type), title_and_body)`. Otherwise returns
/// `("blockquote", None, original_text)`.
fn detect_callout(text: &str) -> (String, Option<String>, String) {
    let trimmed = text.trim_start();
    if let Some(rest) = trimmed.strip_prefix("[!") {
        if let Some(close) = rest.find(']') {
            let callout_type_raw = &rest[..close];
            if !callout_type_raw.is_empty()
                && callout_type_raw.chars().all(|c| c.is_ascii_alphabetic())
            {
                let after_bracket = &rest[close + 1..];
                // Title is text on the same line after `]`, body is remaining lines.
                let content = after_bracket.trim().to_string();
                return (
                    "callout".to_string(),
                    Some(callout_type_raw.to_ascii_lowercase()),
                    content,
                );
            }
        }
    }
    ("blockquote".to_string(), None, text.to_string())
}

/// Detect a trailing block reference anchor (` ^some-id`) at the end of content.
///
/// Returns `(cleaned_content, Some(id))` when a valid anchor is found, or
/// `(original_content, None)` otherwise. The id must consist solely of
/// `[a-zA-Z0-9_-]` characters.
fn extract_block_ref(content: &str) -> (String, Option<String>) {
    if let Some(pos) = content.rfind(" ^") {
        let candidate = &content[pos + 2..];
        if !candidate.is_empty()
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            let cleaned = content[..pos].to_string();
            return (cleaned, Some(candidate.to_string()));
        }
    }
    (content.to_string(), None)
}

/// Split a link target on the first `#` to separate the path from a fragment.
///
/// Returns `(path, Some(fragment))` when `#` is present, or
/// `(original, None)` otherwise.
fn split_fragment(target: &str) -> (String, Option<String>) {
    if let Some(hash_pos) = target.find('#') {
        let path = target[..hash_pos].to_string();
        let fragment = target[hash_pos + 1..].to_string();
        (path, Some(fragment))
    } else {
        (target.to_string(), None)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── content_hash ──────────────────────────────────────────────────────

    #[test]
    fn content_hash_produces_hex_string() {
        let h = content_hash(b"hello");
        assert_eq!(h.len(), 64);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()), "not all hex: {h}");
    }

    #[test]
    fn content_hash_is_deterministic() {
        assert_eq!(content_hash(b"hello"), content_hash(b"hello"));
    }

    #[test]
    fn content_hash_differs_for_different_input() {
        assert_ne!(content_hash(b"hello"), content_hash(b"world"));
    }

    // ── parse_markdown basics ─────────────────────────────────────────────

    #[test]
    fn parse_empty_file() {
        let pf = parse_markdown("").unwrap();
        assert!(pf.blocks.is_empty());
        assert!(pf.frontmatter.is_empty());
        assert!(pf.links.is_empty());
        assert!(pf.tags.is_empty());
    }

    #[test]
    fn parse_simple_heading() {
        let pf = parse_markdown("# Hello\n").unwrap();
        assert_eq!(pf.blocks.len(), 1);
        let b = &pf.blocks[0];
        assert_eq!(b.block_type, "heading");
        assert_eq!(b.level, Some(1));
        assert_eq!(b.content, "Hello");
    }

    #[test]
    fn parse_multiple_headings() {
        let pf = parse_markdown("# H1\n## H2\n### H3\n").unwrap();
        let headings: Vec<_> = pf
            .blocks
            .iter()
            .filter(|b| b.block_type == "heading")
            .collect();
        assert_eq!(headings.len(), 3);
        assert_eq!(headings[0].level, Some(1));
        assert_eq!(headings[1].level, Some(2));
        assert_eq!(headings[2].level, Some(3));
    }

    #[test]
    fn parse_paragraph() {
        let pf = parse_markdown("Hello world\n").unwrap();
        assert_eq!(pf.blocks.len(), 1);
        assert_eq!(pf.blocks[0].block_type, "paragraph");
        assert_eq!(pf.blocks[0].content, "Hello world");
    }

    #[test]
    fn parse_code_block() {
        let pf = parse_markdown("```rust\nfn main() {}\n```\n").unwrap();
        assert_eq!(pf.blocks.len(), 1);
        let b = &pf.blocks[0];
        assert_eq!(b.block_type, "codeblock");
        assert!(b.content.contains("fn main()"));
    }

    // ── frontmatter ───────────────────────────────────────────────────────

    #[test]
    fn parse_frontmatter() {
        let md = "---\ntitle: My Note\n---\n# Body\n";
        let pf = parse_markdown(md).unwrap();
        let title = pf.frontmatter.iter().find(|p| p.key == "title");
        assert!(title.is_some(), "title property missing");
        assert_eq!(title.unwrap().value, r#""My Note""#);
    }

    #[test]
    fn parse_frontmatter_with_tags() {
        let md = "---\ntags:\n  - rust\n  - programming\n---\nHello\n";
        let pf = parse_markdown(md).unwrap();
        let fm_tags: Vec<_> = pf
            .tags
            .iter()
            .filter(|t| t.source == "frontmatter")
            .collect();
        assert_eq!(fm_tags.len(), 2);
        assert!(fm_tags.iter().any(|t| t.name == "rust"));
        assert!(fm_tags.iter().any(|t| t.name == "programming"));
    }

    #[test]
    fn parse_no_frontmatter() {
        let pf = parse_markdown("Just a paragraph\n").unwrap();
        assert!(pf.frontmatter.is_empty());
    }

    // ── inline tags ───────────────────────────────────────────────────────

    #[test]
    fn parse_inline_tags() {
        let pf = parse_markdown("Hello #rust and #programming\n").unwrap();
        let inline_tags: Vec<_> = pf
            .tags
            .iter()
            .filter(|t| t.source == "inline")
            .collect();
        assert!(inline_tags.iter().any(|t| t.name == "rust"), "rust missing");
        assert!(
            inline_tags.iter().any(|t| t.name == "programming"),
            "programming missing"
        );
    }

    // ── links ─────────────────────────────────────────────────────────────

    #[test]
    fn parse_wikilink() {
        let pf = parse_markdown("See [[other note]]\n").unwrap();
        let wl = pf.links.iter().find(|l| l.link_type == "wikilink");
        assert!(wl.is_some(), "no wikilink found");
        assert_eq!(wl.unwrap().link_text, "other note");
    }

    #[test]
    fn parse_wikilink_with_display_text() {
        let pf = parse_markdown("See [[path/to/note|display text]]\n").unwrap();
        let wl = pf
            .links
            .iter()
            .find(|l| l.link_type == "wikilink")
            .expect("wikilink");
        assert_eq!(wl.target_path, Some("path/to/note".to_string()));
        assert_eq!(wl.link_text, "display text");
    }

    #[test]
    fn parse_markdown_link() {
        let pf = parse_markdown("Click [here](https://example.com)\n").unwrap();
        let ml = pf
            .links
            .iter()
            .find(|l| l.link_type == "markdown")
            .expect("markdown link");
        assert_eq!(ml.target_path, Some("https://example.com".to_string()));
    }

    #[test]
    fn parse_embed() {
        let pf = parse_markdown("![[embedded-note]]\n").unwrap();
        let em = pf
            .links
            .iter()
            .find(|l| l.link_type == "embed")
            .expect("embed");
        assert_eq!(em.link_text, "embedded-note");
    }

    // ── table ─────────────────────────────────────────────────────────────

    #[test]
    fn parse_table() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |\n";
        let pf = parse_markdown(md).unwrap();
        let tbl = pf.blocks.iter().find(|b| b.block_type == "table");
        assert!(tbl.is_some(), "no table block found");
    }

    // ── list ──────────────────────────────────────────────────────────────

    #[test]
    fn parse_list() {
        let pf = parse_markdown("- item one\n- item two\n").unwrap();
        let lst = pf.blocks.iter().find(|b| b.block_type == "list");
        assert!(lst.is_some(), "no list block found");
    }

    // ── block references ─────────────────────────────────────────────────

    #[test]
    fn parse_block_ref_anchor() {
        let pf = parse_markdown("Hello world ^abc123\n").unwrap();
        assert_eq!(pf.blocks[0].block_ref_id, Some("abc123".to_string()));
        assert_eq!(pf.blocks[0].content, "Hello world");
    }

    #[test]
    fn parse_block_ref_mid_paragraph_ignored() {
        let pf = parse_markdown("Hello ^mid world\n").unwrap();
        assert_eq!(pf.blocks[0].block_ref_id, None);
        assert!(pf.blocks[0].content.contains("^mid"));
    }

    #[test]
    fn parse_heading_with_block_ref() {
        let pf = parse_markdown("## Section Title ^sec1\n").unwrap();
        let b = &pf.blocks[0];
        assert_eq!(b.block_ref_id, Some("sec1".to_string()));
        assert_eq!(b.content, "Section Title");
    }

    // ── link fragments ───────────────────────────────────────────────────

    #[test]
    fn parse_wikilink_with_heading_fragment() {
        let pf = parse_markdown("See [[note#Heading]]\n").unwrap();
        let wl = pf.links.iter().find(|l| l.link_type == "wikilink").unwrap();
        assert_eq!(wl.target_path, Some("note".to_string()));
        assert_eq!(wl.fragment, Some("Heading".to_string()));
    }

    #[test]
    fn parse_wikilink_with_block_ref_fragment() {
        let pf = parse_markdown("See [[note#^ref1]]\n").unwrap();
        let wl = pf.links.iter().find(|l| l.link_type == "wikilink").unwrap();
        assert_eq!(wl.target_path, Some("note".to_string()));
        assert_eq!(wl.fragment, Some("^ref1".to_string()));
    }

    #[test]
    fn parse_wikilink_no_fragment() {
        let pf = parse_markdown("See [[note]]\n").unwrap();
        let wl = pf.links.iter().find(|l| l.link_type == "wikilink").unwrap();
        assert_eq!(wl.fragment, None);
    }

    #[test]
    fn parse_wikilink_display_text_with_fragment() {
        let pf = parse_markdown("See [[note#Heading|display text]]\n").unwrap();
        let wl = pf.links.iter().find(|l| l.link_type == "wikilink").unwrap();
        assert_eq!(wl.target_path, Some("note".to_string()));
        assert_eq!(wl.fragment, Some("Heading".to_string()));
        assert_eq!(wl.link_text, "display text");
    }

    // ── callout detection ────────────────────────────────────────────────

    #[test]
    fn parse_callout_with_title() {
        let md = "> [!warning] Be careful\n> This is dangerous\n";
        let pf = parse_markdown(md).unwrap();
        let callout = pf.blocks.iter().find(|b| b.block_type == "callout");
        assert!(callout.is_some(), "no callout block found");
        let c = callout.unwrap();
        assert_eq!(c.callout_type, Some("warning".to_string()));
        assert!(c.content.contains("Be careful"));
    }

    #[test]
    fn parse_callout_no_title() {
        let md = "> [!tip]\n> Just a tip\n";
        let pf = parse_markdown(md).unwrap();
        let callout = pf.blocks.iter().find(|b| b.block_type == "callout");
        assert!(callout.is_some(), "no callout block found");
        let c = callout.unwrap();
        assert_eq!(c.callout_type, Some("tip".to_string()));
    }

    #[test]
    fn parse_regular_blockquote_not_callout() {
        let md = "> Just a regular quote\n";
        let pf = parse_markdown(md).unwrap();
        let blocks: Vec<_> = pf.blocks.iter().filter(|b| b.block_type == "callout").collect();
        assert!(blocks.is_empty(), "regular blockquote should not be a callout");
    }

    // ── task extraction ──────────────────────────────────────────────────

    #[test]
    fn parse_tasks_from_list() {
        let md = "- [ ] Buy groceries\n- [x] Write tests\n- [ ] Deploy app\n";
        let pf = parse_markdown(md).unwrap();
        assert_eq!(pf.tasks.len(), 3);
        assert!(!pf.tasks[0].completed);
        assert_eq!(pf.tasks[0].content, "Buy groceries");
        assert!(pf.tasks[1].completed);
        assert_eq!(pf.tasks[1].content, "Write tests");
        assert!(!pf.tasks[2].completed);
    }

    #[test]
    fn parse_no_tasks_in_regular_list() {
        let md = "- item one\n- item two\n";
        let pf = parse_markdown(md).unwrap();
        assert!(pf.tasks.is_empty());
    }
}
