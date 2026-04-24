//! Stable IPC arg/return types for the Phase 4 WI-36 pilot handlers.
//!
//! These types are the **authoritative contract** for the 5 pilot kernel IPC
//! commands exposed by `com.nexus.storage`:
//!
//! - `search`      — [`StorageSearchArgs`]  -> [`StorageSearchResult`]
//! - `read_file`   — [`StorageReadFileArgs`] -> [`StorageReadFileResult`]
//! - `write_file`  — [`StorageWriteFileArgs`] -> [`StorageWriteFileResult`]
//! - `list_dir`    — [`StorageListDirArgs`] -> [`StorageListDirResult`]
//!
//! Under the `ts-export` feature each type emits a TypeScript binding into
//! `packages/nexus-extension-api/src/generated/ipc/` and (via the
//! `nexus-bootstrap` harness) a JSON Schema into
//! `crates/nexus-bootstrap/schemas/ipc/`.
//!
//! Remaining 47+ handlers migrate to this pattern opportunistically in v1.1
//! per `docs/planning/PHASE-4-IMPLEMENTATION-PLAN.md` §3.1.
//!
//! # Why mirror types instead of deriving on existing types?
//!
//! The current dispatch (`core_plugin.rs`) decodes args inline from
//! `serde_json::Value` — there are no existing named arg structs. Rather than
//! refactor the dispatch path (out of scope for the taxonomy pilot), this
//! module hand-authors `serde`-compatible types whose shapes match what the
//! dispatch decodes today. Any drift becomes visible the moment a real handler
//! is wired through the schema consumer, at which point the dispatch can be
//! refactored to call `parse_args::<StorageSearchArgs>(…)` directly.
//!
//! Return types mirror existing public structs (e.g. `SearchResult`,
//! `FileMetadata`, `TreeEntry`) field-for-field; the mirrors carry the
//! `#[derive(TS, JsonSchema)]` attributes while leaving the originals alone
//! so downstream crates that consume them are not forced to pull `ts-rs` /
//! `schemars` into their default builds.

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

// ── com.nexus.storage::search ────────────────────────────────────────────────

/// Args for `com.nexus.storage::search` (handler id `7`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
pub struct StorageSearchArgs {
    /// Full-text query string (Tantivy syntax).
    pub query: String,
    /// Maximum number of results to return. Omit for the default of 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// One hit in [`StorageSearchResult::results`]. Mirror of
/// [`crate::SearchResult`] — kept in sync manually; compared via
/// `cargo test -p nexus-bootstrap --test ipc_schema_emit`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
pub struct StorageSearchHit {
    /// Path to the file containing the matching block.
    pub file_path: String,
    /// Unique ID of the matching block.
    pub block_id: u64,
    /// Type of the matching block (e.g. `"paragraph"`, `"heading"`).
    pub block_type: String,
    /// Excerpt of the matching content (may be empty).
    pub excerpt: String,
    /// BM25 relevance score.
    pub score: f32,
}

/// Return type for `com.nexus.storage::search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
pub struct StorageSearchResult {
    /// Ordered (descending by score) list of hits.
    pub results: Vec<StorageSearchHit>,
}

// ── com.nexus.storage::read_file ─────────────────────────────────────────────

/// Args for `com.nexus.storage::read_file` (handler id `2`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
pub struct StorageReadFileArgs {
    /// Forge-relative path of the file to read.
    pub path: String,
}

/// Return type for `com.nexus.storage::read_file`.
///
/// `bytes` is `null` when the file does not exist — the dispatch uses this
/// to distinguish a missing file from a genuine failure without collapsing
/// into `PluginCrashedDuringCall`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
pub struct StorageReadFileResult {
    /// Raw bytes of the file content, or `null` if the file does not exist.
    pub bytes: Option<Vec<u8>>,
}

// ── com.nexus.storage::write_file ────────────────────────────────────────────

/// Args for `com.nexus.storage::write_file` (handler id `8`).
///
/// Security posture: Phase 3 WI-32 hardened this handler. Writes are confined
/// to the forge root; path traversal attempts are rejected at the engine
/// boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
pub struct StorageWriteFileArgs {
    /// Forge-relative path of the file to write.
    pub path: String,
    /// Raw bytes to write. Atomic write semantics apply per
    /// [`crate::atomic_write`].
    pub bytes: Vec<u8>,
}

/// Return type for `com.nexus.storage::write_file`. Mirror of
/// [`crate::FileMetadata`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
pub struct StorageWriteFileResult {
    /// Vault-relative path.
    pub path: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Unix timestamp of last modification.
    pub modified_at: i64,
    /// SHA-256 hex digest of the file content.
    pub content_hash: String,
}

// ── com.nexus.storage::list_dir ──────────────────────────────────────────────

/// Args for `com.nexus.storage::list_dir` (handler id `27`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
pub struct StorageListDirArgs {
    /// Forge-relative path of the directory to list. Empty string means the
    /// forge root.
    #[serde(default)]
    pub relpath: String,
}

/// One entry in [`StorageListDirResult::entries`]. Mirror of
/// [`crate::TreeEntry`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
#[serde(rename_all = "camelCase")]
pub struct StorageListDirEntry {
    /// File or directory name (no path separators).
    pub name: String,
    /// Path relative to the forge root, using forward slashes.
    pub relpath: String,
    /// `true` if this entry is a directory.
    pub is_dir: bool,
    /// Last-modified time, unix millis. `None` when the filesystem /
    /// platform does not expose it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_ms: Option<i64>,
    /// Created time, unix millis. `None` when the filesystem / platform
    /// does not expose it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_ms: Option<i64>,
}

/// Return type for `com.nexus.storage::list_dir`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
pub struct StorageListDirResult {
    /// Entries in the requested directory. Order is filesystem-dependent.
    pub entries: Vec<StorageListDirEntry>,
}
