# Nexus PRD 03 — Storage Engine (M1) Design Spec

**Version:** 1.0
**Date:** 2026-04-12
**Status:** Approved (brainstorming session output)
**Scope:** M1-scoped design for `nexus-storage` — forge directory layout, atomic writes, SQLite indexing, markdown parsing, file watching, reconciliation, and Tantivy full-text search. CRDT sync and caching are deferred.

**Parent docs:**
- [`PRDs/03-storage-engine.md`](../../../PRDs/03-storage-engine.md) — full PRD (this spec implements the M1 slice)
- [`2026-04-11-nexus-m1-foundation-spec.md`](2026-04-11-nexus-m1-foundation-spec.md) — M1 spec §7 (Storage Architecture)
- [`2026-04-11-nexus-roadmap-design.md`](2026-04-11-nexus-roadmap-design.md) — roadmap §3 (PRD 03 is uncut)

---

## 1. Architecture Overview

Single `nexus-storage` workspace member crate, following the one-crate-per-PRD pattern established by PRD 01 and 02. All subsystems are internal modules behind a `StorageEngine` facade struct.

Dependencies:
- `nexus-kernel` — for `NexusEvent` types (watcher events bridge to the kernel event bus at the CLI/kernel layer, not inside storage)
- `nexus-types` — shared types; `SyncProvider` trait defined here but not implemented in M1

The crate is **synchronous**. File I/O and SQLite are naturally blocking, `rayon` handles parallelism for hashing/parsing, and the watcher uses `std::sync::mpsc`. No `tokio` runtime required.

---

## 2. Crate Structure

```
crates/nexus-storage/
├── Cargo.toml
└── src/
    ├── lib.rs              # public re-exports, StorageEngine facade
    ├── error.rs            # StorageError enum
    ├── forge.rs            # forge directory init, layout, temp file cleanup
    ├── atomic.rs           # atomic write (temp → fsync → rename)
    ├── schema.rs           # SQLite table definitions, migration runner
    ├── index.rs            # file/block/link/tag/property insert and query
    ├── parser.rs           # markdown/MDX parsing, block/link/tag extraction
    ├── search.rs           # Tantivy index: build, query
    ├── watcher.rs          # notify + debouncer, rename detection, git batch mode
    └── reconcile.rs        # full directory scan, hash-based delta, index sync
```

---

## 3. Dependencies

New workspace dependencies (added to root `Cargo.toml`):

| Crate | Version | Purpose |
|---|---|---|
| `rusqlite` | 0.31+ | SQLite (with `bundled` + `backup` features) |
| `r2d2` | 0.8 | Connection pool |
| `r2d2_sqlite` | 0.24 | SQLite adapter for r2d2 |
| `tantivy` | 0.22+ | Full-text search engine |
| `comrak` | 0.28+ | GFM markdown parser |
| `notify` | 7.0+ | File system event detection |
| `notify-debouncer-mini` | 0.5+ | Event debouncing (300ms window) |
| `rayon` | 1.10 | Parallel hashing and parsing |
| `sha2` | 0.10 | SHA-256 content hashing |
| `serde_yaml` | 0.9+ | YAML frontmatter parsing |

`nexus-storage` does **not** depend on `nexus-security`. Path validation (via `ForgePathValidator`) is called by the CLI/kernel layer before reaching storage.

---

## 4. StorageError Enum

```rust
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("file not found: {0}")]
    FileNotFound(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("corrupt file {path}: {reason}")]
    CorruptFile { path: String, reason: String },

    #[error("index inconsistency: {details}")]
    IndexInconsistency { details: String },

    #[error("write failed for {path}: {reason}")]
    WriteFailed { path: String, reason: String },

    #[error("parse error in {file}: {error}")]
    ParseError { file: String, error: String },

    #[error("forge locked by another process: {0}")]
    LockHeld(String),

    #[error("invalid configuration: {0}")]
    ConfigInvalid(String),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("search error: {0}")]
    Search(#[from] tantivy::TantivyError),

    #[error("watcher error: {0}")]
    Watcher(#[from] notify::Error),
}
```

Follows PRD 03 §12 with library-specific wrappers added and sync-related variants removed.

---

## 5. Forge Directory & Atomic Writes

### 5.1 Forge init

`Forge::init(root: &Path)` creates:

```
forge-root/
├── notes/                  # user markdown/MDX content
├── attachments/            # binary files, images
└── .forge/
    ├── index.db            # SQLite index (created by schema module)
    ├── config.toml         # forge-level config
    ├── lock                # exclusive write lock
    └── temp/               # temp files during atomic writes
```

`canvases/` and `databases/` are **not** created until PRD 06 (M2) and PRD 10 (M3) respectively.

`Forge::init` is idempotent — calling it on an existing forge returns `Ok` without overwriting config or re-creating the schema.

### 5.2 Forge lock

File lock on `.forge/lock` using `flock()`. Acquired on `StorageEngine::open()`, released on drop. Prevents concurrent Nexus processes from writing to the same forge. Read-only operations (queries, search) do not require the lock.

### 5.3 Atomic writes

Per PRD 03 §4, temp-fsync-rename pattern:

1. Write content to `.forge/temp/{uuid}.tmp`
2. `fsync()` the file descriptor
3. `rename()` temp file to target path (atomic on Unix)
4. On failure: delete temp file, retry up to 3 times with exponential backoff (100ms, 400ms, 1600ms)
5. Update SQLite index within the same logical operation

**Error recovery:**
- Disk full: write fails before fsync, original untouched, temp deleted
- Process crash: stale `.forge/temp/*.tmp` files cleaned on next `StorageEngine::open()` (delete any temp files older than 1 hour)

**Platform scope:** Unix path only (`rename(2)`, `flock()`, `fsync()`). Windows support (`ReplaceFileW`, `FlushFileBuffers`) deferred to M6.

---

## 6. SQLite Index Schema

### 6.1 Configuration

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA cache_size = -16000;    -- 16MB
PRAGMA foreign_keys = ON;
```

Connection pool: `r2d2` with default size 4 (configurable in `nexus.toml`). Writers serialized behind a `Mutex<rusqlite::Connection>` separate from the read pool.

### 6.2 Tables

**`files`** — one row per tracked file:

```sql
CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    file_type TEXT NOT NULL,          -- 'note', 'attachment'
    content_hash TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    modified_at INTEGER NOT NULL,
    is_deleted BOOLEAN DEFAULT 0
);
CREATE INDEX idx_files_path_type ON files(path, file_type);
CREATE INDEX idx_files_hash ON files(content_hash);
```

**`blocks`** — parsed content blocks:

```sql
CREATE TABLE blocks (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL,
    block_type TEXT NOT NULL,         -- 'heading', 'paragraph', 'codeblock', 'list', 'table'
    level INTEGER,                   -- heading level 1-6, NULL for non-headings
    content TEXT NOT NULL,
    raw_markdown TEXT,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    parent_block_id INTEGER,
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
    FOREIGN KEY(parent_block_id) REFERENCES blocks(id) ON DELETE CASCADE
);
CREATE INDEX idx_blocks_file_id ON blocks(file_id);
CREATE INDEX idx_blocks_type ON blocks(block_type);
```

**`links`** — wikilinks, markdown links, embeds:

```sql
CREATE TABLE links (
    id INTEGER PRIMARY KEY,
    source_file_id INTEGER NOT NULL,
    source_block_id INTEGER,
    target_path TEXT,
    target_file_id INTEGER,
    link_text TEXT NOT NULL,
    link_type TEXT NOT NULL,          -- 'wikilink', 'markdown', 'embed'
    is_resolved BOOLEAN DEFAULT 0,
    FOREIGN KEY(source_file_id) REFERENCES files(id) ON DELETE CASCADE,
    FOREIGN KEY(target_file_id) REFERENCES files(id) ON DELETE SET NULL
);
CREATE INDEX idx_links_source ON links(source_file_id);
CREATE INDEX idx_links_target ON links(target_file_id);
```

**`tags`** — from frontmatter, inline `#tag`, or inferred:

```sql
CREATE TABLE tags (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    file_id INTEGER NOT NULL,
    block_id INTEGER,
    source TEXT NOT NULL,             -- 'frontmatter', 'inline'
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
    FOREIGN KEY(block_id) REFERENCES blocks(id) ON DELETE CASCADE
);
CREATE INDEX idx_tags_name ON tags(name);
CREATE INDEX idx_tags_file ON tags(file_id);
```

**`properties`** — frontmatter key-value pairs:

```sql
CREATE TABLE properties (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,              -- JSON-serialized
    property_type TEXT,               -- 'string', 'number', 'date', 'list', 'object'
    FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
    UNIQUE(file_id, key)
);
```

**`fts_blocks`** — FTS5 virtual table for SQLite-native full-text search:

```sql
CREATE VIRTUAL TABLE fts_blocks USING fts5(
    file_path UNINDEXED,
    block_content,
    block_type UNINDEXED,
    content=blocks,
    content_rowid=id
);
```

### 6.3 Migration system

- `_schema_version` table: `(version INTEGER PRIMARY KEY, applied_at INTEGER)`
- Migrations embedded in binary via `include_str!` (no on-disk SQL files)
- Migration 001 creates all tables, indexes, and FTS5 virtual table
- Applied sequentially on `StorageEngine::open()` inside a transaction
- On failure, transaction rolls back, `open()` returns `StorageError::Database`
- Rollback support deferred from M1

---

## 7. Markdown Parser Pipeline

### 7.1 Parser crate

`comrak` 0.28+ with GFM extensions: tables, strikethrough, autolink, task lists. Per PRD 03 §6 and M1 spec §7.2.

### 7.2 Output type

```rust
pub struct ParsedFile {
    pub content_hash: String,
    pub frontmatter: Vec<Property>,
    pub blocks: Vec<Block>,
    pub links: Vec<Link>,
    pub tags: Vec<Tag>,
}
```

### 7.3 Pipeline steps

1. **Extract YAML frontmatter** — regex `^---\n(.*?)\n---` (dotall), parse via `serde_yaml`, produce `Vec<Property>`
2. **Parse markdown body** — `comrak::parse_document` with `ComrakOptions { extension: { strikethrough: true, table: true, autolink: true, tasklist: true }, parse: { smart: false } }`
3. **Flatten AST to blocks** — walk comrak AST depth-first, emit `Block` structs: heading (with level), paragraph, codeblock (with language from info string), list, table. Track line ranges and parent references.
4. **Extract links** — scan inline text nodes for:
   - Wikilinks: regex `\[\[([^\]]+)\]\]`, split on `|` for display text
   - Standard markdown links: from comrak AST `Link` nodes
   - Embeds: `![[path]]` syntax, classified as `link_type = "embed"`
5. **Extract tags** — `#tag` syntax from inline text (regex `(?:^|\s)#([a-zA-Z0-9_/-]+)`), plus tags from frontmatter `tags:` field

### 7.4 Wikilink resolution

Resolution is performed in `index.rs` during insert, not in the parser, because it requires the `files` table:

1. **Exact path match** (case-insensitive on case-insensitive filesystems)
2. **Basename match** if unique (e.g., `[[nexus]]` resolves to `projects/nexus.md` if only one `nexus.md` exists)
3. Otherwise, store as unresolved with `is_resolved = false`

Fuzzy matching (Levenshtein) deferred from M1.

---

## 8. File Watcher & Reconciliation

### 8.1 Watcher

Wraps `notify` 7.0+ and `notify-debouncer-mini` with a 300ms debounce window. Watches `notes/` and `attachments/` under the forge root.

**Ignore patterns:** `.git/`, `.forge/temp/`, `*~`, `.DS_Store`, `.swp`, `node_modules/`

**Event flow:**

```
notify (raw OS events)
   ↓
notify-debouncer-mini (300ms window)
   ↓
nexus-storage::Watcher
   ├─ ignore pattern filtering
   ├─ rename detection (hash match within debounce window)
   ↓
std::sync::mpsc::Sender<StorageEvent>
```

**Storage events emitted:**

```rust
pub enum StorageEvent {
    FileCreated { path: String, content_hash: String },
    FileModified { path: String, content_hash: String },
    FileDeleted { path: String },
    FileRenamed { from: String, to: String, content_hash: String },
}
```

**Rename detection:** When a `Deleted` event is followed within the debounce window by a `Created` event whose content hash matches the deleted file's hash in the index, emit `FileRenamed` instead of separate delete + create.

**Git batch mode:** Detect `.git/index.lock` presence, suppress individual events, trigger full reconcile on lock removal.

### 8.2 Reconciliation

`reconcile.rs` implements the full-scan delta algorithm (PRD 03 §3):

1. Walk `notes/` and `attachments/` directories (skip ignore patterns)
2. Compute content hashes in parallel via `rayon`
3. Compare each file against the index:
   - Path + hash match → no-op
   - Path match, hash differs → re-parse, update index
   - Hash match, path differs → rename (update path, preserve metadata)
   - New path + hash → insert
4. For index entries with no corresponding file on disk → set `is_deleted = 1`
5. Return `ReconcileDelta { created: usize, modified: usize, renamed: usize, deleted: usize }`

Called on: `StorageEngine::open()` (startup), git batch mode completion, and explicit `rebuild_index()`.

### 8.3 Integration with kernel

The watcher does **not** publish directly to the kernel's `EventBus`. `StorageEngine` owns the watcher and exposes `watch_changes() -> mpsc::Receiver<StorageEvent>`. The CLI or kernel layer bridges `StorageEvent` values to `NexusEvent` variants on the event bus. This keeps `nexus-storage` usable standalone (indexing, querying) without a running kernel.

---

## 9. Tantivy Full-Text Search

### 9.1 Index location

`.forge/search/` directory under the forge root.

### 9.2 Schema

| Field | Type | Stored | Indexed | Purpose |
|---|---|---|---|---|
| `path` | text | yes | no | File path for result display |
| `block_id` | u64 | yes | no | Back-reference to SQLite block |
| `block_type` | text | yes | no | Block classification |
| `content` | text | no | yes | Block text, tokenized + stemmed |
| `mtime` | date | yes | yes | File modification time |

### 9.3 Tokenizer

Tantivy's default English tokenizer: lowercase normalization + English stemming filter. Per M1 spec §7.3.

### 9.4 Query

Uses Tantivy's `QueryParser` with lucene-like syntax:
- Phrase search: `"exact phrase"`
- Boolean: `deep AND learning`, `neural OR cognitive`, `NOT deprecated`
- Field-scoped: `path:notes/ai/*`

BM25 scoring (Tantivy default). Recency boost deferred from M1.

### 9.5 Build strategy

**Full rebuild only** in M1. `rebuild_search_index()` iterates all blocks from SQLite, indexes each into Tantivy, and commits. Called after SQLite index rebuild completes.

Incremental delta updates deferred to v0.2.

### 9.6 FTS5 complementarity

FTS5 is available in the SQLite schema for internal use (e.g., exact-match lookups during link resolution). Tantivy is the primary search interface exposed through `SearchProvider`. Both index the same block content.

---

## 10. Public API Surface

### 10.1 StorageEngine

```rust
pub struct StorageEngine { /* private */ }

impl StorageEngine {
    /// Initialize a new forge at root. Creates directories, schema, returns engine.
    pub fn init(root: &Path) -> Result<Self, StorageError>;

    /// Open an existing forge. Acquires lock, runs migrations, reconciles, starts watcher.
    pub fn open(root: &Path, config: &StorageConfig) -> Result<Self, StorageError>;
}
```

### 10.2 File operations (StorageProvider)

```rust
impl StorageEngine {
    pub fn write_file(&self, path: &str, content: &[u8]) -> Result<FileMetadata, StorageError>;
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, StorageError>;
    pub fn delete_file(&self, path: &str) -> Result<(), StorageError>;
    pub fn list_files(&self, prefix: &str) -> Result<Vec<FileMetadata>, StorageError>;
    pub fn file_exists(&self, path: &str) -> Result<bool, StorageError>;
}
```

### 10.3 Index operations (IndexProvider)

```rust
impl StorageEngine {
    pub fn query_files(&self, filter: &FileFilter) -> Result<Vec<FileRecord>, StorageError>;
    pub fn query_blocks(&self, file_id: u64) -> Result<Vec<Block>, StorageError>;
    pub fn query_links(&self, file_id: u64) -> Result<Vec<Link>, StorageError>;
    pub fn query_backlinks(&self, file_id: u64) -> Result<Vec<Link>, StorageError>;
    pub fn query_tags(&self, name: &str) -> Result<Vec<TagResult>, StorageError>;
    pub fn rebuild_index(&self) -> Result<RebuildStats, StorageError>;
}
```

### 10.4 Search operations (SearchProvider)

```rust
impl StorageEngine {
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, StorageError>;
    pub fn rebuild_search_index(&self) -> Result<(), StorageError>;
}
```

### 10.5 Watcher

```rust
impl StorageEngine {
    pub fn watch_changes(&self) -> mpsc::Receiver<StorageEvent>;
}
```

### 10.6 Design decisions

- **Concrete struct, not trait objects.** Exactly one implementation of each provider in M1. Methods live directly on `StorageEngine`. Traits are introduced when PRD 04 plugins need to abstract over storage. `SyncProvider` is defined in `nexus-types` with no implementation.
- **Synchronous API.** Matches the kernel (PRD 01). Blocking I/O + `rayon` parallelism. `std::sync::mpsc` for watcher channel.
- **Paths are relative to forge root.** All `path` parameters are relative (e.g., `notes/project.md`), not absolute. `StorageEngine` resolves them against the forge root internally.

---

## 11. Key Data Types

```rust
pub struct FileMetadata {
    pub path: String,
    pub size_bytes: u64,
    pub modified_at: i64,
    pub content_hash: String,
}

pub struct FileRecord {
    pub id: u64,
    pub path: String,
    pub file_type: String,
    pub content_hash: String,
    pub size_bytes: u64,
    pub created_at: i64,
    pub modified_at: i64,
    pub is_deleted: bool,
}

pub struct Block {
    pub id: u64,
    pub file_id: u64,
    pub block_type: String,
    pub level: Option<i32>,
    pub content: String,
    pub raw_markdown: Option<String>,
    pub start_line: u32,
    pub end_line: u32,
    pub parent_block_id: Option<u64>,
}

pub struct Link {
    pub id: u64,
    pub source_file_id: u64,
    pub source_block_id: Option<u64>,
    pub target_path: Option<String>,
    pub target_file_id: Option<u64>,
    pub link_text: String,
    pub link_type: String,
    pub is_resolved: bool,
}

pub struct Tag {
    pub id: u64,
    pub name: String,
    pub file_id: u64,
    pub block_id: Option<u64>,
    pub source: String,
}

pub struct Property {
    pub key: String,
    pub value: String,
    pub property_type: Option<String>,
}

pub struct SearchResult {
    pub file_path: String,
    pub block_id: u64,
    pub block_type: String,
    pub excerpt: String,
    pub score: f32,
}

pub struct RebuildStats {
    pub files_processed: usize,
    pub blocks_indexed: usize,
    pub links_found: usize,
    pub tags_found: usize,
    pub duration_ms: u64,
}

pub struct ReconcileDelta {
    pub created: usize,
    pub modified: usize,
    pub renamed: usize,
    pub deleted: usize,
}

pub struct FileFilter {
    pub prefix: Option<String>,      // path prefix (e.g., "notes/")
    pub file_type: Option<String>,   // "note" or "attachment"
    pub include_deleted: bool,       // default: false
}

pub struct TagResult {
    pub tag: Tag,
    pub file_path: String,
}

pub struct StorageConfig {
    pub pool_size: u32,              // default: 4
    pub debounce_ms: u64,            // default: 300
    pub rayon_threads: usize,        // default: 0 (rayon auto-detect)
}
```

---

## 12. Deferred from M1

| Item | PRD Section | Rationale | Revisit |
|---|---|---|---|
| CRDT sync (entire subsystem) | §8 | No multi-device use case. Single user, single machine. | M2+ if need arises |
| `SyncProvider` implementation | §11 | Trait defined in `nexus-types`, no concrete impl until sync lands | With CRDT sync |
| `canvases/` directory | §1 | Canvas file format is M2 (PRD 06) | PRD 06 |
| `databases/` directory | §1 | Database engine is M3 (PRD 10) | PRD 10 |
| In-memory caches (block, link, search) | §9 | No usage data to tune eviction/TTL. Premature for headless CLI. | v0.2 after dogfooding |
| Recency boost in search ranking | §7 | BM25 alone is sufficient. Boost curve needs real queries. | v0.2 |
| Incremental Tantivy updates | §7 | Full rebuild is simpler and correct. Optimize when rebuild time hurts. | v0.2 |
| Concurrent writer optimization | §10 | Mutex-serialized writes fine for single-user CLI. | v0.2 if friction observed |
| Fuzzy wikilink matching (Levenshtein) | §6 | Exact + basename match covers personal forge. | v0.2 if needed |
| Migration rollback | §14 | Dev-only feature, no value in M1. | v0.2 |
| Windows atomic write path | §4 | Target is Linux/WSL. Cross-platform deferred indefinitely. | M6 |
