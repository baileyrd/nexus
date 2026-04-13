//! Markdown parsing pipeline.
//!
//! Integrates comrak (`CommonMark` + GFM), YAML frontmatter, wikilinks,
//! inline tags, callouts, block-ref anchors, and math spans.

use std::collections::HashMap;
use std::path::Path;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, Options, parse_document};

use crate::error::MarkdownError;
use crate::util::sha256_hex;

pub mod embed;
pub mod extensions;
pub mod frontmatter;
pub mod wikilinks;

pub use embed::MAX_EMBED_DEPTH;
pub use extensions::{MathSpan, Tag, TagSource, detect_callout, extract_block_ref,
                     extract_inline_tags, extract_math_spans};
pub use frontmatter::{Frontmatter, extract as extract_frontmatter_raw};
pub use wikilinks::{LinkType, WikiLink, scan as scan_wikilinks};

// ── Public types ──────────────────────────────────────────────────────────────

/// Kind of a content block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// ATX or setext heading.
    Heading,
    /// Regular paragraph.
    Paragraph,
    /// Fenced or indented code block.
    CodeBlock,
    /// Ordered or unordered list.
    List,
    /// GFM table.
    Table,
    /// `[!TYPE]` callout.
    Callout,
    /// Regular `>` blockquote.
    BlockQuote,
}

/// A structured content block extracted from the AST.
#[derive(Debug, Clone)]
pub struct Block {
    /// What kind of block this is.
    pub kind: BlockKind,
    /// Heading level 1-6; `None` for non-headings.
    pub level: Option<u8>,
    /// Plain-text content (code literal for `CodeBlock`).
    pub content: String,
    /// 1-based start line in the source.
    pub start_line: u32,
    /// 1-based end line in the source.
    pub end_line: u32,
    /// Block-reference anchor ID (e.g. `"abc123"` from ` ^abc123`).
    pub block_ref_id: Option<String>,
    /// Callout type for callout blocks (e.g. `"warning"`, `"tip"`).
    pub callout_type: Option<String>,
}

/// A task item extracted from a checkbox list.
#[derive(Debug, Clone)]
pub struct Task {
    /// Task description text.
    pub content: String,
    /// Whether the task is checked.
    pub completed: bool,
    /// 1-based source line.
    pub line: u32,
}

/// Full parse result for a single `.md` / `.mdx` file.
#[derive(Debug, Clone)]
pub struct ParsedMarkdown {
    /// SHA-256 hex digest of the raw bytes.
    pub content_hash: String,
    /// Parsed YAML frontmatter.
    pub frontmatter: Frontmatter,
    /// Top-level content blocks.
    pub blocks: Vec<Block>,
    /// All wikilinks and embeds.
    pub links: Vec<WikiLink>,
    /// All tags (frontmatter + inline).
    pub tags: Vec<Tag>,
    /// Task items.
    pub tasks: Vec<Task>,
    /// Math spans (inline and block).
    pub math: Vec<MathSpan>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Parse a markdown string into structured output.
///
/// Extracts frontmatter, walks the AST for blocks/links/tags/tasks, and
/// scans for math spans.
///
/// # Errors
///
/// Returns [`MarkdownError::FrontmatterParse`] if YAML frontmatter is malformed.
#[allow(clippy::too_many_lines)]
pub fn parse(content: &str) -> Result<ParsedMarkdown, MarkdownError> {
    let content_hash = sha256_hex(content.as_bytes());

    let (fm, body) = frontmatter::extract(content)?;

    // Seed tag list with frontmatter tags.
    let mut tags: Vec<Tag> = fm.tags.iter().map(|t| Tag {
        name: t.clone(),
        source: TagSource::Frontmatter,
    }).collect();

    let arena = Arena::new();
    let mut opts = Options::default();
    opts.extension.strikethrough = true;
    opts.extension.table = true;
    opts.extension.autolink = true;
    opts.extension.tasklist = true;

    let root = parse_document(&arena, body, &opts);

    let mut blocks   = Vec::new();
    let mut links    = Vec::new();
    let mut tasks    = Vec::new();
    let mut math_all = Vec::new();

    for child in root.children() {
        let ast = child.data.borrow();
        let sp = &ast.sourcepos;
        let start_line = u32::try_from(sp.start.line).unwrap_or(0);
        let end_line   = u32::try_from(sp.end.line).unwrap_or(0);

        match &ast.value {
            NodeValue::Heading(h) => {
                let raw = collect_text(child);
                let (text, block_ref_id) = extract_block_ref(&raw);
                scan_and_add_wikilinks(&text, &mut links);
                extract_inline_tags(&text, &mut tags);
                let math = extract_math_spans(&text);
                math_all.extend(math);
                blocks.push(Block {
                    kind: BlockKind::Heading,
                    level: Some(h.level),
                    content: text,
                    start_line, end_line,
                    block_ref_id,
                    callout_type: None,
                });
            }
            NodeValue::Paragraph => {
                let raw = collect_text(child);
                let (text, block_ref_id) = extract_block_ref(&raw);
                scan_and_add_wikilinks(&text, &mut links);
                extract_inline_tags(&text, &mut tags);
                let math = extract_math_spans(&text);
                math_all.extend(math);
                blocks.push(Block {
                    kind: BlockKind::Paragraph,
                    level: None,
                    content: text,
                    start_line, end_line,
                    block_ref_id,
                    callout_type: None,
                });
            }
            NodeValue::CodeBlock(cb) => {
                blocks.push(Block {
                    kind: BlockKind::CodeBlock,
                    level: None,
                    content: cb.literal.clone(),
                    start_line, end_line,
                    block_ref_id: None,
                    callout_type: None,
                });
            }
            NodeValue::List(_) => {
                extract_task_items(child, &mut tasks);
                let raw = collect_text(child);
                let (text, block_ref_id) = extract_block_ref(&raw);
                scan_and_add_wikilinks(&text, &mut links);
                extract_inline_tags(&text, &mut tags);
                blocks.push(Block {
                    kind: BlockKind::List,
                    level: None,
                    content: text,
                    start_line, end_line,
                    block_ref_id,
                    callout_type: None,
                });
            }
            NodeValue::Table(_) => {
                let text = collect_text(child);
                blocks.push(Block {
                    kind: BlockKind::Table,
                    level: None,
                    content: text,
                    start_line, end_line,
                    block_ref_id: None,
                    callout_type: None,
                });
            }
            NodeValue::BlockQuote => {
                let raw = collect_text(child);
                let (btype, callout_type, after_callout) = detect_callout(&raw);
                let (content, block_ref_id) = extract_block_ref(&after_callout);
                scan_and_add_wikilinks(&content, &mut links);
                extract_inline_tags(&content, &mut tags);
                let kind = if btype == "callout" { BlockKind::Callout } else { BlockKind::BlockQuote };
                blocks.push(Block {
                    kind,
                    level: None,
                    content,
                    start_line, end_line,
                    block_ref_id,
                    callout_type,
                });
            }
            _ => {}
        }
    }

    // Deduplicate tags by (name, source).
    tags.dedup_by(|a, b| a.name == b.name && a.source == b.source);

    Ok(ParsedMarkdown {
        content_hash,
        frontmatter: fm,
        blocks,
        links,
        tags,
        tasks,
        math: math_all,
    })
}

/// Parse only the YAML frontmatter from a file path (cheap — no body parse).
///
/// Useful for sidebar indexing and search without a full document parse.
///
/// # Errors
///
/// Returns [`MarkdownError::FrontmatterParse`] if frontmatter is malformed,
/// or propagates I/O errors wrapped in the standard `std::io::Error` path.
pub fn parse_frontmatter(path: &Path) -> Result<HashMap<String, serde_json::Value>, MarkdownError> {
    let content = std::fs::read_to_string(path).map_err(|e| MarkdownError::FrontmatterParse {
        file: path.display().to_string(),
        reason: e.to_string(),
    })?;

    let (fm, _) = frontmatter::extract(&content)?;

    let mut map: HashMap<String, serde_json::Value> = HashMap::new();

    // Reserved keys → JSON values.
    if let Some(v) = fm.title    { map.insert("title".into(),    v.into()); }
    if let Some(v) = fm.doc_type { map.insert("type".into(),     v.into()); }
    if let Some(v) = fm.status   { map.insert("status".into(),   v.into()); }
    if let Some(v) = fm.cssclass { map.insert("cssclass".into(), v.into()); }
    if let Some(v) = fm.date     { map.insert("date".into(),     v.into()); }
    if let Some(v) = fm.created  { map.insert("created".into(),  v.into()); }
    if let Some(v) = fm.modified { map.insert("modified".into(), v.into()); }
    if !fm.aliases.is_empty() {
        map.insert("aliases".into(), fm.aliases.into());
    }
    if !fm.tags.is_empty() {
        map.insert("tags".into(), fm.tags.into());
    }

    // Custom keys.
    map.extend(fm.custom);

    Ok(map)
}

/// Resolve a wikilink target to a vault file path.
///
/// 1. Tries `source_dir/target` (relative to the containing file).
/// 2. Tries walking `forge_root` for a file whose stem matches the target name.
/// 3. Returns `None` for broken links.
#[must_use]
pub fn resolve_wikilink(
    target: &str,
    source_dir: &Path,
    forge_root: &Path,
) -> Option<std::path::PathBuf> {
    // Attempt 1: relative to the containing file.
    let rel = source_dir.join(target);
    if rel.exists() {
        return Some(rel);
    }

    // Attempt 2: global stem match within forge root.
    let target_name = std::path::Path::new(target)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(target);

    find_by_stem(forge_root, target_name)
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn scan_and_add_wikilinks(text: &str, links: &mut Vec<WikiLink>) {
    links.extend(wikilinks::scan(text));
}

fn collect_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut buf = String::new();
    collect_text_into(node, &mut buf);
    buf
}

fn collect_text_into<'a>(node: &'a AstNode<'a>, buf: &mut String) {
    match &node.data.borrow().value {
        NodeValue::Text(t)               => buf.push_str(t),
        NodeValue::Code(c)               => buf.push_str(&c.literal),
        NodeValue::SoftBreak | NodeValue::LineBreak => buf.push(' '),
        _ => {}
    }
    for child in node.children() {
        collect_text_into(child, buf);
    }
}

fn extract_task_items<'a>(list_node: &'a AstNode<'a>, tasks: &mut Vec<Task>) {
    for item in list_node.children() {
        let ast = item.data.borrow();
        if let NodeValue::TaskItem(nti) = &ast.value {
            let text = collect_text(item).trim().to_string();
            let line = u32::try_from(ast.sourcepos.start.line).unwrap_or(0);
            tasks.push(Task {
                content: text,
                completed: nti.symbol.is_some(),
                line,
            });
        }
    }
}

/// Recursively search `dir` for a file whose file-stem matches `name`.
fn find_by_stem(dir: &Path, name: &str) -> Option<std::path::PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return None };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_by_stem(&path, name) {
                return Some(found);
            }
        } else {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let full_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if stem == name || full_name == name {
                return Some(path);
            }
        }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty() {
        let r = parse("").unwrap();
        assert!(r.blocks.is_empty());
        assert!(r.links.is_empty());
        assert!(r.tags.is_empty());
    }

    #[test]
    fn parse_heading() {
        let r = parse("# Hello\n").unwrap();
        assert_eq!(r.blocks.len(), 1);
        assert_eq!(r.blocks[0].kind, BlockKind::Heading);
        assert_eq!(r.blocks[0].level, Some(1));
        assert_eq!(r.blocks[0].content, "Hello");
    }

    #[test]
    fn parse_multiple_headings() {
        let r = parse("# H1\n## H2\n### H3\n").unwrap();
        let headings: Vec<_> = r.blocks.iter().filter(|b| b.kind == BlockKind::Heading).collect();
        assert_eq!(headings.len(), 3);
        assert_eq!(headings[0].level, Some(1));
        assert_eq!(headings[2].level, Some(3));
    }

    #[test]
    fn parse_paragraph() {
        let r = parse("Hello world\n").unwrap();
        assert_eq!(r.blocks[0].kind, BlockKind::Paragraph);
        assert_eq!(r.blocks[0].content, "Hello world");
    }

    #[test]
    fn parse_code_block() {
        let r = parse("```rust\nfn main() {}\n```\n").unwrap();
        let cb = r.blocks.iter().find(|b| b.kind == BlockKind::CodeBlock).unwrap();
        assert!(cb.content.contains("fn main()"));
    }

    #[test]
    fn parse_frontmatter_title() {
        let md = "---\ntitle: My Note\n---\n# Body\n";
        let r = parse(md).unwrap();
        assert_eq!(r.frontmatter.title.as_deref(), Some("My Note"));
    }

    #[test]
    fn parse_frontmatter_tags_in_tags_list() {
        let md = "---\ntags:\n  - rust\n  - testing\n---\nHello\n";
        let r = parse(md).unwrap();
        let fm_tags: Vec<_> = r.tags.iter().filter(|t| t.source == TagSource::Frontmatter).collect();
        assert_eq!(fm_tags.len(), 2);
    }

    #[test]
    fn parse_inline_tags() {
        let r = parse("Hello #rust and #nexus\n").unwrap();
        let inline: Vec<_> = r.tags.iter().filter(|t| t.source == TagSource::Inline).collect();
        assert!(inline.iter().any(|t| t.name == "rust"));
        assert!(inline.iter().any(|t| t.name == "nexus"));
    }

    #[test]
    fn parse_wikilink() {
        let r = parse("See [[other note]]\n").unwrap();
        let wl = r.links.iter().find(|l| l.link_type == LinkType::Wikilink).unwrap();
        assert_eq!(wl.target, "other note");
    }

    #[test]
    fn parse_wikilink_display_text() {
        let r = parse("See [[path/to/note|display text]]\n").unwrap();
        let wl = r.links.iter().find(|l| l.link_type == LinkType::Wikilink).unwrap();
        assert_eq!(wl.target, "path/to/note");
        assert_eq!(wl.display.as_deref(), Some("display text"));
    }

    #[test]
    fn parse_embed() {
        let r = parse("![[embedded-note]]\n").unwrap();
        let em = r.links.iter().find(|l| l.link_type == LinkType::Embed).unwrap();
        assert_eq!(em.target, "embedded-note");
    }

    #[test]
    fn parse_table() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |\n";
        let r = parse(md).unwrap();
        assert!(r.blocks.iter().any(|b| b.kind == BlockKind::Table));
    }

    #[test]
    fn parse_list() {
        let r = parse("- item one\n- item two\n").unwrap();
        assert!(r.blocks.iter().any(|b| b.kind == BlockKind::List));
    }

    #[test]
    fn parse_block_ref_anchor() {
        let r = parse("Hello world ^abc123\n").unwrap();
        assert_eq!(r.blocks[0].block_ref_id, Some("abc123".to_string()));
        assert_eq!(r.blocks[0].content, "Hello world");
    }

    #[test]
    fn parse_callout() {
        let md = "> [!warning] Be careful\n> This is dangerous\n";
        let r = parse(md).unwrap();
        let callout = r.blocks.iter().find(|b| b.kind == BlockKind::Callout).unwrap();
        assert_eq!(callout.callout_type, Some("warning".to_string()));
    }

    #[test]
    fn parse_tasks() {
        let md = "- [ ] Buy groceries\n- [x] Write tests\n";
        let r = parse(md).unwrap();
        assert_eq!(r.tasks.len(), 2);
        assert!(!r.tasks[0].completed);
        assert!(r.tasks[1].completed);
    }

    #[test]
    fn content_hash_is_64_hex_chars() {
        let r = parse("hello\n").unwrap();
        assert_eq!(r.content_hash.len(), 64);
        assert!(r.content_hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn wikilink_fragment_preserved() {
        let r = parse("[[note#Section]]\n").unwrap();
        let wl = r.links.iter().find(|l| l.link_type == LinkType::Wikilink).unwrap();
        assert_eq!(wl.fragment.as_deref(), Some("Section"));
    }
}
