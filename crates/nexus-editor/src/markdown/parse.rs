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

use super::database_view_spec::{bare_database_view_target, parse_database_view_spec};
use super::id::{deterministic_block_id, parse_stable_id_marker, strip_trailing_stable_id_marker};
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
        pending_stamp: None,
    };
    walker.visit_siblings(None, root.children());

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
    /// Carried stamp id for the next [`Self::new_block`] call, set by
    /// [`Self::visit_siblings`] when a sibling `<!-- ^<uuid> -->`
    /// `HtmlBlock` is peeked ahead, or by an inline-content match arm
    /// after [`strip_trailing_stable_id_marker`] consumes a trailing
    /// marker out of the block's text. ADR 0017.
    pending_stamp: Option<BlockId>,
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
        let stamp = self.pending_stamp.take();
        let id = stamp.unwrap_or_else(|| {
            deterministic_block_id(&self.options.file_path, self.visit_order, &ty)
        });
        self.visit_order += 1;
        let mut block = Block::new(ty);
        block.id = id;
        block.stable_id = stamp;
        block
    }

    /// Walk a sequence of sibling AST nodes, consuming any
    /// `<!-- ^<uuid> -->` `HtmlBlock` that follows a block as a stamp
    /// marker for the previous block (ADR 0017 block-level form).
    /// Inline-form markers are stripped per-block inside
    /// [`Self::visit_block`] from the collected content.
    fn visit_siblings<'a, I>(&mut self, parent: Option<BlockId>, children: I)
    where
        I: IntoIterator<Item = &'a AstNode<'a>>,
    {
        let nodes: Vec<&AstNode> = children.into_iter().collect();
        let mut i = 0;
        while i < nodes.len() {
            // Peek next sibling for a block-level stamp marker.
            let stamp = nodes
                .get(i + 1)
                .and_then(|n| extract_html_block_marker(n));
            self.pending_stamp = stamp;
            self.visit_block(nodes[i], parent);
            i += if stamp.is_some() { 2 } else { 1 };
            // Clear residual: visit_block normally consumes pending_stamp
            // through new_block, but skipping a block (e.g. an inline node
            // with no match arm) would leak it into the next iteration.
            self.pending_stamp = None;
        }
    }

    /// Handle a top-level (or block-context) AST node.
    #[allow(clippy::too_many_lines)]
    fn visit_block<'a>(&mut self, node: &'a AstNode<'a>, parent: Option<BlockId>) {
        let value = node.data.borrow().value.clone();
        match value {
            NodeValue::Heading(h) => {
                let (raw, mut anns) = collect_inline(node);
                let content = self.strip_inline_stamp(raw, &mut anns);
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
                let (raw, mut anns) = collect_inline(node);
                let content = self.strip_inline_stamp(raw, &mut anns);

                // Promote a bare `[[{db:…}]]` paragraph to a
                // DatabaseView block (BL-012 close-out). Order matters
                // — the embed check below would otherwise consume any
                // `![[…]]` shape, but the database-view syntax has no
                // bang prefix so the two paths can't collide. A
                // malformed spec (empty path, traversal, unknown view)
                // falls through to the regular paragraph path so the
                // user sees their typed source verbatim and can fix it
                // inline.
                if let Some(spec) = bare_database_view_target(&content) {
                    if let Ok((database_path, view_config)) = parse_database_view_spec(spec) {
                        let block = self.new_block(BlockType::DatabaseView {
                            database_path,
                            view_config,
                        });
                        self.insert_block(block, parent);
                        return;
                    }
                }

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
                //
                // A stamp captured by `visit_siblings` on the list as a
                // whole has no defined semantics (it could mean "stamp
                // the last item" or "stamp the list" — neither maps
                // cleanly today), so drop it rather than mis-attributing
                // to the first item. Inline-form stamps inside each
                // item still flow through `visit_list_item`.
                self.pending_stamp = None;
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

                    // Steal the first paragraph's inline content onto
                    // the Callout itself (stripping the `[!type]` prefix).
                    let prefix = format!("[!{alert_type}]");
                    let mut stolen_content = String::new();
                    let mut stolen_anns: Vec<Annotation> = Vec::new();
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
                            let mut anns: Vec<Annotation> = raw_anns
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
                            stolen_content = self.strip_inline_stamp(trimmed, &mut anns);
                            stolen_anns = anns;
                            first_para_stolen = true;
                            break;
                        }
                    }

                    // `new_block` consumes any inline-form stamp set above,
                    // taking precedence over a sibling block-form stamp.
                    let mut block = self.new_block(BlockType::Callout {
                        icon: String::new(),
                        color: String::new(),
                        alert_type: alert_type.clone(),
                    });
                    block.content = stolen_content;
                    block.annotations = stolen_anns;

                    let callout_id = self.insert_block(block, parent);

                    // Process remaining children, skipping the first paragraph.
                    let mut skipped_first = !first_para_stolen;
                    let remaining: Vec<&AstNode> = node
                        .children()
                        .filter(|child| {
                            if !skipped_first
                                && matches!(child.data.borrow().value, NodeValue::Paragraph)
                            {
                                skipped_first = true;
                                return false;
                            }
                            true
                        })
                        .collect();
                    self.visit_siblings(Some(callout_id), remaining);
                } else {
                    let block = self.new_block(BlockType::Quote);
                    let id = self.insert_block(block, parent);
                    self.visit_siblings(Some(id), node.children());
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
        let content = self.strip_inline_stamp(content, &mut anns);

        // `new_block` consumes any inline-form stamp captured above; the
        // sibling-block stamp on the list item itself was already
        // captured in `pending_stamp` by the surrounding `visit_siblings`
        // call (or, for items inside a list, by the list arm's own
        // sibling walk).
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
        let remaining: Vec<&AstNode> = item
            .children()
            .filter(|child| {
                if !saw_first_para && matches!(child.data.borrow().value, NodeValue::Paragraph) {
                    saw_first_para = true;
                    return false;
                }
                true
            })
            .collect();
        self.visit_siblings(Some(item_id), remaining);
    }

    /// Strip a trailing `<!-- ^<uuid> -->` marker from inline content
    /// and stash the parsed id in [`Self::pending_stamp`] so the next
    /// [`Self::new_block`] call promotes it onto the resulting block.
    /// Annotations whose end falls past the truncated content are
    /// trimmed in place.
    fn strip_inline_stamp(&mut self, raw: String, anns: &mut Vec<Annotation>) -> String {
        let Some((head, id)) = strip_trailing_stable_id_marker(&raw) else {
            return raw;
        };
        anns.retain(|a| a.end <= head.len());
        // Inline-form marker takes precedence over a sibling block-form
        // marker captured by visit_siblings — both shouldn't co-occur in
        // serializer output, but if they do, the inline form is more
        // authoritative.
        self.pending_stamp = Some(id);
        head
    }
}

/// Return the parsed stamp id when `node` is an `HtmlBlock` whose body
/// is exactly a `<!-- ^<uuid> -->` marker (whitespace tolerated).
fn extract_html_block_marker<'a>(node: &'a AstNode<'a>) -> Option<BlockId> {
    match node.data.borrow().value.clone() {
        NodeValue::HtmlBlock(html) => parse_stable_id_marker(&html.literal),
        _ => None,
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

    // ── ADR 0017: lazy block-id stamping ──

    #[test]
    fn inline_stamp_marker_promotes_paragraph_id() {
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let src = format!("Hello world <!-- ^{id} -->\n");
        let tree = parse_default(&src);
        assert_eq!(tree.root_blocks.len(), 1);
        let block = tree.get(tree.root_blocks[0]).unwrap();
        // The marker is stripped from the visible content.
        assert_eq!(block.content, "Hello world");
        // The id is the stamped uuid (not the positional fallback).
        assert_eq!(block.id, id);
        assert_eq!(block.stable_id, Some(id));
    }

    #[test]
    fn inline_stamp_marker_promotes_heading_id() {
        let id = uuid::Uuid::parse_str("11111111-2222-4333-8444-555555555555").unwrap();
        let src = format!("# Title <!-- ^{id} -->\n");
        let tree = parse_default(&src);
        let block = tree.get(tree.root_blocks[0]).unwrap();
        assert_eq!(block.content, "Title");
        assert_eq!(block.stable_id, Some(id));
    }

    #[test]
    fn block_form_stamp_attaches_to_previous_code_block() {
        let id = uuid::Uuid::parse_str("aaaabbbb-cccc-4ddd-8eee-ffffffffffff").unwrap();
        let src = format!("```rust\nfn main() {{}}\n```\n<!-- ^{id} -->\n");
        let tree = parse_default(&src);
        // Code block is the only root block; the marker is consumed.
        assert_eq!(tree.root_blocks.len(), 1);
        let block = tree.get(tree.root_blocks[0]).unwrap();
        assert!(matches!(block.ty, BlockType::CodeBlock { .. }));
        assert_eq!(block.stable_id, Some(id));
        assert_eq!(block.id, id);
    }

    #[test]
    fn block_form_stamp_attaches_to_previous_divider() {
        let id = uuid::Uuid::parse_str("11111111-2222-4333-8444-aaaaaaaaaaaa").unwrap();
        let src = format!("Before\n\n---\n<!-- ^{id} -->\n\nAfter\n");
        let tree = parse_default(&src);
        assert_eq!(tree.root_blocks.len(), 3);
        let divider = tree.get(tree.root_blocks[1]).unwrap();
        assert!(matches!(divider.ty, BlockType::Divider));
        assert_eq!(divider.stable_id, Some(id));
        // Surrounding blocks are not stamped.
        assert!(tree.get(tree.root_blocks[0]).unwrap().stable_id.is_none());
        assert!(tree.get(tree.root_blocks[2]).unwrap().stable_id.is_none());
    }

    #[test]
    fn unstamped_blocks_use_positional_hash() {
        let opts = ParseOptions {
            file_path: "f.md".into(),
            ..ParseOptions::default()
        };
        let tree = parse("# Hi\n\nBody\n", &opts).unwrap();
        for id in &tree.root_blocks {
            assert!(tree.get(*id).unwrap().stable_id.is_none());
        }
        // The first block's id is the positional hash for slot 0.
        let expected = deterministic_block_id("f.md", 0, &BlockType::Heading { level: 1 });
        assert_eq!(tree.root_blocks[0], expected);
    }

    #[test]
    fn stamped_id_survives_upstream_insertion() {
        // Insert a new heading above a stamped paragraph; the stamped
        // paragraph's id must NOT change, even though its visit_order
        // (and thus its positional id) has shifted.
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

        let opts = ParseOptions {
            file_path: "f.md".into(),
            ..ParseOptions::default()
        };
        let before = format!("Body <!-- ^{id} -->\n");
        let after = format!("# New top\n\nBody <!-- ^{id} -->\n");
        let tree_before = parse(&before, &opts).unwrap();
        let tree_after = parse(&after, &opts).unwrap();

        // Find the paragraph block in each tree (last root in `after`).
        let para_before = tree_before.root_blocks[0];
        let para_after = *tree_after.root_blocks.last().unwrap();
        assert_eq!(para_before, id);
        assert_eq!(para_after, id, "stamped id must survive upstream insert");

        // Tree-after's heading is NOT stamped and uses its positional id.
        let heading = tree_after.get(tree_after.root_blocks[0]).unwrap();
        assert!(heading.stable_id.is_none());
        assert!(matches!(heading.ty, BlockType::Heading { level: 1 }));
    }

    #[test]
    fn invalid_stamp_marker_left_as_content() {
        // A trailing comment that doesn't match the stamp pattern stays
        // in the content (parser falls back to deterministic id).
        let src = "Hello <!-- not-a-stamp -->\n";
        let tree = parse_default(src);
        let block = tree.get(tree.root_blocks[0]).unwrap();
        assert!(block.stable_id.is_none());
        // The HtmlInline content is preserved verbatim.
        assert!(block.content.contains("<!-- not-a-stamp -->"));
    }
}
