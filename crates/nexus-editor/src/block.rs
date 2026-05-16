//! Block tree data model (PRD 08 §1.1–1.2).
//!
//! This module declares the full shape of a block, block type, block
//! properties and document metadata. The tree structure itself lives in
//! [`crate::tree::BlockTree`].

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::annotation::Annotation;

/// Unique identifier for a block within a document.
///
/// Runtime-created blocks use `Uuid::new_v4()`. Deterministic IDs (for
/// markdown re-parse stability) are deferred to the roundtrip layer.
pub type BlockId = Uuid;

// ── Block ─────────────────────────────────────────────────────────────────────

/// A single block within the in-memory document tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Block {
    /// Unique identifier within the document.
    pub id: BlockId,

    /// Stamped cross-session id (ADR 0017). `Some` when the markdown
    /// source carried a trailing `<!-- ^<uuid> -->` marker on this
    /// block, or when [`crate::core_plugin::HANDLER_STAMP_BLOCK`] has
    /// been called against it. The stamped uuid is also reflected in
    /// [`Self::id`] (the tree is keyed by the effective id), so callers
    /// generally read `id` and use `stable_id.is_some()` to test whether
    /// the id is preserved across edits upstream of this block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stable_id: Option<BlockId>,

    /// Type discriminant and type-specific data.
    pub ty: BlockType,

    /// Plain-text content (excluding inline formatting).
    pub content: String,

    /// Annotations: inline formatting, links, mentions over `content`.
    pub annotations: Vec<Annotation>,

    /// Rich attributes (properties-panel data).
    pub properties: BlockProperties,

    /// Parent block ID (`None` for root blocks).
    pub parent_id: Option<BlockId>,

    /// Ordered child block IDs.
    pub children: Vec<BlockId>,

    /// This block's position within its parent's `children` vector.
    ///
    /// Kept in sync by [`crate::tree::BlockTree`] on insert/remove.
    pub index_in_parent: usize,

    /// Unix epoch milliseconds at creation time.
    pub created_at: i64,

    /// Unix epoch milliseconds of the most recent mutation.
    pub updated_at: i64,

    /// Soft-deletion flag. Reserved for undo grouping; not enforced by
    /// the tree today.
    pub is_deleted: bool,
}

impl Block {
    /// Create a fresh block with a new UUID and current timestamps.
    #[must_use]
    pub fn new(ty: BlockType) -> Self {
        let ts = now_ms();
        Self {
            id: Uuid::new_v4(),
            stable_id: None,
            ty,
            content: String::new(),
            annotations: Vec::new(),
            properties: BlockProperties::default(),
            parent_id: None,
            children: Vec::new(),
            index_in_parent: 0,
            created_at: ts,
            updated_at: ts,
            is_deleted: false,
        }
    }

    /// Effective block id — the stamped id when present, else
    /// [`Self::id`].
    ///
    /// In practice the two are identical: parse-side stamping rewrites
    /// `id` to match `stable_id`, and the in-memory [`crate::BlockTree`]
    /// is keyed by `id`. The accessor exists so cross-session callers
    /// (BL-048 / BL-049 / BL-050) can express "the id that survives
    /// edits" symmetrically with the absent-stamp fallback path
    /// described in ADR 0017.
    #[must_use]
    pub fn id(&self) -> BlockId {
        self.stable_id.unwrap_or(self.id)
    }

    /// Builder-style: set plain text content.
    #[must_use]
    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    /// Builder-style: attach annotations.
    #[must_use]
    pub fn with_annotations(mut self, annotations: Vec<Annotation>) -> Self {
        self.annotations = annotations;
        self
    }
}

// ── BlockType ─────────────────────────────────────────────────────────────────

/// Discriminant + type-specific data for every supported block variant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum BlockType {
    /// Plain paragraph text.
    Paragraph,

    /// ATX heading with a 1-6 level.
    Heading {
        /// Heading level (1-6).
        level: u8,
    },

    /// Bulleted list item.
    BulletList {
        /// Nesting depth (0 = top level).
        indent_level: u8,
    },

    /// Numbered list item.
    NumberedList {
        /// Nesting depth.
        indent_level: u8,
        /// Current number in the sequence.
        number: u32,
    },

    /// Collapsible toggle list item.
    ToggleList {
        /// Nesting depth.
        indent_level: u8,
        /// Whether the toggle is currently expanded.
        is_open: bool,
    },

    /// Fenced code block.
    CodeBlock {
        /// Language hint (e.g. `"rust"`, `""` for plain).
        language: String,
        /// Show line numbers in the UI.
        line_numbers: bool,
        /// BL-142 Phase 2 — when `true`, the editor renders a Run
        /// gutter button next to the block and `Shift-Enter` inside
        /// the block sends its contents to a REPL kernel for the
        /// block's `language` (resolved via the
        /// `nexus.editor.replKernels` shell config). Source markdown
        /// expresses this via a `repl` token in the fence info string
        /// (e.g. ` ```python repl ` or ` ```python repl,no-numbers `).
        /// Defaults to `false` so existing code blocks are
        /// unaffected.
        #[serde(default, skip_serializing_if = "is_false")]
        repl: bool,
    },

    /// Block-level LaTeX formula.
    MathBlock {
        /// LaTeX source.
        formula: String,
    },

    /// Callout / admonition.
    Callout {
        /// Emoji or icon name.
        icon: String,
        /// CSS color token.
        color: String,
        /// Semantic class (e.g. `"warning"`).
        alert_type: String,
    },

    /// Block quote.
    Quote,

    /// Horizontal rule.
    Divider,

    /// Table wrapper (rows are child blocks of type `TableRow`).
    Table {
        /// Declared column count.
        column_count: usize,
        /// Whether the first row is a header.
        header_row: bool,
    },

    /// One row of a table.
    TableRow {
        /// Cell content, plain text.
        cells: Vec<String>,
    },

    /// External or internal embed.
    Embed {
        /// External URL or internal wikilink.
        url: String,
        /// How the embed should be rendered.
        embed_type: EmbedType,
        /// Cached title, if fetched.
        title: Option<String>,
        /// Arbitrary plugin-provided metadata.
        metadata: HashMap<String, PropertyValue>,
    },

    /// Rendered view over a `.bases` database.
    DatabaseView {
        /// Path to the database file, relative to forge root.
        database_path: String,
        /// View configuration.
        view_config: DatabaseViewConfig,
    },

    /// Inline image.
    Image {
        /// Relative path in `attachments/` or URL.
        src: String,
        /// Accessibility text.
        alt_text: String,
        /// Optional explicit pixel width.
        width: Option<u32>,
        /// Optional explicit pixel height.
        height: Option<u32>,
    },

    /// Inline video.
    Video {
        /// Source URL or attachments path.
        src: String,
        /// Poster frame URL.
        poster: Option<String>,
        /// Optional explicit pixel width.
        width: Option<u32>,
        /// Optional explicit pixel height.
        height: Option<u32>,
    },

    /// Inline audio clip.
    Audio {
        /// Source URL or attachments path.
        src: String,
    },

    /// Generic file attachment.
    File {
        /// Relative path in `attachments/`.
        src: String,
        /// Display file name.
        file_name: String,
    },

    /// Rich link preview.
    Bookmark {
        /// URL the bookmark points at.
        url: String,
        /// Display title.
        title: String,
        /// Optional description/summary.
        description: Option<String>,
        /// Optional favicon URL.
        icon: Option<String>,
    },

    /// Synced block: a mirror of another block's content.
    SyncedBlock {
        /// The source block whose content is mirrored.
        source_block_id: BlockId,
        /// `true` if this block is the original source.
        is_source: bool,
    },

    /// Multi-column layout container.
    ColumnLayout {
        /// Width of each column, as a fraction in `[0, 1]`.
        column_widths: Vec<f32>,
    },

    /// A single column inside a `ColumnLayout`.
    Column {
        /// Width as a fraction in `[0, 1]`.
        width: f32,
    },

    /// Auto-generated table of contents.
    TableOfContents {
        /// Deepest heading level to include.
        max_depth: u8,
    },

    /// BL-141 — read-only excerpt from another file, rendered inline as
    /// part of a synthetic multibuffer session. The `content` field of
    /// the parent [`Block`] holds the snapshot text (the lines from
    /// `source_relpath` between `line_start` and `line_end`, inclusive,
    /// captured at `open_excerpts` time). `label` is an optional
    /// caller-supplied header (e.g. the diagnostic message, the
    /// reference site, the rename target name).
    ///
    /// Excerpt blocks live in synthetic sessions only — they're never
    /// produced by markdown parse and never written back to disk
    /// through `save`. Multibuffer sessions reject `apply_transaction`
    /// in this first cut; read-write routing is deferred to BL-141
    /// Phase 2.
    Excerpt {
        /// Forge-relative path of the source file this excerpt was
        /// captured from.
        source_relpath: String,
        /// First line of the captured range (1-based, inclusive).
        line_start: u32,
        /// Last line of the captured range (1-based, inclusive).
        line_end: u32,
        /// Optional caller-supplied label rendered alongside the
        /// `{source_relpath}#L{line_start}-L{line_end}` header (e.g.
        /// the diagnostic message for a multibuffer of errors).
        label: Option<String>,
    },
}

/// Rendering mode for an [`BlockType::Embed`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EmbedType {
    /// A file attachment embedded inline.
    File,
    /// Another note in the forge.
    Note,
    /// A database view.
    Database,
    /// An image.
    Image,
    /// A video.
    Video,
    /// An audio clip.
    Audio,
    /// A generic web page.
    Web,
    /// Plugin-provided embed with a type tag.
    Custom(String),
}

/// Configuration for a [`BlockType::DatabaseView`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct DatabaseViewConfig {
    /// Visual layout: table, kanban, etc.
    pub view_type: DatabaseViewType,
    /// Applied filters.
    pub filters: Vec<String>,
    /// Applied sorts.
    pub sorts: Vec<String>,
    /// Optional group-by field.
    pub group_by: Option<String>,
    /// Columns hidden from the view.
    pub hidden_columns: Vec<String>,
}

/// Visual layout variants for a database view.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DatabaseViewType {
    /// Flat table.
    #[default]
    Table,
    /// Kanban board, grouped by the named field.
    Kanban {
        /// Field to group cards by.
        column_by: String,
    },
    /// Calendar view, positioned by the named date field.
    Calendar {
        /// Date field controlling placement.
        date_field: String,
    },
    /// Gallery view with the named title field.
    Gallery {
        /// Field used as card title.
        title_field: String,
    },
    /// Plugin-provided view type.
    Custom(String),
}

// ── BlockProperties ───────────────────────────────────────────────────────────

/// Rich attributes attached to a block (properties panel).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct BlockProperties {
    /// User-editable attributes.
    pub attributes: HashMap<String, PropertyValue>,

    /// Computed (read-only) properties like word count.
    pub computed: HashMap<String, PropertyValue>,
}

// ── PropertyValue ─────────────────────────────────────────────────────────────

/// Loosely-typed property values usable in block attributes,
/// annotations and frontmatter.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue {
    /// String value.
    String(String),
    /// Numeric value.
    Number(f64),
    /// Boolean value.
    Boolean(bool),
    /// Ordered list of values.
    List(Vec<PropertyValue>),
    /// Keyed object.
    Object(HashMap<String, PropertyValue>),
}

impl PartialEq for PropertyValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a.to_bits() == b.to_bits(),
            (Self::Boolean(a), Self::Boolean(b)) => a == b,
            (Self::List(a), Self::List(b)) => a == b,
            (Self::Object(a), Self::Object(b)) => a == b,
            _ => false,
        }
    }
}

// ── DocumentMetadata ──────────────────────────────────────────────────────────

/// Per-document metadata attached to a tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentMetadata {
    /// Path relative to the forge root.
    pub file_path: String,

    /// Markdown or MDX.
    pub file_type: FileType,

    /// Unix epoch ms at creation.
    pub created_at: i64,

    /// Unix epoch ms of the most recent write.
    pub updated_at: i64,

    /// Whole-document word count.
    pub word_count: usize,

    /// Reading-time estimate in seconds (~200 wpm).
    pub read_time_seconds: usize,

    /// Frontmatter / custom properties.
    pub properties: HashMap<String, PropertyValue>,
}

impl DocumentMetadata {
    /// Empty metadata with current timestamps and no path.
    #[must_use]
    pub fn empty() -> Self {
        let ts = now_ms();
        Self {
            file_path: String::new(),
            file_type: FileType::Markdown,
            created_at: ts,
            updated_at: ts,
            word_count: 0,
            read_time_seconds: 0,
            properties: HashMap::new(),
        }
    }
}

impl Default for DocumentMetadata {
    fn default() -> Self {
        Self::empty()
    }
}

/// File format variants a document can roundtrip to on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    /// Plain `.md` markdown.
    #[default]
    Markdown,
    /// `.mdx` with component embeds.
    Mdx,
}

// ── Time helper ───────────────────────────────────────────────────────────────

/// Current Unix epoch in milliseconds.
///
/// Public so transaction-layer callers stamp their own mutations with
/// the same clock source.
#[must_use]
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Helper for `#[serde(skip_serializing_if = "is_false")]` so default
/// boolean flags (BL-142's `CodeBlock.repl`) round-trip cleanly
/// without polluting serialized JSON for the common `false` case.
fn is_false(b: &bool) -> bool {
    !*b
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_block_has_fresh_uuid_and_timestamps() {
        let a = Block::new(BlockType::Paragraph);
        let b = Block::new(BlockType::Paragraph);
        assert_ne!(a.id, b.id);
        assert!(a.created_at > 0);
        assert_eq!(a.created_at, a.updated_at);
    }

    #[test]
    fn block_builders_compose() {
        let ann = Annotation {
            start: 0,
            end: 3,
            ty: crate::annotation::AnnotationType::Bold,
        };
        let blk = Block::new(BlockType::Paragraph)
            .with_content("hi there")
            .with_annotations(vec![ann.clone()]);
        assert_eq!(blk.content, "hi there");
        assert_eq!(blk.annotations, vec![ann]);
    }

    #[test]
    fn construct_every_representative_variant() {
        let _ = BlockType::Paragraph;
        let _ = BlockType::Heading { level: 2 };
        let _ = BlockType::BulletList { indent_level: 0 };
        let _ = BlockType::NumberedList {
            indent_level: 0,
            number: 1,
        };
        let _ = BlockType::ToggleList {
            indent_level: 0,
            is_open: true,
        };
        let _ = BlockType::CodeBlock {
            language: "rust".into(),
            line_numbers: false,
            repl: false,
        };
        let _ = BlockType::MathBlock {
            formula: "E=mc^2".into(),
        };
        let _ = BlockType::Callout {
            icon: "⚠".into(),
            color: "yellow".into(),
            alert_type: "warning".into(),
        };
        let _ = BlockType::Quote;
        let _ = BlockType::Divider;
        let _ = BlockType::Table {
            column_count: 3,
            header_row: true,
        };
        let _ = BlockType::TableRow {
            cells: vec!["a".into(), "b".into()],
        };
        let _ = BlockType::Embed {
            url: "https://example".into(),
            embed_type: EmbedType::Web,
            title: None,
            metadata: HashMap::new(),
        };
        let _ = BlockType::DatabaseView {
            database_path: "x.bases".into(),
            view_config: DatabaseViewConfig::default(),
        };
        let _ = BlockType::Image {
            src: "a.png".into(),
            alt_text: "a".into(),
            width: None,
            height: None,
        };
        let _ = BlockType::Column { width: 0.5 };
        let _ = BlockType::ColumnLayout {
            column_widths: vec![0.3, 0.7],
        };
        let _ = BlockType::SyncedBlock {
            source_block_id: Uuid::new_v4(),
            is_source: true,
        };
        let _ = BlockType::TableOfContents { max_depth: 3 };
    }

    #[test]
    fn property_value_partial_eq_covers_number_bits() {
        let a = PropertyValue::Number(1.5_f64);
        let b = PropertyValue::Number(1.5_f64);
        assert_eq!(a, b);

        // f64::NAN != f64::NAN by IEEE, but to_bits equality is
        // reflexive — same NaN bit pattern compares equal.
        let nan = PropertyValue::Number(f64::NAN);
        let nan_clone = nan.clone();
        assert_eq!(nan, nan_clone);

        assert_ne!(
            PropertyValue::Number(1.0),
            PropertyValue::Number(1.0 + f64::EPSILON)
        );
    }

    #[test]
    fn property_value_different_variants_are_unequal() {
        assert_ne!(
            PropertyValue::String("1".into()),
            PropertyValue::Number(1.0)
        );
    }

    #[test]
    fn property_value_object_equality() {
        let mut a = HashMap::new();
        a.insert("k".into(), PropertyValue::Boolean(true));
        let mut b = HashMap::new();
        b.insert("k".into(), PropertyValue::Boolean(true));
        assert_eq!(PropertyValue::Object(a), PropertyValue::Object(b));
    }

    #[test]
    fn document_metadata_empty_is_zeroed_counts() {
        let m = DocumentMetadata::empty();
        assert_eq!(m.word_count, 0);
        assert_eq!(m.file_type, FileType::Markdown);
    }
}
