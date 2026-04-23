//! Nexus storage engine: forge layout, atomic writes, `SQLite` index,
//! markdown parsing, file watching, and Tantivy full-text search.
//!
//! See `docs/superpowers/specs/2026-04-12-nexus-prd-03-storage-design.md`
//! for the public contract this crate implements.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod error;
mod forge;
mod atomic;
pub mod schema;
mod parser;
mod index;
mod search;
mod watcher;
mod reconcile;
mod tasks;
mod graph;
mod search_scope;
mod export;
pub mod mdx;
mod canvas;
pub mod config;
pub mod bases;
pub mod core_plugin;
pub mod vectorstore;

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
pub use parser::{parse_markdown, ParsedBlock, ParsedFile, ParsedLink, ParsedTag, Property};
pub use tasks::{ParsedTask, TaskRecord, TaskFilter, insert_tasks, query_tasks, toggle_task, toggle_task_in_file};
pub use index::{BlockRecord, FileFilter, FileMetadata, FileRecord, LinkRecord, RebuildStats, TagResult};
pub use index::{insert_file, query_files, query_blocks, query_links, query_backlinks, query_tags, delete_file, soft_delete_file, file_by_path};
pub use search::{SearchIndex, SearchResult};
pub use search_scope::{CmpOp, PropertyOp, ScopeFilter, parse_scoped_query};
pub use reconcile::{ReconcileDelta, reconcile};
pub use watcher::{relative_path, should_ignore, StorageEvent, Watcher};
pub use graph::{KnowledgeGraph, BacklinkResult, OutgoingLink, UnresolvedLink, GraphStats, EdgeData};
pub use export::export_to_html;
pub use mdx::{ParsedJsxComponent, MdxParseResult, parse_mdx};
pub use index::{JsxRecord, insert_jsx_components, query_jsx_components};
pub use canvas::{
    CanvasFile, CanvasNode, CanvasNodeType, CanvasEdge, CanvasEdgeType,
    CanvasNodeRecord, CanvasEdgeRecord,
    CanvasPatchOp, CanvasPatchError,
    parse_canvas, serialize_canvas, apply_patch, extract_file_links,
};

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
    watcher: Option<watcher::Watcher>,
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
        // 1. Atomic write to disk.
        let abs_target = self.forge.root().join(path);
        atomic_write(&abs_target, content, &self.forge.temp_dir())?;

        // 2. Decode content as UTF-8.
        let text = std::str::from_utf8(content).map_err(|e| StorageError::CorruptFile {
            path: path.to_string(),
            reason: e.to_string(),
        })?;

        // 3. Lock write_conn.
        let conn = self.write_conn.lock().expect("write_conn mutex poisoned");

        // 4. Delete existing index entry if the path is already indexed.
        if let Some(existing) = file_by_path(&conn, path)? {
            conn.execute(
                "DELETE FROM fts_blocks WHERE file_path = ?1",
                rusqlite::params![path],
            )?;
            canvas::delete_canvas(&conn, existing.id.cast_signed())?;
            delete_file(&conn, existing.id)?;
        }

        let size_bytes = content.len() as u64;
        let file_type = infer_file_type(path);

        // 5. Branch by file type.
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
            let file_id = insert_file(&conn, path, &file_type, size_bytes, &empty_parsed)?;
            canvas::insert_canvas(&conn, file_id.cast_signed(), &canvas_data)?;

            // Update knowledge graph: file-type nodes create links.
            {
                let mut g = self.graph.write().expect("graph lock poisoned");
                g.add_note(path);
                g.remove_links_from(path);
                for target in canvas::extract_file_links(&canvas_data) {
                    g.add_link(path, &target, graph::EdgeData {
                        link_type: "canvas-embed".to_string(),
                        link_text: target.clone(),
                        fragment: None,
                    });
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

            let file_id = insert_file(&conn, path, &file_type, size_bytes, &parsed)?;

            if !jsx_components.is_empty() {
                insert_jsx_components(&conn, file_id, &jsx_components)?;
            }

            // Update knowledge graph.
            {
                let mut g = self.graph.write().expect("graph lock poisoned");
                g.add_note(path);
                g.remove_links_from(path);
                for link in &parsed.links {
                    let target = link.target_path.as_deref()
                        .unwrap_or(&link.link_text);
                    g.add_link(path, target, graph::EdgeData {
                        link_type: link.link_type.clone(),
                        link_text: link.link_text.clone(),
                        fragment: link.fragment.clone(),
                    });
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
        let abs_target = self.forge.root().join(path);
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
        let abs = self.forge.root().join(path);
        std::fs::read(&abs).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StorageError::FileNotFound(path.to_string())
            } else {
                StorageError::Io(e)
            }
        })
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
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
        canvas::query_canvas_nodes(&conn, file_id)
    }

    /// Query canvas edges for a file by its index ID.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn canvas_edges(&self, file_id: i64) -> Result<Vec<CanvasEdgeRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
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
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
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
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
        let Some(record) = file_by_path(&conn, path)? else {
            return Ok(Vec::new());
        };
        canvas::query_canvas_edges(&conn, record.id.cast_signed())
    }

    /// Serialize `canvas` and write it through [`Self::write_file`] so the
    /// SQLite canvas index + knowledge graph stay in sync.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on serialize, I/O, or database failure.
    pub fn write_canvas(&self, path: &str, canvas: &CanvasFile) -> Result<FileMetadata, StorageError> {
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
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
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
        schema: nexus_types::bases::BaseSchema,
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
        let mut base = nexus_types::bases::init_base(&abs_dir, &name, &schema)?;
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
            .ok_or_else(|| {
                StorageError::FileNotFound(format!("record {record_id} in {path}"))
            })?;

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
        definition: serde_json::Value,
        migrate_values: bool,
    ) -> Result<(), StorageError> {
        let abs_dir = self.forge.root().join(path);
        let mut base = nexus_types::bases::load_base(&abs_dir)?;
        let old_def = base.schema.fields.get(name).cloned().ok_or_else(|| {
            StorageError::FileNotFound(format!("property {name} in {path}"))
        })?;
        base.schema.fields.insert(name.to_string(), definition.clone());
        if migrate_values {
            let old_type = property_type(&old_def);
            let new_type = property_type(&definition);
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
        let def = base.schema.fields.remove(old_name).ok_or_else(|| {
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
            .ok_or_else(|| {
                StorageError::FileNotFound(format!("view {} in {path}", view.name))
            })?;
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

    /// Remove the record identified by `record_id` from the base at `path`.
    ///
    /// Hard-delete: the record is removed from `records.json` and the index
    /// is rebuilt. A dedicated soft-delete slot (`deleted_at`) on
    /// [`nexus_types::bases::BaseRecord`] is tracked as a separate backlog
    /// item — see the bases shell plan.
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
        // Remove from disk if it exists.
        let abs = self.forge.root().join(path);
        if abs.exists() {
            std::fs::remove_file(&abs)?;
        }

        // Remove from index.
        let conn = self.write_conn.lock().expect("write_conn mutex poisoned");
        if let Some(record) = file_by_path(&conn, path)? {
            conn.execute(
                "DELETE FROM fts_blocks WHERE file_path = ?1",
                rusqlite::params![path],
            )?;
            delete_file(&conn, record.id)?;
        }

        // Remove from graph.
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
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
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
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
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
    /// entries not yet in the SQLite index are included. The `.forge/`
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
        for entry in std::fs::read_dir(&target)? {
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
                Ok(md) => (system_time_to_ms(md.modified().ok()), system_time_to_ms(md.created().ok())),
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
    /// Does not update the SQLite index; the storage watcher reconcile pass
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
            let conn = self.write_conn.lock().expect("write_conn mutex poisoned");
            if let Some(record) = file_by_path(&conn, from)? {
                conn.execute(
                    "UPDATE files SET path = ?1 WHERE id = ?2;",
                    rusqlite::params![to, record.id.cast_signed()],
                )?;
                conn.execute(
                    "UPDATE fts_blocks SET file_path = ?1 WHERE file_path = ?2;",
                    rusqlite::params![to, from],
                )?;
                // Graph node path needs to move as well.
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
            let prefix = format!("{}/", relpath.trim_end_matches('/'));
            let conn = self.write_conn.lock().expect("write_conn mutex poisoned");
            conn.execute(
                "DELETE FROM fts_blocks WHERE file_path LIKE ?1;",
                rusqlite::params![format!("{prefix}%")],
            )?;
            conn.execute(
                "UPDATE files SET is_deleted = 1 WHERE path LIKE ?1;",
                rusqlite::params![format!("{prefix}%")],
            )?;
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
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
        query_files(&conn, filter)
    }

    /// Return all blocks belonging to the file identified by `file_id`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn query_blocks(&self, file_id: u64) -> Result<Vec<BlockRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
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
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
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
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
        query_links(&conn, file_id)
    }

    /// Return all backlinks pointing to the file identified by `file_id`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn query_backlinks(&self, file_id: u64) -> Result<Vec<LinkRecord>, StorageError> {
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
        query_backlinks(&conn, file_id)
    }

    /// Return all tags with `name`, joined to their originating file path.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Database`] on any `SQLite` failure.
    pub fn query_tags(&self, name: &str) -> Result<Vec<TagResult>, StorageError> {
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
        query_tags(&conn, name)
    }

    /// Rebuild the `SQLite` index from scratch by reconciling the filesystem.
    ///
    /// Clears all index tables, then runs a full reconciliation pass. Returns
    /// summary statistics.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O or database failure.
    ///
    /// # Panics
    ///
    /// Panics if the internal write-connection mutex is poisoned.
    pub fn rebuild_index(&self) -> Result<RebuildStats, StorageError> {
        let start = std::time::Instant::now();
        let conn = self.write_conn.lock().expect("write_conn mutex poisoned");

        // Clear all tables (FTS first, then files which cascades the rest).
        conn.execute_batch(
            "DELETE FROM fts_blocks;
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

        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        Ok(RebuildStats {
            files_processed,
            blocks_indexed,
            links_found,
            tags_found,
            duration_ms,
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
        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
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
            let conn = self.pool.get().map_err(|e| StorageError::Database(
                rusqlite::Error::InvalidParameterName(e.to_string()),
            ))?;
            let mut stmt = conn.prepare(
                "SELECT f.path, b.id, b.block_type, b.content
                 FROM blocks b JOIN files f ON f.id = b.file_id
                 WHERE f.is_deleted = 0
                 ORDER BY f.path, b.start_line
                 LIMIT ?1;"
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
            let conn = self.pool.get().map_err(|e| StorageError::Database(
                rusqlite::Error::InvalidParameterName(e.to_string()),
            ))?;
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

        let conn = self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;

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

    // ── Watcher ───────────────────────────────────────────────────────────────

    /// Return the watcher event receiver, if the watcher started successfully.
    #[must_use]
    pub fn watch_changes(&self) -> Option<&std::sync::mpsc::Receiver<StorageEvent>> {
        self.watcher.as_ref().map(watcher::Watcher::events)
    }

    // ── Watcher Reconcile ────────────────────────────────────────────────────

    /// Process pending file watcher events, re-indexing changed files.
    ///
    /// Drains all pending events from the watcher (non-blocking). For each event:
    /// - `FileCreated`/`FileModified`: re-reads from disk and re-indexes
    /// - `FileDeleted`: removes from index and graph
    /// - `FileRenamed`: removes old path, indexes new path
    ///
    /// Returns the number of events processed.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] on I/O or database failure.
    pub fn process_watcher_events(&self) -> Result<usize, StorageError> {
        let rx = match self.watcher.as_ref() {
            Some(w) => w.events(),
            None => return Ok(0),
        };

        let mut count = 0;
        while let Ok(event) = rx.try_recv() {
            match &event {
                StorageEvent::FileCreated { path, .. }
                | StorageEvent::FileModified { path, .. } => {
                    let abs = self.forge.root().join(path);
                    if let Ok(bytes) = std::fs::read(&abs) {
                        let _ = self.write_file(path, &bytes);
                    }
                }
                StorageEvent::FileDeleted { path } => {
                    let _ = self.delete_file(path);
                }
                StorageEvent::FileRenamed { from, to, .. } => {
                    let _ = self.delete_file(from);
                    let abs = self.forge.root().join(to);
                    if let Ok(bytes) = std::fs::read(&abs) {
                        let _ = self.write_file(to, &bytes);
                    }
                }
            }
            count += 1;
        }

        Ok(count)
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
    pub fn pool_connection(&self) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>, StorageError> {
        self.pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))
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
        .map_err(|e| StorageError::Database(rusqlite::Error::InvalidParameterName(e.to_string())))?;

    // 4. Configure pragmas and run migrations on a pool connection.
    {
        let conn = pool.get().map_err(|e| StorageError::Database(
            rusqlite::Error::InvalidParameterName(e.to_string()),
        ))?;
        schema::configure_pragmas(&conn)?;
        schema::migrate(&conn)?;
    }

    // 5. Open a separate write connection with pragmas.
    let write_conn = rusqlite::Connection::open(&db_path)?;
    schema::configure_pragmas(&write_conn)?;

    // 6. Open SearchIndex.
    let search_index = SearchIndex::open(&forge.search_dir())?;

    // 7. Start file watcher (best-effort).
    let watcher = Watcher::start(forge.root(), config.debounce_ms).ok();

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
        watcher,
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
        return Err(StorageError::PermissionDenied(
            "empty relpath".to_string(),
        ));
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
fn infer_file_type(path: &str) -> String {
    if path.starts_with("attachments/") {
        "attachment".to_string()
    } else if Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("canvas"))
    {
        "canvas".to_string()
    } else if Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mdx"))
    {
        "mdx".to_string()
    } else {
        "markdown".to_string()
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
    def.get("type")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Best-effort coercion of a record value to the target property
/// type. Values that cannot be converted land as `null` rather than
/// producing garbage. The rules mirror the shell's cell editor
/// `coerce()` helper so round-trips (edit → migrate → edit) stay
/// stable.
fn coerce_property_value(
    value: &serde_json::Value,
    new_type: Option<&str>,
) -> serde_json::Value {
    use serde_json::Value;
    let Some(kind) = new_type else { return value.clone() };
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
            Value::Bool(b) => Value::Number((*b as i64).into()),
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
        "text" | "long-text" | "title" | "url" | "email" | "phone" | "select"
        | "date" | "time" | "datetime" => match value {
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
            other => Value::String(other.to_string()),
        },
        _ => value.clone(),
    }
}

// ── Integration tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // ── 1. init_creates_working_engine ────────────────────────────────────────

    #[test]
    fn init_creates_working_engine() {
        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");

        assert!(
            engine.forge().forge_dir().join("index.db").exists(),
            ".forge/index.db should exist"
        );
        assert!(
            engine.forge().notes_dir().exists(),
            "notes/ should exist"
        );
    }

    // ── 2. write_and_read_file ────────────────────────────────────────────────

    #[test]
    fn write_and_read_file() {
        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");

        let content = b"# Hello\n\nWorld paragraph.";
        engine.write_file("notes/hello.md", content).expect("write");

        let read_back = engine.read_file("notes/hello.md").expect("read");
        assert_eq!(read_back, content);
    }

    // ── 3. write_file_is_indexed ──────────────────────────────────────────────

    #[test]
    fn write_file_is_indexed() {
        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");

        engine
            .write_file("notes/indexed.md", b"# Indexed\n\nContent.")
            .expect("write");

        assert!(engine.file_exists("notes/indexed.md").expect("file_exists"));
        let files = engine.list_files("notes/").expect("list_files");
        assert_eq!(files.len(), 1);
    }

    // ── 4. delete_file_removes_from_index ────────────────────────────────────

    #[test]
    fn delete_file_removes_from_index() {
        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");

        engine
            .write_file("notes/gone.md", b"# Gone\n\nBye.")
            .expect("write");
        assert!(engine.file_exists("notes/gone.md").expect("file_exists"));

        engine.delete_file("notes/gone.md").expect("delete");

        assert!(!engine.file_exists("notes/gone.md").expect("file_exists"));
        assert!(
            !dir.path().join("notes/gone.md").exists(),
            "file should be removed from disk"
        );
    }

    // ── 5. query_blocks_after_write ───────────────────────────────────────────

    #[test]
    fn query_blocks_after_write() {
        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");

        engine
            .write_file("notes/blocks.md", b"# Title\n\nParagraph text.")
            .expect("write");

        let files = engine.list_files("notes/").expect("list_files");
        assert_eq!(files.len(), 1);

        // Get the file record to obtain the file ID.
        let filter = FileFilter::default();
        let records = engine.query_files(&filter).expect("query_files");
        assert_eq!(records.len(), 1);

        let blocks = engine.query_blocks(records[0].id).expect("query_blocks");
        assert!(
            blocks.len() >= 2,
            "expected >= 2 blocks, got {}",
            blocks.len()
        );
    }

    // ── 6. query_tags_after_write ─────────────────────────────────────────────

    #[test]
    fn query_tags_after_write() {
        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");

        engine
            .write_file("notes/tagged.md", b"# Tagged\n\nThis has #rust tag.")
            .expect("write");

        let tags = engine.query_tags("rust").expect("query_tags");
        assert_eq!(tags.len(), 1, "expected 1 tag result for 'rust', got {}", tags.len());
    }

    // ── 7. rebuild_index_reindexes_all ────────────────────────────────────────

    #[test]
    fn rebuild_index_reindexes_all() {
        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");

        engine
            .write_file("notes/a.md", b"# Alpha\n\nContent A.")
            .expect("write a");
        engine
            .write_file("notes/b.md", b"# Beta\n\nContent B.")
            .expect("write b");

        let stats = engine.rebuild_index().expect("rebuild_index");
        assert_eq!(
            stats.files_processed, 2,
            "expected 2 files_processed, got {}",
            stats.files_processed
        );
    }

    // ── 8. read_nonexistent_file_returns_error ────────────────────────────────

    #[test]
    fn read_nonexistent_file_returns_error() {
        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");

        let result = engine.read_file("notes/nonexistent.md");
        assert!(
            matches!(result, Err(StorageError::FileNotFound(_))),
            "expected FileNotFound, got: {result:?}"
        );
    }

    // ── 9. write_raw_bypasses_index ───────────────────────────────────────────

    #[test]
    fn write_raw_bypasses_index() {
        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");

        // Write a markdown-like file via write_raw to a path the index would
        // normally pick up. Contains a tag that would show up in query_tags
        // if the indexing pipeline ran.
        let rel = ".forge/workspace.json";
        let content = b"# Raw\n\nHas a #rawtag inside.";
        engine.write_raw(rel, content).expect("write_raw");

        // Bytes are on disk, exactly as written.
        let abs = dir.path().join(rel);
        assert!(abs.exists(), "file must exist on disk after write_raw");
        assert_eq!(
            std::fs::read(&abs).expect("read back"),
            content,
            "disk content must match bytes passed to write_raw"
        );

        // Index must NOT have picked up the file: no row in the files table,
        // no tag inserted, no graph node created. Contrast with write_file
        // which always runs the full pipeline (see write_and_read_file test).
        assert!(
            !engine.file_exists(rel).expect("file_exists"),
            "write_raw must not insert an index row"
        );
        let tags = engine.query_tags("rawtag").expect("query_tags");
        assert!(
            tags.is_empty(),
            "write_raw must not index tags, got {tags:?}"
        );
        let stats = engine.graph_stats().expect("graph_stats");
        assert_eq!(
            stats.node_count, 0,
            "write_raw must not add graph nodes, got {} nodes",
            stats.node_count
        );
    }

    // ── 10. canvas_write_read_patch_roundtrip ─────────────────────────────────

    #[test]
    fn canvas_write_read_patch_roundtrip() {
        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");

        let mut initial = CanvasFile::default();
        initial.nodes.push(CanvasNode {
            id: "a".to_string(),
            node_type: CanvasNodeType::Text,
            x: 0.0, y: 0.0, width: 100.0, height: 100.0,
            color: None, label: None, collapsed: false,
            file: None, text: Some("hi".to_string()),
            url: None, source: None, command: None, extra: serde_json::Map::new(),
        });

        engine
            .write_canvas("boards/one.canvas", &initial)
            .expect("write_canvas");

        let nodes = engine.canvas_nodes_by_path("boards/one.canvas").expect("nodes");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_id, "a");

        engine
            .patch_canvas(
                "boards/one.canvas",
                &[CanvasPatchOp::NodeMove { id: "a".to_string(), x: 42.0, y: 7.0 }],
            )
            .expect("patch_canvas");

        let parsed = engine.read_canvas("boards/one.canvas").expect("read_canvas");
        assert!((parsed.nodes[0].x - 42.0).abs() < f64::EPSILON);
        assert!((parsed.nodes[0].y - 7.0).abs() < f64::EPSILON);

        let after = engine.canvas_nodes_by_path("boards/one.canvas").expect("nodes2");
        assert_eq!(after.len(), 1);
        assert!((after[0].x - 42.0).abs() < f64::EPSILON);
    }

    // ── 11. canvas_queries_by_path_on_missing_return_empty ────────────────────

    #[test]
    fn canvas_queries_by_path_on_missing_return_empty() {
        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");
        assert!(engine.canvas_nodes_by_path("nope.canvas").expect("nodes").is_empty());
        assert!(engine.canvas_edges_by_path("nope.canvas").expect("edges").is_empty());
    }

    // ── 12. base_record_crud_roundtrip ────────────────────────────────────────

    #[test]
    fn base_record_crud_roundtrip() {
        use nexus_types::bases::{Base, BaseRecord, BaseSchema, BaseMetadata};

        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");

        let base_rel = "tasks.bases";
        let abs = dir.path().join(base_rel);
        let mut fields = serde_json::Map::new();
        fields.insert(
            "title".to_string(),
            serde_json::json!({ "type": "title", "required": true }),
        );
        let seed = Base {
            name: "Tasks".to_string(),
            schema: BaseSchema { version: "1.0".to_string(), fields },
            records: Vec::new(),
            views: Vec::new(),
            relations: Vec::new(),
            metadata: BaseMetadata::default(),
        };
        nexus_types::bases::save_base(&abs, &seed).expect("save seed");
        engine.index_base(base_rel, &seed).expect("index seed");

        // Create with server-generated id.
        let created = engine
            .base_record_create(
                base_rel,
                BaseRecord {
                    id: String::new(),
                    fields: {
                        let mut m = serde_json::Map::new();
                        m.insert("title".to_string(), serde_json::json!("Buy milk"));
                        m
                    },
                },
            )
            .expect("create");
        assert!(!created.id.is_empty(), "id should be generated");
        let created_id = created.id.clone();

        // Update — patch one field.
        let patch = {
            let mut m = serde_json::Map::new();
            m.insert("title".to_string(), serde_json::json!("Buy oat milk"));
            m
        };
        let updated = engine
            .base_record_update(base_rel, &created_id, &patch)
            .expect("update");
        assert_eq!(updated.fields.get("title").unwrap(), "Buy oat milk");

        // Re-read from disk to confirm round-trip.
        let reloaded = nexus_types::bases::load_base(&abs).expect("load");
        assert_eq!(reloaded.records.len(), 1);
        assert_eq!(reloaded.records[0].fields.get("title").unwrap(), "Buy oat milk");

        // Delete.
        engine.base_record_delete(base_rel, &created_id).expect("delete");
        let reloaded = nexus_types::bases::load_base(&abs).expect("load2");
        assert!(reloaded.records.is_empty());

        // Delete again — idempotent no-op.
        engine.base_record_delete(base_rel, &created_id).expect("delete noop");
    }

    // ── 13. base_record_create_rejects_duplicate_id ───────────────────────────

    #[test]
    fn base_record_create_rejects_duplicate_id() {
        use nexus_types::bases::{Base, BaseRecord, BaseSchema, BaseMetadata};

        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");

        let base_rel = "d.bases";
        let seed = Base {
            name: "D".to_string(),
            schema: BaseSchema { version: "1.0".to_string(), fields: serde_json::Map::new() },
            records: vec![BaseRecord { id: "r1".into(), fields: serde_json::Map::new() }],
            views: Vec::new(),
            relations: Vec::new(),
            metadata: BaseMetadata::default(),
        };
        nexus_types::bases::save_base(&dir.path().join(base_rel), &seed).expect("save");
        engine.index_base(base_rel, &seed).expect("index");

        let err = engine
            .base_record_create(
                base_rel,
                BaseRecord { id: "r1".into(), fields: serde_json::Map::new() },
            )
            .expect_err("duplicate should fail");
        assert!(matches!(err, StorageError::CorruptFile { .. }));
    }

    // ── 14. base_record_update_unknown_id_errors ──────────────────────────────

    #[test]
    fn base_record_update_unknown_id_errors() {
        use nexus_types::bases::{Base, BaseSchema, BaseMetadata};

        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");

        let base_rel = "u.bases";
        let seed = Base {
            name: "U".to_string(),
            schema: BaseSchema { version: "1.0".to_string(), fields: serde_json::Map::new() },
            records: Vec::new(),
            views: Vec::new(),
            relations: Vec::new(),
            metadata: BaseMetadata::default(),
        };
        nexus_types::bases::save_base(&dir.path().join(base_rel), &seed).expect("save");
        engine.index_base(base_rel, &seed).expect("index");

        let err = engine
            .base_record_update(base_rel, "ghost", &serde_json::Map::new())
            .expect_err("unknown id should fail");
        assert!(matches!(err, StorageError::FileNotFound(_)));
    }

    // ── 15. base_property_crud ────────────────────────────────────────────────

    #[test]
    fn base_property_crud() {
        use nexus_types::bases::{Base, BaseRecord, BaseSchema, BaseMetadata};

        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");
        let base_rel = "p.bases";
        let abs = dir.path().join(base_rel);
        let seed = Base {
            name: "P".to_string(),
            schema: BaseSchema {
                version: "1.0".to_string(),
                fields: {
                    let mut m = serde_json::Map::new();
                    m.insert("legacy".to_string(), serde_json::json!({ "type": "text" }));
                    m
                },
            },
            records: vec![BaseRecord {
                id: "r1".into(),
                fields: {
                    let mut m = serde_json::Map::new();
                    m.insert("legacy".to_string(), serde_json::json!("stale"));
                    m
                },
            }],
            views: Vec::new(),
            relations: Vec::new(),
            metadata: BaseMetadata::default(),
        };
        nexus_types::bases::save_base(&abs, &seed).expect("save");
        engine.index_base(base_rel, &seed).expect("index");

        // Create.
        engine
            .base_property_create(base_rel, "title", serde_json::json!({ "type": "title" }))
            .expect("create");
        let loaded = nexus_types::bases::load_base(&abs).expect("load");
        assert!(loaded.schema.fields.contains_key("title"));

        // Duplicate create → error.
        let err = engine
            .base_property_create(base_rel, "title", serde_json::json!({ "type": "text" }))
            .expect_err("dup");
        assert!(matches!(err, StorageError::CorruptFile { .. }));

        // Update.
        engine
            .base_property_update(base_rel, "title", serde_json::json!({ "type": "text", "required": true }), false)
            .expect("update");
        let loaded = nexus_types::bases::load_base(&abs).expect("load2");
        assert_eq!(
            loaded.schema.fields["title"].get("required"),
            Some(&serde_json::Value::Bool(true))
        );

        // Update unknown → error.
        let err = engine
            .base_property_update(base_rel, "nope", serde_json::json!({}), false)
            .expect_err("unknown");
        assert!(matches!(err, StorageError::FileNotFound(_)));

        // Delete drops record key.
        engine.base_property_delete(base_rel, "legacy").expect("delete legacy");
        let loaded = nexus_types::bases::load_base(&abs).expect("load3");
        assert!(!loaded.records[0].fields.contains_key("legacy"));

        // Delete unknown → no-op.
        engine.base_property_delete(base_rel, "ghost").expect("delete ghost");
    }

    // ── 15b. base_create + property rename + retype-migration ─────────────────

    #[test]
    fn base_create_and_property_rename_retype() {
        use nexus_types::bases::BaseSchema;

        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");
        let base_rel = "new.bases";

        let schema = BaseSchema {
            version: "1.0".to_string(),
            fields: {
                let mut m = serde_json::Map::new();
                m.insert(
                    "title".to_string(),
                    serde_json::json!({ "type": "title", "required": true, "primary": true }),
                );
                m.insert("count".to_string(), serde_json::json!({ "type": "number" }));
                m
            },
        };

        // base_create — empty.
        let created = engine
            .base_create(base_rel, schema.clone(), Vec::new())
            .expect("create");
        assert_eq!(created.name, "new");
        assert_eq!(created.records.len(), 0);

        // Seed a record with a numeric value, then retype count → text
        // with migration and verify it serialized as a string.
        let record = nexus_types::bases::BaseRecord {
            id: String::new(),
            fields: {
                let mut m = serde_json::Map::new();
                m.insert("title".to_string(), serde_json::json!("Hello"));
                m.insert("count".to_string(), serde_json::json!(42));
                m
            },
        };
        let stored = engine.base_record_create(base_rel, record).expect("record");
        assert!(!stored.id.is_empty());

        engine
            .base_property_update(
                base_rel,
                "count",
                serde_json::json!({ "type": "text" }),
                true,
            )
            .expect("retype with migrate");
        let abs = dir.path().join(base_rel);
        let loaded = nexus_types::bases::load_base(&abs).expect("load1");
        assert_eq!(loaded.records[0].fields["count"], serde_json::json!("42"));

        // Rename column → schema key moves and record field key moves.
        engine
            .base_property_rename(base_rel, "count", "total")
            .expect("rename");
        let loaded = nexus_types::bases::load_base(&abs).expect("load2");
        assert!(loaded.schema.fields.contains_key("total"));
        assert!(!loaded.schema.fields.contains_key("count"));
        assert_eq!(loaded.records[0].fields["total"], serde_json::json!("42"));
        assert!(!loaded.records[0].fields.contains_key("count"));

        // Rename collision → error.
        let err = engine
            .base_property_rename(base_rel, "total", "title")
            .expect_err("collision");
        assert!(matches!(err, StorageError::CorruptFile { .. }));

        // base_create on existing path → error.
        let err = engine
            .base_create(base_rel, schema, Vec::new())
            .expect_err("exists");
        assert!(matches!(err, StorageError::CorruptFile { .. }));
    }

    // ── 16. base_view_crud ────────────────────────────────────────────────────

    #[test]
    fn base_view_crud() {
        use nexus_types::bases::{Base, BaseSchema, BaseMetadata, BaseView, ViewType};

        let dir = tmp();
        let engine = StorageEngine::init(dir.path()).expect("init");
        let base_rel = "v.bases";
        let abs = dir.path().join(base_rel);
        let seed = Base {
            name: "V".to_string(),
            schema: BaseSchema { version: "1.0".to_string(), fields: serde_json::Map::new() },
            records: Vec::new(),
            views: Vec::new(),
            relations: Vec::new(),
            metadata: BaseMetadata::default(),
        };
        nexus_types::bases::save_base(&abs, &seed).expect("save");
        engine.index_base(base_rel, &seed).expect("index");

        let board = BaseView {
            name: "Board".to_string(),
            view_type: ViewType::Kanban,
            fields: vec!["title".to_string()],
            sort: Vec::new(),
            filter: Vec::new(),
            group_field: Some("status".to_string()),
            date_field: None,
        };
        engine.base_view_create(base_rel, board.clone()).expect("create");
        let loaded = nexus_types::bases::load_base(&abs).expect("load");
        assert_eq!(loaded.views.len(), 1);
        assert_eq!(loaded.views[0].name, "Board");

        let err = engine.base_view_create(base_rel, board.clone()).expect_err("dup");
        assert!(matches!(err, StorageError::CorruptFile { .. }));

        let mut updated = board.clone();
        updated.group_field = Some("priority".to_string());
        engine.base_view_update(base_rel, updated).expect("update");
        let loaded = nexus_types::bases::load_base(&abs).expect("load2");
        assert_eq!(loaded.views[0].group_field.as_deref(), Some("priority"));

        let ghost = BaseView { name: "Ghost".to_string(), ..board };
        let err = engine.base_view_update(base_rel, ghost).expect_err("unknown");
        assert!(matches!(err, StorageError::FileNotFound(_)));

        engine.base_view_delete(base_rel, "Board").expect("delete");
        let loaded = nexus_types::bases::load_base(&abs).expect("load3");
        assert!(loaded.views.is_empty());

        engine.base_view_delete(base_rel, "noop").expect("noop delete");
    }

    // ── 17. open_nonexistent_forge_returns_error ──────────────────────────────

    #[test]
    fn open_nonexistent_forge_returns_error() {
        let dir = tmp();
        let result = StorageEngine::open(dir.path(), &StorageConfig::default());
        assert!(
            matches!(result, Err(StorageError::FileNotFound(_))),
            "expected FileNotFound, got: {result:?}"
        );
    }
}
