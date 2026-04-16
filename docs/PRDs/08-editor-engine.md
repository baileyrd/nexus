# Editor Engine PRD — Nexus v1.0

**Version:** 1.0  
**Date:** April 2026  
**Status:** Implementation Ready  
**Owner:** Editor & UX Team

---

## Executive Summary

The Editor Engine is a hybrid markdown/MDX editor that bridges file-based storage with rich, interactive editing. Documents are stored as plaintext markdown on disk (the source of truth), parsed into an in-memory block tree for efficient rich editing, and rendered live in the UI via CodeMirror 6 decorations. Every editing action is a transaction that updates both the block tree and the underlying markdown file atomically. The editor supports Notion-style block-based navigation and composition, slash commands for rapid content creation, MDX components for interactive elements, and AI-assisted editing at multiple levels of granularity (inline, block, document).

This PRD specifies the data models (block tree, annotations, transactions), the markdown roundtrip pipeline, CodeMirror 6 integration architecture, extension points for plugins and AI, and UX flows for core editing scenarios.

---

## 1. Block Tree Data Model

### 1.1 Core Structures

The block tree is the in-memory representation of a document. Every element — paragraph, heading, list item, code block, embed, table — is a `Block`. Blocks form a tree where each block has at most one parent and zero or more children.

#### Block Struct

```rust
pub struct Block {
    /// Unique identifier within the document (UUID v4)
    pub id: BlockId,
    
    /// Type discriminant and type-specific data
    pub ty: BlockType,
    
    /// Plain-text content (excluding inline formatting)
    pub content: String,
    
    /// Annotations: formatting, links, mentions, embeds over ranges
    pub annotations: Vec<Annotation>,
    
    /// Rich attributes (properties panel data)
    pub properties: BlockProperties,
    
    /// Parent block ID (None for root blocks)
    pub parent_id: Option<BlockId>,
    
    /// Child block IDs (empty vec for leaves)
    pub children: Vec<BlockId>,
    
    /// Position in parent's children list
    pub index_in_parent: usize,
    
    /// Metadata
    pub created_at: i64,     // Unix timestamp, ms
    pub updated_at: i64,
    pub is_deleted: bool,    // Soft delete for undo
}

pub type BlockId = Uuid;

pub struct BlockProperties {
    /// User-editable attributes (color, icon, bookmark, etc.)
    pub attributes: HashMap<String, PropertyValue>,
    
    /// Computed properties (word count, char count)
    pub computed: HashMap<String, PropertyValue>,
}

#[derive(Clone, Debug)]
pub enum PropertyValue {
    String(String),
    Number(f64),
    Boolean(bool),
    List(Vec<PropertyValue>),
    Object(HashMap<String, PropertyValue>),
}
```

### 1.2 BlockType Enum

```rust
pub enum BlockType {
    // Text blocks
    Paragraph,
    Heading { level: u8 },      // 1-6
    
    // Lists
    BulletList {
        indent_level: u8,
    },
    NumberedList {
        indent_level: u8,
        number: u32,             // Current number in sequence
    },
    ToggleList {
        indent_level: u8,
        is_open: bool,           // Collapsed/expanded state
    },
    
    // Code & math
    CodeBlock {
        language: String,        // "rust", "python", etc., empty for plain
        line_numbers: bool,
    },
    MathBlock {
        formula: String,         // LaTeX source (content is formatted for display)
    },
    
    // Rich blocks
    Callout {
        icon: String,            // emoji or icon name
        color: String,           // "blue", "red", etc.
        alert_type: String,      // "info", "warning", "error", "success"
    },
    Quote,
    Divider,
    
    // Structured
    Table {
        column_count: usize,
        header_row: bool,
    },
    TableRow {
        cells: Vec<String>,      // Cell content, plain text
    },
    
    // Embeds & references
    Embed {
        url: String,             // External URL or internal wikilink
        embed_type: EmbedType,
        title: Option<String>,   // Cached title
        metadata: HashMap<String, PropertyValue>,
    },
    DatabaseView {
        database_path: String,   // Path to .db file in attachments/
        view_config: DatabaseViewConfig,
    },
    Image {
        src: String,             // Relative path in attachments/
        alt_text: String,
        width: Option<u32>,
        height: Option<u32>,
    },
    Video {
        src: String,
        poster: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
    },
    Audio {
        src: String,
    },
    File {
        src: String,             // Relative path in attachments/
        file_name: String,
    },
    Bookmark {
        url: String,
        title: String,
        description: Option<String>,
        icon: Option<String>,
    },
    SyncedBlock {
        source_block_id: BlockId,
        is_source: bool,         // True if this is the original
    },
    
    // Layout
    ColumnLayout {
        column_widths: Vec<f32>, // Percentages or ratios
    },
    Column {
        width: f32,
    },
    
    // Document structure
    TableOfContents {
        max_depth: u8,           // Max heading level to include
    },
}

pub enum EmbedType {
    File,
    Note,
    Database,
    Image,
    Video,
    Audio,
    Web,
    Custom(String),  // Plugin-provided embed types
}

pub struct DatabaseViewConfig {
    pub view_type: DatabaseViewType,  // table, kanban, calendar, gallery
    pub filters: Vec<Filter>,
    pub sorts: Vec<Sort>,
    pub group_by: Option<String>,
    pub hidden_columns: Vec<String>,
}

pub enum DatabaseViewType {
    Table,
    Kanban { column_by: String },
    Calendar { date_field: String },
    Gallery { title_field: String },
    Custom(String),
}
```

### 1.3 Block Tree Structure

```rust
pub struct BlockTree {
    /// All blocks, indexed by ID for O(1) lookup
    pub blocks: HashMap<BlockId, Block>,
    
    /// Root block IDs (blocks with no parent)
    pub root_blocks: Vec<BlockId>,
    
    /// Document-level metadata
    pub metadata: DocumentMetadata,
}

pub struct DocumentMetadata {
    pub file_path: String,                    // Relative to forge root
    pub file_type: FileType,                  // .md, .mdx
    pub created_at: i64,
    pub updated_at: i64,
    pub word_count: usize,
    pub read_time_seconds: usize,             // ~200 wpm
    pub properties: HashMap<String, PropertyValue>,  // From frontmatter
}

pub enum FileType {
    Markdown,
    Mdx,
}

impl BlockTree {
    /// Navigate: get parent block
    pub fn parent(&self, block_id: BlockId) -> Option<&Block> {
        self.blocks.get(block_id)
            .and_then(|b| b.parent_id)
            .and_then(|pid| self.blocks.get(&pid))
    }
    
    /// Navigate: get children blocks in order
    pub fn children(&self, block_id: BlockId) -> Vec<&Block> {
        self.blocks.get(block_id)
            .map(|b| b.children.iter()
                .filter_map(|cid| self.blocks.get(cid))
                .collect())
            .unwrap_or_default()
    }
    
    /// Navigate: sibling navigation
    pub fn next_sibling(&self, block_id: BlockId) -> Option<&Block> {
        let block = self.blocks.get(&block_id)?;
        let parent = self.parent(block_id)?;
        let next_idx = block.index_in_parent + 1;
        let next_id = parent.children.get(next_idx)?;
        self.blocks.get(next_id)
    }
    
    pub fn prev_sibling(&self, block_id: BlockId) -> Option<&Block> {
        let block = self.blocks.get(&block_id)?;
        let parent = self.parent(block_id)?;
        if block.index_in_parent == 0 { return None; }
        let prev_idx = block.index_in_parent - 1;
        let prev_id = parent.children.get(prev_idx)?;
        self.blocks.get(prev_id)
    }
    
    /// Get all descendants in document order
    pub fn descendants(&self, block_id: BlockId) -> Vec<BlockId> {
        let mut result = Vec::new();
        fn traverse(tree: &BlockTree, id: BlockId, result: &mut Vec<BlockId>) {
            let block = match tree.blocks.get(&id) {
                Some(b) => b,
                None => return,
            };
            for &child_id in &block.children {
                result.push(child_id);
                traverse(tree, child_id, result);
            }
        }
        traverse(self, block_id, &mut result);
        result
    }
}
```

---

## 2. Rich Text Model: Annotations

Inline formatting (bold, italic, link, mention, color, highlight) is represented as annotations over text ranges. This model is inspired by Notion's approach and enables flexible composition of multiple formatting layers on the same text.

### 2.1 Annotation Structure

```rust
pub struct Annotation {
    /// Start position (inclusive) in block.content
    pub start: usize,
    
    /// End position (exclusive) in block.content
    pub end: usize,
    
    /// Type and data
    pub ty: AnnotationType,
}

pub enum AnnotationType {
    Bold,
    Italic,
    Strikethrough,
    Underline,
    Code,
    
    // Color/background
    TextColor(String),        // "#FF0000" or "red"
    HighlightColor(String),
    
    // Links
    Link {
        url: String,
        title: Option<String>,
    },
    Wikilink {
        path: String,
        display_text: Option<String>,
        is_resolved: bool,     // True if path exists
    },
    
    // Inline objects
    Mention {
        user_id: String,
        display_name: String,
    },
    MathInline {
        formula: String,       // LaTeX
    },
    
    // Plugin-defined
    Custom {
        plugin_id: String,
        ty: String,
        data: HashMap<String, PropertyValue>,
    },
}

impl Annotation {
    /// Check if two annotations overlap
    pub fn overlaps(&self, other: &Annotation) -> bool {
        !(self.end <= other.start || self.start >= other.end)
    }
    
    /// Merge adjacent annotations of the same type
    pub fn merge(mut annotations: Vec<Annotation>) -> Vec<Annotation> {
        if annotations.is_empty() { return vec![]; }
        annotations.sort_by_key(|a| (a.start, a.end));
        
        let mut result = vec![annotations[0].clone()];
        for ann in annotations.into_iter().skip(1) {
            if let Some(last) = result.last_mut() {
                if last.end >= ann.start && std::mem::discriminant(&last.ty) == std::mem::discriminant(&ann.ty) {
                    last.end = last.end.max(ann.end);
                    continue;
                }
            }
            result.push(ann);
        }
        result
    }
}
```

### 2.2 Handling Nested Annotations

When annotations overlap, they compose. For example, text can be both bold and a link:

```
content: "Click here"
annotations:
  - (0, 10, Bold)
  - (0, 10, Link { url: "..." })
```

The UI renderer stacks these in order, applying CSS classes or markup for each. The editor prevents invalid nesting (e.g., code inside bold is allowed; nested lists are handled via BlockType).

### 2.3 Annotation Transformations on Edit

When text is inserted or deleted, annotations are adjusted:

```rust
pub fn adjust_annotations(
    annotations: &mut [Annotation],
    edit_start: usize,
    edit_length: isize,  // Positive for insert, negative for delete
) {
    for ann in annotations {
        if edit_start <= ann.start {
            // Edit before annotation: shift start and end
            ann.start = (ann.start as isize + edit_length).max(0) as usize;
            ann.end = (ann.end as isize + edit_length).max(0) as usize;
        } else if edit_start < ann.end {
            // Edit within annotation: expand or contract end
            ann.end = (ann.end as isize + edit_length).max(ann.start as isize) as usize;
        }
        // Edit after annotation: no change
    }
}
```

---

## 3. Markdown ↔ Block Tree Roundtrip

### 3.1 Parsing Pipeline: Markdown → Block Tree

The parsing pipeline converts markdown source text into a block tree. It is lossless in the sense that the block tree can be serialized back to markdown that renders identically.

#### Step-by-Step Pipeline

```
markdown_source
    ↓
[1] Extract frontmatter (YAML between --- markers)
    ↓
[2] Parse YAML frontmatter → DocumentMetadata.properties
    ↓
[3] Parse remaining markdown with comrak
    ↓
[4] Walk AST, convert to blocks
    ├─ heading → Block::Heading
    ├─ paragraph → Block::Paragraph (may contain inline elements)
    ├─ code_block → Block::CodeBlock
    ├─ list → Block::BulletList/NumberedList/ToggleList (children: list_items)
    ├─ list_item → Block with parent_id pointing to list
    ├─ table → Block::Table (children: Block::TableRow for each row)
    ├─ block_quote → Block::Quote
    ├─ html_block → skip (non-markdown)
    └─ custom (MDX components, HTML) → Block::Embed or skip
    ↓
[5] Extract and annotate inline elements
    ├─ **bold** → content + Bold annotation
    ├─ *italic* → content + Italic annotation
    ├─ `code` → content + Code annotation
    ├─ [link](url) → content + Link annotation
    ├─ [[wikilink]] → content + Wikilink annotation
    ├─ #tag → inline tag (stored in block metadata)
    ├─ $math$ → MathInline annotation
    └─ <Color>text</Color> → TextColor annotation
    ↓
[6] Build block tree
    ├─ Establish parent/child relationships
    ├─ Assign block IDs (hash-based for determinism on reparse)
    ├─ Compute word counts
    └─ Validate tree structure
    ↓
block_tree
```

### 3.2 Parser Implementation

```rust
pub struct MarkdownParser {
    options: ParseOptions,
}

pub struct ParseOptions {
    /// Enable GitHub Flavored Markdown extensions
    pub gfm_enabled: bool,
    
    /// Enable custom Nexus syntax (wikilinks, tags, etc.)
    pub nexus_syntax_enabled: bool,
    
    /// Enable MDX components
    pub mdx_enabled: bool,
}

impl MarkdownParser {
    pub fn parse(&self, source: &str) -> Result<BlockTree> {
        // 1. Extract frontmatter
        let (yaml_str, markdown_body) = extract_frontmatter(source);
        let properties = parse_yaml(&yaml_str).unwrap_or_default();
        
        // 2. Parse with comrak
        let ast = comrak::parse_document(
            &comrak::Arena::new(),
            markdown_body,
            &ComrakOptions {
                extension: ComrakExtensionOptions {
                    strikethrough: true,
                    tagfilter: false,
                    table: true,
                    autolink: true,
                    tasklist: true,
                    header_ids: Some("".into()),  // For TOC
                    footnotes: false,
                    description_lists: false,
                },
                parse: ComrakParseOptions {
                    smart: false,
                    default_info_string: None,
                },
                render: ComrakRenderOptions::default(),
            },
        );
        
        // 3. Walk AST, collect block info
        let mut blocks = HashMap::new();
        let mut root_blocks = Vec::new();
        let mut id_counter = 0u64;
        
        let mut visitor = BlockExtractor {
            blocks: &mut blocks,
            root_blocks: &mut root_blocks,
            parent_id_stack: vec![],
            id_counter: &mut id_counter,
            arena: &ast_arena,
        };
        visitor.visit(&ast);
        
        // 4. Build final tree
        Ok(BlockTree {
            blocks,
            root_blocks,
            metadata: DocumentMetadata {
                file_path: "".into(),
                file_type: FileType::Markdown,
                created_at: now_ms(),
                updated_at: now_ms(),
                word_count: count_words_in_tree(&blocks, &root_blocks),
                read_time_seconds: (word_count / 200).max(1),
                properties,
            },
        })
    }
}

/// Deterministic block ID generation
fn block_id_from_hash(content: &str, ty: &BlockType) -> BlockId {
    let mut hasher = sha2::Sha256::new();
    hasher.update(content.as_bytes());
    hasher.update(format!("{:?}", ty).as_bytes());
    let hash = hasher.finalize();
    Uuid::from_bytes(hash[0..16].try_into().unwrap())
}
```

### 3.3 Serialization: Block Tree → Markdown

Serialization is the inverse pipeline. Every block is rendered to markdown, preserving formatting, structure, and wikilinks.

```rust
pub struct MarkdownSerializer;

impl MarkdownSerializer {
    pub fn serialize(tree: &BlockTree) -> String {
        let mut output = String::new();
        
        // 1. Serialize frontmatter
        if !tree.metadata.properties.is_empty() {
            output.push_str("---\n");
            output.push_str(&serialize_yaml(&tree.metadata.properties));
            output.push_str("\n---\n\n");
        }
        
        // 2. Serialize blocks
        for root_id in &tree.root_blocks {
            serialize_block(tree, *root_id, 0, &mut output);
        }
        
        output
    }
}

fn serialize_block(
    tree: &BlockTree,
    block_id: BlockId,
    depth: usize,
    output: &mut String,
) {
    let block = match tree.blocks.get(&block_id) {
        Some(b) => b,
        None => return,
    };
    
    // Render block type + content
    match &block.ty {
        BlockType::Heading { level } => {
            output.push_str(&format!("{} {}\n\n", "#".repeat(*level as usize), block.content));
        }
        BlockType::Paragraph => {
            let text = serialize_annotations(&block.content, &block.annotations);
            output.push_str(&format!("{}\n\n", text));
        }
        BlockType::BulletList { indent_level } => {
            let indent = "  ".repeat(*indent_level as usize);
            let text = serialize_annotations(&block.content, &block.annotations);
            output.push_str(&format!("{}* {}\n", indent, text));
        }
        BlockType::NumberedList { indent_level, number } => {
            let indent = "  ".repeat(*indent_level as usize);
            let text = serialize_annotations(&block.content, &block.annotations);
            output.push_str(&format!("{}{}. {}\n", indent, number, text));
        }
        BlockType::CodeBlock { language, .. } => {
            output.push_str(&format!("```{}\n{}\n```\n\n", language, block.content));
        }
        BlockType::Image { src, alt_text, .. } => {
            output.push_str(&format!("![{}]({})\n\n", alt_text, src));
        }
        _ => {
            // Other types as needed
        }
    }
    
    // Recurse to children
    for &child_id in &block.children {
        serialize_block(tree, child_id, depth + 1, output);
    }
}

fn serialize_annotations(content: &str, annotations: &[Annotation]) -> String {
    // Collect ranges: (start, end, is_open, annotation)
    let mut ranges: Vec<_> = annotations.iter()
        .flat_map(|a| vec![(a.start, true, a.clone()), (a.end, false, a.clone())])
        .collect();
    ranges.sort_by_key(|(pos, is_open, _)| (*pos, if *is_open { 0 } else { 1 }));
    
    let mut result = String::new();
    let mut char_idx = 0;
    let mut active_annotations: Vec<&Annotation> = Vec::new();
    
    for &byte_idx in content.char_indices().map(|(i, _)| i).collect::<Vec<_>>() {
        // Process range events at this position
        while let Some((pos, is_open, ann)) = ranges.first() {
            if *pos != byte_idx { break; }
            if *is_open {
                active_annotations.push(ann);
            } else {
                active_annotations.retain(|a| a != ann);
            }
            ranges.remove(0);
        }
        
        // Emit markdown for active annotations
        // (This is simplified; real impl would handle nesting and markup order)
        
        char_idx += 1;
    }
    
    result
}
```

### 3.4 Lossless Roundtrip Guarantees

The parser and serializer ensure:

- **Structural losslessness:** All block types, nesting, and ordering preserved.
- **Formatting losslessness:** Inline annotations (bold, italic, link) roundtrip exactly.
- **Content losslessness:** No text content is lost or mangled.
- **Normalization:** Some markdown variations are normalized (e.g., `**bold**` and `__bold__` both serialize as `**bold**`).
- **Frontmatter preservation:** YAML properties are preserved as-is.

**Exceptions (intentionally not preserved):**
- HTML comments or raw HTML blocks (stripped).
- Whitespace normalization (leading/trailing spaces in lines removed).
- Blank line consolidation (multiple consecutive blank lines become one).

---

## 4. CodeMirror 6 Integration

### 4.1 Editor Instance Architecture

The editor UI is a CodeMirror 6 instance running in a Tauri WebView (`app/src/editor/EditorSurface.tsx`). **CM6 is the canonical owner of character-level text state**; the Rust layer maintains an in-memory block tree updated via debounced IPC rather than per-keystroke round-trips.

**Document model ownership:**

| Layer | Owns |
|-------|------|
| CM6 (WebView, TypeScript) | Live document text, cursor, selection, undo/redo history, extension compartments |
| Rust block tree (`nexus-editor` core plugin) | In-memory parse of the markdown source; used for outline navigation, word count, AI hints, plugin decoration queries |

**Debounced sync flow:**

1. CM6 `updateListener` fires on `update.docChanged`.
2. `EditorSurface.tsx` waits **800 ms** of inactivity (debounced), then calls the `editor_sync_content` Tauri command with the full document string.
3. The Rust `nexus-editor` core plugin re-parses the markdown and replaces its in-memory `BlockTree`. No disk I/O occurs.
4. The AI-inline-edit pipeline and outline sidebar subscribe to the block tree and see the updated state within the next debounce window.

**Why 800 ms:** fast enough for outline updates to feel responsive, slow enough to avoid saturating the IPC channel during rapid typing. If a future collaborative-editing or AI-streaming feature requires tighter coupling, the debounce can be shortened or replaced with a structural-change detector that only syncs when paragraph or heading boundaries change.

**Rust session state (actual):** `EditorCorePlugin` in `crates/nexus-editor/src/core_plugin.rs` holds an `Arc<Mutex<Option<BlockTree>>>` per open file, keyed by path. There is no per-session `cm_state` mirror or `EditorListener` trait on the Rust side — CM6 holds all of that locally in the WebView.

> **Implementation divergence from original spec:** The original spec described a mirrored `EditorSession` with `cm_state: RwLock<EditorState>` and an `EditorListener` trait on the Rust side. The actual implementation chose the simpler debounced-push model: CM6 owns the live text; Rust sees snapshots. This was a deliberate tradeoff — per-keystroke IPC latency is unacceptable in a local desktop app, and the block tree does not need sub-second freshness for any current consumer.

### 4.2 CodeMirror Extensions

The editor loads these extensions:

| Extension | Purpose |
|-----------|---------|
| `lineNumbers()` | Display line numbers |
| `highlightActiveLineGutter()` | Highlight gutter for active line |
| `foldGutter()` | Code folding (by block) |
| `foldCode()` | Support folding |
| `syntaxHighlighting()` | Markdown syntax highlighting (custom) |
| `linter()` | Real-time markdown validation |
| `autocompletion()` | Autocomplete words + slash commands |
| `bracketMatching()` | Highlight matching brackets |
| `closeBrackets()` | Auto-close brackets |
| `indentOnInput()` | Smart indentation |
| `defaultKeymap()` | Standard key bindings (customizable) |
| `searchKeymap()` | Cmd+F search |
| `undo/redoKeymap()` | Undo/redo (hooked to our tx log) |
| `customLivePreviewExt` | Render decorations for blocks and inline formatting |
| `slashCommandExt` | Slash command overlay |
| `blockSelectExt` | Notion-style block selection (Cmd+A on line) |

### 4.3 Live Preview Decoration Strategy

Live preview is implemented via CodeMirror 6 Decorations. As the user types, the editor emits range sets of decorations that style the source text in place.

```typescript
// TypeScript side: src/plugins/editor/codemirror-extensions.ts

import { Decoration, DecorationSet, MatchDecorator, ViewPlugin, EditorView } from "@codemirror/view";
import { RangeSetBuilder } from "@codemirror/state";

// Extension: render **bold** as <strong>
const boldDecorator = new MatchDecorator({
  regexp: /\*\*([^*]+)\*\*/g,
  decoration: Decoration.mark({ class: "cm-bold", attributes: { style: "font-weight: bold;" } }),
});

// Extension: render [[wikilink]] with preview
const wikiLinkPreview = ViewPlugin.define((view) => {
  const decBuilder = new RangeSetBuilder<Decoration>();
  const { doc } = view.state;
  
  // Scan for [[ ]] patterns
  const regex = /\[\[([^\]]+)\]\]/g;
  let match;
  while ((match = regex.exec(doc.toString())) !== null) {
    const [fullMatch, linkText] = match;
    const start = match.index;
    const end = start + fullMatch.length;
    
    // Emit a decoration with custom styling
    decBuilder.add(
      start, end,
      Decoration.mark({
        class: "cm-wikilink",
        attributes: { "data-link": linkText },
      })
    );
  }
  
  return {
    decorations: decBuilder.finish(),
    update(u) {
      // Recompute decorations on change
      return u.docChanged;
    },
  };
});
```

### 4.4 Block Boundary Mapping

> **`BlockPositionMap` spec retired.** A live `BlockPositionMap` synchronised on every keystroke was not built and is not planned. CM6 natively tracks character positions inside the WebView; the Rust block tree is rebuilt from the full document text on each debounced sync (see §4.1) and does not maintain a persistent position index.

Character-to-block resolution for current consumers (outline scroll-to-heading, word-count badge) is performed by a linear scan over the block tree at query time, which is acceptably fast for documents of realistic size.

**Future path:** If AI-inline-edit or an LSP bridge (§14.5) requires stable block IDs that survive partial edits without a full re-parse, the correct approach is a CM6-side `StateField<RangeSet<BlockIdMark>>` — a decoration range set that CM6 keeps consistent across insertions and deletions automatically using its O(log n) range-set update semantics. This keeps position tracking in the layer that owns the text (CM6), rather than building a parallel Rust-side map that would need to stay in sync via IPC.

### 4.5 Keybinding Configuration

The editor supports multiple keybinding modes via configuration:

```rust
pub enum KeybindingMode {
    Default,   // Standard editor keys
    Vim,       // Vi/Vim emulation
    Emacs,     // Emacs-style bindings
}

pub struct KeybindingConfig {
    mode: KeybindingMode,
    custom_bindings: HashMap<String, Command>,
}

// User configures in .forge/config.toml:
// [editor]
// keybinding_mode = "vim"
// [[editor.custom_bindings]]
// key = "Ctrl-J"
// command = "editor:focus-next-block"
```

**Vim Mode Implementation:**
- Uses `vim` crate (Rust) or `@replit/codemirror-vim` (TypeScript) for basic vim operations.
- Supports: motions (hjkl, w, b, e, etc.), operators (d, c, y), counts, registers.
- Not full vim; focus is on common editing patterns (yank, delete line, find-replace).
- Visual mode: block selection via `v` and motion keys.

**Emacs Mode Implementation:**
- Standard emacs keys: Ctrl+A (line start), Ctrl+E (line end), Alt+F (forward word), Alt+B (backward word), etc.
- Kill ring (copy/paste) via Ctrl+W, Alt+W, Ctrl+Y.
- Search: Ctrl+S (forward), Ctrl+R (reverse).

---

## 5. Transaction & Undo/Redo System

### 5.1 Transaction Model

Every user action (keystroke, block operation, paste) is wrapped in a transaction. Transactions are atomic: either fully applied or fully rolled back.

```rust
pub struct Transaction {
    /// Unique ID
    pub id: Uuid,
    
    /// The operation(s) in this transaction
    pub operations: Vec<Operation>,
    
    /// Timestamp
    pub created_at: i64,
    
    /// Metadata (for debugging/history)
    pub metadata: TransactionMetadata,
}

pub enum Operation {
    InsertText {
        pos: usize,
        text: String,
    },
    DeleteText {
        pos: usize,
        length: usize,
    },
    InsertBlock {
        id: BlockId,
        parent_id: Option<BlockId>,
        index_in_parent: usize,
        block: Block,
    },
    DeleteBlock {
        id: BlockId,
        was_parent_id: Option<BlockId>,
    },
    ReparentBlock {
        id: BlockId,
        old_parent_id: Option<BlockId>,
        new_parent_id: Option<BlockId>,
        new_index_in_parent: usize,
    },
    UpdateBlockContent {
        id: BlockId,
        old_content: String,
        new_content: String,
        old_annotations: Vec<Annotation>,
        new_annotations: Vec<Annotation>,
    },
    UpdateAnnotations {
        block_id: BlockId,
        old_annotations: Vec<Annotation>,
        new_annotations: Vec<Annotation>,
    },
}

pub struct TransactionMetadata {
    pub user_action: UserAction,  // What the user did
    pub source: TransactionSource, // Who triggered it
    pub ai_edit: bool,             // Was this an AI-generated edit?
}

pub enum UserAction {
    Keystroke,
    Paste,
    Delete,
    SlashCommand { command: String },
    BlockOperation { op: BlockOp },
    DragDrop,
}

pub enum BlockOp {
    Create { block_type: String },
    Delete,
    Move { direction: String },  // up, down, left, right
    Transform { from_type: String, to_type: String },
    Indent,
    Unindent,
}

pub enum TransactionSource {
    User,
    Ai,
    Sync,
    System,
}

impl Transaction {
    /// Apply all operations to block tree in order
    pub fn apply(&self, tree: &mut BlockTree) -> Result<()> {
        for op in &self.operations {
            op.apply(tree)?;
        }
        tree.metadata.updated_at = now_ms();
        Ok(())
    }
    
    /// Reverse all operations in reverse order
    pub fn reverse(&self, tree: &mut BlockTree) -> Result<()> {
        for op in self.operations.iter().rev() {
            op.reverse(tree)?;
        }
        tree.metadata.updated_at = now_ms();
        Ok(())
    }
}
```

### 5.2 Undo Tree (Not Linear Stack)

Rather than a simple linear undo stack, we maintain an undo tree. This allows non-linear navigation through edit history.

```rust
pub struct UndoTree {
    /// All transactions ever executed
    transactions: Vec<Arc<Transaction>>,
    
    /// Current position in tree (index in transactions)
    current: usize,
    
    /// Links: parent_idx → children_indices
    children_map: HashMap<usize, Vec<usize>>,
}

impl UndoTree {
    /// Execute a new transaction
    pub fn execute(&mut self, tx: Transaction, tree: &mut BlockTree) -> Result<()> {
        tx.apply(tree)?;
        
        // If we're not at the leaf, create a branch
        self.transactions.truncate(self.current + 1);
        self.transactions.push(Arc::new(tx));
        self.current += 1;
        
        Ok(())
    }
    
    /// Undo to parent
    pub fn undo(&mut self, tree: &mut BlockTree) -> Result<()> {
        if self.current == 0 { return Ok(()); }
        
        self.transactions[self.current].reverse(tree)?;
        self.current -= 1;
        
        Ok(())
    }
    
    /// Redo to child
    pub fn redo(&mut self, tree: &mut BlockTree) -> Result<()> {
        if self.current + 1 >= self.transactions.len() { return Ok(()); }
        
        self.current += 1;
        self.transactions[self.current].apply(tree)?;
        
        Ok(())
    }
    
    /// Navigate to a specific transaction (if reachable from current)
    pub fn goto(&mut self, target_idx: usize, tree: &mut BlockTree) -> Result<()> {
        // Walk up to LCA, then down to target
        let lca = self.lowest_common_ancestor(self.current, target_idx);
        
        // Undo back to LCA
        while self.current > lca {
            self.undo(tree)?;
        }
        
        // Redo forward to target
        while self.current < target_idx {
            self.redo(tree)?;
        }
        
        Ok(())
    }
}
```

### 5.3 AI Edit Integration

When the AI modifies a block or document, it generates a transaction with `tx.metadata.source = TransactionSource::Ai`. This allows:

1. **Grouping:** The undo/redo UI can show "AI generated N edits" as a single undoable step.
2. **Selective acceptance:** Users can accept/reject AI edits block-by-block.
3. **Tracking:** The audit log distinguishes user edits from AI edits for analytics.

---

## 6. Slash Command System

### 6.1 Command Registry & Discovery

Slash commands (`/`) trigger a command palette. Commands are discovered from core plugins and community plugins.

```rust
pub struct SlashCommand {
    pub id: String,                    // "toggle-bold", "insert-image"
    pub label: String,                 // "Toggle Bold"
    pub description: String,           // "Make text bold or unbold"
    pub category: String,              // "formatting", "blocks", "ai"
    pub icon: Option<String>,          // Emoji or icon name
    pub keybinding: Option<String>,    // "Ctrl+B"
    pub can_execute: Box<dyn Fn(&EditorContext) -> bool>,
    pub execute: Box<dyn Fn(&EditorContext) -> Result<()>>,
    pub plugin_id: String,             // Which plugin provides this
}

pub struct SlashCommandRegistry {
    commands: HashMap<String, Arc<SlashCommand>>,
}

impl SlashCommandRegistry {
    pub fn register(&mut self, cmd: SlashCommand) {
        self.commands.insert(cmd.id.clone(), Arc::new(cmd));
    }
    
    pub fn search(&self, query: &str) -> Vec<Arc<SlashCommand>> {
        let mut results: Vec<_> = self.commands.values()
            .filter(|cmd| {
                cmd.label.to_lowercase().contains(&query.to_lowercase())
                    || cmd.description.to_lowercase().contains(&query.to_lowercase())
            })
            .cloned()
            .collect();
        
        results.sort_by_key(|cmd| {
            // Exact match ranks first, then by relevance
            if cmd.label.to_lowercase() == query.to_lowercase() { 0 } else { 1 }
        });
        
        results
    }
    
    pub fn execute(&self, cmd_id: &str, ctx: &EditorContext) -> Result<()> {
        let cmd = self.commands.get(cmd_id)
            .ok_or_else(|| anyhow::anyhow!("Command not found: {}", cmd_id))?;
        
        if !(cmd.can_execute)(ctx) {
            return Err(anyhow::anyhow!("Command cannot execute in current context"));
        }
        
        (cmd.execute)(ctx)
    }
}
```

### 6.2 Built-In Commands (Core Plugin)

The editor ships with these slash commands:

**Block Types:**
- `/heading1`, `/heading2`, ..., `/heading6` — Convert to heading
- `/paragraph` — Convert to paragraph
- `/bullet`, `/numbered`, `/toggle` — Convert to list type
- `/code` — Code block
- `/math` — Math block
- `/callout` — Callout (with icon/color)
- `/quote` — Quote block
- `/divider` — Horizontal divider
- `/table` — Insert table (prompt for rows/cols)
- `/toggle` — Toggle list item
- `/columns` — Insert column layout

**Embeds & Rich Content:**
- `/image` — Upload or link image
- `/video` — Embed video
- `/audio` — Embed audio
- `/bookmark` — Add bookmark from URL
- `/database` — Insert database view (with database picker)
- `/toc` — Table of contents
- `/embed` — Embed note/block (with search)

**Formatting:**
- `/bold`, `/italic`, `/code`, `/strikethrough`, `/underline` — Apply inline formatting

**Actions:**
- `/duplicate` — Duplicate current block
- `/delete` — Delete current block
- `/move-up`, `/move-down` — Move block
- `/indent`, `/unindent` — Change nesting level
- `/link` — Create or edit link

**AI:**
- `/ai-assist` — Open AI inline editor
- `/ai-continue` — Auto-complete text (paragraph continuation)
- `/ai-expand` — Expand brief note into full paragraph
- `/ai-summarize` — Summarize selected blocks
- `/ai-explain` — Add explanation/context

### 6.3 Slash Command UI

When user types `/`, a command palette appears:

```
───────────────────────────────────
/ |[search box]                ×
───────────────────────────────────

📝 Formatting
  ✓ Bold           (⌘B)
    Italic        (⌘I)
    Code          (⌘')
    Strikethrough
    Underline

📦 Blocks
    Heading 1
    Heading 2
    Bullet List
    Numbered List
    Code Block
    
✨ AI
    AI Assist
    Continue Writing
    Summarize
───────────────────────────────────
```

Navigation:
- Type to filter
- Arrow keys to select
- Enter to execute
- Escape to close

---

## 7. MDX Component Runtime

### 7.1 Component Discovery & Loading

MDX files can include JSX components. These are discovered from:

1. **Built-in components:** Nexus provides standard components (Button, Toggle, Form, etc.)
2. **Plugin components:** Plugins can register custom components
3. **Installed packages:** User can import npm packages (if in node_modules)

```rust
pub struct MdxComponentRegistry {
    components: HashMap<String, Arc<MdxComponent>>,
    plugin_sources: HashMap<String, Arc<PluginComponentSource>>,
}

pub struct MdxComponent {
    pub name: String,
    pub source: ComponentSource,
    pub schema: JsonSchema,          // Props schema for validation
    pub sandbox: bool,               // Should component run sandboxed?
    pub error_boundary: bool,        // Wrap with error boundary?
}

pub enum ComponentSource {
    BuiltIn { module_path: String },
    PluginProvided { plugin_id: String, path: String },
    FromPackage { package: String, export: String },
    UserDefined { file_path: String },
}

impl MdxComponentRegistry {
    /// Load a component by name; fetch from source if needed
    pub async fn load(&self, name: &str) -> Result<MdxComponent> {
        if let Some(comp) = self.components.get(name) {
            return Ok((**comp).clone());
        }
        
        // Search plugin sources
        for source in self.plugin_sources.values() {
            if let Ok(comp) = source.load_component(name).await {
                return Ok(comp);
            }
        }
        
        Err(anyhow::anyhow!("Component not found: {}", name))
    }
}
```

### 7.2 MDX Parsing & Rendering

The editor parses MDX files (markdown + JSX) using a standard MDX parser. Components are rendered within the editor as interactive elements.

```typescript
// TypeScript: src/plugins/editor/mdx-renderer.ts

import { evaluate } from '@mdx-js/mdx';
import { Fragment, jsx, jsxs } from 'react/jsx-runtime';

export async function renderMdxBlock(
  mdxSource: string,
  components: Record<string, React.ComponentType>
): Promise<React.ReactElement> {
  const { default: MDXContent } = await evaluate(mdxSource, {
    jsx, jsxs, Fragment,
    ...components,  // Inject our components
  });
  
  return <MDXContent />;
}
```

### 7.3 Sandboxing for Community Components

Community-provided components run in a Web Worker sandbox to prevent malicious code execution.

```rust
pub struct SandboxedComponentRuntime {
    worker: web_worker::Worker,
    component_cache: HashMap<String, Arc<LoadedComponent>>,
}

impl SandboxedComponentRuntime {
    /// Execute a component in a worker; receive rendered HTML back
    pub async fn render(
        &self,
        component_id: String,
        props: serde_json::Value,
    ) -> Result<String> {
        let (tx, rx) = oneshot::channel();
        
        self.worker.post_message(serde_json::json!({
            "action": "render",
            "component_id": component_id,
            "props": props,
        }))?;
        
        rx.await.map_err(|e| anyhow::anyhow!("Sandbox render failed: {}", e))
    }
}
```

### 7.4 Error Boundaries

Components that fail to render are wrapped in error boundaries:

```typescript
// src/plugins/editor/component-error-boundary.tsx
export const ComponentErrorBoundary: React.FC<{
  children: React.ReactElement;
  componentName: string;
}> = ({ children, componentName }) => {
  return (
    <ErrorBoundary
      FallbackComponent={({ error, resetErrorBoundary }) => (
        <div className="component-error">
          <div className="error-icon">⚠️</div>
          <div className="error-message">
            Component "{componentName}" failed to render
          </div>
          <details>
            <summary>Details</summary>
            <pre>{error.message}</pre>
          </details>
          <button onClick={resetErrorBoundary}>Retry</button>
        </div>
      )}
      onReset={() => { /* reset state */ }}
    >
      {children}
    </ErrorBoundary>
  );
};
```

---

## 8. Collaborative Editing Foundations

### 8.1 Block Tree + CRDT Integration

The block tree integrates with the storage engine's CRDT sync (via automerge). When two peers edit the same document concurrently, the CRDT merges changes automatically.

```rust
pub struct CollaborativeBlockTree {
    /// The Automerge document
    doc: automerge::Document,
    
    /// Cached block tree (computed from doc)
    cached_tree: RwLock<BlockTree>,
    
    /// Local changes not yet synced
    local_changes: RwLock<Vec<Operation>>,
}

impl CollaborativeBlockTree {
    /// Apply a local operation; broadcast to peers
    pub fn apply_local_operation(&self, op: Operation) -> Result<()> {
        // Transact on automerge doc
        let mut tx = self.doc.transaction();
        // (encode operation into automerge change)
        tx.commit()?;
        
        // Invalidate cache
        self.cached_tree.write().unwrap().metadata.updated_at = now_ms();
        
        // Queue for sync
        self.local_changes.write().unwrap().push(op);
        
        Ok(())
    }
    
    /// Receive remote changes; merge and rebuild tree
    pub async fn apply_remote_changes(&self, changes: Vec<automerge::Change>) -> Result<()> {
        self.doc.apply_changes(changes)?;
        
        // Rebuild tree from doc
        let new_tree = Self::tree_from_automerge(&self.doc)?;
        *self.cached_tree.write().unwrap() = new_tree;
        
        Ok(())
    }
}
```

### 8.2 Cursor Position Sharing (Future)

For future implementation, we'll track cursor positions and broadcast them to peers:

```rust
pub struct CursorState {
    pub user_id: String,
    pub user_name: String,
    pub user_color: String,
    pub position: usize,           // Character offset in document
    pub selection_start: Option<usize>,
    pub selection_end: Option<usize>,
    pub active_block_id: BlockId,
}

pub struct CursorBroadcaster {
    cursors: Arc<RwLock<HashMap<String, CursorState>>>,
    broadcast_tx: mpsc::Sender<CursorState>,
}
```

---

## 9. Vim/Emacs Keybinding Modes

### 9.1 Vim Mode Implementation

Vim mode is built atop CodeMirror's extensibility. We use a combination of native CM6 keybindings and custom state tracking.

```typescript
// src/plugins/editor/vim-keybindings.ts

import { vim, Vim } from '@replit/codemirror-vim';

export function enableVimMode(editor: EditorView) {
  // Register custom commands
  Vim.defineCommand('gotoBlock', (cm) => {
    // Jump to specific block by ID (custom command)
  });
  
  Vim.defineMotion('selectBlock', (cm) => {
    // Select entire block (custom motion)
  });
  
  editor.dispatch({
    effects: vim.setVimMode(true),
  });
}
```

**Vim Motions Supported:**
- `h`, `j`, `k`, `l` — Character navigation
- `w`, `b`, `e` — Word navigation
- `0`, `$` — Line start/end
- `gg`, `G` — Document start/end
- `{`, `}` — Block navigation
- `Ctrl+U`, `Ctrl+D` — Page up/down
- `f<char>`, `t<char>` — Find character
- `/pattern`, `?pattern` — Search

**Vim Operators:**
- `d` — Delete
- `c` — Change
- `y` — Yank (copy)
- `~` — Swap case
- `>`, `<` — Indent/unindent

**Vim Modes:**
- Normal, Insert, Visual, Visual Line

### 9.2 Emacs Mode Implementation

Emacs mode uses a similar approach, mapping Ctrl+/Alt+ key combinations to operations:

```typescript
// src/plugins/editor/emacs-keybindings.ts

const emacsKeymap: KeyBinding[] = [
  { key: 'Ctrl-a', run: goToLineStart },
  { key: 'Ctrl-e', run: goToLineEnd },
  { key: 'Alt-f', run: moveForwardByWord },
  { key: 'Alt-b', run: moveBackwardByWord },
  { key: 'Ctrl-w', run: killWord },
  { key: 'Alt-w', run: copyRegion },
  { key: 'Ctrl-y', run: yankRegion },
  { key: 'Ctrl-s', run: searchForward },
  { key: 'Ctrl-r', run: searchBackward },
];
```

---

## 10. Document-Level Features

### 10.1 Table of Contents Generation

A Table of Contents block (`BlockType::TableOfContents`) scans the document for headings and generates a linked outline.

```rust
pub fn generate_toc(tree: &BlockTree, max_depth: u8) -> Vec<TocEntry> {
    let mut toc = Vec::new();
    
    fn traverse(tree: &BlockTree, id: BlockId, toc: &mut Vec<TocEntry>, max_depth: u8, current_depth: u8) {
        if current_depth > max_depth { return; }
        
        let block = match tree.blocks.get(&id) {
            Some(b) => b,
            None => return,
        };
        
        if let BlockType::Heading { level } = block.ty {
            toc.push(TocEntry {
                level: level as u8,
                text: block.content.clone(),
                block_id: id,
            });
        }
        
        for &child_id in &block.children {
            traverse(tree, child_id, toc, max_depth, current_depth + 1);
        }
    }
    
    for &root_id in &tree.root_blocks {
        traverse(tree, root_id, &mut toc, max_depth, 1);
    }
    
    toc
}

#[derive(Clone)]
pub struct TocEntry {
    pub level: u8,
    pub text: String,
    pub block_id: BlockId,
}
```

### 10.2 Word Count & Reading Time

```rust
pub fn compute_stats(tree: &BlockTree) -> DocumentStats {
    let mut word_count = 0;
    let mut char_count = 0;
    
    for block in tree.blocks.values() {
        word_count += block.content.split_whitespace().count();
        char_count += block.content.len();
    }
    
    let reading_time_seconds = (word_count / 200).max(1);
    
    DocumentStats {
        word_count,
        char_count,
        reading_time_seconds,
        block_count: tree.blocks.len(),
    }
}
```

### 10.3 Outline View

An outline view displays the document's heading hierarchy, allowing quick navigation.

```rust
pub struct OutlineView {
    entries: Vec<OutlineEntry>,
}

pub struct OutlineEntry {
    heading_level: u8,
    text: String,
    block_id: BlockId,
    children: Vec<Box<OutlineEntry>>,
}

impl OutlineView {
    pub fn from_tree(tree: &BlockTree) -> Self {
        // Build outline from headings
        let entries = Self::build_outline(&tree, &tree.root_blocks, 0);
        OutlineView { entries }
    }
}
```

### 10.4 Breadcrumbs Navigation

As the user navigates to nested blocks, breadcrumbs show the path:

```
Home > Project Plan > Phase 2 > MVP Tasks > [Current Block]
```

---

## 11. Image & Media Handling

### 11.1 Image Insertion Flow

Users can insert images via:
1. **Paste:** Cmd+V with image in clipboard
2. **Drag & Drop:** Drag image file into editor
3. **Slash Command:** `/image` → file picker

```rust
pub async fn handle_image_insert(
    data: Vec<u8>,
    filename: String,
    editor: &EditorSession,
) -> Result<()> {
    // 1. Save to attachments/images/
    let hash = compute_hash(&data);
    let dest_path = format!("attachments/images/{}", filename);
    storage::write_file(&dest_path, &data).await?;
    
    // 2. Insert image block at cursor
    let image_block = Block {
        id: BlockId::new_v4(),
        ty: BlockType::Image {
            src: dest_path.clone(),
            alt_text: filename.clone(),
            width: None,
            height: None,
        },
        content: String::new(),
        annotations: vec![],
        properties: BlockProperties::default(),
        parent_id: Some(editor.current_block_id()?),
        children: vec![],
        index_in_parent: 0,
        created_at: now_ms(),
        updated_at: now_ms(),
        is_deleted: false,
    };
    
    editor.insert_block(image_block).await?;
    
    // 3. Save file
    editor.save().await?;
    
    Ok(())
}
```

### 11.2 Media Rendering

Images, videos, and audio are rendered in live preview with resize handles.

```typescript
// src/plugins/editor/media-renderer.tsx

export const ImageRenderer: React.FC<{
  src: string;
  altText: string;
  width?: number;
  height?: number;
  onResize?: (width: number, height: number) => void;
}> = ({ src, altText, width, height, onResize }) => {
  const [size, setSize] = React.useState({ width, height });
  
  return (
    <div className="image-container" style={{ width: size.width, height: size.height }}>
      <img src={src} alt={altText} style={{ width: '100%', height: '100%' }} />
      <ResizeHandle onResize={(w, h) => {
        setSize({ width: w, height: h });
        onResize?.(w, h);
      }} />
    </div>
  );
};
```

---

## 12. Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                    UI Layer (React)                         │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Editor Surface  │ Properties Panel │ Outline View   │  │
│  └────────────┬─────────────────────────────────────────┘  │
└───────────────┼────────────────────────────────────────────┘
                │ (Tauri IPC)
┌───────────────▼─────────────────────────────────────────────┐
│              Editor Engine (Rust)                           │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  CodeMirror 6 Integration                            │  │
│  │  ├─ Block Position Map                               │  │
│  │  ├─ Live Preview Decorations                         │  │
│  │  └─ Keybinding Manager (Vim/Emacs/Default)           │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Block Tree (In-Memory)                              │  │
│  │  ├─ BlockType enum (15+ variants)                    │  │
│  │  ├─ Annotation system (inline formatting)            │  │
│  │  ├─ Parent/child relationships                       │  │
│  │  └─ Document metadata                                │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Transaction System                                  │  │
│  │  ├─ Undo/Redo Tree (non-linear)                      │  │
│  │  ├─ Operation log                                    │  │
│  │  └─ AI edit tracking                                 │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Markdown ↔ Block Tree Roundtrip                     │  │
│  │  ├─ Parser (comrak-based)                            │  │
│  │  └─ Serializer (lossless)                            │  │
│  └──────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Command & Plugin System                             │  │
│  │  ├─ Slash command registry                           │  │
│  │  ├─ MDX component runtime                            │  │
│  │  └─ Editor extension API                             │  │
│  └──────────────────────────────────────────────────────┘  │
└──────────────────┬───────────────────────────────────────────┘
                   │
                   ├─────────────────────────┬──────────────────┐
                   │                         │                  │
        ┌──────────▼──────────┐  ┌──────────▼─────┐  ┌─────────▼──────┐
        │  Storage Engine     │  │  AI Engine      │  │  Plugin System │
        │  (File + Index)     │  │  (Inference)    │  │  (Commands)    │
        └─────────────────────┘  └─────────────────┘  └────────────────┘
```

---

## 13. Performance Targets

| Operation | Target | Notes |
|-----------|--------|-------|
| Keystroke latency | <50ms | Time from key press to visible update |
| Undo/redo | <10ms | Even with large blocks |
| Open file (1MB, 10k blocks) | <200ms | Parse + tree construction |
| Markdown → tree parsing | <100ms | For typical notes (5-50KB) |
| Tree → markdown serialization | <50ms | For export/save |
| Search within document | <100ms | Filter/find via regex |
| Block navigation (jump to block) | <5ms | Index lookup |
| Live preview render | <150ms | Update all decorations |
| AI block generation (200 tokens) | <2s | Network dependent |
| Memory per 1000-block document | ~10MB | Tree + undo history |

---

## 14. Extension Points for Plugins

Plugins can extend the editor via:

### 14.1 Custom Block Types

```rust
pub trait BlockTypeProvider: Send + Sync {
    /// Register a new block type
    fn register_block_type(&self, ty: String, metadata: BlockTypeMetadata);
    
    /// Render a block (for UI)
    fn render_block(&self, block: &Block) -> Result<HtmlString>;
    
    /// Serialize block to markdown
    fn to_markdown(&self, block: &Block) -> String;
}

pub struct BlockTypeMetadata {
    pub icon: String,
    pub label: String,
    pub description: String,
}
```

### 14.2 Custom Decorations (Live Preview)

```rust
pub trait DecorationProvider: Send + Sync {
    /// Return ranges and decoration styles for a block
    fn get_decorations(&self, block: &Block) -> Vec<(Range, DecorationStyle)>;
}
```

### 14.3 Custom Keybindings

```rust
pub trait KeybindingProvider: Send + Sync {
    fn get_keybindings(&self) -> Vec<KeyBinding>;
}

pub struct KeyBinding {
    pub key: String,        // "Ctrl-Shift-P"
    pub command: String,    // Command ID to execute
    pub when: Option<String>, // Condition (e.g., "editorFocus")
}
```

### 14.4 Slash Commands

Plugins register commands with the SlashCommandRegistry (see §6.1).

---

## 15. UX: Editing Modes

### 15.1 Source Mode

Raw markdown source is visible and editable. Useful for:
- Tweaking frontmatter
- Working with low-level formatting
- Copy/pasting markdown from other tools

### 15.2 Live Preview Mode (Default)

The editor renders markdown in place with live decorations. This is the default mode for most users.

```
# My Document

This is **bold** text with a [[wikilink]].

[Code block below]
```rust
fn main() {
    println!("Hello");
}
```

[Rendered above]
```

### 15.3 Reading Mode

The document is fully rendered (no markdown syntax visible). Useful for:
- Reading notes without editing
- Distraction-free reading
- Sharing rendered view with others

**Transition:** Users can switch modes via:
- Toggle button in toolbar
- Keyboard shortcut (Cmd+Shift+E)
- Editor settings

---

## 16. User Flows

### 16.1 Creating a New Note

```
1. Click "New" or press Cmd+N
2. Default filename: "Untitled [timestamp]"
3. Editor opens with cursor in document
4. User types heading + content
5. Slash command available (press / for palette)
6. Save on Cmd+S or auto-save every 30s
```

### 16.2 Inserting a Database View

```
1. Cursor in block
2. Press / → slash command palette
3. Type "database" → filter
4. Select "/database" command
5. Modal opens: Select database to embed
   [List of available databases]
6. Select database + view type (table/kanban/calendar)
7. View inserted at cursor with live data
```

### 16.3 Using Slash Commands

```
1. Type /
2. Slash command palette appears
3. Type to filter (e.g., "code" shows code-related commands)
4. Arrow keys to navigate
5. Enter to execute
6. Command applies and palette closes

Example: /heading2 → converts current block to H2
Example: /ai-assist → opens AI sidebar
```

### 16.4 Embedding an Image

**Option A: Drag & Drop**
```
1. Drag image file from Finder into editor
2. Image appears as Block::Image at drop location
3. Resize handles visible
4. Auto-saved to attachments/images/
```

**Option B: Clipboard**
```
1. Copy image (Cmd+C)
2. Paste in editor (Cmd+V)
3. Image inserted, file saved to attachments/
```

**Option C: Slash Command**
```
1. Type /image
2. File picker opens
3. Select image
4. Image inserted at cursor
```

### 16.5 AI Inline Assist

```
1. Select text or position cursor
2. Press Cmd+Shift+A (or /ai-assist from slash menu)
3. AI sidebar opens on right
4. User selects operation:
   - Continue writing
   - Summarize
   - Expand
   - Explain
   - Rephrase
5. AI generates replacement text
6. User accepts (Cmd+Enter) or rejects (Esc)
7. Accepted text replaces selection in a new transaction
```

---

## 17. Mobile Editing (Adaptive UI)

On mobile (screen width < 600px), the editor adapts:

- **Block selection:** Tap block to select; swipe left/right to indent/unindent
- **Slash commands:** Smaller command palette, touch-friendly buttons
- **Toolbar:** Bottom toolbar with common formatting and block options
- **Properties panel:** Collapse/expand via toggle; defaults to hidden
- **Outline view:** Swipe-accessible sidebar on right

---

## 18. Accessibility

### 18.1 Screen Reader Support

- All blocks have ARIA labels: `<div role="article" aria-label="Heading 2: My Section">`
- Annotations exposed via ARIA attributes: `aria-label="Bold text"`
- Navigation: Cmd+Arrow for block jumping (announced)
- Landmarks: Document regions marked with `<section>`, `<nav>`, etc.

### 18.2 Keyboard Navigation

- **Tab:** Move focus to next block
- **Shift+Tab:** Previous block
- **Arrow keys:** Within block (line navigation)
- **Cmd+Arrow:** Jump to document start/end
- **Cmd+K:** Focus search
- **Ctrl+/:** Toggle keybinding cheat sheet

### 18.3 Focus Management

- Focus visible outline (3px border, clear color contrast)
- Focus moves logically through document
- Skip links for jumping to main content (Shift+Alt+M)

### 18.4 Color & Contrast

- All text meets WCAG AA minimum contrast (4.5:1 for normal, 3:1 for large)
- Information not conveyed by color alone (icons + text)
- Color themes audited for accessibility

---

## Acceptance Criteria

- [ ] Block tree data model fully implemented with all BlockType variants
- [ ] Markdown parser produces valid block tree; roundtrip lossless for GFM + Nexus syntax
- [ ] Serializer produces valid markdown from block tree; no data loss
- [ ] CodeMirror 6 integration renders live preview with decorations
- [ ] Slash command system functional; 20+ commands available
- [ ] Undo/redo tree supports branching; non-linear navigation works
- [ ] MDX components parse and render within editor
- [ ] Keystroke latency <50ms for typical documents
- [ ] File open time <200ms for 10k-block documents
- [ ] Vim keybinding mode supports motions, operators, modes
- [ ] Emacs keybinding mode supports common Ctrl+/Alt+ commands
- [ ] Collaborative editing integrated with storage CRDT (cursor sync deferred)
- [ ] Mobile UI adapts to <600px screens
- [ ] Accessibility: WCAG 2.1 AA compliance for keyboard nav, screen readers, contrast
- [ ] All core slash commands implemented and tested
- [ ] Plugin API for custom blocks, decorations, keybindings functional
- [ ] Error handling for malformed markdown, missing images, component failures
- [ ] Documentation: API reference, user guide, plugin authoring guide

---

## Dependencies & Risks

| Dependency | Status | Risk | Mitigation |
|-----------|--------|------|-----------|
| `comrak` parser | Stable | Parsing edge cases | Comprehensive test suite; fallback to text |
| CodeMirror 6 | Stable | Large bundle size (~500KB) | Tree-shake, lazy-load extensions |
| `automerge-rs` CRDT | Beta | Merge semantics edge cases | Extensive testing; fallback to operational transform if needed |
| MDX `@mdx-js` | Stable | Security of component loading | Sandboxing, prop validation, error boundaries |
| Vim/Emacs modes | Partial | Full vim emulation difficult | Focus on common motions; extensibility via plugins |

---

## Timeline & Phases

- **Phase 1 (Weeks 1-2):** Block tree data model + Markdown parser/serializer
- **Phase 2 (Weeks 3-4):** CodeMirror 6 integration + live preview decorations
- **Phase 3 (Weeks 5-6):** Slash commands + MDX runtime + transaction system
- **Phase 4 (Weeks 7-8):** Keybinding modes (Vim/Emacs) + collaborative editing integration
- **Phase 5 (Weeks 9-10):** Mobile adaptation + accessibility audit + plugin API
- **Phase 6 (Week 11):** Testing, performance tuning, documentation
- **Phase 7 (Week 12):** UI integration, dogfooding, launch prep

---

**Document Version:** 1.0  
**Last Updated:** April 2026  
**Next Review:** Q3 2026
