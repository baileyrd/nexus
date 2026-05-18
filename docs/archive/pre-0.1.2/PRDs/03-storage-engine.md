# Storage Engine PRD — Nexus v1.0

**Version:** 1.0  
**Date:** April 2026  
**Status:** ✅ Shipped — Complete (see [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md), 2026-04-18)  
**Owner:** Storage & Indexing Team

---

## Executive Summary

The Storage Engine implements the "file-as-truth" principle: markdown/MDX files on disk are the authoritative source. An SQLite index provides fast query access and is fully rebuildable. The engine supports atomic writes, cross-platform file watching, full-text search via Tantivy, and optional CRDT-based peer-to-peer sync. This document specifies the architecture, implementation details, and UX flows required for production-ready storage.

---

## 1. Forge Directory Structure

### Root Layout
```
forge-root/
├── notes/                          # User-created markdown/MDX files
│   ├── README.md
│   ├── projects/
│   │   ├── nexus.md
│   │   └── ...
│   └── ...
├── attachments/                    # Binary files, images, PDFs
│   ├── images/
│   ├── documents/
│   └── ...
├── canvases/                       # Canvas data (JSON+metadata)
│   ├── canvas-id-1.canvas
│   └── ...
├── databases/                      # User-created SQLite databases
│   └── custom.db
└── .forge/                         # Configuration & runtime data
    ├── index.db                    # SQLite index (single file, WAL mode)
    ├── index-wal                   # SQLite WAL file
    ├── index-shm                   # SQLite shared memory
    ├── search/                     # Tantivy full-text index
    │   ├── meta.json
    │   ├── segments_*/
    │   └── ...
    ├── config.toml                 # Forge-level config (sync, watch settings)
    ├── state.json                  # Sync state vector, peer list, last reconcile timestamp
    ├── lock                        # Lock file (exclusive write access)
    ├── temp/                       # Temporary files during writes
    └── logs/                       # Operation logs (reconcile, sync, errors)
```

### File Naming Conventions
- **Notes:** `*.md` or `*.mdx` (UTF-8, Unix line endings)
- **Attachments:** Preserve original extensions; store hash in metadata for deduplication
- **Canvases:** UUID-based names `{uuid}.canvas`, internal JSON structure
- **Temp files:** `.forge/temp/{uuid}-{timestamp}.tmp` during atomic writes

---

## 2. SQLite Index Schema

### Core Tables

#### `files`
```sql
CREATE TABLE files (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,        -- Relative to forge root
  file_type TEXT NOT NULL,          -- 'note', 'attachment', 'canvas'
  content_hash TEXT NOT NULL,       -- SHA256 of file content
  size_bytes INTEGER NOT NULL,
  created_at INTEGER NOT NULL,      -- Unix timestamp
  modified_at INTEGER NOT NULL,
  accessed_at INTEGER,
  is_deleted BOOLEAN DEFAULT 0,     -- Soft delete for sync
  content_hash_index TEXT           -- For fast hash lookups
);
CREATE INDEX idx_files_path_type ON files(path, file_type);
CREATE INDEX idx_files_hash ON files(content_hash);
```

#### `blocks`
```sql
CREATE TABLE blocks (
  id INTEGER PRIMARY KEY,
  file_id INTEGER NOT NULL,
  block_type TEXT NOT NULL,         -- 'heading', 'paragraph', 'codeblock', 'list'
  level INTEGER,                    -- Heading level (1-6)
  content TEXT NOT NULL,            -- Plaintext of block
  raw_markdown TEXT,                -- Original markdown
  start_line INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  parent_block_id INTEGER,          -- For nested structures
  FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
  FOREIGN KEY(parent_block_id) REFERENCES blocks(id) ON DELETE CASCADE
);
CREATE INDEX idx_blocks_file_id ON blocks(file_id);
CREATE INDEX idx_blocks_type ON blocks(block_type);
```

#### `links`
```sql
CREATE TABLE links (
  id INTEGER PRIMARY KEY,
  source_file_id INTEGER NOT NULL,
  source_block_id INTEGER,
  target_path TEXT,                 -- Path if resolved
  target_file_id INTEGER,           -- Backref if resolved
  link_text TEXT NOT NULL,          -- User's link syntax
  link_type TEXT NOT NULL,          -- 'wikilink', 'markdown', 'embed', 'backref'
  is_resolved BOOLEAN DEFAULT 0,
  is_valid BOOLEAN DEFAULT 1,
  FOREIGN KEY(source_file_id) REFERENCES files(id) ON DELETE CASCADE,
  FOREIGN KEY(target_file_id) REFERENCES files(id) ON DELETE SET NULL
);
CREATE INDEX idx_links_source ON links(source_file_id);
CREATE INDEX idx_links_target ON links(target_file_id);
CREATE INDEX idx_links_unresolved ON links(is_resolved, link_text);
```

#### `tags`
```sql
CREATE TABLE tags (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  file_id INTEGER,
  block_id INTEGER,
  source TEXT NOT NULL,             -- 'frontmatter', 'inline', 'inferred'
  FOREIGN KEY(file_id) REFERENCES files(id) ON DELETE CASCADE,
  FOREIGN KEY(block_id) REFERENCES blocks(id) ON DELETE CASCADE
);
CREATE INDEX idx_tags_name ON tags(name);
CREATE INDEX idx_tags_file ON tags(file_id);
```

#### `properties`
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

#### `fts_blocks` (FTS5 Virtual Table)
```sql
CREATE VIRTUAL TABLE fts_blocks USING fts5(
  file_path UNINDEXED,
  block_content,
  block_type UNINDEXED,
  content=blocks,
  content_rowid=id
);
```

### Migration System
- **Version tracking:** `_schema_version` table with `version INTEGER, applied_at INTEGER`
- **Migrations stored:** `.forge/migrations/` directory, numbered `001-initial.sql`, `002-add-property-type.sql`, etc.
- **Migration runner:** Nexus CLI applies pending migrations on startup; rollback supported for dev environments only
- **Backward compatibility:** Index rebuild guarantees N-1 version compatibility; users may force rebuild

---

## 3. File Watcher Architecture

### Core Components

#### Event Detection
- **Crate:** `notify` v6+ with debouncing via `notify-debounce-rs`
- **Debounce strategy:** 300ms window for filesystem events; coalesce creates, modifies, deletes
- **Watched paths:** `notes/`, `attachments/`, `canvases/`, `.forge/` (selective)

#### Rename Detection Algorithm
```
On file system event:
  1. Compute content hash of changed file
  2. Check if hash exists in files table with different path
  3. If match found:
     - Logical rename detected
     - Update files.path, preserve all metadata
     - Update blocks.file_id (via join)
     - Emit reconcile event
  4. Else:
     - Treat as create or modify
```

#### Git Operation Handling
- **Problem:** `git checkout` fires hundreds of events in milliseconds
- **Solution:** 
  - Detect `.git/index.lock` presence → enter "batch mode"
  - Suppress reconcile until lock clears
  - On lock removal, full directory scan with hash-based matching
  - Parallel hash computation (rayon, 4 workers)
  - Reconcile generates minimal delta (moved files, not recreated)

#### Reconciliation Algorithm
```
Reconcile(filesystem_state, index_state):
  1. Scan all files in forge-root (breadth-first, skip .git, node_modules, .forge/temp)
  2. For each file on disk:
     a. Compute content hash
     b. If path in index and hash matches → no-op
     c. If path in index and hash differs → update modified_at, rehash blocks
     d. If hash in index but path differs → rename detected (see above)
     e. If new path, new hash → insert new file entry
  3. For each path in index but not on disk:
     a. Set is_deleted=1 (soft delete for sync)
     b. Retain metadata for conflict resolution
  4. Update index.modified_at = now()
  5. Emit events: file_created, file_modified, file_moved, file_deleted
  6. Return delta summary (counts by operation type)
```

#### Performance Targets
- **Typical event latency:** <100ms from filesystem event to index update
- **Large batch (1000 files):** <5s reconciliation time
- **Hash computation:** ~100MB/s (adjustable worker count)

---

## 4. Atomic Write Implementation

### Algorithm
```
AtomicWrite(file_path, content):
  1. Generate temp_path = .forge/temp/{uuid}-{timestamp}.tmp
  2. Acquire exclusive lock on file_path (or global lock on Windows)
  3. Open temp file (create or truncate)
  4. Write content in 64KB chunks
  5. Call fsync() on file descriptor (or FlushFileBuffers on Windows)
  6. Close file descriptor
  7. Attempt atomic rename (temp_path → file_path)
     - On Unix: rename() is atomic
     - On Windows: use ReplaceFileW() with REPLACE_FILE_FAIL_IF_NOT_EXIST
  8. On success: update index immediately, emit file_modified event
  9. On failure:
     - Delete temp file
     - Check if original file still exists
     - Retry up to 3 times with exponential backoff
     - Return error if all retries fail
  10. Release lock
```

### Error Recovery
- **Disk full during write:** Write fails before fsync, original file untouched, temp deleted
- **Permission denied on rename:** Temp file left in .forge/temp; reconcile cleanup task removes stale temps >1h old
- **Process crash mid-write:** Temp files automatically cleaned on next startup (glob .forge/temp/*.tmp)

### Cross-Platform Differences
| Platform | Atomic Rename | Sync Behavior | Lock Mechanism |
|----------|---------------|---------------|---|
| Unix/Linux | rename(2) | fsync() | flock() |
| macOS | rename(2) | fsync() | flock() |
| Windows | ReplaceFileW | FlushFileBuffers | CreateFileW with exclusive flag |

---

## 5. Index Rebuild Process

### Full Rebuild Algorithm
```
RebuildIndex():
  1. Create .forge/index.db.new (temp database, same schema)
  2. Get list of all .md/.mdx/.canvas files in forge-root
  3. Partition files into 16 chunks (processor count)
  4. For each chunk, spawn worker thread:
     a. Read file (UTF-8 with error recovery)
     b. Parse with markdown parser (see §6)
     c. Extract frontmatter, links, tags
     d. Compute content hash
     e. Insert into index.db.new: files, blocks, links, tags, properties
     f. Report progress (chunk N/total)
  5. Build FTS5 index on all blocks
  6. Build Tantivy index on all blocks (separate, see §7)
  7. Close all database connections
  8. Swap: rename index.db → index.db.old, rename index.db.new → index.db
  9. Verify integrity (PRAGMA integrity_check)
  10. Delete index.db.old
  11. Emit rebuild_complete event with stats
```

### Performance Targets (SSD)
- **10k files:** ~30s (1000 blocks/file)
- **50k files:** ~120s
- **100k files:** ~250s
- **Memory usage:** <2GB peak

### Progress Reporting
- Emit progress events every 500 files or 5s: `{processed: N, total: M, percent: P, eta_seconds: S}`
- UI displays indeterminate progress bar → determinate as estimate refines

### Handling Corrupt Files
```
On parse error:
  1. Log warning with file path and error details
  2. Create stub entry in files table with is_corrupted=1
  3. Insert single "error" block with error message
  4. Continue processing next file
  5. After rebuild, UI surface list of corrupt files with action "Repair"
```

---

## 6. Markdown/MDX Parser Pipeline

### Parser Choice: `comrak` over `pulldown-cmark`
- **Why comrak:** Better table support, GitHub Flavored Markdown spec-compliant, strikethrough, autolink
- **Why not pulldown:** No table parsing, less feature-complete

### Parsing Pipeline
```
markdown_source:
  ↓
[1] Extract YAML frontmatter (regex: /^---\n(.*?)\n---/s)
  ↓
[2] Parse frontmatter as YAML → properties table
  ↓
[3] Parse remaining markdown with comrak (ComrakOptions {
      extension: { strikethrough: true, table: true, autolink: true },
      parse: { smart: false, default_info_string: "text" }
    })
  ↓
[4] Traverse AST, flatten to blocks:
      - heading → block (level: 1-6)
      - paragraph → block
      - code_block → block (language: infostring)
      - list → block (item count)
      - list_item → parent_block_id ref
      - table → block (rows: N, cols: M)
  ↓
[5] Extract wikilinks from inline text:
      - Regex: /\[\[([^\]]+)\]\]/g
      - Extract link_text, resolve to target_path if possible
      - Insert into links table with is_resolved flag
  ↓
[6] Extract embeds: ![[path]] or ![alt](path)
      - Insert into links table with link_type='embed'
  ↓
[7] Extract inline tags: #tag syntax
      - Tag per block + global tag per file
  ↓
[8] Compute content hash, insert file + all blocks
```

### Wikilink Resolution
- **Format accepted:** `[[path]]`, `[[path|display text]]`, `[[../parent/file]]`
- **Resolution order:**
  1. Exact path match (case-insensitive on Windows/Mac)
  2. Basename match if unique (e.g., `[[nexus]]` → `projects/nexus.md`)
  3. Fuzzy match (Levenshtein distance <3)
  4. Mark unresolved, store link_text for UI suggestion

### Embed Resolution
- **Syntax:** `![[file.md]]` (displays heading + first 3 blocks)
- **Transitive embeds:** Depth limit of 3 to prevent infinite loops
- **Image embeds:** `![alt](attachments/image.png)` stored as attachment links

---

## 7. Tantivy Search Integration

### Index Schema
```json
{
  "fields": [
    {"name": "file_path", "type": "text", "stored": true, "indexed": false},
    {"name": "block_id", "type": "u64", "stored": true, "indexed": false},
    {"name": "block_type", "type": "text", "stored": true, "indexed": false},
    {"name": "content", "type": "text", "stored": false, "indexed": true},
    {"name": "tags", "type": "text", "stored": true, "indexed": true},
    {"name": "created_date", "type": "date", "stored": true, "indexed": true}
  ]
}
```

### Tokenizer Configuration
- **Language:** English stemming (via Tantivy's `SimpleTokenizer` + stemming filter)
- **Stop words:** Default English stop words removed
- **Case:** Lowercase all tokens
- **Accents:** Remove diacritics

### Query Syntax (User Facing)
```
Simple:
  "machine learning"        → phrase search
  machine learning          → OR across all fields
  +required -excluded       → must include, must exclude
  path:notes/*              → faceted by file path
  type:heading              → faceted by block type
  #tag                      → tag faceted search
  
Advanced (elastic-ish):
  (deep OR machine) AND learning
  content:"neural network"~5  → proximity search, 5-word distance
```

### Search Result Ranking
- **BM25 scoring:** Default, field-weighted (content: 2.0x, tags: 1.5x)
- **Boost recent:** Multiply score by `(1 + days_since_modified / 365)^0.5`
- **Penalize deleted:** Exclude is_deleted=1 files from results

### Tantivy + FTS5 Complementarity
| Query Type | Tantivy | FTS5 | Strategy |
|-----------|---------|------|----------|
| Full-text phrase | ✓ | ✓ | Tantivy (faster) |
| Fuzzy/typo | ✓ | - | Tantivy only |
| Structured queries (AND/OR/NOT) | ✓ | ✓ | Tantivy (better ranking) |
| Exact match | - | ✓ | FTS5 (exact semantics) |
| Faceted (by path/type) | ✓ | - | Tantivy only |

---

## 8. CRDT Sync Design (Optional)

### Protocol Architecture
```
Peer A ←→ [Discovery/Relay] ←→ Peer B

State Vector: {file_id: (hash, clock)}
```

### Implementation Choice: `automerge-rs` over `yrs`
- **Why automerge:** Better JSON semantics, more mature Rust bindings, clearer change semantics for markdown blocks
- **Why not yrs:** Stronger position-based text CRDT, but overkill for block-level sync

### Sync Protocol
```
1. Handshake (one-time or per session):
   A → B: {peer_id, state_vector, supported_types}
   B → A: {peer_id, state_vector, supported_types}

2. Delta Exchange:
   A computes: delta = state_vector_B.changes_since(state_vector_A)
   A → B: {deltas: [...], new_state_vector}
   B applies deltas, increments own state vector
   B → A: {ack, delta: [...]}  (reverse sync if concurrent changes)

3. Conflict Resolution (per data type):
   - Files: Last-write-wins (timestamp + peer_id tiebreaker)
   - Blocks: Automerge structural merge (CRDTs handle)
   - Links: Union of sets (resolved + unresolved tracked separately)
   - Tags: Union of sets
   - Properties: Last-write-wins per key

4. State Vector Management:
   - Stored in .forge/state.json: {clock: N, peer_id, lamport_ts}
   - Incremented on every local write
   - Compacted after 1000 entries (GC unreachable versions)
```

### Peer Discovery
- **LAN:** mDNS (via `mdns-sd` crate), advertise as `_nexus._tcp.local`
- **Relay:** Optional relay.nexus.example.com for WAN; server relays deltas, never stores data
- **Manual:** Users can specify peer IP:port in .forge/config.toml

### Large Forge Handling (>10k files)
- **State vector compression:** Summarize by path prefix (e.g., `notes/*` → single entry)
- **Chunked delta exchange:** Split large deltas into 10MB chunks, parallel upload
- **Incremental sync:** Only sync changed blocks, not entire files
- **Bandwidth target:** <1MB/s for typical 50k-file forge over LAN

---

## 9. Caching Strategy

### In-Memory Caches

#### Block Cache
- **LRU eviction, max 2000 blocks (~10MB)**
- **TTL:** 5min or invalidated on file change
- **Warmed on:** Index rebuild complete, file opened

#### Link Resolution Cache
- **Map: wikilink_text → file_id**
- **LRU, max 5000 entries**
- **TTL:** 1h or invalidated when target files change

#### Search Result Cache
- **Last 20 queries + results**
- **TTL:** 10min or invalidated on files change

### Cache Invalidation
```
On file_modified event:
  1. Invalidate blocks for that file_id
  2. Invalidate links originating from file_id
  3. Invalidate link resolution cache entries referencing file_id
  4. Invalidate search cache (conservative; invalidate all)
```

### Startup Cache Warming
- **Phase 1 (blocking):** Load block cache with most-accessed files (from access_log)
- **Phase 2 (background):** Warm link resolution cache, then search cache

---

## 10. Concurrency Model

### SQLite Configuration
```sql
PRAGMA journal_mode = WAL;        -- Write-Ahead Logging for concurrent reads
PRAGMA synchronous = NORMAL;      -- Balance durability/performance
PRAGMA cache_size = -16000;       -- 16MB cache
PRAGMA foreign_keys = ON;
```

### Read/Write Coordination
```
AtomicWrite(file_path, content):
  1. Acquire global_write_lock (mutex)
  2. Perform atomic write (see §4)
  3. Update SQLite in single transaction
  4. Release global_write_lock

Query(sql):
  1. Acquire SQLite connection (from pool, max 4 connections)
  2. Execute read (no lock needed, WAL handles)
  3. Release connection
```

### Concurrent File Access
- **Same file, concurrent edits:** Last write wins; index updated to winning version; sync reconciles via CRDT
- **Different files, concurrent edits:** Fully parallelizable; WAL manages isolation
- **Blocking scenarios:** Only during index rebuild; UI shows progress, unblocks after ~1min

---

## 11. Trait Definitions

### StorageProvider
```rust
pub trait StorageProvider: Send + Sync {
  async fn write_file(&self, path: &str, content: &[u8]) -> Result<FileMetadata>;
  async fn read_file(&self, path: &str) -> Result<Vec<u8>>;
  async fn delete_file(&self, path: &str) -> Result<()>;
  async fn list_files(&self, prefix: &str) -> Result<Vec<FileMetadata>>;
  async fn file_exists(&self, path: &str) -> Result<bool>;
  fn watch_changes(&self) -> mpsc::Receiver<FsEvent>;
}

pub struct FileMetadata {
  pub path: String,
  pub size_bytes: u64,
  pub modified_at: i64,
  pub content_hash: String,
}
```

### IndexProvider
```rust
pub trait IndexProvider: Send + Sync {
  async fn query_blocks(&self, sql: &str) -> Result<Vec<Block>>;
  async fn insert_block(&self, block: &Block) -> Result<u64>;
  async fn update_file_metadata(&self, file_id: u64, meta: &FileMetadata) -> Result<()>;
  async fn rebuild_index(&self) -> Result<RebuildStats>;
  fn index_ready(&self) -> bool;
}
```

### SyncProvider
```rust
pub trait SyncProvider: Send + Sync {
  async fn start_sync(&self, peer: &PeerConfig) -> Result<SyncSession>;
  async fn apply_delta(&self, delta: &Delta) -> Result<()>;
  async fn generate_delta(&self, state_vector: &StateVector) -> Result<Delta>;
  fn state_vector(&self) -> StateVector;
}
```

### SearchProvider
```rust
pub trait SearchProvider: Send + Sync {
  async fn index_block(&self, block: &Block) -> Result<()>;
  async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>;
  async fn rebuild_search_index(&self) -> Result<()>;
}

pub struct SearchResult {
  pub file_path: String,
  pub block_id: u64,
  pub block_type: String,
  pub excerpt: String,
  pub score: f32,
}
```

---

## 12. Error Taxonomy

```rust
pub enum StorageError {
  FileNotFound(String),
  PermissionDenied(String),
  IoError(io::Error),
  CorruptFile { path: String, reason: String },
  IndexInconsistency { details: String },
  SyncConflict { file_id: u64, peer: String },
  WriteFailure { path: String, reason: String },
  ParseError { file: String, error: String },
  LockedExclusively(String),
  ConfigInvalid(String),
}

pub enum SyncError {
  PeerNotFound(String),
  HandshakeFailed(String),
  DeltaApplicationFailed { reason: String },
  StateVectorDiverged { reason: String },
  RelayUnavailable,
}
```

---

## 13. Performance Targets

| Operation | Target Latency | Notes |
|-----------|---|---|
| Open existing file | <50ms | Index hit, no parse |
| Create new file | <200ms | Parse + index insert |
| Edit existing file (typical 5KB) | <100ms | Atomic write + index update |
| Search (1000 blocks, "machine learning") | <300ms | Tantivy query + BM25 ranking |
| Index query (SQL, simple) | <10ms | SQLite with hot cache |
| Index rebuild (50k files) | <120s | Parallel, SSD-based |
| Sync delta (10k changed blocks) | <2s | Compression + state vector |
| Memory (idle, 50k files) | <500MB | Index cache + Tantivy |

---

## 14. Upgrade Path

### Schema Migrations
- Version tracked in `_schema_version` table
- Each version in separate SQL file: `.forge/migrations/XXX-description.sql`
- Applied on startup, skipped if already applied
- Rollback only supported in dev mode (CLI: `nexus migrate --rollback`)

### Backward Compatibility
- Current version (v1) supports reading indices from v0.9+
- Index rebuild available: `nexus index --rebuild` forces full reindex
- On version mismatch, prompt: "Index version mismatch. [Rebuild] or [Migrate]?"

### Example Upgrade Scenario
```
v1.0 released with new "canvas" file type.
User with v0.9 index opens Nexus v1.0:
  1. Startup detects version mismatch
  2. Apply migration: add canvas_metadata table
  3. Rebuild search index to include canvas blocks
  4. Display progress bar: "Updating index... 30%"
  5. Ready for use (~30s for 50k files)
```

---

## 15. First Launch Experience

### Initialization Sequence
```
User opens Nexus pointing to existing folder with 10k .md files:

1. Scan forge root, detect existing files (2s)
2. Create .forge/ directory structure, init index.db (1s)
3. Start background index rebuild
   - UI shows: "Indexing your notes... 0%" (indeterminate progress)
   - Computed estimate at 1000 files: "~25 seconds remaining"
   - Progress bar transitions to determinate
4. Parallel file parsing (4 workers)
5. Every 500 files, emit progress event, update UI
6. Full-text index built (Tantivy + FTS5)
7. UI: "Ready! 10,562 notes indexed"
8. User can immediately:
   - Browse note list (from index, no files loaded yet)
   - Search (limited to indexed blocks)
9. Cannot edit until index completes (files locked during rebuild)
```

### Usable Before Indexing
- ✓ View file list (from files table)
- ✓ Read file content (direct filesystem read)
- ✓ Search (limited to indexed blocks, ~70% complete)
- ✗ Create/edit/delete (blocked)

---

## 16. Sync UI Flows

### Initial Sync Setup
```
Settings → Sync → [Configure Sync]
  1. Radio: "Local Network" | "Relay Server" | "Git-based"
  2. If Local: Discover nearby peers via mDNS
     - Display list: "Peer A (192.168.1.5), Peer B (desktop)"
     - User selects peer
  3. If Relay: Enter server URL (default: relay.nexus.local)
  4. Handshake: "Connecting to Peer A..."
  5. State sync: "Peer has 12,453 notes, you have 11,891 (362 new)"
  6. Confirm: [Sync Now] or [Ignore]
```

### During Sync
```
Progress modal:
  ↓ Downloading...
  [████████░░░░░░░░░░░░] 45% (2.3MB/10.5MB)
  ETA: 15 seconds
  
  Status: Merging blocks...
```

### Conflict Resolution
```
Conflict detected: Peer A modified notes/project.md 2 hours ago
Your local version is 15 minutes old.

[Keep Local] [Accept Theirs] [Manual Review]

If [Manual Review]:
  Side-by-side diff:
    Left: Your version (15min ago)
    Right: Their version (2h ago)
  [Choose blocks to keep, apply mapping]
```

---

## 17. Search UX

### Query Syntax (Displayed to User)
```
Search bar with inline help:
  Placeholder: "Search notes, code blocks, tags..."
  
User types: "neural"
  Results: 42 blocks matching
  Autocomplete: neural network, neural *, etc.
  
User types: "#project"
  Results: All blocks tagged #project
  
User types: "path:notes/ai/* deep learning"
  Results: Blocks in notes/ai/ matching "deep learning"
  
User types: "created:last-week"
  Results: Files created in last 7 days
```

### Result Display
```
Search result item:
  ├─ File: notes/ai/neural-networks.md
  ├─ Block type: heading (##)
  ├─ Excerpt: "...foundations of neural networks, covering..."
  ├─ Score: 0.95 (BM25 ranking)
  ├─ Modified: 2 hours ago
  └─ [Preview] [Open]
```

---

## Acceptance Criteria

- [ ] Forge directory structure created with atomic write semantics
- [ ] SQLite schema migrates cleanly from v0 → v1
- [ ] File watcher detects and debounces 1000 files/sec
- [ ] Index rebuild completes for 50k files in <120s
- [ ] Markdown parser handles GitHub-flavored syntax, frontmatter, wikilinks
- [ ] Tantivy search returns results within 300ms for typical queries
- [ ] CRDT sync merges 10k-file deltas with <2% data loss in conflict scenarios
- [ ] UI functional before index rebuild completes (70%+ blocks indexed)
- [ ] Search supports phrase, AND/OR/NOT, faceted queries
- [ ] Upgrade path preserves user data; no manual migration required
- [ ] Concurrent reads scale to 16+ queries; writes atomic + isolated
- [ ] File watching continues during sync and index operations

---

## Dependencies & Risks

| Dependency | Status | Risk Mitigation |
|-----------|--------|---|
| `comrak` parser | Stable | Fork if required; parser is well-tested |
| `notify` file watcher | Stable | Debounce layer isolates OS bugs |
| `automerge-rs` CRDT | Beta | Fallback to last-write-wins if issues arise |
| `tantivy` FTS | Stable | FTS5 fallback if Tantivy index corrupts |
| SQLite WAL | Proven | 10+ years production; edge cases rare |

---

## Timeline & Phases

- **Phase 1 (Weeks 1-2):** Core storage + index schema + file watcher
- **Phase 2 (Weeks 3-4):** Parser pipeline + Tantivy integration
- **Phase 3 (Weeks 5-6):** CRDT sync engine (optional for v1.0)
- **Phase 4 (Week 7):** Caching + performance tuning + UI flows
- **Phase 5 (Week 8):** Testing, docs, upgrade path validation

---

**Document Version:** 1.0  
**Last Updated:** April 2026  
**Next Review:** Q3 2026
