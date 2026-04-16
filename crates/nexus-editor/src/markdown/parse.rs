//! Markdown source → [`BlockTree`] (PRD 08 §3.1–3.2).
//!
//! The walker runs comrak on the post-frontmatter body, then converts
//! each top-level `NodeValue` to one or more [`Block`]s. Lists expand
//! each `Item` into its own block; tables flatten rows into
//! [`BlockType::TableRow`] children. Inline extraction is delegated to
//! [`super::inline`].

use std::collections::HashMap;

use comrak::nodes::{AstNode, ListType, NodeValue, TableAlignment};
use comrak::{parse_document, Arena, Options};
use nexus_formats::markdown::extensions::{detect_callout, extract_block_ref};
use nexus_formats::markdown::frontmatter;

use crate::annotation::{Annotation, AnnotationType};
use crate::block::{now_ms, Block, BlockId, BlockType, DocumentMetadata, FileType, PropertyValue};
use crate::error::{EditorError, Result};
use crate::tree::BlockTree;

use super::id::deterministic_block_id;
use super::inline::collect_inline;
use super::ParseOptions;

/// Parse `source` into a [`BlockTree`] using `options`.
///
/// # Errors
/// - [`EditorError::TransactionInvalid`] if frontmatter YAML is malformed.
/// - [`EditorError::InvalidTree`] from validation at the end of the walk.
pub fn parse(source: &str, options: &ParseOptions) -> Result<BlockTree> {
    // 1. Frontmatter extraction.
    let (fm, body) = frontmatter::extract(source)
        .map_err(|e| EditorError::TransactionInvalid(format!("frontmatter parse failed: {e}")))?;
    let properties = frontmatter_to_properties(&fm);

    // 2. Comrak parse.
    let arena = Arena::new();
    let opts = comrak_options(options);
    let root = parse_document(&arena, body, &opts);

    // 3. Walk AST.
    let mut tree = BlockTree::new(DocumentMetadata {
        file_path: options.file_path.clone(),
        file_type: FileType::Markdown,
        created_at: now_ms(),
        updated_at: now_ms(),
        word_count: 0,
        read_time_seconds: 0,
        properties,
    });
    let mut walker = Walker {
        tree: &mut tree,
        options,
        visit_order: 0,
    };
    for child in root.children() {
        walker.visit_block(child, None);
    }

    // 4. Populate derived metadata.
    let words = count_words(&tree);
    tree.metadata.word_count = words;
    tree.metadata.read_time_seconds = (words / 200).max(1);

    // 5. Validate invariants.
    tree.validate()?;
    Ok(tree)
}

// ── Walker ────────────────────────────────────────────────────────────────────

struct Walker<'a, 't> {
    tree: &'t mut BlockTree,
    options: &'a ParseOptions,
    visit_order: usize,
}

impl Walker<'_, '_> {
    fn insert_block(&mut self, block: Block, parent: Option<BlockId>) -> BlockId {
        let id = block.id;
        let index = match parent {
            Some(pid) => self.tree.get(pid).map_or(0, |b| b.children.len()),
            None => self.tree.root_blocks.len(),
        };
        // SAFETY: insert only fails on duplicate id / bad index; both are
        // ruled out by construction (deterministic IDs are unique per
        // slot and `index` is the current length).
        self.tree
            .insert(block, parent, index)
            .expect("walker insertion always well-formed");
        id
    }

    fn new_block(&mut self, ty: BlockType) -> Block {
        let id = deterministic_block_id(&self.options.file_path, self.visit_order, &ty);
        self.visit_order += 1;
        let mut block = Block::new(ty);
        block.id = id;
        block
    }

    /// Handle a top-level (or block-context) AST node.
    #[allow(clippy::too_many_lines)]
    fn visit_block<'a>(&mut self, node: &'a AstNode<'a>, parent: Option<BlockId>) {
        let value = node.data.borrow().value.clone();
        match value {
            NodeValue::Heading(h) => {
                let (content, mut anns) = collect_inline(node);
                let (clean, block_ref_id) = extract_block_ref(&content);
                // Re-shift: extract_block_ref only mutates a trailing
                // " ^id" segment. If anns reference bytes past clean.len(),
                // trim them.
                anns.retain(|a| a.end <= clean.len());
                let mut block = self.new_block(BlockType::Heading { level: h.level });
                if let Some(bref) = block_ref_id {
                    block
                        .properties
                        .computed
                        .insert("block_ref".into(), PropertyValue::String(bref));
                }
                block.content = clean;
                block.annotations = anns;
                self.insert_block(block, parent);
            }
            NodeValue::Paragraph => {
                let (content, mut anns) = collect_inline(node);

                // Promote a bare `![[target]]` paragraph to an Embed block.
                if let Some(embed_url) = bare_embed_target(&content) {
                    let block = self.new_block(BlockType::Embed {
                        url: embed_url,
                        embed_type: crate::EmbedType::Note,
                        title: None,
                        metadata: HashMap::new(),
                    });
                    self.insert_block(block, parent);
                    return;
                }

                let (clean, block_ref_id) = extract_block_ref(&content);
                anns.retain(|a| a.end <= clean.len());

                let mut block = self.new_block(BlockType::Paragraph);
                if let Some(bref) = block_ref_id {
                    block
                        .properties
                        .computed
                        .insert("block_ref".into(), PropertyValue::String(bref));
                }
                block.content = clean;
                block.annotations = anns;
                self.insert_block(block, parent);
            }
            NodeValue::CodeBlock(cb) => {
                // `cb.info` is the language tag. Math blocks `$$..$$`
                // sometimes parse as code blocks with empty info; we
                // also catch them downstream via paragraph detection.
                let block = self.new_block(BlockType::CodeBlock {
                    language: cb.info.clone(),
                    line_numbers: false,
                });
                let mut block = block;
                block.content = cb.literal.trim_end_matches('\n').to_string();
                self.insert_block(block, parent);
            }
            NodeValue::ThematicBreak => {
                let block = self.new_block(BlockType::Divider);
                self.insert_block(block, parent);
            }
            NodeValue::List(list) => {
                // Expand each Item into a list-item block. Nested lists
                // inside an item become children of that item.
                let indent_level = parent
                    .and_then(|pid| self.tree.get(pid).map(list_indent_level))
                    .unwrap_or(0);
                let list_type = list.list_type;
                let start_number = list.start;
                for (i, item) in node.children().enumerate() {
                    let number = u32::try_from(start_number + i).unwrap_or(u32::MAX);
                    self.visit_list_item(item, parent, list_type, indent_level, number);
                }
            }
            NodeValue::Table(t) => {
                let block = self.new_block(BlockType::Table {
                    column_count: t.num_columns,
                    header_row: true,
                });
                let table_id = self.insert_block(block, parent);

                // Rows are children.
                for row in node.children() {
                    if let NodeValue::TableRow(_header) = row.data.borrow().value.clone() {
                        let cells: Vec<String> = row
                            .children()
                            .map(|cell| flatten_text(cell).trim().to_string())
                            .collect();
                        let mut row_block = self.new_block(BlockType::TableRow { cells });
                        row_block.content = String::new();
                        self.insert_block(row_block, Some(table_id));
                    }
                }
                // Persist alignments on the Table block for serializer use.
                let alignments: Vec<String> = t
                    .alignments
                    .iter()
                    .copied()
                    .map(alignment_to_string)
                    .collect();
                if let Some(b) = self.tree.get_mut(table_id) {
                    b.properties.computed.insert(
                        "table.alignments".into(),
                        PropertyValue::List(
                            alignments.into_iter().map(PropertyValue::String).collect(),
                        ),
                    );
                }
            }
            NodeValue::BlockQuote => {
                // Flatten inner text to decide Callout vs Quote.
                let inner_text = flatten_text(node);
                let (kind, callout_type, _after) = detect_callout(&inner_text);
                if kind == "callout" {
                    let alert_type = callout_type.unwrap_or_else(|| "note".into());
                    let mut block = self.new_block(BlockType::Callout {
                        icon: String::new(),
                        color: String::new(),
                        alert_type: alert_type.clone(),
                    });

                    // Steal the first paragraph's inline content onto
                    // the Callout itself (stripping the `[!type]` prefix).
                    let prefix = format!("[!{alert_type}]");
                    let mut first_para_stolen = false;
                    for child in node.children() {
                        if matches!(child.data.borrow().value, NodeValue::Paragraph) {
                            let (raw, raw_anns) = collect_inline(child);
                            let trimmed = raw
                                .strip_prefix(&prefix)
                                .unwrap_or(&raw)
                                .trim_start()
                                .to_string();
                            let offset = raw.len() - trimmed.len();
                            let anns: Vec<Annotation> = raw_anns
                                .into_iter()
                                .filter_map(|mut a| {
                                    if a.start >= offset {
                                        a.start -= offset;
                                        a.end -= offset;
                                        Some(a)
                                    } else if a.end > offset {
                                        a.start = 0;
                                        a.end -= offset;
                                        Some(a)
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            block.content = trimmed;
                            block.annotations = anns;
                            first_para_stolen = true;
                            break;
                        }
                    }

                    let callout_id = self.insert_block(block, parent);

                    // Process remaining children, skipping the first paragraph.
                    let mut skipped_first = !first_para_stolen;
                    for child in node.children() {
                        if !skipped_first
                            && matches!(child.data.borrow().value, NodeValue::Paragraph)
                        {
                            skipped_first = true;
                            continue;
                        }
                        self.visit_block(child, Some(callout_id));
                    }
                } else {
                    let block = self.new_block(BlockType::Quote);
                    let id = self.insert_block(block, parent);
                    for child in node.children() {
                        self.visit_block(child, Some(id));
                    }
                }
            }
            NodeValue::Image(l) => {
                // Rare: a top-level Image (not wrapped in a paragraph).
                let alt = flatten_text(node);
                let block = self.new_block(BlockType::Image {
                    src: l.url.clone(),
                    alt_text: alt,
                    width: None,
                    height: None,
                });
                self.insert_block(block, parent);
            }
            NodeValue::HtmlBlock(html) => {
                // Preserve raw HTML as a paragraph to avoid data loss.
                let mut block = self.new_block(BlockType::Paragraph);
                block.content = html.literal.trim_end_matches('\n').to_string();
                block.annotations = vec![Annotation {
                    start: 0,
                    end: block.content.len(),
                    ty: AnnotationType::Custom {
                        plugin_id: "nexus-editor".into(),
                        ty: "html".into(),
                        data: HashMap::new(),
                    },
                }];
                self.insert_block(block, parent);
            }
            _ => {
                // Inline / structural / unhandled block nodes. Any
                // `NodeValue` that slips through here is outside our
                // scope for this session (MDX components, footnotes,
                // description lists, etc.) — dropped silently.
            }
        }
    }

    fn visit_list_item<'a>(
        &mut self,
        item: &'a AstNode<'a>,
        parent: Option<BlockId>,
        list_type: ListType,
        indent_level: u8,
        number: u32,
    ) {
        let (is_task, checked) = match item.data.borrow().value.clone() {
            NodeValue::Item(_) => (false, false),
            NodeValue::TaskItem(mark) => (true, mark.symbol.is_some()),
            _ => return,
        };

        let ty = match list_type {
            ListType::Bullet => BlockType::BulletList { indent_level },
            ListType::Ordered => BlockType::NumberedList {
                indent_level,
                number,
            },
        };

        // The first paragraph (if any) supplies the item's content.
        let mut content = String::new();
        let mut anns: Vec<Annotation> = Vec::new();
        for child in item.children() {
            if matches!(child.data.borrow().value, NodeValue::Paragraph) {
                let (c, a) = collect_inline(child);
                content = c;
                anns = a;
                break;
            }
        }

        let mut block = self.new_block(ty);
        block.content = content;
        block.annotations = anns;
        if is_task {
            block
                .properties
                .attributes
                .insert("task.completed".into(), PropertyValue::Boolean(checked));
        }
        let item_id = self.insert_block(block, parent);

        // Remaining children (nested lists, additional paragraphs, etc.)
        // become children of this list-item block.
        let mut saw_first_para = false;
        for child in item.children() {
            if !saw_first_para && matches!(child.data.borrow().value, NodeValue::Paragraph) {
                saw_first_para = true;
                continue;
            }
            self.visit_block(child, Some(item_id));
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn comrak_options(options: &ParseOptions) -> Options<'_> {
    let mut opts = Options::default();
    if options.gfm_enabled {
        opts.extension.strikethrough = true;
        opts.extension.table = true;
        opts.extension.autolink = true;
        opts.extension.tasklist = true;
    }
    opts.extension.tagfilter = false;
    opts.extension.footnotes = false;
    opts.extension.description_lists = false;
    opts.extension.header_id_prefix = Some(String::new());
    opts.parse.smart = false;
    opts
}

fn flatten_into<'a>(node: &'a AstNode<'a>, buf: &mut String) {
    let value = node.data.borrow().value.clone();
    match value {
        NodeValue::Text(s) => buf.push_str(&s),
        NodeValue::SoftBreak | NodeValue::LineBreak => buf.push(' '),
        NodeValue::Code(c) => {
            let bt = "`".repeat(c.num_backticks.max(1));
            buf.push_str(&bt);
            buf.push_str(&c.literal);
            buf.push_str(&bt);
        }
        NodeValue::Link(l) => {
            if l.url.is_empty() {
                for child in node.children() {
                    flatten_into(child, buf);
                }
            } else {
                // Child text is the link text; we don't re-emit
                // `[text](url)` here because the serializer will do it
                // via the Link annotation produced by collect_inline.
                for child in node.children() {
                    flatten_into(child, buf);
                }
            }
        }
        NodeValue::Image(l) => {
            buf.push_str("![");
            for child in node.children() {
                flatten_into(child, buf);
            }
            buf.push_str("](");
            buf.push_str(&l.url);
            buf.push(')');
        }
        NodeValue::HtmlInline(s) => buf.push_str(&s),
        _ => {
            for child in node.children() {
                flatten_into(child, buf);
            }
        }
    }
}

fn flatten_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut buf = String::new();
    for child in node.children() {
        flatten_into(child, &mut buf);
    }
    buf
}

fn bare_embed_target(content: &str) -> Option<String> {
    let trimmed = content.trim();
    let rest = trimmed.strip_prefix("![[")?.strip_suffix("]]")?;
    if rest.is_empty() || rest.contains("[[") {
        return None;
    }
    Some(rest.to_string())
}

fn list_indent_level(parent_item: &Block) -> u8 {
    match parent_item.ty {
        BlockType::BulletList { indent_level }
        | BlockType::NumberedList { indent_level, .. }
        | BlockType::ToggleList { indent_level, .. } => indent_level.saturating_add(1),
        _ => 0,
    }
}

fn frontmatter_to_properties(fm: &frontmatter::Frontmatter) -> HashMap<String, PropertyValue> {
    let mut map: HashMap<String, PropertyValue> = HashMap::new();
    if let Some(v) = &fm.title {
        map.insert("title".into(), PropertyValue::String(v.clone()));
    }
    if let Some(v) = &fm.doc_type {
        map.insert("type".into(), PropertyValue::String(v.clone()));
    }
    if let Some(v) = &fm.status {
        map.insert("status".into(), PropertyValue::String(v.clone()));
    }
    if let Some(v) = &fm.cssclass {
        map.insert("cssclass".into(), PropertyValue::String(v.clone()));
    }
    if let Some(v) = &fm.date {
        map.insert("date".into(), PropertyValue::String(v.clone()));
    }
    if let Some(v) = &fm.created {
        map.insert("created".into(), PropertyValue::String(v.clone()));
    }
    if let Some(v) = &fm.modified {
        map.insert("modified".into(), PropertyValue::String(v.clone()));
    }
    if !fm.aliases.is_empty() {
        map.insert(
            "aliases".into(),
            PropertyValue::List(
                fm.aliases
                    .iter()
                    .cloned()
                    .map(PropertyValue::String)
                    .collect(),
            ),
        );
    }
    if !fm.tags.is_empty() {
        map.insert(
            "tags".into(),
            PropertyValue::List(fm.tags.iter().cloned().map(PropertyValue::String).collect()),
        );
    }
    for (k, v) in &fm.custom {
        map.insert(k.clone(), json_to_property(v));
    }
    map
}

fn json_to_property(v: &serde_json::Value) -> PropertyValue {
    match v {
        serde_json::Value::Null => PropertyValue::String(String::new()),
        serde_json::Value::Bool(b) => PropertyValue::Boolean(*b),
        serde_json::Value::Number(n) => PropertyValue::Number(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => PropertyValue::String(s.clone()),
        serde_json::Value::Array(a) => {
            PropertyValue::List(a.iter().map(json_to_property).collect())
        }
        serde_json::Value::Object(o) => PropertyValue::Object(
            o.iter()
                .map(|(k, v)| (k.clone(), json_to_property(v)))
                .collect(),
        ),
    }
}

fn alignment_to_string(a: TableAlignment) -> String {
    match a {
        TableAlignment::None => "none".into(),
        TableAlignment::Left => "left".into(),
        TableAlignment::Center => "center".into(),
        TableAlignment::Right => "right".into(),
    }
}

fn count_words(tree: &BlockTree) -> usize {
    tree.blocks
        .values()
        .map(|b| b.content.split_whitespace().count())
        .sum()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_default(src: &str) -> BlockTree {
        parse(src, &ParseOptions::default()).unwrap()
    }

    #[test]
    fn empty_document_yields_empty_tree() {
        let tree = parse_default("");
        assert!(tree.is_empty());
    }

    #[test]
    fn heading_levels() {
        for level in 1_u8..=6 {
            let src = format!("{} H{level}\n", "#".repeat(level as usize));
            let tree = parse_default(&src);
            let b = tree.get(tree.root_blocks[0]).unwrap();
            assert_eq!(b.ty, BlockType::Heading { level });
            assert_eq!(b.content, format!("H{level}"));
        }
    }

    #[test]
    fn paragraph_extracts_plain_text() {
        let tree = parse_default("Hello world\n");
        let b = tree.get(tree.root_blocks[0]).unwrap();
        assert_eq!(b.ty, BlockType::Paragraph);
        assert_eq!(b.content, "Hello world");
    }

    #[test]
    fn bullet_list_yields_one_block_per_item() {
        let tree = parse_default("- a\n- b\n- c\n");
        assert_eq!(tree.root_blocks.len(), 3);
        for id in &tree.root_blocks {
            let b = tree.get(*id).unwrap();
            assert!(matches!(b.ty, BlockType::BulletList { indent_level: 0 }));
        }
    }

    #[test]
    fn nested_bullet_list_becomes_child_block() {
        let tree = parse_default("- outer\n  - inner\n- outer2\n");
        assert_eq!(tree.root_blocks.len(), 2);
        let outer = tree.get(tree.root_blocks[0]).unwrap();
        assert_eq!(outer.children.len(), 1);
        let inner = tree.get(outer.children[0]).unwrap();
        assert!(matches!(
            inner.ty,
            BlockType::BulletList { indent_level: 1 }
        ));
        assert_eq!(inner.content, "inner");
    }

    #[test]
    fn numbered_list_carries_number() {
        let tree = parse_default("1. a\n2. b\n");
        let first = tree.get(tree.root_blocks[0]).unwrap();
        assert_eq!(
            first.ty,
            BlockType::NumberedList {
                indent_level: 0,
                number: 1
            }
        );
        let second = tree.get(tree.root_blocks[1]).unwrap();
        assert_eq!(
            second.ty,
            BlockType::NumberedList {
                indent_level: 0,
                number: 2
            }
        );
    }

    #[test]
    fn task_items_set_property() {
        let tree = parse_default("- [ ] first\n- [x] second\n");
        let first = tree.get(tree.root_blocks[0]).unwrap();
        assert_eq!(
            first.properties.attributes.get("task.completed"),
            Some(&PropertyValue::Boolean(false))
        );
        let second = tree.get(tree.root_blocks[1]).unwrap();
        assert_eq!(
            second.properties.attributes.get("task.completed"),
            Some(&PropertyValue::Boolean(true))
        );
    }

    #[test]
    fn code_block_captures_language() {
        let tree = parse_default("```rust\nfn main() {}\n```\n");
        let b = tree.get(tree.root_blocks[0]).unwrap();
        match &b.ty {
            BlockType::CodeBlock { language, .. } => assert_eq!(language, "rust"),
            other => panic!("expected CodeBlock, got {other:?}"),
        }
        assert!(b.content.contains("fn main()"));
    }

    #[test]
    fn divider_recognized() {
        let tree = parse_default("a\n\n---\n\nb\n");
        let has_divider = tree
            .root_blocks
            .iter()
            .any(|id| matches!(tree.get(*id).unwrap().ty, BlockType::Divider));
        assert!(has_divider);
    }

    #[test]
    fn quote_vs_callout_detection() {
        let q = parse_default("> plain\n");
        assert!(matches!(
            q.get(q.root_blocks[0]).unwrap().ty,
            BlockType::Quote
        ));

        let c = parse_default("> [!info] hello\n> more\n");
        match &c.get(c.root_blocks[0]).unwrap().ty {
            BlockType::Callout { alert_type, .. } => assert_eq!(alert_type, "info"),
            other => panic!("expected Callout, got {other:?}"),
        }
    }

    #[test]
    fn table_with_header_and_rows() {
        let tree = parse_default("| a | b |\n| --- | --- |\n| 1 | 2 |\n");
        let tbl = tree.get(tree.root_blocks[0]).unwrap();
        match &tbl.ty {
            BlockType::Table {
                column_count,
                header_row,
            } => {
                assert_eq!(*column_count, 2);
                assert!(*header_row);
            }
            other => panic!("expected Table, got {other:?}"),
        }
        assert_eq!(tbl.children.len(), 2);
        let header = tree.get(tbl.children[0]).unwrap();
        match &header.ty {
            BlockType::TableRow { cells } => assert_eq!(cells, &["a", "b"]),
            other => panic!("expected TableRow, got {other:?}"),
        }
    }

    #[test]
    fn bare_embed_paragraph_promotes_to_embed_block() {
        let tree = parse_default("![[my-note]]\n");
        let b = tree.get(tree.root_blocks[0]).unwrap();
        match &b.ty {
            BlockType::Embed { url, .. } => assert_eq!(url, "my-note"),
            other => panic!("expected Embed, got {other:?}"),
        }
    }

    #[test]
    fn block_ref_anchor_extracted_to_properties() {
        let tree = parse_default("Some text ^block-id-1\n");
        let b = tree.get(tree.root_blocks[0]).unwrap();
        assert_eq!(b.content, "Some text");
        assert_eq!(
            b.properties.computed.get("block_ref"),
            Some(&PropertyValue::String("block-id-1".into()))
        );
    }

    #[test]
    fn frontmatter_flows_into_document_metadata() {
        let src = "---\ntitle: Hi\ntags: [a, b]\n---\n# Body\n";
        let tree = parse_default(src);
        assert_eq!(
            tree.metadata.properties.get("title"),
            Some(&PropertyValue::String("Hi".into()))
        );
        match tree.metadata.properties.get("tags").unwrap() {
            PropertyValue::List(items) => assert_eq!(items.len(), 2),
            _ => panic!("expected tags list"),
        }
    }

    #[test]
    fn word_count_populated() {
        let tree = parse_default("# Heading\n\nOne two three four five.\n");
        assert_eq!(tree.metadata.word_count, 6);
        assert_eq!(tree.metadata.read_time_seconds, 1);
    }

    #[test]
    fn deterministic_ids_are_stable() {
        let src = "# Hi\n\nHello\n";
        let opts = ParseOptions {
            file_path: "/x.md".into(),
            ..ParseOptions::default()
        };
        let a = parse(src, &opts).unwrap();
        let b = parse(src, &opts).unwrap();
        assert_eq!(a.root_blocks, b.root_blocks);
    }
}
