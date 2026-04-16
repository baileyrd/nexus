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
}
pub use parser::{parse_markdown, ParsedBlock, ParsedFile, ParsedLink, ParsedTag, Property};
pub use tasks::{ParsedTask, TaskRecord, TaskFilter, insert_tasks, query_tasks, toggle_task, toggle_task_in_file};
pub use index::{BlockRecord, FileFilter, FileMetadata, FileRecord, LinkRecord, RebuildStats, TagResult};
pub use index::{insert_file, query_files, query_blocks, query_links, query_backlinks, query_tags, delete_file, soft_delete_file, file_by_path};
pub use search::{SearchIndex, SearchResult};
pub use search_scope::{ScopeFilter, parse_scoped_query};
pub use reconcile::{ReconcileDelta, reconcile};
pub use watcher::{relative_path, should_ignore, StorageEvent, Watcher};
pub use graph::{KnowledgeGraph, BacklinkResult, OutgoingLink, UnresolvedLink, GraphStats, EdgeData};
pub use export::export_to_html;
pub use mdx::{ParsedJsxComponent, MdxParseResult, parse_mdx};
pub use index::{JsxRecord, insert_jsx_components, query_jsx_components};
pub use canvas::{
    CanvasFile, CanvasNode, CanvasNodeType, CanvasEdge, CanvasEdgeType,
    CanvasNodeRecord, CanvasEdgeRecord,
    parse_canvas, serialize_canvas, extract_file_links,
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
            entries.push(TreeEntry {
                name,
                relpath: rel,
                is_dir: ft.is_dir(),
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
/// Accepts only [`Path::Component::Normal`] components; absolute paths,
/// `..`, root dirs, and Windows drive prefixes are all rejected. An empty
/// `relpath` resolves to the root itself.
///
/// Unlike `fs::canonicalize`, this does not require the target to exist —
/// callers that need an existing path should stat the result themselves.
fn resolve_within(root: &Path, relpath: &str) -> Result<std::path::PathBuf, StorageError> {
    use std::path::Component;
    if relpath.is_empty() {
        return Ok(root.to_path_buf());
    }
    let rel = Path::new(relpath);
    for c in rel.components() {
        match c {
            Component::Normal(_) => {}
            _ => {
                return Err(StorageError::PermissionDenied(format!(
                    "invalid relpath: {relpath}"
                )));
            }
        }
    }
    Ok(root.join(rel))
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

    // ── 9. open_nonexistent_forge_returns_error ────────────────────────────────

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
