//! Markdown ↔ Block Tree roundtrip (PRD 08 §3).
//!
//! See [`crate::markdown::MarkdownParser`] and
//! [`crate::markdown::MarkdownSerializer`] for the public entry points.
//! Implementation is split across four private submodules:
//!
//! - [`parse`] — comrak AST → [`crate::BlockTree`]
//! - [`serialize`] — [`crate::BlockTree`] → `String`
//! - [`inline`] — inline annotation extract + serialize
//! - [`id`] — deterministic block-id generation

mod database_view_spec;
mod id;
mod inline;
mod parse;
mod serialize;

pub use id::{
    deterministic_block_id, format_stable_id_marker, parse_stable_id_marker,
    strip_trailing_stable_id_marker,
};

/// Options that control [`MarkdownParser`] behaviour (PRD §3.2).
#[derive(Clone, Debug)]
pub struct ParseOptions {
    /// Enable GitHub Flavored Markdown extensions (strikethrough,
    /// tables, tasklists, autolink).
    pub gfm_enabled: bool,
    /// Enable Nexus-specific syntax: wikilinks, inline tags, math,
    /// callouts, block-ref anchors.
    pub nexus_syntax_enabled: bool,
    /// Path of the document being parsed, used to seed deterministic
    /// block IDs. Empty string produces globally-deterministic IDs
    /// that only vary by slot — useful for tests.
    pub file_path: String,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            gfm_enabled: true,
            nexus_syntax_enabled: true,
            file_path: String::new(),
        }
    }
}

/// Parse markdown source into a [`crate::BlockTree`].
///
/// `MarkdownParser` is cheap to construct and holds no state between
/// calls; you can reuse one instance or rebuild per document.
pub struct MarkdownParser {
    options: ParseOptions,
}

impl MarkdownParser {
    /// Build a parser with the given options.
    #[must_use]
    pub fn new(options: ParseOptions) -> Self {
        Self { options }
    }

    /// Parse `source` end-to-end.
    ///
    /// # Errors
    /// - [`crate::EditorError::TransactionInvalid`] wrapping any
    ///   frontmatter parse failure (the only recoverable error in the
    ///   pipeline — comrak is infallible for well-formed UTF-8).
    pub fn parse(&self, source: &str) -> crate::Result<crate::BlockTree> {
        parse::parse(source, &self.options)
    }
}

/// Serialize a [`crate::BlockTree`] back to markdown.
///
/// Zero-sized wrapper; kept as a type for API symmetry with
/// [`MarkdownParser`].
pub struct MarkdownSerializer;

impl MarkdownSerializer {
    /// Serialize `tree`. Inverse of [`MarkdownParser::parse`].
    #[must_use]
    pub fn serialize(tree: &crate::BlockTree) -> String {
        serialize::serialize(tree)
    }
}

// ── Roundtrip integration tests ───────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockType;

    fn parser() -> MarkdownParser {
        MarkdownParser::new(ParseOptions::default())
    }

    fn roundtrip(source: &str) -> String {
        let tree = parser().parse(source).unwrap();
        MarkdownSerializer::serialize(&tree)
    }

    fn parse_serialize_parse_is_idempotent(source: &str) {
        let tree1 = parser().parse(source).unwrap();
        let md = MarkdownSerializer::serialize(&tree1);
        let tree2 = parser().parse(&md).unwrap();
        // Compare structural shape: same roots, same block count, same
        // content and types. IDs are deterministic so they also match.
        assert_eq!(tree1.root_blocks.len(), tree2.root_blocks.len());
        assert_eq!(tree1.blocks.len(), tree2.blocks.len());
        for (id, b1) in &tree1.blocks {
            let b2 = tree2
                .blocks
                .get(id)
                .unwrap_or_else(|| panic!("block {id} missing from second parse"));
            assert_eq!(b1.ty, b2.ty, "block type mismatch for {id}");
            assert_eq!(b1.content, b2.content, "content mismatch for {id}");
            assert_eq!(
                b1.annotations, b2.annotations,
                "annotations mismatch for {id}"
            );
        }
    }

    #[test]
    fn single_paragraph() {
        let out = roundtrip("hello\n");
        assert!(out.contains("hello"));
        parse_serialize_parse_is_idempotent("hello\n");
    }

    #[test]
    fn heading_and_paragraph() {
        let src = "# Title\n\nBody text.\n";
        let tree = parser().parse(src).unwrap();
        assert_eq!(tree.root_blocks.len(), 2);
        let first = tree.get(tree.root_blocks[0]).unwrap();
        assert_eq!(first.ty, BlockType::Heading { level: 1 });
        assert_eq!(first.content, "Title");
        parse_serialize_parse_is_idempotent(src);
    }

    #[test]
    fn bullet_list_flat() {
        let src = "- one\n- two\n- three\n";
        let tree = parser().parse(src).unwrap();
        assert_eq!(tree.root_blocks.len(), 3);
        parse_serialize_parse_is_idempotent(src);
    }

    #[test]
    fn numbered_list_flat() {
        let src = "1. alpha\n2. beta\n";
        let tree = parser().parse(src).unwrap();
        assert_eq!(tree.root_blocks.len(), 2);
        for id in &tree.root_blocks {
            assert!(matches!(
                tree.get(*id).unwrap().ty,
                BlockType::NumberedList { .. }
            ));
        }
        parse_serialize_parse_is_idempotent(src);
    }

    #[test]
    fn nested_bullet_list() {
        let src = "- outer\n  - inner\n  - inner2\n- outer2\n";
        parse_serialize_parse_is_idempotent(src);
    }

    #[test]
    fn task_list_roundtrip() {
        let src = "- [ ] todo\n- [x] done\n";
        let tree = parser().parse(src).unwrap();
        assert_eq!(tree.root_blocks.len(), 2);
        let first = tree.get(tree.root_blocks[0]).unwrap();
        assert_eq!(
            first.properties.attributes.get("task.completed"),
            Some(&crate::PropertyValue::Boolean(false))
        );
        let second = tree.get(tree.root_blocks[1]).unwrap();
        assert_eq!(
            second.properties.attributes.get("task.completed"),
            Some(&crate::PropertyValue::Boolean(true))
        );
        parse_serialize_parse_is_idempotent(src);
    }

    #[test]
    fn fenced_code_block() {
        let src = "```rust\nfn main() {}\n```\n";
        let tree = parser().parse(src).unwrap();
        let b = tree.get(tree.root_blocks[0]).unwrap();
        assert!(matches!(b.ty, BlockType::CodeBlock { .. }));
        assert!(b.content.contains("fn main"));
        parse_serialize_parse_is_idempotent(src);
    }

    #[test]
    fn divider_roundtrip() {
        let src = "Before\n\n---\n\nAfter\n";
        let tree = parser().parse(src).unwrap();
        let types: Vec<_> = tree
            .root_blocks
            .iter()
            .map(|id| tree.get(*id).unwrap().ty.clone())
            .collect();
        assert!(types.iter().any(|t| matches!(t, BlockType::Divider)));
        parse_serialize_parse_is_idempotent(src);
    }

    #[test]
    fn quote_vs_callout() {
        let quote = "> plain quote\n";
        let tree = parser().parse(quote).unwrap();
        assert!(matches!(
            tree.get(tree.root_blocks[0]).unwrap().ty,
            BlockType::Quote
        ));

        let callout = "> [!warning] Watch out\n> Be careful here\n";
        let tree = parser().parse(callout).unwrap();
        let b = tree.get(tree.root_blocks[0]).unwrap();
        match &b.ty {
            BlockType::Callout { alert_type, .. } => assert_eq!(alert_type, "warning"),
            other => panic!("expected Callout, got {other:?}"),
        }
        parse_serialize_parse_is_idempotent(callout);
    }

    #[test]
    fn table_with_header() {
        let src = "| a | b |\n| --- | --- |\n| 1 | 2 |\n";
        let tree = parser().parse(src).unwrap();
        let first = tree.get(tree.root_blocks[0]).unwrap();
        match &first.ty {
            BlockType::Table {
                column_count,
                header_row,
            } => {
                assert_eq!(*column_count, 2);
                assert!(*header_row);
            }
            other => panic!("expected Table, got {other:?}"),
        }
        parse_serialize_parse_is_idempotent(src);
    }

    #[test]
    fn wikilink_embed_becomes_embed_block() {
        let src = "![[note-name]]\n";
        let tree = parser().parse(src).unwrap();
        let b = tree.get(tree.root_blocks[0]).unwrap();
        match &b.ty {
            BlockType::Embed { url, .. } => assert_eq!(url, "note-name"),
            other => panic!("expected Embed, got {other:?}"),
        }
    }

    #[test]
    fn inline_formatting_roundtrip() {
        let src = "This is **bold** and *italic* and `code`.\n";
        parse_serialize_parse_is_idempotent(src);
    }

    #[test]
    fn inline_link_roundtrip() {
        let src = "Check [the docs](https://example.com/docs).\n";
        parse_serialize_parse_is_idempotent(src);
    }

    #[test]
    fn inline_wikilink_roundtrip() {
        let src = "See [[other note|display text]].\n";
        parse_serialize_parse_is_idempotent(src);
    }

    #[test]
    fn inline_math_roundtrip() {
        let src = "Euler's identity $e^{i\\pi}+1=0$ is elegant.\n";
        parse_serialize_parse_is_idempotent(src);
    }

    #[test]
    fn frontmatter_roundtrip() {
        let src = "---\ntitle: My Note\ntags:\n  - rust\n  - markdown\n---\n# Body\n";
        let tree = parser().parse(src).unwrap();
        assert!(tree.metadata.properties.contains_key("title"));
        let md = MarkdownSerializer::serialize(&tree);
        assert!(md.starts_with("---\n"));
        // Parse the serialized form and confirm frontmatter survives.
        let tree2 = parser().parse(&md).unwrap();
        assert_eq!(
            tree.metadata.properties.get("title"),
            tree2.metadata.properties.get("title")
        );
    }

    #[test]
    fn deterministic_ids_stable_across_reparse() {
        let src = "# Hi\n\nPara.\n\n- item\n";
        let opts = ParseOptions {
            file_path: "/some/path.md".into(),
            ..ParseOptions::default()
        };
        let a = MarkdownParser::new(opts.clone()).parse(src).unwrap();
        let b = MarkdownParser::new(opts).parse(src).unwrap();
        assert_eq!(a.root_blocks, b.root_blocks);
    }

    #[test]
    fn multi_paragraph_document() {
        let src =
            "# Overview\n\nThis is the intro.\n\n## Section\n\nMore detail here.\n\n- Point A\n- Point B\n";
        parse_serialize_parse_is_idempotent(src);
    }

    // ── ADR 0017: stamp marker round-trip ──

    #[test]
    fn stamped_paragraph_round_trips_through_external_editor() {
        // Simulate the external-editor case: the marker is in the source
        // (as if a user opened the file in vim and preserved the comment).
        // After parse → serialize → parse, the id must remain the same.
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let src = format!("Hello world <!-- ^{id} -->\n");
        let tree1 = parser().parse(&src).unwrap();
        let md = MarkdownSerializer::serialize(&tree1);
        // The serializer emits the marker too (lossless round-trip).
        assert!(md.contains(&format!("<!-- ^{id} -->")));
        let tree2 = parser().parse(&md).unwrap();
        assert_eq!(tree1.root_blocks, tree2.root_blocks);
        let para = tree2.get(tree2.root_blocks[0]).unwrap();
        assert_eq!(para.id, id);
        assert_eq!(para.stable_id, Some(id));
        assert_eq!(para.content, "Hello world");
    }

    #[test]
    fn stamped_code_block_round_trips_block_form() {
        let id = uuid::Uuid::parse_str("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee").unwrap();
        let src = format!("```rust\nfn main() {{}}\n```\n<!-- ^{id} -->\n");
        let tree1 = parser().parse(&src).unwrap();
        let md = MarkdownSerializer::serialize(&tree1);
        let tree2 = parser().parse(&md).unwrap();
        assert_eq!(tree1.root_blocks, tree2.root_blocks);
        let code = tree2.get(tree2.root_blocks[0]).unwrap();
        assert_eq!(code.stable_id, Some(id));
        assert_eq!(code.id, id);
    }

    // ── BL-012 close-out: native [[{db:…}]] parser/serializer ──

    #[test]
    fn bare_database_view_paragraph_promotes_to_database_view_block() {
        use crate::DatabaseViewType;
        let src = "[[{db:Tasks.bases}]]\n";
        let tree = parser().parse(src).unwrap();
        assert_eq!(tree.root_blocks.len(), 1);
        let b = tree.get(tree.root_blocks[0]).unwrap();
        match &b.ty {
            BlockType::DatabaseView {
                database_path,
                view_config,
            } => {
                assert_eq!(database_path, "Tasks.bases");
                assert_eq!(view_config.view_type, DatabaseViewType::Table);
                assert!(view_config.filters.is_empty());
                assert!(view_config.sorts.is_empty());
            }
            other => panic!("expected DatabaseView, got {other:?}"),
        }
    }

    #[test]
    fn database_view_with_kanban_view_and_filters_round_trips() {
        let src =
            "[[{db:Tasks.bases?view=kanban&group=status&filter=status%20%3D%20Done&sort=due_date%20asc}]]\n";
        parse_serialize_parse_is_idempotent(src);
        let tree = parser().parse(src).unwrap();
        let b = tree.get(tree.root_blocks[0]).unwrap();
        match &b.ty {
            BlockType::DatabaseView { view_config, .. } => {
                assert_eq!(view_config.filters, vec!["status = Done"]);
                assert_eq!(view_config.sorts, vec!["due_date asc"]);
            }
            other => panic!("expected DatabaseView, got {other:?}"),
        }
    }

    #[test]
    fn malformed_database_view_falls_back_to_paragraph() {
        // Empty path → parser drops back to a paragraph rather than
        // panicking, so the user sees their typed source verbatim
        // and can fix it inline. A loud parse error here would
        // make typo-recovery hostile.
        let src = "[[{db:}]]\n";
        let tree = parser().parse(src).unwrap();
        let b = tree.get(tree.root_blocks[0]).unwrap();
        assert!(matches!(b.ty, BlockType::Paragraph));
    }

    #[test]
    fn database_view_serialization_uses_native_db_syntax_not_wiki_embed() {
        // Pre-BL-012-close-out, the serializer emitted
        // `![[Tasks.bases]]` (wiki-embed shape) and lost the view
        // config entirely; the close-out switches to the native
        // `[[{db:…}]]` form so config edits round-trip.
        use crate::DatabaseViewConfig;
        let mut tree = crate::BlockTree::new(crate::DocumentMetadata::default());
        let id = uuid::Uuid::new_v4();
        let block = crate::Block {
            id,
            ty: BlockType::DatabaseView {
                database_path: "Tasks.bases".to_string(),
                view_config: DatabaseViewConfig::default(),
            },
            ..crate::Block::new(BlockType::Paragraph)
        };
        tree.blocks.insert(id, block);
        tree.root_blocks.push(id);

        let md = MarkdownSerializer::serialize(&tree);
        assert!(md.contains("[[{db:Tasks.bases}]]"));
        assert!(!md.contains("![["));
    }

    #[test]
    fn database_view_stamp_marker_round_trips() {
        // The stamp anchor (`<!-- ^<uuid> -->`) must travel with the
        // database-view block the same way it does for every other
        // block type — otherwise BL-049 / BL-050 anchors against a
        // db block would orphan on reload.
        let id = uuid::Uuid::parse_str("ffffffff-1111-4222-8333-444444444444").unwrap();
        let src = format!("[[{{db:Tasks.bases?view=kanban&group=status}}]]\n<!-- ^{id} -->\n");
        let tree1 = parser().parse(&src).unwrap();
        let md = MarkdownSerializer::serialize(&tree1);
        assert!(md.contains(&format!("<!-- ^{id} -->")));
        let tree2 = parser().parse(&md).unwrap();
        let b = tree2.get(tree2.root_blocks[0]).unwrap();
        assert_eq!(b.stable_id, Some(id));
        assert!(matches!(b.ty, BlockType::DatabaseView { .. }));
    }

    #[test]
    fn unstamped_documents_emit_no_stamp_markers() {
        // Sanity check: nothing in this body has been stamped, so the
        // serialized form should not contain `<!-- ^` anywhere.
        let src = "# Hi\n\nBody.\n\n- One\n- Two\n";
        let tree = parser().parse(src).unwrap();
        let md = MarkdownSerializer::serialize(&tree);
        assert!(!md.contains("<!-- ^"));
    }
}
