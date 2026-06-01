//! Nexus storage engine: forge layout, atomic writes, `SQLite` index,
//! markdown parsing, file watching, and Tantivy full-text search.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-03-storage-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod atomic;
pub mod bases;
mod canvas;
/// BL-114: tree-sitter code-symbol index. Populated from the storage
/// engine's `write_file` / `rebuild_index` paths and the
/// `com.nexus.git.commit` subscription in [`core_plugin`].
pub mod code_index;
pub mod config;
pub mod core_plugin;
/// BL-128 thin slice: file-backed personal entity index. Lives under
/// `<forge>/entities/` and powers the agent's `entity_search` /
/// `entity_get` / `entity_relations` IPC handlers.
pub mod entity_index;
mod error;
mod export;
mod find_replace;
mod forge;
mod graph;
mod handlers;
/// BL-083: forge-to-forge import / migration planning + apply.
pub mod import;
mod index;
/// BL-091: Git-LFS pointer detection + smudge passthrough for read paths.
pub mod lfs;
pub mod mdx;
pub mod obsidian_base;
mod parser;
mod reconcile;
pub mod schema;
mod search;
mod search_scope;
mod tasks;
pub mod vectorstore;
mod watcher;

pub mod ipc;

pub use atomic::atomic_write;
pub use core_plugin::StorageCorePlugin;
pub use error::StorageError;
pub use forge::{Forge, ForgeLock};

use serde::{Deserialize, Serialize};

/// One entry returned by [`StorageEngine::list_dir`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeEntry {
    /// File or directory name (no path separators).
    pub name: String,
    /// Path relative to the forge root, using forward slashes.
    pub relpath: String,
    /// `true` if this entry is a directory.
    pub is_dir: bool,
    /// Last-modified time, unix millis. `None` when the filesystem /
    /// platform does not expose it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_ms: Option<i64>,
    /// Created time, unix millis. `None` when the filesystem /
    /// platform does not expose it (Linux with older `statx`, some
    /// network mounts, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_ms: Option<i64>,
}
pub use canvas::{
    apply_patch, extract_file_links, parse_canvas, serialize_canvas, CanvasEdge, CanvasEdgeRecord,
    CanvasEdgeType, CanvasFile, CanvasNode, CanvasNodeRecord, CanvasNodeType, CanvasPatchError,
    CanvasPatchOp,
};
pub use export::export_to_html;
pub use find_replace::{
    find_in_files, replace_in_files, FileMatches, FindInFilesArgs, LineMatch, ReplaceError,
    ReplaceInFilesArgs, ReplaceReport,
};
pub use graph::{
    BacklinkResult, EdgeData, GraphStats, KnowledgeGraph, OutgoingLink, UnresolvedLink,
};
pub use index::{
    delete_file, file_by_path, insert_file, query_backlinks, query_blocks, query_files,
    query_links, query_tags, soft_delete_file,
};
pub use index::{insert_jsx_components, query_jsx_components, JsxRecord};
pub use index::{
    BlockRecord, FileFilter, FileMetadata, FileRecord, LinkRecord, RebuildStats, TagResult,
};
pub use mdx::{parse_mdx, MdxParseResult, ParsedJsxComponent};
pub use parser::{parse_markdown, ParsedBlock, ParsedFile, ParsedLink, ParsedTag, Property};
pub use reconcile::{reconcile, ReconcileDelta};
pub use search::{SearchIndex, SearchResult};
pub use search_scope::{parse_scoped_query, CmpOp, PropertyOp, ScopeFilter};
pub use tasks::{
    insert_tasks, query_tasks, toggle_task, toggle_task_in_file, ParsedTask, TaskFilter, TaskRecord,
};
pub use watcher::{relative_path, should_ignore, StorageEvent, Watcher};

use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use r2d2_sqlite::SqliteConnectionManager;

// ── StorageConfig ─────────────────────────────────────────────────────────────

/// Configuration for [`StorageEngine`].
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Number of read connections in the r2d2 pool. Default: 4.
    pub pool_size: u32,
    /// Debounce delay for the file watcher in milliseconds. Default: 300.
    pub debounce_ms: u64,
    /// Number of Rayon threads (0 = auto-detect). Default: 0.
    pub rayon_threads: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            pool_size: 4,
            debounce_ms: 300,
            rayon_threads: 0,
        }
    }
}

// ── StorageEngine ─────────────────────────────────────────────────────────────

/// Facade that composes all storage subsystems into a single public API.
///
/// Obtain an instance via [`StorageEngine::init`] (new forge) or
/// [`StorageEngine::open`] (existing forge).
pub struct StorageEngine {
    forge: Forge,
    _lock: ForgeLock,
    pool: r2d2::Pool<SqliteConnectionManager>,
    write_conn: Mutex<rusqlite::Connection>,
    search_index: SearchIndex,
    graph: Arc<RwLock<graph::KnowledgeGraph>>,
}

impl std::fmt::Debug for StorageEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageEngine")
            .field("root", &self.forge.root())
            .finish_non_exhaustive()
    }
}

impl StorageEngine {
    // ── Constructors ──────────────────────────────────────────────────────────

    /// Create and initialise a brand-new forge at `root`.
    ///
    /// Calls [`Forge::init`] to create the directory structure, then opens
    /// all subsystems with [`StorageConfig::default`].
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on any I/O, lock, or database failure.
    pub fn init(root: &Path) -> Result<Self, StorageError> {
        let forge = Forge::new(root);
        forge.init()?;
        open_internal(forge, &StorageConfig::default(), true)
    }

    /// Open an existing forge at `root` with custom configuration.
    ///
    /// Verifies that `.forge/` exists (returns [`StorageError::FileNotFound`]
    /// otherwise) then opens all subsystems.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::FileNotFound`] when the forge directory does not
    /// exist, or [`StorageError`] on any lock, I/O, or database failure.
    pub fn open(root: &Path, config: &StorageConfig) -> Result<Self, StorageError> {
        let forge = Forge::new(root);
        if !forge.forge_dir().exists() {
            return Err(StorageError::FileNotFound(
                forge.forge_dir().display().to_string(),
            ));
        }
        open_internal(forge, config, false)
    }

    // ── File operations ───────────────────────────────────────────────────────

    /// Write `content` to `path` (vault-relative) atomically and update the index.
    ///
    /// `path` must be relative to the forge root (e.g. `"notes/hello.md"`).
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O or database failure.
    ///
    /// # Panics
    ///
    /// Panics if the internal write-connection mutex is poisoned.
    pub fn write_file(&self, path: &str, content: &[u8]) -> Result<FileMetadata, StorageError> {
        // 1. Atomic write to disk. Confine to the forge root: `Path::join`
        //    does not normalize `..` so a raw caller-supplied relpath would
        //    otherwise let `fs::write` resolve traversal at the syscall
        //    level. See issue #72.
        let abs_target = resolve_within(self.forge.root(), path)?;
        atomic_write(&abs_target, content, &self.forge.temp_dir())?;

        // 2. Decode content as UTF-8.
        let text = std::str::from_utf8(content).map_err(|e| StorageError::CorruptFile {
            path: path.to_string(),
            reason: e.to_string(),
        })?;

        // 3. Lock write_conn and open a transaction. All SQLite mutations below
        //    go through `&tx`; the in-memory knowledge graph is only touched
        //    AFTER `tx.commit()` succeeds, so a mid-sequence DB failure cannot
        //    leave the graph holding state that doesn't exist on disk.
        let mut conn = self.write_conn.lock().expect("write_conn mutex poisoned");
        let tx = conn.transaction()?;

        // 4. Delete existing index entry if the path is already indexed.
        if let Some(existing) = file_by_path(&tx, path)? {
            tx.execute(
                "DELETE FROM fts_blocks WHERE file_path = ?1",
                rusqlite::params![path],
            )?;
            canvas::delete_canvas(&tx, existing.id.cast_signed())?;
            // BL-114: drop any prior code symbols for this path so a
            // code→non-code rename doesn't leave stale rows.
            let _ = code_index::delete_file_symbols(&tx, path);
            delete_file(&tx, existing.id)?;
        }

        let size_bytes = content.len() as u64;
        let file_type = infer_file_type(path);

        // 5. Branch by file type. Each branch finishes the DB work, commits the
        //    transaction, then applies the corresponding graph mutations.
        if path.ends_with(".canvas") {
            // ── Canvas path ──────────────────────────────────────────────
            let canvas_data = canvas::parse_canvas(text)?;
            let content_hash = nexus_formats::sha256_hex(text.as_bytes());
            let empty_parsed = ParsedFile {
                content_hash: content_hash.clone(),
                blocks: Vec::new(),
                links: Vec::new(),
                tags: Vec::new(),
                frontmatter: Vec::new(),
                tasks: Vec::new(),
            };
            let file_id = insert_file(&tx, path, &file_type, size_bytes, &empty_parsed)?;
            canvas::insert_canvas(&tx, file_id.cast_signed(), &canvas_data)?;

            let canvas_link_targets = canvas::extract_file_links(&canvas_data);
            tx.commit()?;

            // Update knowledge graph: file-type nodes create links.
            {
                let mut g = self.graph.write().expect("graph lock poisoned");
                g.add_note(path);
                g.remove_links_from(path);
                for target in canvas_link_targets {
                    g.add_link(
                        path,
                        &target,
                        graph::EdgeData {
                            link_type: "canvas-embed".to_string(),
                            link_text: target.clone(),
                            fragment: None,
                        },
                    );
                }
            }

            Ok(FileMetadata {
                path: path.to_string(),
                size_bytes,
                modified_at: unix_now(),
                content_hash,
            })
        } else {
            // ── Markdown / MDX path ──────────────────────────────────────
            let is_mdx = Path::new(path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("mdx"));
            let (parsed, jsx_components) = if is_mdx {
                let result = mdx::parse_mdx(text)?;
                (result.parsed_file, result.components)
            } else {
                (parse_markdown(text)?, Vec::new())
            };

            let file_id = insert_file(&tx, path, &file_type, size_bytes, &parsed)?;

            if !jsx_components.is_empty() {
                insert_jsx_components(&tx, file_id, &jsx_components)?;
            }

            tx.commit()?;

            // BL-114: refresh the tree-sitter code-symbol index for
            // code-language files. detect_language returns None for
            // markdown / non-code files, so the call is a no-op there.
            // Runs OUTSIDE the file/blocks transaction with its own
            // inner tx: errors are logged and swallowed — a broken
            // parser must not block a normal save and must not roll
            // back the file/blocks write that already succeeded.
            if let Some(lang) = code_index::detect_language(path) {
                let symbols = code_index::extract_symbols(lang, text);
                if let Err(e) = code_index::upsert_file_symbols(&conn, path, lang, &symbols) {
                    tracing::warn!(
                        path,
                        error = %e,
                        "BL-114: code_symbols upsert failed; index entry skipped",
                    );
                }
            }

            // Update knowledge graph.
            {
                let mut g = self.graph.write().expect("graph lock poisoned");
                g.add_note(path);
                g.remove_links_from(path);
                for link in &parsed.links {
                    let target = link.target_path.as_deref().unwrap_or(&link.link_text);
                    g.add_link(
                        path,
                        target,
                        graph::EdgeData {
                            link_type: link.link_type.clone(),
                            link_text: link.link_text.clone(),
                            fragment: link.fragment.clone(),
                        },
                    );
                }
            }

            Ok(FileMetadata {
                path: path.to_string(),
                size_bytes,
                modified_at: unix_now(),
                content_hash: parsed.content_hash,
            })
        }
    }

    /// Write `content` to `path` (vault-relative) atomically **without**
    /// touching the FTS index, knowledge graph, or any post-write listeners.
    ///
    /// Intended for shell-owned metadata under `.forge/` (e.g.
    /// `workspace.json`) that the user never sees as vault content and must
    /// not pollute search results. User-facing writes MUST continue to go
    /// through [`StorageEngine::write_file`] so indexing stays consistent.
    ///
    /// `path` must be relative to the forge root. Parent directories are
    /// created as needed.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O failure.
    pub fn write_raw(&self, path: &str, content: &[u8]) -> Result<(), StorageError> {
        // Confine to the forge root; see `write_file` and issue #72.
        let abs_target = resolve_within(self.forge.root(), path)?;
        atomic_write(&abs_target, content, &self.forge.temp_dir())?;
        Ok(())
    }

    /// Read the raw bytes of the file at `path` from disk.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::FileNotFound`] when the file does not exist,
    /// or [`StorageError::Io`] on other I/O failures.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, StorageError> {
        // Confine to the forge root; see `write_file` and issue #72.
        let abs = resolve_within(self.forge.root(), path)?;
        let bytes = std::fs::read(&abs).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::FileNotFound(path.to_string())
            } else {
                StorageError::Io(e)
            }
        })?;

        // BL-091: if the file looks like a Git-LFS pointer, try to
        // resolve the real content via `git lfs smudge`. Failure
        // (no git-lfs binary, offline + no cache, etc.) degrades
        // gracefully to handing back the pointer text with a
        // tracing warning so the caller still gets a deterministic
        // result and operators see the degradation in logs.
        if lfs::is_pointer(&bytes) {
            if let Some(resolved) = lfs::smudge(self.forge.root(), &bytes) {
                return Ok(resolved);
            }
            tracing::warn!(
                path,
                "BL-091: file is a Git-LFS pointer but smudge \
                 failed (git-lfs missing or offline); returning \
                 pointer text",
            );
        }
        Ok(bytes)
    }

    // ── Canvas operations ─────────────────────────────────────────────────

    /// Read and parse a `.canvas` file from disk.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O or parse failure.
    pub fn read_canvas(&self, path: &str) -> Result<CanvasFile, StorageError> {
        let bytes = self.read_file(path)?;
        let text = std::str::from_utf8(&bytes).map_err(|e| StorageError::CorruptFile {
            path: path.to_string(),
            reason: e.to_string(),
        })?;
        canvas::parse_canvas(text)
    }

    /// Query canvas nodes for a file by its index ID.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn canvas_nodes(&self, file_id: i64) -> Result<Vec<CanvasNodeRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        canvas::query_canvas_nodes(&conn, file_id)
    }

    /// Query canvas edges for a file by its index ID.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn canvas_edges(&self, file_id: i64) -> Result<Vec<CanvasEdgeRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        canvas::query_canvas_edges(&conn, file_id)
    }

    /// Resolve `path` to its index file id and return all canvas nodes.
    ///
    /// Returns an empty vector when the path is not indexed (the caller can
    /// treat this the same as "no nodes yet").
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn canvas_nodes_by_path(&self, path: &str) -> Result<Vec<CanvasNodeRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        let Some(record) = file_by_path(&conn, path)? else {
            return Ok(Vec::new());
        };
        canvas::query_canvas_nodes(&conn, record.id.cast_signed())
    }

    /// Resolve `path` to its index file id and return all canvas edges.
    ///
    /// Returns an empty vector when the path is not indexed.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn canvas_edges_by_path(&self, path: &str) -> Result<Vec<CanvasEdgeRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        let Some(record) = file_by_path(&conn, path)? else {
            return Ok(Vec::new());
        };
        canvas::query_canvas_edges(&conn, record.id.cast_signed())
    }

    /// Serialize `canvas` and write it through [`Self::write_file`] so the
    /// `SQLite` canvas index + knowledge graph stay in sync.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on serialize, I/O, or database failure.
    pub fn write_canvas(
        &self,
        path: &str,
        canvas: &CanvasFile,
    ) -> Result<FileMetadata, StorageError> {
        let json = canvas::serialize_canvas(canvas)?;
        self.write_file(path, json.as_bytes())
    }

    /// Read `path`, apply `ops`, and write the result back through
    /// [`Self::write_canvas`] so the canvas index is kept current.
    ///
    /// Used by the `canvas_patch` IPC handler; the shell debounces patch
    /// flushes so this is called once per idle burst rather than per frame.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O / parse / serialize failure. Patch
    /// errors (duplicate add) surface as [`StorageError::CorruptFile`] with
    /// the originating message so callers get a typed failure without
    /// widening [`StorageError`]'s variant set for a single caller.
    pub fn patch_canvas(
        &self,
        path: &str,
        ops: &[canvas::CanvasPatchOp],
    ) -> Result<FileMetadata, StorageError> {
        let mut canvas_file = self.read_canvas(path)?;
        canvas::apply_patch(&mut canvas_file, ops).map_err(|e| StorageError::CorruptFile {
            path: path.to_string(),
            reason: e.to_string(),
        })?;
        self.write_canvas(path, &canvas_file)
    }

    // ── Bases operations ──────────────────────────────────────────────────

    /// Index a base in `SQLite`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    ///
    /// # Panics
    ///
    /// Panics if the internal write-connection mutex is poisoned.
    pub fn index_base(&self, path: &str, base: &bases::Base) -> Result<i64, StorageError> {
        let conn = self.write_conn.lock().expect("write_conn mutex poisoned");
        bases::insert_base(&conn, path, base)
    }

    /// List all indexed bases.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn list_bases(&self) -> Result<Vec<bases::BaseSummary>, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        bases::query_bases(&conn)
    }

    /// Create a new `.bases` directory at `path` with `schema` and
    /// zero records, then index it. Rejects if the target directory
    /// already exists to avoid clobbering an existing base.
    ///
    /// The directory name (stripped of `.bases`) is used as the
    /// human-readable base name in `metadata.json`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::CorruptFile`] if the path already
    /// exists, or [`StorageError`] on I/O / validation / DB failure.
    pub fn base_create(
        &self,
        path: &str,
        schema: &nexus_types::bases::BaseSchema,
        seed_records: Vec<nexus_types::bases::BaseRecord>,
    ) -> Result<nexus_types::bases::Base, StorageError> {
        let abs_dir = self.forge.root().join(path);
        if abs_dir.exists() {
            return Err(StorageError::CorruptFile {
                path: path.to_string(),
                reason: "base directory already exists".to_string(),
            });
        }
        let name = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();
        let mut base = nexus_types::bases::init_base(&abs_dir, &name, schema)?;
        for mut record in seed_records {
            if record.id.is_empty() {
                record.id = uuid::Uuid::new_v4().to_string();
            }
            nexus_types::bases::validate_record(&base.schema, &record)?;
            base.records.push(record);
        }
        if !base.records.is_empty() {
            nexus_types::bases::save_base(&abs_dir, &base)?;
        }
        self.index_base(path, &base)?;
        Ok(base)
    }

    /// Append `record` to the base at `path` (forge-relative `.bases`
    /// directory), save the TOML / JSON files back to disk, and reindex.
    ///
    /// When `record.id` is empty a v4 UUID is generated. A collision with an
    /// existing record id is rejected with [`StorageError::CorruptFile`].
    /// Reindex uses `insert_base` which is `INSERT OR REPLACE` + record wipe
    /// + reinsert, so there's no orphan risk.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O, parse, validation, or DB failure.
    pub fn base_record_create(
        &self,
        path: &str,
        mut record: nexus_types::bases::BaseRecord,
    ) -> Result<nexus_types::bases::BaseRecord, StorageError> {
        let abs_dir = self.forge.root().join(path);
        let mut base = nexus_types::bases::load_base(&abs_dir)?;

        if record.id.is_empty() {
            record.id = uuid::Uuid::new_v4().to_string();
        } else if base.records.iter().any(|r| r.id == record.id) {
            return Err(StorageError::CorruptFile {
                path: path.to_string(),
                reason: format!("record id '{}' already exists", record.id),
            });
        }

        nexus_types::bases::validate_record(&base.schema, &record)?;
        base.records.push(record.clone());
        nexus_types::bases::save_base(&abs_dir, &base)?;
        self.index_base(path, &base)?;
        Ok(record)
    }

    /// Merge `fields` into the record identified by `record_id` in the base
    /// at `path`. Keys present in `fields` overwrite existing values; keys
    /// absent from `fields` are left intact. The `id` key in `fields` is
    /// ignored.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::FileNotFound`] when the record id is unknown,
    /// or [`StorageError`] on I/O / parse / DB failure.
    pub fn base_record_update(
        &self,
        path: &str,
        record_id: &str,
        fields: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<nexus_types::bases::BaseRecord, StorageError> {
        let abs_dir = self.forge.root().join(path);
        let mut base = nexus_types::bases::load_base(&abs_dir)?;

        let record = base
            .records
            .iter_mut()
            .find(|r| r.id == record_id)
            .ok_or_else(|| StorageError::FileNotFound(format!("record {record_id} in {path}")))?;

        for (k, v) in fields {
            if k == "id" {
                continue;
            }
            record.fields.insert(k.clone(), v.clone());
        }
        let updated = record.clone();
        nexus_types::bases::validate_record(&base.schema, &updated)?;
        nexus_types::bases::save_base(&abs_dir, &base)?;
        self.index_base(path, &base)?;
        Ok(updated)
    }

    /// Append a property to the schema of the base at `path`.
    ///
    /// `definition` is the raw JSON object describing the field (type,
    /// required, options, etc.) as stored under the field name in
    /// `schema.json`. Rejects duplicate names with
    /// [`StorageError::CorruptFile`].
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O / parse / DB failure or duplicate.
    pub fn base_property_create(
        &self,
        path: &str,
        name: &str,
        definition: serde_json::Value,
    ) -> Result<(), StorageError> {
        let abs_dir = self.forge.root().join(path);
        let mut base = nexus_types::bases::load_base(&abs_dir)?;
        if base.schema.fields.contains_key(name) {
            return Err(StorageError::CorruptFile {
                path: path.to_string(),
                reason: format!("property '{name}' already exists"),
            });
        }
        base.schema.fields.insert(name.to_string(), definition);
        nexus_types::bases::save_base(&abs_dir, &base)?;
        self.index_base(path, &base)?;
        Ok(())
    }

    /// Replace the definition of an existing property. When
    /// `migrate_values` is true, walks every record and coerces the
    /// value stored under `name` to the new type using a conservative
    /// best-effort cast (number ↔ string, bool ↔ checkbox, array
    /// round-trip for multi-select). Values that cannot be coerced
    /// are dropped to null rather than silently producing garbage.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::FileNotFound`] when the property is unknown,
    /// or [`StorageError`] on I/O / parse / DB failure.
    pub fn base_property_update(
        &self,
        path: &str,
        name: &str,
        definition: &serde_json::Value,
        migrate_values: bool,
    ) -> Result<(), StorageError> {
        let abs_dir = self.forge.root().join(path);
        let mut base = nexus_types::bases::load_base(&abs_dir)?;
        let old_def = base
            .schema
            .fields
            .get(name)
            .cloned()
            .ok_or_else(|| StorageError::FileNotFound(format!("property {name} in {path}")))?;
        base.schema
            .fields
            .insert(name.to_string(), definition.clone());
        if migrate_values {
            let old_type = property_type(&old_def);
            let new_type = property_type(definition);
            if old_type.as_deref() != new_type.as_deref() {
                for record in &mut base.records {
                    if let Some(v) = record.fields.get(name).cloned() {
                        let coerced = coerce_property_value(&v, new_type.as_deref());
                        record.fields.insert(name.to_string(), coerced);
                    }
                }
            }
        }
        nexus_types::bases::save_base(&abs_dir, &base)?;
        self.index_base(path, &base)?;
        Ok(())
    }

    /// Rename a schema column from `old_name` to `new_name`. The
    /// schema key is moved (preserving its definition) and every
    /// record's fields map has the old key copied to the new key and
    /// the old key removed. Rejects when `old_name` is missing or
    /// `new_name` already exists.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O / parse / DB failure.
    pub fn base_property_rename(
        &self,
        path: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), StorageError> {
        if old_name == new_name {
            return Ok(());
        }
        let abs_dir = self.forge.root().join(path);
        let mut base = nexus_types::bases::load_base(&abs_dir)?;
        if base.schema.fields.contains_key(new_name) {
            return Err(StorageError::CorruptFile {
                path: path.to_string(),
                reason: format!("property '{new_name}' already exists"),
            });
        }
        let def =
            base.schema.fields.remove(old_name).ok_or_else(|| {
                StorageError::FileNotFound(format!("property {old_name} in {path}"))
            })?;
        base.schema.fields.insert(new_name.to_string(), def);
        for record in &mut base.records {
            if let Some(v) = record.fields.remove(old_name) {
                record.fields.insert(new_name.to_string(), v);
            }
        }
        nexus_types::bases::save_base(&abs_dir, &base)?;
        self.index_base(path, &base)?;
        Ok(())
    }

    /// Remove the property `name` from the schema of the base at `path`
    /// and drop the corresponding key from every record's fields map.
    ///
    /// Missing names are a no-op so deletes are idempotent.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O / parse / DB failure.
    pub fn base_property_delete(&self, path: &str, name: &str) -> Result<(), StorageError> {
        let abs_dir = self.forge.root().join(path);
        let mut base = nexus_types::bases::load_base(&abs_dir)?;
        if base.schema.fields.remove(name).is_none() {
            return Ok(());
        }
        for record in &mut base.records {
            record.fields.remove(name);
        }
        nexus_types::bases::save_base(&abs_dir, &base)?;
        self.index_base(path, &base)?;
        Ok(())
    }

    /// Append `view` to the views list of the base at `path`. Views are
    /// keyed by `view.name`; a duplicate name is rejected with
    /// [`StorageError::CorruptFile`].
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O / parse / DB failure or duplicate.
    pub fn base_view_create(
        &self,
        path: &str,
        view: nexus_types::bases::BaseView,
    ) -> Result<(), StorageError> {
        let abs_dir = self.forge.root().join(path);
        let mut base = nexus_types::bases::load_base(&abs_dir)?;
        if base.views.iter().any(|v| v.name == view.name) {
            return Err(StorageError::CorruptFile {
                path: path.to_string(),
                reason: format!("view '{}' already exists", view.name),
            });
        }
        base.views.push(view);
        nexus_types::bases::save_base(&abs_dir, &base)?;
        self.index_base(path, &base)?;
        Ok(())
    }

    /// Replace a view by name with `view`. Uses `view.name` as the key — to
    /// rename a view, delete the old one and create a new one with the
    /// desired name.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::FileNotFound`] when the view name is
    /// unknown, or [`StorageError`] on I/O / parse / DB failure.
    pub fn base_view_update(
        &self,
        path: &str,
        view: nexus_types::bases::BaseView,
    ) -> Result<(), StorageError> {
        let abs_dir = self.forge.root().join(path);
        let mut base = nexus_types::bases::load_base(&abs_dir)?;
        let slot = base
            .views
            .iter_mut()
            .find(|v| v.name == view.name)
            .ok_or_else(|| StorageError::FileNotFound(format!("view {} in {path}", view.name)))?;
        *slot = view;
        nexus_types::bases::save_base(&abs_dir, &base)?;
        self.index_base(path, &base)?;
        Ok(())
    }

    /// Remove a view by `name` from the base at `path`. Missing names are
    /// a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O / parse / DB failure.
    pub fn base_view_delete(&self, path: &str, name: &str) -> Result<(), StorageError> {
        let abs_dir = self.forge.root().join(path);
        let mut base = nexus_types::bases::load_base(&abs_dir)?;
        let before = base.views.len();
        base.views.retain(|v| v.name != name);
        if base.views.len() == before {
            return Ok(());
        }
        nexus_types::bases::save_base(&abs_dir, &base)?;
        self.index_base(path, &base)?;
        Ok(())
    }

    /// Set the `deleted_at` slot on the record identified by
    /// `record_id` to `now` (Unix epoch seconds). The record stays in
    /// `records.json` but shell consumers that honour the slot will
    /// filter it out of visible lists. Missing ids are a no-op.
    ///
    /// Pairs with [`Self::base_record_restore`] — the two form the
    /// soft-delete primitive. [`Self::base_record_delete`] still
    /// hard-removes from disk.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O / parse / DB failure.
    pub fn base_record_soft_delete(&self, path: &str, record_id: &str) -> Result<(), StorageError> {
        let abs_dir = self.forge.root().join(path);
        let mut base = nexus_types::bases::load_base(&abs_dir)?;
        let mut touched = false;
        for record in &mut base.records {
            if record.id == record_id && record.deleted_at.is_none() {
                record.deleted_at = Some(unix_now());
                touched = true;
            }
        }
        if !touched {
            return Ok(());
        }
        nexus_types::bases::save_base(&abs_dir, &base)?;
        self.index_base(path, &base)?;
        Ok(())
    }

    /// Clear the `deleted_at` slot on the record identified by
    /// `record_id`. Missing ids or records with no `deleted_at` set
    /// are a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O / parse / DB failure.
    pub fn base_record_restore(&self, path: &str, record_id: &str) -> Result<(), StorageError> {
        let abs_dir = self.forge.root().join(path);
        let mut base = nexus_types::bases::load_base(&abs_dir)?;
        let mut touched = false;
        for record in &mut base.records {
            if record.id == record_id && record.deleted_at.is_some() {
                record.deleted_at = None;
                touched = true;
            }
        }
        if !touched {
            return Ok(());
        }
        nexus_types::bases::save_base(&abs_dir, &base)?;
        self.index_base(path, &base)?;
        Ok(())
    }

    /// Remove the record identified by `record_id` from the base at `path`.
    ///
    /// Hard-delete: the record is removed from `records.json` and the index
    /// is rebuilt. The soft-delete variant ([`Self::base_record_soft_delete`])
    /// sets a `deleted_at` timestamp instead, leaving the record on disk.
    ///
    /// Returns `Ok(())` when the record is removed. A missing id is a no-op
    /// so the caller can dedupe retries without racing.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O / parse / DB failure.
    pub fn base_record_delete(&self, path: &str, record_id: &str) -> Result<(), StorageError> {
        let abs_dir = self.forge.root().join(path);
        let mut base = nexus_types::bases::load_base(&abs_dir)?;
        let before = base.records.len();
        base.records.retain(|r| r.id != record_id);
        if base.records.len() == before {
            return Ok(());
        }
        nexus_types::bases::save_base(&abs_dir, &base)?;
        self.index_base(path, &base)?;
        Ok(())
    }

    /// Delete a file from disk and remove its index entry.
    ///
    /// Silently succeeds when the file does not exist on disk.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O or database failure.
    ///
    /// # Panics
    ///
    /// Panics if the internal write-connection mutex is poisoned.
    pub fn delete_file(&self, path: &str) -> Result<(), StorageError> {
        // Confine to the forge root; see `write_file` and issue #72.
        let abs = resolve_within(self.forge.root(), path)?;
        if abs.exists() {
            std::fs::remove_file(&abs)?;
        }

        // Remove from index. All SQLite mutations go through `&tx`; the
        // graph node is only removed AFTER `tx.commit()` succeeds, so a
        // mid-sequence DB failure cannot leave the graph believing the
        // file is gone while its rows are still in `files` / `fts_blocks`.
        let mut conn = self.write_conn.lock().expect("write_conn mutex poisoned");
        let tx = conn.transaction()?;
        if let Some(record) = file_by_path(&tx, path)? {
            tx.execute(
                "DELETE FROM fts_blocks WHERE file_path = ?1",
                rusqlite::params![path],
            )?;
            delete_file(&tx, record.id)?;
        }
        // BL-114: code symbols are path-keyed; drop them outside the
        // FTS/files cascade so a delete of a non-markdown file still
        // cleans up its symbols.
        let _ = code_index::delete_file_symbols(&tx, path);
        tx.commit()?;

        // Remove from graph. `remove_note` is a no-op for unknown
        // paths, so it's safe even if the file was never indexed.
        {
            let mut g = self.graph.write().expect("graph lock poisoned");
            g.remove_note(path);
        }

        Ok(())
    }

    /// List all indexed files whose path starts with `prefix`.
    ///
    /// Use an empty string to list everything.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn list_files(&self, prefix: &str) -> Result<Vec<FileMetadata>, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        let filter = FileFilter {
            prefix: if prefix.is_empty() {
                None
            } else {
                Some(prefix.to_string())
            },
            ..Default::default()
        };
        let records = query_files(&conn, &filter)?;
        Ok(records
            .into_iter()
            .map(|r| FileMetadata {
                path: r.path,
                size_bytes: r.size_bytes,
                modified_at: r.modified_at,
                content_hash: r.content_hash,
            })
            .collect())
    }

    /// Return `true` if a file with the given `path` is present in the index.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn file_exists(&self, path: &str) -> Result<bool, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        Ok(file_by_path(&conn, path)?.is_some())
    }

    // ── Forge tree operations ────────────────────────────────────────────────
    //
    // These operate on the on-disk layout directly rather than the SQLite
    // index. They are the IPC surface the Tauri shell's forge commands route
    // through so the shell does not need to import `std::fs` for file-tree
    // CRUD (see `docs/superpowers/specs/.../PRD-04.md` invariant #3).

    /// List entries under `relpath` within the forge.
    ///
    /// Returns both files and directories. Reads from disk, so newly-created
    /// entries not yet in the `SQLite` index are included. The `.forge/`
    /// internal directory is hidden from the root listing.
    ///
    /// Path confinement: rejects absolute paths, parent traversal, and any
    /// non-`Normal` component.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::PermissionDenied`] if `relpath` escapes the
    /// forge root, or [`StorageError::Io`] on read failure.
    pub fn list_dir(&self, relpath: &str) -> Result<Vec<TreeEntry>, StorageError> {
        let target = resolve_within(self.forge.root(), relpath)?;
        let mut entries: Vec<TreeEntry> = Vec::new();
        let dir_iter = match std::fs::read_dir(&target) {
            Ok(it) => it,
            // A caller asking "what's in <relpath>?" before <relpath> has been
            // created (e.g. `.forge/agent/sessions` on first agent boot, before
            // any session has been written) is a normal lifecycle state, not
            // an error. Surfacing NotFound as `Err` forces every caller to
            // duplicate the same fallback; emptying it here lets the IPC
            // contract stay "list returns rows" without crash-on-first-use.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(entries),
            Err(e) => return Err(e.into()),
        };
        for entry in dir_iter {
            let Ok(entry) = entry else { continue };
            let Ok(ft) = entry.file_type() else { continue };
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if relpath.is_empty() && name == ".forge" {
                continue;
            }
            let rel = if relpath.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", relpath.trim_end_matches('/'), name)
            };
            let (modified_ms, created_ms) = match entry.metadata() {
                Ok(md) => (
                    system_time_to_ms(md.modified().ok()),
                    system_time_to_ms(md.created().ok()),
                ),
                Err(_) => (None, None),
            };
            entries.push(TreeEntry {
                name,
                relpath: rel,
                is_dir: ft.is_dir(),
                modified_ms,
                created_ms,
            });
        }
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });
        Ok(entries)
    }

    /// Create a new empty file at `relpath`. Refuses to overwrite.
    ///
    /// Does not update the `SQLite` index; the storage watcher reconcile pass
    /// picks up the new empty file on its next sweep.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::PermissionDenied`] if `relpath` escapes the
    /// forge root, [`StorageError::WriteFailed`] if the file already exists,
    /// or [`StorageError::Io`] on other I/O failures.
    pub fn create_file(&self, relpath: &str) -> Result<(), StorageError> {
        let target = resolve_target(self.forge.root(), relpath)?;
        if target.exists() {
            return Err(StorageError::WriteFailed {
                path: relpath.to_string(),
                reason: "already exists".to_string(),
            });
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&target)?;
        Ok(())
    }

    /// Create a new empty directory at `relpath`. Refuses if it exists.
    ///
    /// # Errors
    ///
    /// See [`create_file`](Self::create_file).
    pub fn create_dir(&self, relpath: &str) -> Result<(), StorageError> {
        let target = resolve_target(self.forge.root(), relpath)?;
        if target.exists() {
            return Err(StorageError::WriteFailed {
                path: relpath.to_string(),
                reason: "already exists".to_string(),
            });
        }
        std::fs::create_dir(&target)?;
        Ok(())
    }

    /// Rename or move an entry within the forge. Both `from` and `to` must
    /// resolve under the forge root; `to` must not already exist.
    ///
    /// If `from` refers to an indexed file, its index row is updated to the
    /// new path. Directory renames are left for the watcher to reconcile.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::PermissionDenied`] if either path escapes the
    /// forge root, [`StorageError::WriteFailed`] if `to` already exists, or
    /// [`StorageError::Io`] on rename failure.
    ///
    /// # Panics
    ///
    /// Panics if the internal write-connection mutex is poisoned.
    pub fn rename_entry(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let src = resolve_within(self.forge.root(), from)?;
        let dst = resolve_target(self.forge.root(), to)?;
        if dst.exists() {
            return Err(StorageError::WriteFailed {
                path: to.to_string(),
                reason: "already exists".to_string(),
            });
        }
        std::fs::rename(&src, &dst)?;

        // If `from` was a regular file and was indexed, point its row at the
        // new path so searches/backlinks stay consistent without waiting for
        // a reconcile pass.
        if src.is_file() || dst.is_file() {
            let mut conn = self.write_conn.lock().expect("write_conn mutex poisoned");
            let tx = conn.transaction()?;
            let was_indexed = if let Some(record) = file_by_path(&tx, from)? {
                tx.execute(
                    "UPDATE files SET path = ?1 WHERE id = ?2;",
                    rusqlite::params![to, record.id.cast_signed()],
                )?;
                tx.execute(
                    "UPDATE fts_blocks SET file_path = ?1 WHERE file_path = ?2;",
                    rusqlite::params![to, from],
                )?;
                true
            } else {
                false
            };
            tx.commit()?;
            // Graph node path needs to move as well. Only re-key if the
            // file was actually indexed — otherwise add_note(to) would
            // create a node we don't want.
            if was_indexed {
                let mut g = self.graph.write().expect("graph lock poisoned");
                g.remove_note(from);
                g.add_note(to);
            }
        }
        Ok(())
    }

    /// Delete an entry within the forge. Handles both files and directories.
    ///
    /// Files are removed via [`delete_file`](Self::delete_file) (which also
    /// removes index + graph rows). Directories are removed recursively from
    /// disk; stale index entries under that prefix are soft-deleted so queries
    /// don't return rows for files the watcher hasn't yet reconciled.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::PermissionDenied`] if `relpath` escapes the
    /// forge root, or [`StorageError::Io`] on delete failure.
    ///
    /// # Panics
    ///
    /// Panics if the internal write-connection mutex is poisoned.
    pub fn delete_entry(&self, relpath: &str) -> Result<(), StorageError> {
        let target = resolve_within(self.forge.root(), relpath)?;
        let meta = std::fs::metadata(&target)?;
        if meta.is_dir() {
            std::fs::remove_dir_all(&target)?;
            // Best-effort index cleanup for files under this directory.
            // Wrapped in a tx so the fts_blocks DELETE and files
            // soft-delete are atomic — readers never observe one
            // applied without the other.
            let prefix = format!("{}/", relpath.trim_end_matches('/'));
            let mut conn = self.write_conn.lock().expect("write_conn mutex poisoned");
            let tx = conn.transaction()?;
            tx.execute(
                "DELETE FROM fts_blocks WHERE file_path LIKE ?1;",
                rusqlite::params![format!("{prefix}%")],
            )?;
            tx.execute(
                "UPDATE files SET is_deleted = 1 WHERE path LIKE ?1;",
                rusqlite::params![format!("{prefix}%")],
            )?;
            tx.commit()?;
            // Graph cleanup is deferred to the watcher reconcile pass — stale
            // nodes under the prefix only surface as unresolved links until then.
            Ok(())
        } else {
            self.delete_file(relpath)
        }
    }

    // ── Index queries ─────────────────────────────────────────────────────────

    /// Query the file index with optional prefix and type filters.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn query_files(&self, filter: &FileFilter) -> Result<Vec<FileRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        query_files(&conn, filter)
    }

    /// BL-114: query the code-symbol index. See
    /// [`code_index::SymbolFilter`].
    ///
    /// # Errors
    /// Returns [`StorageError::Database`] on any SQLite failure.
    pub fn query_symbols(
        &self,
        filter: &code_index::SymbolFilter,
    ) -> Result<Vec<code_index::SymbolRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        code_index::query_symbols(&conn, filter)
    }

    /// Return all blocks belonging to the file identified by `file_id`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn query_blocks(&self, file_id: u64) -> Result<Vec<BlockRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        query_blocks(&conn, file_id)
    }

    /// Return all blocks belonging to the file at `path`.
    ///
    /// Returns an empty `Vec` when the path is unknown to the index.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn query_blocks_by_path(&self, path: &str) -> Result<Vec<BlockRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        let Some(file) = file_by_path(&conn, path)? else {
            return Ok(Vec::new());
        };
        query_blocks(&conn, file.id)
    }

    /// Return all outgoing links from the file identified by `file_id`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn query_links(&self, file_id: u64) -> Result<Vec<LinkRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        query_links(&conn, file_id)
    }

    /// Return all backlinks pointing to the file identified by `file_id`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn query_backlinks(&self, file_id: u64) -> Result<Vec<LinkRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        query_backlinks(&conn, file_id)
    }

    /// Return all tags with `name`, joined to their originating file path.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn query_tags(&self, name: &str) -> Result<Vec<TagResult>, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        query_tags(&conn, name)
    }

    /// Plan a forge-to-forge import (BL-083). Walks `source_root`,
    /// classifies every file against the engine's destination forge
    /// root, and returns the resulting plan without touching disk.
    /// Use this for `--dry-run` reporting; pair with
    /// [`Self::apply_import`] to actually copy.
    ///
    /// # Errors
    /// Returns [`StorageError::Io`] for any IO failure inside the walk.
    pub fn plan_import(
        &self,
        source_root: &Path,
    ) -> Result<crate::import::ImportPlan, StorageError> {
        crate::import::plan_import(source_root, self.forge.root())
    }

    /// Apply a previously-prepared import plan to the engine's
    /// destination forge (BL-083). Caller typically reruns
    /// [`Self::rebuild_index`] afterwards so the destination index
    /// reflects the imported files.
    ///
    /// # Errors
    /// Returns [`StorageError::Io`] for any IO failure during copy.
    pub fn apply_import(
        &self,
        source_root: &Path,
        plan: &crate::import::ImportPlan,
        options: &crate::import::ImportOptions,
    ) -> Result<crate::import::ImportReport, StorageError> {
        crate::import::apply_import(source_root, self.forge.root(), plan, options)
    }

    /// Rebuild the `SQLite` index from scratch by reconciling the filesystem,
    /// then refresh the Tantivy search index from the new SQL state.
    ///
    /// Clears all index tables, runs a full reconciliation pass, then rebuilds
    /// the FTS index so search results match the rebuilt SQL state. Returns
    /// summary statistics for the SQL pass; the FTS rebuild is a slave step
    /// without separate stats today.
    ///
    /// The FTS rebuild is part of the contract — callers do NOT need to invoke
    /// [`rebuild_search_index`](Self::rebuild_search_index) afterwards. If the
    /// FTS rebuild fails the error is returned as-is; the SQL rebuild is
    /// already committed, so the user can retry safely (both passes are
    /// idempotent).
    ///
    /// # Errors
    /// Returns [`StorageError`] on I/O, database, or Tantivy failure.
    ///
    /// # Panics
    /// Panics if the internal write-connection mutex is poisoned.
    pub fn rebuild_index(&self) -> Result<RebuildStats, StorageError> {
        let start = std::time::Instant::now();
        let stats = {
            let conn = self.write_conn.lock().expect("write_conn mutex poisoned");

            // Clear all tables (FTS first, then files which cascades the rest).
            // BL-114: code_symbols is path-keyed, not foreign-keyed to files,
            // so it needs its own DELETE before the rebuild walk.
            conn.execute_batch(
                "DELETE FROM fts_blocks;
                 DELETE FROM code_symbols;
                 DELETE FROM files;",
            )?;

            // Run reconcile to re-index from disk.
            reconcile(&conn, self.forge.root())?;

            // Count what ended up in the DB.
            let files_processed: usize = usize::try_from(
                conn.query_row(
                    "SELECT COUNT(*) FROM files WHERE is_deleted = 0;",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0),
            )
            .unwrap_or(0);

            let blocks_indexed: usize = usize::try_from(
                conn.query_row("SELECT COUNT(*) FROM blocks;", [], |r| r.get::<_, i64>(0))
                    .unwrap_or(0),
            )
            .unwrap_or(0);

            let links_found: usize = usize::try_from(
                conn.query_row("SELECT COUNT(*) FROM links;", [], |r| r.get::<_, i64>(0))
                    .unwrap_or(0),
            )
            .unwrap_or(0);

            let tags_found: usize = usize::try_from(
                conn.query_row("SELECT COUNT(*) FROM tags;", [], |r| r.get::<_, i64>(0))
                    .unwrap_or(0),
            )
            .unwrap_or(0);

            RebuildStats {
                files_processed,
                blocks_indexed,
                links_found,
                tags_found,
                duration_ms: 0,
            }
            // write_conn lock drops here so rebuild_search_index can take its
            // own pool connection without contending with the writer.
        };

        // FTS would otherwise stay populated with documents pointing at the
        // pre-rebuild block ids — search would return stale or missing rows
        // until somebody remembered to call rebuild_search_index. Couple them.
        self.rebuild_search_index()?;

        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(RebuildStats {
            duration_ms,
            ..stats
        })
    }

    /// Incremental reconcile: sync the index against the filesystem without
    /// clearing existing data.
    ///
    /// Adds new files, updates changed ones, and soft-deletes removed ones.
    /// Cheaper than [`rebuild_index`] — use after a git batch-mode burst.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O or database failure.
    ///
    /// # Panics
    ///
    /// Panics if the internal write-connection mutex is poisoned.
    pub fn reconcile_index(&self) -> Result<ReconcileDelta, StorageError> {
        let conn = self.write_conn.lock().expect("write_conn mutex poisoned");
        reconcile(&conn, self.forge.root())
    }

    // ── Graph queries ────────────────────────────────────────────────────────

    /// Return all files that link to the file at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if the graph lock is poisoned.
    ///
    /// # Panics
    ///
    /// Panics if the internal graph `RwLock` is poisoned.
    pub fn backlinks(&self, path: &str) -> Result<Vec<graph::BacklinkResult>, StorageError> {
        let g = self.graph.read().expect("graph lock poisoned");
        Ok(g.backlinks(path))
    }

    /// Return inbound links to `path` whose fragment is the BL-049 block-anchored
    /// form `^<block_id>` (case-insensitive on the UUID).
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if the graph lock is poisoned.
    ///
    /// # Panics
    ///
    /// Panics if the internal graph `RwLock` is poisoned.
    pub fn backlinks_to_block(
        &self,
        path: &str,
        block_id: &str,
    ) -> Result<Vec<graph::BacklinkResult>, StorageError> {
        let g = self.graph.read().expect("graph lock poisoned");
        Ok(g.backlinks_to_block(path, block_id))
    }

    /// Return all outgoing links from the file at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if the graph lock is poisoned.
    ///
    /// # Panics
    ///
    /// Panics if the internal graph `RwLock` is poisoned.
    pub fn outgoing_links(&self, path: &str) -> Result<Vec<graph::OutgoingLink>, StorageError> {
        let g = self.graph.read().expect("graph lock poisoned");
        Ok(g.outgoing_links(path))
    }

    /// Return all unresolved (broken) links in the forge.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if the graph lock is poisoned.
    ///
    /// # Panics
    ///
    /// Panics if the internal graph `RwLock` is poisoned.
    pub fn unresolved_links(&self) -> Result<Vec<graph::UnresolvedLink>, StorageError> {
        let g = self.graph.read().expect("graph lock poisoned");
        Ok(g.unresolved_links())
    }

    /// Return knowledge graph statistics.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if the graph lock is poisoned.
    ///
    /// # Panics
    ///
    /// Panics if the internal graph `RwLock` is poisoned.
    pub fn graph_stats(&self) -> Result<graph::GraphStats, StorageError> {
        let g = self.graph.read().expect("graph lock poisoned");
        Ok(g.stats())
    }

    /// Return a flat snapshot of every node and edge in the knowledge graph.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if the graph lock is poisoned.
    ///
    /// # Panics
    ///
    /// Panics if the internal graph `RwLock` is poisoned.
    pub fn list_all_links(&self) -> Result<graph::GraphSnapshot, StorageError> {
        let g = self.graph.read().expect("graph lock poisoned");
        Ok(g.snapshot())
    }

    /// Return all files within `depth` hops of `path` in the knowledge graph.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if the graph lock is poisoned.
    ///
    /// # Panics
    ///
    /// Panics if the internal graph `RwLock` is poisoned.
    pub fn graph_neighbors(&self, path: &str, depth: usize) -> Result<Vec<String>, StorageError> {
        let g = self.graph.read().expect("graph lock poisoned");
        Ok(g.neighbors(path, depth))
    }

    // ── Tasks ─────────────────────────────────────────────────────────────

    /// Query tasks with optional filters.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn query_tasks(&self, filter: &TaskFilter) -> Result<Vec<TaskRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        tasks::query_tasks(&conn, filter)
    }

    /// Toggle a task's completion state in both the database and the source file.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on database or I/O failure.
    ///
    /// # Panics
    ///
    /// Panics if the internal write-connection mutex is poisoned.
    pub fn toggle_task(&self, task_id: u64) -> Result<TaskRecord, StorageError> {
        let conn = self.write_conn.lock().expect("write_conn mutex poisoned");
        let record = tasks::toggle_task(&conn, task_id)?;

        // Write back to file
        let abs_path = self.forge.root().join(&record.file_path);
        tasks::toggle_task_in_file(&abs_path, record.line_number, record.completed)?;

        Ok(record)
    }

    // ── Search ────────────────────────────────────────────────────────────────

    /// Search the Tantivy index for `query`, returning up to `limit` results.
    ///
    /// Supports scope operators: `tag:NAME`, `path:PREFIX`, `prop:KEY:VALUE`.
    /// Scopes are extracted from the query, Tantivy searches the remaining
    /// text, and results are post-filtered via `SQLite`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on Tantivy or database failure.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, StorageError> {
        let (text, filters) = search_scope::parse_scoped_query(query);

        // Run Tantivy on the plain-text portion.
        let results = if text.is_empty() {
            // Scope-only query: return all blocks up to limit (unscored).
            let conn = self.pool.get().map_err(|e| {
                StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
            })?;
            let mut stmt = conn.prepare(
                "SELECT f.path, b.id, b.block_type, b.content
                 FROM blocks b JOIN files f ON f.id = b.file_id
                 WHERE f.is_deleted = 0
                 ORDER BY f.path, b.start_line
                 LIMIT ?1;",
            )?;
            let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
            let rows = stmt.query_map(rusqlite::params![limit_i64], |row| {
                Ok(SearchResult {
                    file_path: row.get(0)?,
                    block_id: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
                    block_type: row.get(2)?,
                    excerpt: String::new(),
                    score: 0.0,
                })
            })?;
            rows.filter_map(std::result::Result::ok).collect()
        } else {
            self.search_index.search(&text, limit)?
        };

        // Post-filter with scopes if any.
        if filters.is_empty() {
            Ok(results)
        } else {
            let conn = self.pool.get().map_err(|e| {
                StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
            })?;
            search_scope::filter_results(&conn, results, &filters)
        }
    }

    /// Rebuild the Tantivy search index from the current `SQLite` state.
    ///
    /// Clears all Tantivy documents, then re-indexes every block from the
    /// `blocks` table.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on database or Tantivy failure.
    pub fn rebuild_search_index(&self) -> Result<(), StorageError> {
        self.search_index.clear()?;

        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

        // Iterate all blocks joined with files.
        let mut stmt = conn.prepare(
            "SELECT b.id, b.block_type, b.content, f.path
             FROM blocks b JOIN files f ON f.id = b.file_id
             WHERE f.is_deleted = 0;",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        for row in rows {
            let (block_id, block_type, content, file_path) = row?;
            self.search_index.add_block(
                &file_path,
                u64::try_from(block_id).unwrap_or(0),
                &block_type,
                &content,
            )?;
        }

        self.search_index.commit()?;
        Ok(())
    }

    // ── Obsidian `.base` (read-only) ──────────────────────────────────────────

    /// Read, parse, and evaluate an Obsidian single-file `.base`
    /// against the indexed vault. See ADR 0019.
    ///
    /// `path` is the forge-relative path to the `.base` file.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::CorruptFile`] when the `.base` file is
    /// missing or malformed, or [`StorageError::Database`] on `SQLite`
    /// failure.
    pub fn obsidian_base_query(
        &self,
        path: &str,
    ) -> Result<obsidian_base::ObsidianBaseQueryResult, StorageError> {
        let conn = self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        obsidian_base::query(&conn, self.forge.root(), path)
    }

    // ── Accessor ──────────────────────────────────────────────────────────────

    /// Return a reference to the underlying [`Forge`].
    #[must_use]
    pub fn forge(&self) -> &Forge {
        &self.forge
    }

    /// Get a read connection from the pool.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] if the pool is exhausted.
    pub fn pool_connection(
        &self,
    ) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>, StorageError> {
        self.pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })
    }

    // ── Vector store ──────────────────────────────────────────────────────────

    /// Upsert embeddings for a file (replaces all prior rows for that file).
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on transaction or insert failure.
    ///
    /// # Panics
    ///
    /// Panics if the internal write-connection mutex is poisoned.
    pub fn vector_insert(
        &self,
        file_path: &str,
        chunks: &[vectorstore::ChunkEmbedding],
    ) -> Result<(), StorageError> {
        let conn = self.write_conn.lock().expect("write_conn mutex poisoned");
        vectorstore::upsert(&conn, file_path, chunks)
    }

    /// Search the vector store for the `limit` most similar chunks.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if the underlying query fails.
    pub fn vector_query(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<vectorstore::ChunkMatch>, StorageError> {
        let conn = self.pool_connection()?;
        vectorstore::search(&conn, query_embedding, limit)
    }

    /// Delete every embedding row for `file_path`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if the delete statement fails.
    ///
    /// # Panics
    ///
    /// Panics if the internal write-connection mutex is poisoned.
    pub fn vector_delete_by_file(&self, file_path: &str) -> Result<(), StorageError> {
        let conn = self.write_conn.lock().expect("write_conn mutex poisoned");
        vectorstore::delete_by_file(&conn, file_path)
    }

    /// Count all stored embeddings.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if the count query fails.
    pub fn vectorstore_count(&self) -> Result<usize, StorageError> {
        let conn = self.pool_connection()?;
        vectorstore::count(&conn)
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Shared implementation for [`StorageEngine::init`] and [`StorageEngine::open`].
fn open_internal(
    forge: Forge,
    config: &StorageConfig,
    is_new: bool,
) -> Result<StorageEngine, StorageError> {
    // 1. Acquire exclusive forge lock.
    let lock = forge.acquire_lock()?;

    // 2. Clean stale temp files.
    forge.clean_temp()?;

    // 3. Create r2d2 connection pool.
    let db_path = forge.index_db_path();
    let manager = SqliteConnectionManager::file(&db_path);
    let pool = r2d2::Pool::builder()
        .max_size(config.pool_size)
        .build(manager)
        .map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;

    // 4. Configure pragmas and run migrations on a pool connection.
    {
        let conn = pool.get().map_err(|e| {
            StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string()))
        })?;
        schema::configure_pragmas(&conn)?;
        schema::migrate(&conn)?;
    }

    // 5. Open a separate write connection with pragmas.
    let write_conn = rusqlite::Connection::open(&db_path)?;
    schema::configure_pragmas(&write_conn)?;

    // 6. Open SearchIndex.
    let search_index = SearchIndex::open(&forge.search_dir())?;

    // (Watcher creation moved out of the engine in #80 — the engine
    // is now `Send + Sync` so `StorageCorePlugin` can hold it as
    // `Arc<StorageEngine>` and dispatch IPC handlers concurrently
    // without a per-call mutex. The plugin's `on_start` hook starts
    // the production watcher and moves it into a dedicated bridge
    // thread; the engine no longer needs its own.)

    // 8. If not new: run reconcile against write_conn.
    if !is_new {
        reconcile(&write_conn, forge.root())?;
    }

    // 9. Build knowledge graph from DB.
    let kg = if is_new {
        graph::KnowledgeGraph::new()
    } else {
        graph::KnowledgeGraph::rebuild_from_db(&write_conn)?
    };
    let graph = Arc::new(RwLock::new(kg));

    Ok(StorageEngine {
        forge,
        _lock: lock,
        pool,
        write_conn: Mutex::new(write_conn),
        search_index,
        graph,
    })
}

/// Resolve `relpath` against `root`, rejecting anything that escapes the root.
///
/// Thin wrapper over [`nexus_types::paths::resolve_within`] that maps the
/// shared `PathError` onto [`StorageError::PermissionDenied`].
///
/// Unlike `fs::canonicalize`, this does not require the target to exist —
/// callers that need an existing path should stat the result themselves.
fn resolve_within(root: &Path, relpath: &str) -> Result<std::path::PathBuf, StorageError> {
    nexus_types::paths::resolve_within(root, relpath)
        .map_err(|e| StorageError::PermissionDenied(e.to_string()))
}

/// Resolve a not-yet-existing target path. Same validation as [`resolve_within`]
/// plus a requirement that `relpath` name a file (non-empty filename).
fn resolve_target(root: &Path, relpath: &str) -> Result<std::path::PathBuf, StorageError> {
    if relpath.is_empty() {
        return Err(StorageError::PermissionDenied("empty relpath".to_string()));
    }
    let resolved = resolve_within(root, relpath)?;
    if resolved.file_name().is_none() {
        return Err(StorageError::PermissionDenied(format!(
            "missing filename: {relpath}"
        )));
    }
    Ok(resolved)
}

/// Infer a file-type string from a vault-relative path.
///
/// Pre-#84 the rule was strictly directory-based: anything under
/// `attachments/` was an `attachment`, anything else fell through to
/// `markdown` unless the extension matched `canvas` or `mdx`. That
/// misclassified an obvious case — a PDF dropped at the forge root
/// (or anywhere outside `attachments/`) would be tagged
/// `markdown` and the index would fail to render an attachment-type
/// preview. The fix prefers extension-based classification for
/// known binary attachment extensions before falling through to the
/// directory-based rule.
fn infer_file_type(path: &str) -> String {
    let p = Path::new(path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);

    // Specific text-shaped extensions classify regardless of where
    // the file lives.
    if let Some(ext) = ext.as_deref() {
        if ext == "canvas" {
            return "canvas".to_string();
        }
        if ext == "mdx" {
            return "mdx".to_string();
        }
        // Known binary/attachment extensions classify as `attachment`
        // even when the file is at the forge root or in some other
        // user-chosen directory. Issue #84.
        if matches!(
            ext,
            "pdf"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "svg"
                | "mp4"
                | "mov"
                | "webm"
                | "mp3"
                | "wav"
                | "ogg"
                | "zip"
                | "epub"
        ) {
            return "attachment".to_string();
        }
    }

    if path.starts_with("attachments/") {
        "attachment".to_string()
    } else {
        "markdown".to_string()
    }
}

#[cfg(test)]
mod infer_file_type_tests {
    use super::infer_file_type;

    #[test]
    fn attachments_dir_classifies_as_attachment() {
        assert_eq!(infer_file_type("attachments/img.png"), "attachment");
        assert_eq!(infer_file_type("attachments/some.unknown"), "attachment");
    }

    #[test]
    fn known_binary_extension_at_forge_root_is_attachment() {
        // Issue #84 regression — pre-fix `book.pdf` at the forge root
        // would classify as `markdown`.
        assert_eq!(infer_file_type("book.pdf"), "attachment");
        assert_eq!(infer_file_type("photo.png"), "attachment");
        assert_eq!(infer_file_type("clip.mp4"), "attachment");
    }

    #[test]
    fn canvas_and_mdx_classify_by_extension_anywhere() {
        assert_eq!(infer_file_type("notes/board.canvas"), "canvas");
        assert_eq!(infer_file_type("attachments/x.canvas"), "canvas");
        assert_eq!(infer_file_type("doc.mdx"), "mdx");
    }

    #[test]
    fn markdown_is_the_fallback() {
        assert_eq!(infer_file_type("notes/hello.md"), "markdown");
        assert_eq!(infer_file_type("notes/no-extension"), "markdown");
        assert_eq!(infer_file_type("daily/2026-04-12"), "markdown");
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        assert_eq!(infer_file_type("BOOK.PDF"), "attachment");
        assert_eq!(infer_file_type("BOARD.Canvas"), "canvas");
    }
}

/// Return the current Unix timestamp in seconds.
fn unix_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
    .unwrap_or(0)
}

fn system_time_to_ms(t: Option<std::time::SystemTime>) -> Option<i64> {
    use std::time::UNIX_EPOCH;
    t.and_then(|st| st.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok())
}

/// Pull the `"type"` string out of a property definition JSON value.
fn property_type(def: &serde_json::Value) -> Option<String> {
    def.get("type").and_then(|v| v.as_str()).map(str::to_string)
}

/// Best-effort coercion of a record value to the target property
/// type. Values that cannot be converted land as `null` rather than
/// producing garbage. The rules mirror the shell's cell editor
/// `coerce()` helper so round-trips (edit → migrate → edit) stay
/// stable.
fn coerce_property_value(value: &serde_json::Value, new_type: Option<&str>) -> serde_json::Value {
    use serde_json::Value;
    let Some(kind) = new_type else {
        return value.clone();
    };
    if value.is_null() {
        return Value::Null;
    }
    match kind {
        "number" | "currency" | "percent" => match value {
            Value::Number(_) => value.clone(),
            Value::String(s) => {
                if s.trim().is_empty() {
                    Value::Null
                } else {
                    s.parse::<f64>()
                        .ok()
                        .and_then(serde_json::Number::from_f64)
                        .map_or(Value::Null, Value::Number)
                }
            }
            Value::Bool(b) => Value::Number(i64::from(*b).into()),
            _ => Value::Null,
        },
        "checkbox" => match value {
            Value::Bool(_) => value.clone(),
            Value::String(s) => {
                let t = s.trim().to_ascii_lowercase();
                Value::Bool(matches!(t.as_str(), "true" | "1" | "yes" | "y" | "on"))
            }
            Value::Number(n) => Value::Bool(n.as_f64().is_some_and(|f| f != 0.0)),
            _ => Value::Bool(false),
        },
        "multi-select" => match value {
            Value::Array(_) => value.clone(),
            Value::String(s) if !s.is_empty() => Value::Array(
                s.split(',')
                    .map(|p| Value::String(p.trim().to_string()))
                    .filter(|v| !matches!(v, Value::String(s) if s.is_empty()))
                    .collect(),
            ),
            Value::Null => Value::Array(Vec::new()),
            _ => Value::Array(vec![value.clone()]),
        },
        "text" | "long-text" | "title" | "url" | "email" | "phone" | "select" | "date" | "time"
        | "datetime" => match value {
            Value::String(_) => value.clone(),
            Value::Null => Value::Null,
            Value::Bool(b) => Value::String(b.to_string()),
            Value::Number(n) => Value::String(n.to_string()),
            Value::Array(items) => Value::String(
                items
                    .iter()
                    .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            other @ Value::Object(_) => Value::String(other.to_string()),
        },
        _ => value.clone(),
    }
}

// ── Integration tests ─────────────────────────────────────────────────────────

// R8 / #191 — internal tests module lifted into `tests.rs` so the impl-block
// and helper sections in this file stay readable. The split is transparent —
// the test mod still has crate-private access via `use super::*;` in `tests.rs`.
#[cfg(test)]
mod tests;
