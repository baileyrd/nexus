//! [`BlockTree`] → markdown source (PRD 08 §3.3).
//!
//! Dispatches on `BlockType` and recurses into children per type.
//! Post-pass normalizes excess blank lines per §3.4.

use std::collections::HashMap;

use crate::annotation::AnnotationType;
use crate::block::{Block, BlockId, BlockType, PropertyValue};
use crate::tree::BlockTree;

use super::id::format_stable_id_marker;
use super::inline::serialize_inline;

/// Serialize `tree` to a markdown string.
#[must_use]
pub fn serialize(tree: &BlockTree) -> String {
    let mut out = String::new();
    serialize_frontmatter(&tree.metadata.properties, &mut out);
    for id in &tree.root_blocks {
        serialize_block(tree, *id, &mut out);
    }
    normalize_blank_lines(&out)
}

// ── Frontmatter ───────────────────────────────────────────────────────────────

fn serialize_frontmatter(props: &HashMap<String, PropertyValue>, out: &mut String) {
    if props.is_empty() {
        return;
    }
    let yaml_value = properties_to_yaml(props);
    let yaml = serde_yml::to_string(&yaml_value).unwrap_or_default();
    out.push_str("---\n");
    out.push_str(yaml.trim_end_matches('\n'));
    out.push_str("\n---\n\n");
}

fn properties_to_yaml(props: &HashMap<String, PropertyValue>) -> serde_yml::Value {
    // Deterministic key order: reserved keys first in a fixed order,
    // then custom keys in sorted order.
    const RESERVED: &[&str] = &[
        "title", "type", "status", "cssclass", "date", "created", "modified", "aliases", "tags",
    ];
    let mut mapping = serde_yml::Mapping::new();

    for key in RESERVED {
        if let Some(v) = props.get(*key) {
            mapping.insert(
                serde_yml::Value::String((*key).into()),
                property_to_yaml(v),
            );
        }
    }
    let mut other_keys: Vec<&String> = props
        .keys()
        .filter(|k| !RESERVED.contains(&k.as_str()))
        .collect();
    other_keys.sort();
    for key in other_keys {
        mapping.insert(
            serde_yml::Value::String(key.clone()),
            property_to_yaml(&props[key]),
        );
    }
    serde_yml::Value::Mapping(mapping)
}

fn property_to_yaml(v: &PropertyValue) -> serde_yml::Value {
    match v {
        PropertyValue::String(s) => serde_yml::Value::String(s.clone()),
        PropertyValue::Number(n) => serde_yml::Value::Number(serde_yml::Number::from(*n)),
        PropertyValue::Boolean(b) => serde_yml::Value::Bool(*b),
        PropertyValue::List(items) => {
            serde_yml::Value::Sequence(items.iter().map(property_to_yaml).collect())
        }
        PropertyValue::Object(map) => {
            let mut m = serde_yml::Mapping::new();
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                m.insert(
                    serde_yml::Value::String(k.clone()),
                    property_to_yaml(&map[k]),
                );
            }
            serde_yml::Value::Mapping(m)
        }
    }
}

// ── Block dispatch ────────────────────────────────────────────────────────────

fn serialize_block(tree: &BlockTree, id: BlockId, out: &mut String) {
    let Some(block) = tree.get(id) else {
        return;
    };
    match &block.ty {
        BlockType::Heading { level } => serialize_heading(*level, block, out),
        BlockType::Paragraph => serialize_paragraph(block, out),
        BlockType::BulletList { indent_level } | BlockType::ToggleList { indent_level, .. } => {
            serialize_list_item(tree, block, *indent_level, None, out);
        }
        BlockType::NumberedList {
            indent_level,
            number,
        } => {
            serialize_list_item(tree, block, *indent_level, Some(*number), out);
        }
        BlockType::CodeBlock { language, .. } => serialize_code_block(language, block, out),
        BlockType::MathBlock { formula } => {
            out.push_str("$$\n");
            out.push_str(formula);
            if !formula.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("$$\n");
            push_block_stamp_line(block, out);
            out.push('\n');
        }
        BlockType::Callout { alert_type, .. } => serialize_callout(tree, block, alert_type, out),
        BlockType::Quote => serialize_quote(tree, block, out),
        BlockType::Divider => {
            out.push_str("---\n");
            push_block_stamp_line(block, out);
            out.push('\n');
        }
        BlockType::Table { .. } => serialize_table(tree, block, out),
        BlockType::TableRow { .. } => {
            // Serialized by the parent Table; ignore here.
        }
        BlockType::Image { src, alt_text, .. } => {
            out.push_str("![");
            out.push_str(alt_text);
            out.push_str("](");
            out.push_str(src);
            out.push_str(")\n");
            push_block_stamp_line(block, out);
            out.push('\n');
        }
        BlockType::Embed { url, .. } => push_wiki_embed(out, url, block),
        // Deferred types: emit a recognizable fallback that roundtrips
        // through the parser as a paragraph (no data loss).
        BlockType::DatabaseView { database_path, .. } => {
            push_wiki_embed(out, database_path, block);
        }
        BlockType::Video { src, .. } | BlockType::Audio { src } | BlockType::File { src, .. } => {
            push_wiki_embed(out, src, block);
        }
        BlockType::Bookmark { url, title, .. } => {
            out.push('[');
            out.push_str(title);
            out.push_str("](");
            out.push_str(url);
            out.push_str(")\n");
            push_block_stamp_line(block, out);
            out.push('\n');
        }
        BlockType::SyncedBlock { .. }
        | BlockType::ColumnLayout { .. }
        | BlockType::Column { .. }
        | BlockType::TableOfContents { .. } => {
            // No canonical markdown form yet; emit content if any.
            if block.content.is_empty() {
                push_block_stamp_line(block, out);
            } else {
                out.push_str(&block.content);
                out.push('\n');
                push_block_stamp_line(block, out);
                out.push('\n');
            }
        }
    }
}

fn push_wiki_embed(out: &mut String, target: &str, block: &Block) {
    out.push_str("![[");
    out.push_str(target);
    out.push_str("]]\n");
    push_block_stamp_line(block, out);
    out.push('\n');
}

// ── Per-type serializers ──────────────────────────────────────────────────────

fn serialize_heading(level: u8, block: &Block, out: &mut String) {
    let hashes: String = "#".repeat(level as usize);
    out.push_str(&hashes);
    out.push(' ');
    out.push_str(&render_content(block));
    out.push_str("\n\n");
}

fn serialize_paragraph(block: &Block, out: &mut String) {
    // Preserve raw HTML blocks we stashed into paragraphs with a Custom
    // `html` annotation: emit content verbatim, no inline markers.
    let is_html = block
        .annotations
        .iter()
        .any(|a| matches!(&a.ty, AnnotationType::Custom { ty, .. } if ty == "html"));
    if is_html {
        out.push_str(&block.content);
        out.push_str("\n\n");
        return;
    }
    out.push_str(&render_content(block));
    out.push_str("\n\n");
}

fn serialize_list_item(
    tree: &BlockTree,
    block: &Block,
    indent_level: u8,
    number: Option<u32>,
    out: &mut String,
) {
    let indent: String = "  ".repeat(indent_level as usize);
    out.push_str(&indent);
    match number {
        Some(n) => {
            out.push_str(&n.to_string());
            out.push_str(". ");
        }
        None => {
            if let Some(task) = task_marker(block) {
                out.push_str(task);
            } else {
                out.push_str("- ");
            }
        }
    }
    out.push_str(&render_content(block));
    out.push('\n');
    for &child_id in &block.children {
        serialize_block(tree, child_id, out);
    }
    // List items do NOT trail a blank line — the surrounding list
    // context adds spacing at the end via normalize_blank_lines.
}

fn task_marker(block: &Block) -> Option<&'static str> {
    match block.properties.attributes.get("task.completed")? {
        PropertyValue::Boolean(true) => Some("- [x] "),
        PropertyValue::Boolean(false) => Some("- [ ] "),
        _ => None,
    }
}

fn serialize_code_block(language: &str, block: &Block, out: &mut String) {
    out.push_str("```");
    out.push_str(language);
    out.push('\n');
    out.push_str(&block.content);
    if !block.content.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```\n");
    push_block_stamp_line(block, out);
    out.push('\n');
}

fn serialize_callout(tree: &BlockTree, block: &Block, alert_type: &str, out: &mut String) {
    // `render_content` includes the stamp marker when set; check the
    // rendered string rather than `block.content` so a stamped-but-
    // empty callout still emits the marker on the header line.
    let mut body = String::new();
    let rendered = render_content(block);
    if !rendered.is_empty() {
        body.push_str(&rendered);
    }
    // Render children into a scratch buffer so we can prefix `> `.
    let mut child_buf = String::new();
    for &cid in &block.children {
        serialize_block(tree, cid, &mut child_buf);
    }
    if !child_buf.is_empty() {
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(child_buf.trim_end_matches('\n'));
    }
    out.push_str("> [!");
    out.push_str(alert_type);
    out.push(']');
    if body.is_empty() {
        out.push('\n');
    } else {
        // Emit the first line on the same line as the callout header
        // (Obsidian convention); subsequent lines get their own `> `.
        let mut lines = body.lines();
        if let Some(first) = lines.next() {
            out.push(' ');
            out.push_str(first);
            out.push('\n');
        }
        for line in lines {
            out.push_str("> ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push('\n');
}

fn serialize_quote(tree: &BlockTree, block: &Block, out: &mut String) {
    let mut body = String::new();
    let rendered = render_content(block);
    if !rendered.is_empty() {
        body.push_str(&rendered);
    }
    let mut child_buf = String::new();
    for &cid in &block.children {
        serialize_block(tree, cid, &mut child_buf);
    }
    if !child_buf.is_empty() {
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(child_buf.trim_end_matches('\n'));
    }
    if body.is_empty() {
        out.push_str(">\n\n");
        return;
    }
    for line in body.lines() {
        out.push_str("> ");
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
}

fn serialize_table(tree: &BlockTree, block: &Block, out: &mut String) {
    let rows: Vec<Vec<String>> = block
        .children
        .iter()
        .filter_map(|cid| tree.get(*cid))
        .filter_map(|child| match &child.ty {
            BlockType::TableRow { cells } => Some(cells.clone()),
            _ => None,
        })
        .collect();
    if rows.is_empty() {
        return;
    }
    let col_count = rows.iter().map(Vec::len).max().unwrap_or(0);

    // Alignments from properties (stored by parser).
    let alignments: Vec<String> = match block.properties.computed.get("table.alignments") {
        Some(PropertyValue::List(items)) => items
            .iter()
            .filter_map(|v| match v {
                PropertyValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };

    let has_header = matches!(
        &block.ty,
        BlockType::Table {
            header_row: true,
            ..
        }
    );

    for (i, row) in rows.iter().enumerate() {
        out.push('|');
        for col in 0..col_count {
            let cell = row.get(col).cloned().unwrap_or_default();
            out.push(' ');
            out.push_str(&cell);
            out.push(' ');
            out.push('|');
        }
        out.push('\n');
        if i == 0 && has_header {
            out.push('|');
            for col in 0..col_count {
                let align = alignments.get(col).map_or("none", String::as_str);
                out.push(' ');
                match align {
                    "left" => out.push_str(":---"),
                    "right" => out.push_str("---:"),
                    "center" => out.push_str(":---:"),
                    _ => out.push_str("---"),
                }
                out.push(' ');
                out.push('|');
            }
            out.push('\n');
        }
    }
    push_block_stamp_line(block, out);
    out.push('\n');
}

fn render_content(block: &Block) -> String {
    let mut text = serialize_inline(&block.content, &block.annotations);
    if let Some(PropertyValue::String(bref)) = block.properties.computed.get("block_ref") {
        text.push_str(" ^");
        text.push_str(bref);
    }
    // ADR 0017 inline form: trailing `<!-- ^<uuid> -->` after the
    // visible content. The parser strips this back into
    // `Block.stable_id` on re-read.
    if let Some(id) = block.stable_id {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&format_stable_id_marker(&id));
    }
    text
}

/// Append a `<!-- ^<uuid> -->` marker on its own line for a stamped
/// block whose serialization doesn't go through [`render_content`]
/// (code blocks, dividers, images, embeds, math, tables, etc.).
/// The caller is expected to have just emitted the block's content
/// terminated by a single `\n`; this writes `marker\n` so the
/// surrounding `normalize_blank_lines` collapses anything else.
fn push_block_stamp_line(block: &Block, out: &mut String) {
    if let Some(id) = block.stable_id {
        out.push_str(&format_stable_id_marker(&id));
        out.push('\n');
    }
}

// ── Post-processing ───────────────────────────────────────────────────────────

fn normalize_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut newline_run = 0;
    for ch in s.chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                out.push(ch);
            }
        } else {
            newline_run = 0;
            out.push(ch);
        }
    }
    // Trim trailing newlines to exactly one.
    while out.ends_with("\n\n") {
        out.pop();
    }
    if !out.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    out
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BlockType, DocumentMetadata};
    use crate::{Block, BlockTree};

    fn build(types: Vec<BlockType>) -> BlockTree {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        for ty in types {
            let block = Block::new(ty);
            tree.insert(block, None, tree.root_blocks.len()).unwrap();
        }
        tree
    }

    #[test]
    fn heading_serialization() {
        let mut tree = build(vec![BlockType::Heading { level: 2 }]);
        let id = tree.root_blocks[0];
        tree.get_mut(id).unwrap().content = "Title".into();
        let out = serialize(&tree);
        assert!(out.starts_with("## Title\n"));
    }

    #[test]
    fn paragraph_serialization() {
        let mut tree = build(vec![BlockType::Paragraph]);
        let id = tree.root_blocks[0];
        tree.get_mut(id).unwrap().content = "hello".into();
        assert_eq!(serialize(&tree), "hello\n");
    }

    #[test]
    fn bullet_list_indent_respected() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let outer = Block::new(BlockType::BulletList { indent_level: 0 }).with_content("outer");
        let outer_id = tree.insert(outer, None, 0).unwrap();
        let inner = Block::new(BlockType::BulletList { indent_level: 1 }).with_content("inner");
        tree.insert(inner, Some(outer_id), 0).unwrap();
        let out = serialize(&tree);
        assert!(out.contains("- outer\n"));
        assert!(out.contains("  - inner\n"));
    }

    #[test]
    fn task_list_marker() {
        let mut tree = build(vec![BlockType::BulletList { indent_level: 0 }]);
        let id = tree.root_blocks[0];
        let b = tree.get_mut(id).unwrap();
        b.content = "todo".into();
        b.properties
            .attributes
            .insert("task.completed".into(), PropertyValue::Boolean(false));
        let out = serialize(&tree);
        assert!(out.contains("- [ ] todo\n"));
    }

    #[test]
    fn numbered_list_number() {
        let mut tree = build(vec![BlockType::NumberedList {
            indent_level: 0,
            number: 3,
        }]);
        let id = tree.root_blocks[0];
        tree.get_mut(id).unwrap().content = "item".into();
        let out = serialize(&tree);
        assert!(out.starts_with("3. item\n"));
    }

    #[test]
    fn code_block_with_language() {
        let mut tree = build(vec![BlockType::CodeBlock {
            language: "rust".into(),
            line_numbers: false,
        }]);
        let id = tree.root_blocks[0];
        tree.get_mut(id).unwrap().content = "fn main() {}".into();
        let out = serialize(&tree);
        assert!(out.contains("```rust\nfn main() {}\n```\n"));
    }

    #[test]
    fn math_block_wraps() {
        let tree = build(vec![BlockType::MathBlock {
            formula: "a^2 + b^2 = c^2".into(),
        }]);
        let out = serialize(&tree);
        assert!(out.contains("$$\na^2 + b^2 = c^2\n$$\n"));
    }

    #[test]
    fn divider_emits_hr() {
        let tree = build(vec![BlockType::Divider]);
        assert_eq!(serialize(&tree), "---\n");
    }

    #[test]
    fn image_serialization() {
        let tree = build(vec![BlockType::Image {
            src: "a.png".into(),
            alt_text: "alt".into(),
            width: None,
            height: None,
        }]);
        let _ = tree.root_blocks[0];
        let out = serialize(&tree);
        assert!(out.contains("![alt](a.png)\n"));
    }

    #[test]
    fn embed_serialization() {
        let mut tree = build(vec![BlockType::Embed {
            url: "target".into(),
            embed_type: crate::EmbedType::Note,
            title: None,
            metadata: HashMap::new(),
        }]);
        let _ = &mut tree;
        let out = serialize(&tree);
        assert!(out.contains("![[target]]\n"));
    }

    #[test]
    fn quote_prefixes_each_line() {
        let mut tree = build(vec![BlockType::Quote]);
        let id = tree.root_blocks[0];
        tree.get_mut(id).unwrap().content = "line one\nline two".into();
        let out = serialize(&tree);
        assert!(out.contains("> line one\n"));
        assert!(out.contains("> line two\n"));
    }

    #[test]
    fn callout_header_and_body() {
        let mut tree = build(vec![BlockType::Callout {
            icon: String::new(),
            color: String::new(),
            alert_type: "warning".into(),
        }]);
        let id = tree.root_blocks[0];
        tree.get_mut(id).unwrap().content = "Be careful".into();
        let out = serialize(&tree);
        assert!(out.contains("> [!warning] Be careful\n"));
    }

    #[test]
    fn table_serialization_with_header() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        let tbl = Block::new(BlockType::Table {
            column_count: 2,
            header_row: true,
        });
        let tbl_id = tree.insert(tbl, None, 0).unwrap();
        let header = Block::new(BlockType::TableRow {
            cells: vec!["a".into(), "b".into()],
        });
        tree.insert(header, Some(tbl_id), 0).unwrap();
        let body = Block::new(BlockType::TableRow {
            cells: vec!["1".into(), "2".into()],
        });
        tree.insert(body, Some(tbl_id), 1).unwrap();
        let out = serialize(&tree);
        assert!(out.contains("| a | b |"));
        assert!(out.contains("| --- | --- |"));
        assert!(out.contains("| 1 | 2 |"));
    }

    #[test]
    fn frontmatter_serialized_when_present() {
        let mut tree = BlockTree::new(DocumentMetadata::empty());
        tree.metadata
            .properties
            .insert("title".into(), PropertyValue::String("X".into()));
        let out = serialize(&tree);
        assert!(out.starts_with("---\n"));
        assert!(out.contains("title: X"));
    }

    #[test]
    fn blank_line_normalization() {
        let s = "a\n\n\n\n\nb\n";
        let out = normalize_blank_lines(s);
        assert_eq!(out, "a\n\nb\n");
    }

    // ── ADR 0017: stamp emission ──

    #[test]
    fn paragraph_stamp_renders_inline() {
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let mut tree = build(vec![BlockType::Paragraph]);
        let bid = tree.root_blocks[0];
        let block = tree.get_mut(bid).unwrap();
        block.content = "Hello".into();
        block.stable_id = Some(id);
        let out = serialize(&tree);
        assert!(out.contains(&format!("Hello <!-- ^{id} -->")));
    }

    #[test]
    fn divider_stamp_renders_on_separate_line() {
        let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let mut tree = build(vec![BlockType::Divider]);
        let bid = tree.root_blocks[0];
        tree.get_mut(bid).unwrap().stable_id = Some(id);
        let out = serialize(&tree);
        assert!(out.contains("---\n"));
        assert!(out.contains(&format!("<!-- ^{id} -->\n")));
    }

    #[test]
    fn unstamped_blocks_emit_no_marker() {
        let mut tree = build(vec![BlockType::Paragraph, BlockType::Divider]);
        tree.get_mut(tree.root_blocks[0]).unwrap().content = "x".into();
        let out = serialize(&tree);
        assert!(!out.contains("<!-- ^"));
    }
}
