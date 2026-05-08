# Nexus Growth Plan: Native Rustsidian Capabilities

> **Approach**: Grow Nexus organically by rebuilding Rustsidian features natively on the Nexus microkernel — no porting, no bridge layer, no adapter patterns. Each feature is designed from scratch to be event-driven, block-level, and plugin-aware.

---

## Gap Analysis

| Capability | Rustsidian | Nexus (Current) | Gap |
|---|---|---|---|
| Knowledge Graph (petgraph) | In-memory StableGraph, backlinks, unresolved links | Links table in SQLite, no graph queries | **Full build** |
| MDX/JSX Parsing | pulldown-cmark + custom JSX extractor | comrak (CommonMark only) | **Full build** |
| Block References (`^blockid`) | Parsed and tracked | Not parsed | **Parser extension** |
| Callouts (`> [!TYPE]`) | Parsed as blocks | Not parsed | **Parser extension** |
| Task Tracking | DB table, line numbers, due dates, completion | Not tracked | **Schema + parser** |
| AI Provider Integration | Anthropic, OpenAI, Ollama | Stubbed (`ai` command) | **Full build** |
| Embeddings / Vector Store | OpenAI/Ollama embeddings, LanceDB | None | **Full build** |
| RAG Pipeline | Chunking, retrieval, generation, citations | None | **Full build** |
| MCP Server | Full stdio/HTTP with 15+ tools | Stubbed (`mcp` command) | **Full build** |
| Daily Notes | CLI command + template | None | **Small feature** |
| Search Scoping | `tag:`, `path:`, `prop:` operators | Basic BM25 only | **Search extension** |
| Canvas / Base Views | JSON Canvas spec | None | **Full build (low priority)** |
| HTML Export | ammonia-sanitized HTML | None | **Medium feature** |
| Typed Property Index | Separate value columns by type | JSON string column | **Schema migration** |
| JSX Component Index | Dedicated DB table | None | **Schema + parser** |
| TUI Fuzzy Search | SkimMatcherV2 | Skeletal search overlay | **TUI enhancement** |

---

## Phase Overview

| Phase | Name | Duration | Priority Features |
|---|---|---|---|
| **0** | Foundation & Schema | 3–4 days | DB migration, new event types, graph module scaffold |
| **1** | Knowledge Graph + Backlinks | 5–7 days | In-memory graph, backlink queries, link resolution, CLI/TUI |
| **2** | Enhanced Markdown | 5–7 days | Block refs, callouts, tasks, MDX/JSX, search scoping |
| **3** | AI & RAG Pipeline | 7–9 days | AI providers, embeddings, vector store, RAG engine |
| **4** | MCP Server | 4–5 days | Protocol implementation, tool exposure, transport |
| **5** | Polish & Secondary | 4–6 days | Daily notes, export, TUI enhancements, canvas |
| | **Total** | **28–38 days** | |

---

## Phase 0: Foundation & Schema (3–4 days)

**Goal**: Prepare the Nexus storage layer and kernel for all subsequent phases. No user-facing features yet — this is infrastructure.

### Task 0.1 — Schema Migration v2 (1 day)

**File**: `crates/nexus-storage/src/schema.rs`

Add migration `002` that creates:

```sql
-- Task tracking
CREATE TABLE tasks (
    id          INTEGER PRIMARY KEY,
    file_id     INTEGER NOT NULL,
    block_id    INTEGER,
    content     TEXT NOT NULL,
    completed   BOOLEAN DEFAULT 0,
    line_number INTEGER,
    due_date    INTEGER,          -- unix timestamp (nullable)
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    FOREIGN KEY(file_id)  REFERENCES files(id) ON DELETE CASCADE,
    FOREIGN KEY(block_id) REFERENCES blocks(id) ON DELETE CASCADE
);
CREATE INDEX idx_tasks_file ON tasks(file_id);
CREATE INDEX idx_tasks_completed ON tasks(completed);

-- JSX component tracking (for MDX support in Phase 2)
CREATE TABLE jsx_components (
    id           INTEGER PRIMARY KEY,
    file_id      INTEGER NOT NULL,
    name         TEXT NOT NULL,
    props_json   TEXT,
    line_number  INTEGER,
    self_closing BOOLEAN DEFAULT 0,
    created_at   INTEGER NOT NULL,
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE
);
CREATE INDEX idx_jsx_file ON jsx_components(file_id);

-- Typed property index (upgrade from string-only)
ALTER TABLE properties ADD COLUMN value_num  REAL;
ALTER TABLE properties ADD COLUMN value_date INTEGER;
ALTER TABLE properties ADD COLUMN value_bool BOOLEAN;

-- Block reference anchors
ALTER TABLE blocks ADD COLUMN block_ref_id TEXT;
CREATE INDEX idx_blocks_ref ON blocks(block_ref_id) WHERE block_ref_id IS NOT NULL;

-- Graph: mark links as resolved and add alias column
ALTER TABLE links ADD COLUMN alias TEXT;
```

**Acceptance criteria**: `migrate()` returns version 2. All existing data preserved. Tests pass.

### Task 0.2 — New Event Types (0.5 day)

**File**: `crates/nexus-kernel/src/event.rs`

Add to `NexusEvent` enum:

```rust
// Graph events
GraphRebuilt { node_count: usize, edge_count: usize },
BacklinksChanged { file_path: String, backlink_count: usize },

// AI events
EmbeddingStarted { file_count: usize },
EmbeddingProgress { files_processed: usize, total_files: usize },
EmbeddingCompleted { duration_ms: u64 },
AiQueryStarted { query: String },
AiQueryCompleted { duration_ms: u64 },

// Task events
TaskCompleted { file_path: String, task_content: String },
TaskCreated { file_path: String, task_content: String },
```

**Acceptance criteria**: Events serialize/deserialize. EventFilter::Variant matches new names.

### Task 0.3 — Graph Module Scaffold (0.5 day)

**File**: New file `crates/nexus-storage/src/graph.rs`

Create the module with a `KnowledgeGraph` struct stub:

```rust
pub struct KnowledgeGraph { /* petgraph StableGraph inside */ }

impl KnowledgeGraph {
    pub fn new() -> Self { ... }
    pub fn rebuild_from_db(conn: &Connection) -> Result<Self, StorageError> { ... }
    pub fn add_note(&mut self, path: &str) -> NodeIndex { ... }
    pub fn remove_note(&mut self, path: &str) { ... }
    pub fn add_link(&mut self, source: &str, target: &str, link_type: &str, alias: Option<&str>) { ... }
    pub fn backlinks(&self, path: &str) -> Vec<BacklinkResult> { ... }
    pub fn outgoing_links(&self, path: &str) -> Vec<OutgoingLink> { ... }
    pub fn unresolved_links(&self) -> Vec<UnresolvedLink> { ... }
    pub fn stats(&self) -> GraphStats { ... }
}
```

Add `petgraph = "0.7"` to workspace dependencies.

**Acceptance criteria**: Module compiles. Struct is public from `nexus-storage`.

### Task 0.4 — Index Module: Task & JSX Queries (1 day)

**File**: `crates/nexus-storage/src/index.rs`

Add CRUD functions:

```rust
// Tasks
pub fn insert_tasks(conn: &Connection, file_id: u64, tasks: &[ParsedTask]) -> Result<(), StorageError>
pub fn query_tasks(conn: &Connection, filter: &TaskFilter) -> Result<Vec<TaskRecord>, StorageError>
pub fn toggle_task(conn: &Connection, task_id: u64) -> Result<(), StorageError>

// JSX components
pub fn insert_jsx_components(conn: &Connection, file_id: u64, components: &[ParsedJsxComponent]) -> Result<(), StorageError>
pub fn query_jsx_components(conn: &Connection, file_id: u64) -> Result<Vec<JsxRecord>, StorageError>

// Backlinks (SQL-level, before graph is built)
pub fn query_backlinks_by_path(conn: &Connection, target_path: &str) -> Result<Vec<LinkRecord>, StorageError>
```

Define filter/record structs: `TaskFilter`, `TaskRecord`, `JsxRecord`.

**Acceptance criteria**: All new queries pass unit tests with in-memory SQLite.

### Task 0.5 — Wire New Crate Dependency (0.5 day)

**File**: `Cargo.toml` (root)

```toml
petgraph = "0.7"
lancedb = "0.15"              # Phase 3 — add now so workspace resolves
reqwest = { version = "0.12", features = ["json"] }  # Phase 3 — AI HTTP calls
```

**File**: `crates/nexus-storage/Cargo.toml` — add `petgraph = { workspace = true }`

**Acceptance criteria**: `cargo check --workspace` passes.

---

## Phase 1: Knowledge Graph + Backlinks (5–7 days)

**Goal**: Build a live in-memory knowledge graph that updates as files change, with full backlink query support exposed through CLI and TUI.

### Task 1.1 — Implement `KnowledgeGraph` Core (2 days)

**File**: `crates/nexus-storage/src/graph.rs`

Implement the full graph using `petgraph::stable_graph::StableGraph<NodeData, EdgeData, Directed>`:

```rust
struct NodeData { path: String }
struct EdgeData { link_type: String, alias: Option<String>, is_embed: bool }
```

Key behaviors:
- `rebuild_from_db`: Query all non-deleted files as nodes, all links as edges. Resolve `target_path` to `target_file_id` where possible, mark `is_resolved`.
- `add_note`: Idempotent — check HashMap<String, NodeIndex> before inserting.
- `add_link`: Create directed edge from source node to target node. If target doesn't exist as a file, still create the node (marks it as "phantom" / unresolved).
- `backlinks(path)`: Find all edges pointing to this node, return source paths + metadata.
- `outgoing_links(path)`: Find all edges from this node, return target paths + metadata.
- `unresolved_links()`: Walk all nodes, return those that have no corresponding file in the DB (phantom nodes with no content).
- `neighbors(path, depth)`: BFS traversal up to N hops, for graph visualization.

**Data structures returned**:

```rust
pub struct BacklinkResult { pub source_path: String, pub link_text: String, pub link_type: String }
pub struct OutgoingLink { pub target_path: String, pub link_text: String, pub link_type: String, pub is_resolved: bool }
pub struct UnresolvedLink { pub target_path: String, pub referenced_by: Vec<String> }
pub struct GraphStats { pub node_count: usize, pub edge_count: usize, pub unresolved_count: usize }
```

**Tests**: rebuild from mock DB, add/remove notes, backlinks correct, unresolved detection, idempotent operations.

### Task 1.2 — Integrate Graph into StorageEngine (1 day)

**File**: `crates/nexus-storage/src/lib.rs`

Add `graph: Arc<RwLock<KnowledgeGraph>>` to `StorageEngine`.

- `open_internal`: After reconcile, build graph from DB via `KnowledgeGraph::rebuild_from_db`.
- `write_file`: After indexing, update graph (remove old links for file, add new links from parsed result).
- `delete_file`: Remove node from graph.
- Expose public methods:
  ```rust
  pub fn backlinks(&self, path: &str) -> Result<Vec<BacklinkResult>, StorageError>
  pub fn outgoing_links(&self, path: &str) -> Result<Vec<OutgoingLink>, StorageError>
  pub fn unresolved_links(&self) -> Result<Vec<UnresolvedLink>, StorageError>
  pub fn graph_stats(&self) -> Result<GraphStats, StorageError>
  ```

**Acceptance criteria**: `write_file` with wikilinks → `backlinks()` returns correct results. Delete file → backlinks updated.

### Task 1.3 — Link Resolution Engine (1 day)

**File**: `crates/nexus-storage/src/graph.rs` (add method) + `crates/nexus-storage/src/index.rs`

Implement link resolution logic:
- When a new file is written, check all existing unresolved links. If any `target_path` matches the new file (with or without `.md` extension, case-insensitive), mark them as resolved and update `target_file_id`.
- When a file is deleted, mark all links pointing to it as unresolved.
- Resolution rules (matching Obsidian behavior):
  1. Exact path match: `[[folder/note]]` → `folder/note.md`
  2. Filename-only match: `[[note]]` → first file whose stem is `note`
  3. Case-insensitive fallback

**File**: `crates/nexus-storage/src/reconcile.rs` — after full reconcile, run link resolution pass.

**Acceptance criteria**: Create `note-a.md` with `[[note-b]]`, then create `note-b.md` → link resolves. Delete `note-b.md` → link becomes unresolved.

### Task 1.4 — Graph Event Publishing (0.5 day)

**File**: `crates/nexus-storage/src/lib.rs`

After graph mutations, publish events via an optional `EventBus` reference:
- `GraphRebuilt` after `rebuild_from_db`
- `BacklinksChanged` when a file's backlink set changes

Add `event_bus: Option<Arc<EventBus>>` to `StorageEngine`. The kernel passes it in during construction.

### Task 1.5 — CLI: `links`, `backlinks`, `graph` Commands (1 day)

**File**: `crates/nexus-cli/src/main.rs` + new files `crates/nexus-cli/src/commands/graph.rs`

Add subcommands:

```
nexus content links <path>       — show outgoing links from a file
nexus content backlinks <path>   — show all files linking to this file
nexus graph status               — show graph stats (nodes, edges, unresolved)
nexus graph unresolved           — list all unresolved/broken links
nexus graph neighbors <path> -d 2  — show files within N hops
```

Output format respects `--format` flag (text/json/table).

### Task 1.6 — TUI: Backlinks Panel (1 day)

**Files**: `crates/nexus-tui/src/ui.rs`, `crates/nexus-tui/src/app.rs`

When a file is selected in the viewer:
- Show a "Backlinks" section below the content (or in a toggleable right panel)
- List all files that link to the current file, with the link text as context
- Enter on a backlink navigates to that file
- Show count badge in status bar: `← 3 backlinks`

---

## Phase 2: Enhanced Markdown (5–7 days)

**Goal**: Extend the comrak-based parser to handle block references, callouts, tasks, and optionally MDX/JSX. Upgrade search to support scoped queries.

### Task 2.1 — Block Reference Parsing (1 day)

**File**: `crates/nexus-storage/src/parser.rs`

After comrak parses the AST, post-process to detect:

1. **Block ref anchors**: Lines ending with ` ^blockid` — strip the anchor from content, store `block_ref_id` on the `ParsedBlock`.
2. **Block ref links**: `[[Note#^blockid]]` in wikilink extraction — store the `#^blockid` suffix in `ParsedLink.target_path` and add a `fragment` field.

Update `ParsedBlock`:
```rust
pub struct ParsedBlock {
    // ... existing fields ...
    pub block_ref_id: Option<String>,  // NEW: the ^id if present
}
```

Update `ParsedLink`:
```rust
pub struct ParsedLink {
    // ... existing fields ...
    pub fragment: Option<String>,  // NEW: #heading or #^blockid
}
```

**Acceptance criteria**: `parse_markdown("Hello ^abc123\n")` → block has `block_ref_id = Some("abc123")`. `parse_markdown("See [[note#^ref1]]\n")` → link has fragment.

### Task 2.2 — Callout Parsing (1 day)

**File**: `crates/nexus-storage/src/parser.rs`

Detect Obsidian callout syntax in blockquote nodes:

```markdown
> [!warning] Title here
> Callout body text
```

After comrak parses a `NodeValue::BlockQuote`, inspect the first line for `[!TYPE]` pattern:
- Extract type (`warning`, `info`, `tip`, `note`, `danger`, etc.)
- Extract optional title (text after `]`)
- Store as a new block type: `"callout"`

Update `ParsedBlock`:
```rust
pub struct ParsedBlock {
    // ... existing fields ...
    pub callout_type: Option<String>,  // NEW: "warning", "tip", etc.
}
```

**Acceptance criteria**: `parse_markdown("> [!tip] Pro Tip\n> Use Nexus\n")` → block_type="callout", callout_type=Some("tip"), content="Pro Tip\nUse Nexus".

### Task 2.3 — Task Extraction (1 day)

**File**: `crates/nexus-storage/src/parser.rs`

During list node processing, detect task items:
- `- [ ] uncompleted task` → `ParsedTask { content, completed: false, line_number }`
- `- [x] completed task` → `ParsedTask { content, completed: true, line_number }`

Add to `ParsedFile`:
```rust
pub struct ParsedFile {
    // ... existing fields ...
    pub tasks: Vec<ParsedTask>,  // NEW
}

pub struct ParsedTask {
    pub content: String,
    pub completed: bool,
    pub line_number: u32,
}
```

**File**: `crates/nexus-storage/src/lib.rs` — in `write_file`, after `insert_file`, call `insert_tasks` to persist tasks to the new table.

**Acceptance criteria**: Write a file with 3 tasks (2 incomplete, 1 complete) → `query_tasks` returns all 3 with correct completion state.

### Task 2.4 — MDX/JSX Parsing (Optional — 2 days)

**File**: New file `crates/nexus-storage/src/mdx.rs`

Implement a pre-processing pass before comrak:
1. Scan content for JSX-like tags: `<Component prop="value">...</Component>` or `<Component />`
2. Extract: component name, props (as JSON), line number, self-closing flag
3. Replace JSX blocks with placeholder markers in the markdown body
4. Feed cleaned markdown to comrak
5. Return both `Vec<ParsedJsxComponent>` and the comrak-parsed `ParsedFile`

```rust
pub struct ParsedJsxComponent {
    pub name: String,
    pub props_json: Option<String>,
    pub line_number: u32,
    pub self_closing: bool,
}

pub fn parse_mdx(content: &str) -> Result<(ParsedFile, Vec<ParsedJsxComponent>), StorageError>
```

**File**: `crates/nexus-storage/src/lib.rs` — in `write_file`, detect `.mdx` extension and use `parse_mdx` instead of `parse_markdown`.

**Acceptance criteria**: Parse an MDX file with `<Chart data={items} />` → extracts component with name "Chart" and props JSON.

### Task 2.5 — Search Scoping (1 day)

**File**: `crates/nexus-storage/src/search.rs`

Implement scoped query rewriting before passing to Tantivy:

```
tag:rust         →  Filter results to files containing tag "rust" (post-filter via SQLite)
path:notes/      →  Filter results to files under "notes/" (post-filter on file_path)
prop:status:done →  Filter to files where property "status" = "done" (post-filter via SQLite)
type:heading     →  Filter to blocks of type "heading" (Tantivy field filter)
```

Implementation approach:
1. Parse the query string for scope prefixes
2. Run the plain-text portion through Tantivy as usual
3. Post-filter the results using SQLite queries on the pool connection
4. Merge scores and return

Update `SearchIndex::search` signature to accept a pool reference for post-filtering.

**Acceptance criteria**: `search("tag:rust programming")` returns only blocks in files tagged `#rust`. `search("path:notes/ hello")` returns only matches under `notes/`.

### Task 2.6 — CLI: Task & Callout Commands (0.5 day)

**File**: `crates/nexus-cli/src/commands/content.rs`

```
nexus content tasks [--completed] [--pending]  — list all tasks across the forge
nexus content tasks toggle <task-id>           — toggle a task's completion
```

### Task 2.7 — Parser Integration Tests (0.5 day)

**File**: `crates/nexus-storage/src/parser.rs` (test module)

Add comprehensive tests for all new parsing features working together: a single markdown file with headings, callouts, block refs, tasks, wikilinks with fragments, and inline tags. Verify the `ParsedFile` output is correct and complete.

---

## Phase 3: AI & RAG Pipeline (7–9 days)

**Goal**: Build a native AI subsystem with embedding generation, vector storage, and retrieval-augmented generation, all event-driven on the Nexus kernel.

### Task 3.1 — Create `nexus-ai` Crate (0.5 day)

```
crates/nexus-ai/
├── Cargo.toml     (depends on nexus-kernel, nexus-storage, reqwest, lancedb, serde_json)
├── src/
│   ├── lib.rs
│   ├── provider.rs    — AI provider trait + implementations
│   ├── embedding.rs   — Embedding provider trait + implementations
│   ├── vectorstore.rs — LanceDB vector store
│   ├── rag.rs         — RAG pipeline orchestrator
│   ├── chunker.rs     — Content chunking strategies
│   └── config.rs      — AI configuration
```

Add to root `Cargo.toml`:
```toml
[workspace]
members = [
    # ... existing ...
    "crates/nexus-ai",
]
```

### Task 3.2 — AI Provider Trait & Implementations (2 days)

**File**: `crates/nexus-ai/src/provider.rs`

```rust
#[async_trait]
pub trait AiProvider: Send + Sync {
    async fn chat(&self, messages: &[ChatMessage], system: Option<&str>) -> Result<String, AiError>;
    fn model_name(&self) -> &str;
}

pub struct ChatMessage {
    pub role: Role,    // System, User, Assistant
    pub content: String,
}
```

Implementations (each in a sub-module):
- **AnthropicProvider**: Messages API, claude-sonnet-4-20250514 default, API key from env/config
- **OpenAiProvider**: Chat completions API, gpt-4o default
- **OllamaProvider**: Local API (OpenAI-compatible format), llama3.2 default

All use `reqwest` for HTTP. Configuration via `AiConfig`:
```rust
pub struct AiConfig {
    pub provider: String,       // "anthropic" | "openai" | "ollama"
    pub model: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub max_tokens: u32,
}
```

**Acceptance criteria**: Unit tests with mock HTTP responses. Integration test with Ollama (if available locally).

### Task 3.3 — Embedding Provider Trait & Implementations (1 day)

**File**: `crates/nexus-ai/src/embedding.rs`

```rust
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, AiError>;
    fn dimension(&self) -> usize;
}
```

Implementations:
- **OpenAiEmbedding**: `text-embedding-3-small` (1536 dimensions)
- **OllamaEmbedding**: `nomic-embed-text` (768 dimensions)

Truncation: Limit input to 8000 bytes per text chunk.

### Task 3.4 — Content Chunker (0.5 day)

**File**: `crates/nexus-ai/src/chunker.rs`

Nexus advantage: we already have block-level content from the parser. Use blocks as natural chunk boundaries:

```rust
pub struct Chunk {
    pub file_path: String,
    pub block_id: u64,
    pub block_type: String,
    pub content: String,
    pub heading_context: String,  // nearest parent heading for context
}

pub fn chunks_from_blocks(file_path: &str, blocks: &[BlockRecord], max_chunk_size: usize) -> Vec<Chunk>
```

Strategy:
- Each block is one chunk (headings, paragraphs, code blocks)
- If a block exceeds `max_chunk_size` (default 2000 chars), split on sentence boundaries
- Prepend the nearest heading as context prefix: `"## Section Name\n\n{block content}"`

### Task 3.5 — Vector Store (LanceDB) (1.5 days)

**File**: `crates/nexus-ai/src/vectorstore.rs`

```rust
pub struct VectorStore {
    db: lancedb::Connection,
    table_name: String,
    dimension: usize,
}

impl VectorStore {
    pub async fn open(forge_dir: &Path, dimension: usize) -> Result<Self, AiError>
    pub async fn upsert(&self, chunks: &[ChunkEmbedding]) -> Result<(), AiError>
    pub async fn search(&self, query_embedding: &[f32], limit: usize) -> Result<Vec<ChunkMatch>, AiError>
    pub async fn delete_by_file(&self, file_path: &str) -> Result<(), AiError>
    pub async fn clear(&self) -> Result<(), AiError>
}

pub struct ChunkEmbedding {
    pub file_path: String,
    pub block_id: u64,
    pub content: String,
    pub embedding: Vec<f32>,
}

pub struct ChunkMatch {
    pub file_path: String,
    pub block_id: u64,
    pub content: String,
    pub score: f32,
}
```

Store lives at `.forge/vectors/` as a LanceDB directory.

### Task 3.6 — RAG Engine (1.5 days)

**File**: `crates/nexus-ai/src/rag.rs`

```rust
pub struct RagEngine {
    ai_provider: Box<dyn AiProvider>,
    embedding_provider: Box<dyn EmbeddingProvider>,
    vector_store: VectorStore,
}

impl RagEngine {
    pub async fn index_file(&self, file_path: &str, blocks: &[BlockRecord]) -> Result<usize, AiError>
    pub async fn index_all(&self, storage: &StorageEngine, event_bus: Option<&EventBus>) -> Result<usize, AiError>
    pub async fn query(&self, question: &str, limit: usize) -> Result<RagResponse, AiError>
    pub async fn remove_file(&self, file_path: &str) -> Result<(), AiError>
}

pub struct RagResponse {
    pub answer: String,
    pub sources: Vec<ChunkMatch>,
    pub model: String,
}
```

Query flow:
1. Embed the question
2. Search vector store for top-K similar chunks
3. Build system prompt with retrieved context (including `[[wikilink]]` citations)
4. Send to AI provider
5. Return answer + source references

### Task 3.7 — Event-Driven Embedding Updates (0.5 day)

**File**: `crates/nexus-ai/src/lib.rs`

Subscribe to `FileModified` / `FileCreated` / `FileDeleted` events on the EventBus:
- On file change: re-chunk and re-embed the affected file only (incremental)
- On file delete: remove embeddings for that file
- Publish `EmbeddingStarted/Progress/Completed` events

This makes embeddings stay in sync automatically as the user edits files.

### Task 3.8 — CLI: AI Commands (1 day)

**File**: `crates/nexus-cli/src/commands/ai.rs` (replace stub)

```
nexus ai ask <question>              — RAG query against the forge
nexus ai chat                        — interactive chat session with forge context
nexus ai embed                       — rebuild all embeddings
nexus ai embed --file <path>         — embed a single file
nexus ai status                      — show embedding stats (indexed files, vector count)
nexus ai config                      — show/set AI provider configuration
```

---

## Phase 4: MCP Server (4–5 days)

**Goal**: Implement a Model Context Protocol server so AI agents (Claude, Cursor, etc.) can read, write, search, and query the Nexus forge.

### Task 4.1 — Create `nexus-mcp` Crate (0.5 day)

```
crates/nexus-mcp/
├── Cargo.toml     (depends on nexus-kernel, nexus-storage, nexus-ai, rmcp, tokio, axum)
├── src/
│   ├── lib.rs
│   ├── server.rs   — MCP server setup
│   ├── tools.rs    — Tool implementations
│   └── transport.rs — stdio + HTTP transport
```

Add `rmcp` (Rust MCP SDK) to workspace dependencies.

### Task 4.2 — Core Tools: Note CRUD (1 day)

**File**: `crates/nexus-mcp/src/tools.rs`

Implement MCP tools:
- `nexus_read_note(path)` — read note content + metadata
- `nexus_create_note(path, content, tags?)` — create a new note
- `nexus_update_note(path, content)` — overwrite note content
- `nexus_append_note(path, content)` — append to existing note
- `nexus_delete_note(path)` — soft-delete a note

Each tool returns structured JSON with the note content, frontmatter properties, and metadata.

### Task 4.3 — Search & Graph Tools (1 day)

- `nexus_search(query, limit?)` — full-text search with scoping support
- `nexus_backlinks(path)` — list all backlinks to a note
- `nexus_outgoing_links(path)` — list all links from a note
- `nexus_graph_status()` — graph statistics
- `nexus_unresolved_links()` — broken link report

### Task 4.4 — Taxonomy & Task Tools (0.5 day)

- `nexus_list_tags(prefix?)` — list all tags, optionally filtered
- `nexus_search_by_tag(tag)` — find notes with a specific tag
- `nexus_list_tasks(completed?)` — list tasks across the forge
- `nexus_toggle_task(task_id)` — toggle task completion
- `nexus_search_properties(key, value?)` — property-based search

### Task 4.5 — AI Tools (0.5 day)

- `nexus_ask(question)` — RAG query (delegates to nexus-ai)
- `nexus_daily_note(date?)` — create/read today's daily note

### Task 4.6 — Transport: stdio + HTTP (1 day)

**File**: `crates/nexus-mcp/src/transport.rs`

- **stdio**: Default for terminal usage — reads JSON-RPC from stdin, writes to stdout
- **HTTP**: Axum server on configurable port (default 3001), localhost-only by default

**File**: `crates/nexus-cli/src/commands/mcp.rs` (replace stub)

```
nexus mcp                          — start MCP server (stdio mode)
nexus mcp --http                   — start MCP server (HTTP mode)
nexus mcp --http --port 3001       — custom port
nexus mcp --http --host 0.0.0.0   — expose to network
```

### Task 4.7 — Integration Tests (0.5 day)

Test the full MCP flow: start server → send JSON-RPC tool call → verify response. Test both stdio and HTTP transports.

---

## Phase 5: Polish & Secondary Features (4–6 days)

### Task 5.1 — Daily Notes (0.5 day)

**File**: `crates/nexus-cli/src/commands/content.rs`

```
nexus content daily [--date YYYY-MM-DD]
```

- Creates `notes/daily/YYYY-MM-DD.md` with frontmatter template
- Opens existing note if it already exists
- Default template:
  ```markdown
  ---
  date: 2026-04-12
  tags: [daily]
  ---
  # April 12, 2026
  
  ## Tasks
  
  ## Notes
  ```

### Task 5.2 — HTML Export (1 day)

**File**: New file `crates/nexus-storage/src/export.rs`

```rust
pub fn export_to_html(parsed: &ParsedFile, title: &str) -> String
```

- Use comrak's HTML rendering as base
- Add CSS styling for callouts, code blocks, tasks
- Sanitize with ammonia
- Preserve `[[wikilinks]]` as `<a>` tags with `data-wikilink` attribute
- Support `--format html` in `nexus content read`

### Task 5.3 — Enhanced TUI (2 days)

**Files**: `crates/nexus-tui/src/` (multiple files)

Improvements:
- **Fuzzy search**: Add `fuzzy-matcher` crate for SkimMatcherV2-style filtering in file tree
- **Backlinks panel**: Show backlinks for selected file (from Phase 1)
- **Task list view**: Toggle view that shows all tasks across the forge
- **Search results**: Show Tantivy results with block excerpts and scores
- **Graph mini-view**: Show immediate neighbors of selected file (ASCII-art or simple list)
- **Status bar**: File count, link count, search index status, AI embedding status

### Task 5.4 — Canvas Support (Optional — 1.5 days)

**File**: New file `crates/nexus-storage/src/canvas.rs`

Implement JSON Canvas spec:
```rust
pub struct Canvas {
    pub nodes: Vec<CanvasNode>,
    pub edges: Vec<CanvasEdge>,
}

pub fn load_canvas(path: &Path) -> Result<Canvas, StorageError>
pub fn save_canvas(path: &Path, canvas: &Canvas) -> Result<(), StorageError>
```

Node types: text, file (reference to a note), link (URL), group.

### Task 5.5 — Reconcile on Watcher Events (0.5 day)

**File**: `crates/nexus-storage/src/lib.rs`

Currently the watcher emits events but doesn't auto-reconcile. Wire it up:
- On `FileModified` event from watcher → re-parse and re-index the file, update graph, re-embed
- On `FileDeleted` → remove from index, graph, and vector store
- This makes the system fully reactive to external edits (e.g., editing in VS Code)

---

## Dependency Summary

New workspace dependencies to add (Phase 0):

```toml
# Knowledge graph
petgraph = "0.7"

# AI / RAG
reqwest = { version = "0.12", features = ["json"] }
lancedb = "0.15"
async-trait = "0.1"

# MCP server
rmcp = "0.1"
axum = "0.8"

# Enhanced TUI
fuzzy-matcher = "0.3"

# HTML export
ammonia = "4"
```

---

## Architecture: How It All Fits Together

```
                    ┌─────────────────────────────────────┐
                    │            nexus-kernel              │
                    │  EventBus (tokio broadcast)          │
                    │  PluginLifecycle · CapabilitySet     │
                    └──────────────┬──────────────────────┘
                                   │ events
              ┌────────────────────┼────────────────────┐
              ▼                    ▼                     ▼
    ┌─────────────────┐  ┌────────────────┐   ┌──────────────┐
    │  nexus-storage   │  │   nexus-ai     │   │ nexus-plugins│
    │                  │  │                │   │              │
    │  StorageEngine   │  │  RagEngine     │   │ WasmSandbox  │
    │  ├─ SQLite+r2d2  │  │  ├─ Providers  │   │ ├─ Manifest  │
    │  ├─ Tantivy      │  │  ├─ Embeddings │   │ ├─ Settings  │
    │  ├─ KnowledgeGraph│ │  ├─ VectorStore│   │ ├─ HotReload │
    │  ├─ Parser       │  │  └─ Chunker    │   │ └─ HostFns   │
    │  ├─ Watcher      │  └────────────────┘   └──────────────┘
    │  └─ Export       │           │
    └─────────────────┘            │
              │                    │
              ▼                    ▼
    ┌─────────────────────────────────────┐
    │            nexus-mcp                │
    │  MCP Server (stdio + HTTP)          │
    │  15+ tools: CRUD, search, graph,    │
    │  tasks, tags, AI query              │
    └─────────────────────────────────────┘
              │
    ┌─────────┴──────────┐
    ▼                    ▼
┌──────────┐      ┌──────────┐
│ nexus-cli│      │ nexus-tui│
│ Headless │      │ Terminal │
│ Commands │      │ UI       │
└──────────┘      └──────────┘
```

**Event Flow Example** (user saves a file in VS Code):

1. `notify` detects filesystem change → `Watcher` emits `StorageEvent::FileModified`
2. `StorageEngine` re-parses markdown → updates SQLite index → updates `KnowledgeGraph`
3. `EventBus` publishes `NexusEvent::FileModified`
4. `nexus-ai` subscriber receives event → re-chunks → re-embeds → updates `VectorStore`
5. `EventBus` publishes `NexusEvent::BacklinksChanged` (if links changed)
6. Any running TUI refreshes its file tree and backlinks panel
7. Any subscribed WASM plugin receives the event through its event subscriber registration

---

## Risk Register

| Risk | Impact | Mitigation |
|---|---|---|
| LanceDB adds heavy native dependency | Build complexity | Fallback: JSON-backed vector store (like Rustsidian) for simple cases |
| `rmcp` crate may be immature | MCP compatibility | Fallback: Hand-roll JSON-RPC over stdio/HTTP with axum |
| petgraph memory usage on large vaults | OOM on 10K+ notes | Lazy loading: only build graph for accessed subgraph |
| Tantivy + LanceDB dual index | Disk space, startup time | Lazy embedding: only embed on first AI query, not on every file save |
| comrak callout parsing is fragile | Missed callouts | Comprehensive test suite, pre-comrak regex pass as fallback |

---

## Milestone Checkpoints

After each phase, verify:

1. **`cargo check --workspace`** passes
2. **`cargo test --workspace`** passes (all existing + new tests)
3. **`cargo clippy --workspace`** has no warnings
4. **Manual smoke test**: init a forge, write files, run the new commands
5. **No regressions**: existing CLI/TUI/plugin functionality still works
