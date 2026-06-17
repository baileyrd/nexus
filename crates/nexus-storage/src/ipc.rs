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
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
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
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
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
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
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
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
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
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageReadFileResult {
    /// Raw bytes of the file content, or `null` if the file does not exist.
    pub bytes: Option<Vec<u8>>,
    /// The 4-uppercase-hex hashline TAG of the (UTF-8) content, or `null` for a
    /// missing or non-UTF-8 file. Pass this in a `[path#TAG]` section to the
    /// `edit` handler to make precise, drift-safe edits (Phase 5.1 / RFC 0005).
    pub tag: Option<String>,
}

// ── com.nexus.storage::read_lines ────────────────────────────────────────────

/// Args for `com.nexus.storage::read_lines` (handler id `74`).
///
/// A context-efficient partial read: returns a 1-based, inclusive line range of
/// a text file rather than the whole thing. Phase 5.2 / RFC 0005.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageReadLinesArgs {
    /// Forge-relative path of the file to read.
    pub path: String,
    /// First line to return (1-based). Defaults to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
    /// Last line to return (1-based, inclusive). Defaults to `start + 199`
    /// (a 200-line window), clamped to the end of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<u32>,
}

/// Return type for `com.nexus.storage::read_lines`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageReadLinesResult {
    /// The requested line slice (joined by `\n`), `""` when the range is empty,
    /// or `null` when the file is missing or not UTF-8.
    pub content: Option<String>,
    /// First line actually returned (1-based; echoes the clamped `start`).
    pub start: u32,
    /// Last line actually returned (1-based, inclusive); `0` when the slice is
    /// empty (e.g. `start` is past the end of the file).
    pub end: u32,
    /// Total number of lines in the file.
    pub total_lines: u32,
    /// The 4-uppercase-hex hashline TAG of the *whole* file (for the `edit`
    /// handler), or `null` for a missing or non-UTF-8 file.
    pub tag: Option<String>,
}

// ── com.nexus.storage::ast_query ─────────────────────────────────────────────

/// Args for `com.nexus.storage::ast_query` (handler id `75`).
///
/// Runs a [tree-sitter query] over the forge's code files of one `language`.
/// Phase 5.2 / RFC 0005.
///
/// [tree-sitter query]: https://tree-sitter.github.io/tree-sitter/using-parsers#query-syntax
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageAstQueryArgs {
    /// Grammar to parse with: `rust`, `typescript`, `tsx`, `javascript`,
    /// `jsx`, `python`, or `go`. Only files of this language are searched.
    pub language: String,
    /// A tree-sitter query (S-expression pattern with `@capture`s).
    pub query: String,
    /// Optional forge-relative file or directory prefix to scope the search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Maximum matches to return. Defaults to 100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
}

/// One capture from an `ast_query` match.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageAstQueryMatch {
    /// Forge-relative path of the matching file.
    pub path: String,
    /// 1-based line where the captured node begins.
    pub line: u32,
    /// The `@capture` name (empty for an unnamed capture).
    pub capture: String,
    /// The captured node's source text (truncated to 240 bytes).
    pub text: String,
}

/// Return type for `com.nexus.storage::ast_query`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageAstQueryResult {
    /// Matches in file/scan order.
    pub matches: Vec<StorageAstQueryMatch>,
    /// True when the result hit `max_results` and more matches exist.
    pub truncated: bool,
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
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
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
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
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

// ── com.nexus.storage::edit ──────────────────────────────────────────────────

/// Args for `com.nexus.storage::edit` (handler id `73`).
///
/// Applies a [hashline](nexus_hashline) patch — one or more `[PATH#TAG]`
/// sections — against the current forge files. Phase 5.1 (RFC 0005): line and
/// whole-file insert operations on TAG-fresh files. A stale TAG returns an
/// error (re-read and retry); snapshot-backed 3-way-merge recovery is a
/// follow-up (PR B2). Block operations are rejected until tree-sitter lands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageEditArgs {
    /// The hashline patch text.
    pub patch: String,
}

/// One file successfully written by `edit`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageEditFileResult {
    /// Vault-relative path written.
    pub path: String,
    /// How the patch resolved: `"applied"` (TAG matched) or `"merged"`
    /// (recovered via 3-way merge — reserved for PR B2).
    pub status: String,
    /// New file size in bytes.
    pub size_bytes: u64,
}

/// A section whose 3-way merge could not be resolved cleanly (reserved for
/// PR B2; always empty while snapshots are unavailable).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageEditConflict {
    /// Vault-relative path that conflicted.
    pub path: String,
    /// Merged content carrying diff3 conflict markers.
    pub markers: String,
}

/// Return type for `com.nexus.storage::edit`.
///
/// All-or-nothing: when `conflicts` is non-empty no file is written, so the
/// caller can resolve and retry without a partially-applied patch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageEditResult {
    /// Files written, in patch order.
    pub files: Vec<StorageEditFileResult>,
    /// Unresolved sections (empty in Phase 5.1 PR B).
    pub conflicts: Vec<StorageEditConflict>,
}

// ── com.nexus.storage::note_append ───────────────────────────────────────────

/// Args for `com.nexus.storage::note_append` (handler id `54`).
///
/// Atomic "read-existing + append `\n\n{snippet}` + write" primitive that
/// the BL-043 quick-capture hotkey uses to grow a configurable `Inbox.md`
/// without the shell having to read + concatenate + write (which would
/// race against the file watcher).
///
/// Path confinement matches `write_file`: forge-relative paths only,
/// absolute paths and `..` traversal are rejected at the engine boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageNoteAppendArgs {
    /// Forge-relative path of the inbox file. Created on first append.
    pub path: String,
    /// Snippet text to append. The dispatch normalises the trailing
    /// newline shape so the resulting file has exactly one blank line
    /// between snippets and exactly one trailing newline.
    pub snippet: String,
}

/// Return type for `com.nexus.storage::note_append`. Mirror of
/// [`crate::FileMetadata`] — same shape as `write_file`'s return so a
/// caller can use either handler interchangeably.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageNoteAppendResult {
    /// Vault-relative path.
    pub path: String,
    /// File size in bytes (post-append).
    pub size_bytes: u64,
    /// Unix timestamp of the post-append modification.
    pub modified_at: i64,
    /// SHA-256 hex digest of the post-append file content.
    pub content_hash: String,
}

// ── com.nexus.storage::list_dir ──────────────────────────────────────────────

/// Args for `com.nexus.storage::list_dir` (handler id `27`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
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
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageListDirResult {
    /// Entries in the requested directory. Order is filesystem-dependent.
    pub entries: Vec<StorageListDirEntry>,
}

// ── BL-053 Phase 4 — read_frontmatter ────────────────────────────────────────
//
// Args: forge-relative path to a markdown file.
// Returns:
//   - `status`  — the value of the `status:` frontmatter key (single
//     string), or `null` when the key is absent / the file has no
//     frontmatter / the file doesn't exist. Empty / unrecognised
//     values are passed through verbatim so the shell can render
//     them as plain text chips.
//   - `fields`  — flat string-valued map of the remaining frontmatter
//     keys. Lists are joined with `, `; nested objects render via
//     debug. Keeps the wire shape stable for ts-rs without forcing
//     callers to deal with `unknown`-typed values.

/// Args for `com.nexus.storage::read_frontmatter` (handler 59).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageReadFrontmatterArgs {
    /// Forge-relative path to the markdown file.
    pub path: String,
}

/// Reply for `com.nexus.storage::read_frontmatter`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ReadFrontmatterResult {
    /// Value of the `status:` frontmatter key, or `null` when absent.
    /// The value passes through verbatim — the shell maps the known
    /// status set (`info` / `warn` / `risk` / `ok`) to themed pills
    /// and renders unknown values as plain chips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Remaining frontmatter keys, keyed by YAML field name. Lists
    /// are joined with `, `; nested objects render via debug.
    pub fields: std::collections::BTreeMap<String, String>,
}

// ── #190 / R7 — shared `{ path }` args + file_exists reply ──────────────────

/// Shared `{ path: String }` args envelope for storage verbs that
/// take just a forge-relative path. Adopted by `delete_file`,
/// `file_exists`, `write_vault_file` per #190 so the schemars
/// generator + the `ipc_strictness` gate see the same wire shape
/// the handlers read. Path-traversal attempts (`..`) and absolute
/// paths are rejected by the engine boundary, not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StoragePathArgs {
    /// Forge-relative path. Empty string is rejected by the engine.
    pub path: String,
}

/// Reply for `com.nexus.storage::file_exists`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageFileExistsResult {
    /// `true` iff the file exists at the given forge-relative path.
    pub exists: bool,
}

// ── #190 / R7 — graph.rs handlers ───────────────────────────────────────────

/// Args for `com.nexus.storage::backlinks_to_block`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageBacklinksToBlockArgs {
    /// Forge-relative path of the destination file.
    pub path: String,
    /// Stable block identifier within the destination file.
    pub block_id: String,
}

/// Args for `com.nexus.storage::graph_neighbors`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageGraphNeighborsArgs {
    /// Forge-relative path of the seed file.
    pub path: String,
    /// BFS depth — `0` returns the seed only, `1` returns immediate
    /// neighbours, etc.
    pub depth: u64,
}

// ── #190 / R7 — canvas.rs + tasks.rs handlers ───────────────────────────────

/// Args for `com.nexus.storage::canvas_write`. The inner `canvas`
/// pass-through stays as `serde_json::Value` because
/// [`crate::CanvasFile`] doesn't derive `JsonSchema` / `TS`; the
/// handler still runs `serde_json::from_value::<CanvasFile>` on it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageCanvasWriteArgs {
    /// Forge-relative path of the `.canvas` file.
    pub path: String,
    /// `crate::CanvasFile` serialised as JSON.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub canvas: serde_json::Value,
}

/// Args for `com.nexus.storage::canvas_patch`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageCanvasPatchArgs {
    /// Forge-relative path of the `.canvas` file.
    pub path: String,
    /// `Vec<crate::CanvasPatchOp>` serialised as JSON.
    #[cfg_attr(feature = "ts-export", ts(type = "Array<unknown>"))]
    pub ops: Vec<serde_json::Value>,
}

/// Args for `com.nexus.storage::toggle_task`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageToggleTaskArgs {
    /// Primary key of the task row in the storage index.
    pub task_id: u64,
}

// ── #190 / R7 — tree.rs handlers ────────────────────────────────────────────

/// Shared `{ relpath: String }` envelope for the tree handlers
/// (`create_file`, `create_dir`, `delete_entry`). Distinct from
/// [`StoragePathArgs`] because the tree subsystem uses `relpath` as
/// the wire field name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageRelpathArgs {
    /// Forge-relative path. Empty string is rejected by the engine.
    pub relpath: String,
}

/// Args for `com.nexus.storage::rename_entry`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageRenameEntryArgs {
    /// Forge-relative path of the source.
    pub from: String,
    /// Forge-relative path of the destination.
    pub to: String,
}

// ── #190 / R7 — search.rs handlers ──────────────────────────────────────────

/// Args for `com.nexus.storage::query_tags`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageQueryTagsArgs {
    /// Tag name to look up. The engine returns block hits annotated
    /// with this tag.
    pub name: String,
}

// ── #190 / R7 — config.rs handlers ──────────────────────────────────────────

/// Args for `com.nexus.storage::config_read` and
/// `com.nexus.storage::config_reset`. Both verbs take a `{ kind }`
/// discriminator; values outside `app | workspace | mcp | ai` are
/// rejected at the handler boundary (the typed parse accepts any
/// string here so the handler can emit the precise list as part of
/// the error message).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageConfigKindArgs {
    /// One of `"app"`, `"workspace"`, `"mcp"`, `"ai"`.
    pub kind: String,
}

/// Reply for `com.nexus.storage::config_read`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageConfigContentResult {
    /// `"toml"` or `"json"`, matching the `kind`'s on-disk format.
    pub format: String,
    /// Pretty-printed serialized content.
    pub content: String,
}

/// Args for `com.nexus.storage::settings_write`. `value: null`
/// removes the key.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageSettingsWriteArgs {
    /// Setting key (dotted path inside `app.toml`'s `[settings]` table).
    pub key: String,
    /// Replacement value. `null` removes the key. Other JSON scalars
    /// / objects round-trip through `toml::Value`.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub value: serde_json::Value,
}

// ── #190 / R7 — import_forge ────────────────────────────────────────────────

/// Conflict-resolution strategy for `com.nexus.storage::import_forge`.
/// Mirror of [`crate::import::ConflictStrategy`]; the lowercase wire
/// shape (`"skip"`, `"overwrite"`, `"rename"`) is shared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "lowercase")]
pub enum StorageImportConflictStrategy {
    /// Leave the destination unchanged on conflict (default).
    #[default]
    Skip,
    /// Replace the destination with the source bytes.
    Overwrite,
    /// Write the source to `<stem>.imported.<n>.<ext>`.
    Rename,
}

/// Args for `com.nexus.storage::import_forge`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageImportForgeArgs {
    /// Absolute (or relative-to-cwd) path of the source forge.
    pub source: String,
    /// When `true`, only computes + returns the plan without copying
    /// anything. Defaults to `false`.
    #[serde(default)]
    pub dry_run: bool,
    /// How to resolve content-hash collisions. Defaults to
    /// `Skip` per [`StorageImportConflictStrategy::default`].
    #[serde(default)]
    pub on_conflict: StorageImportConflictStrategy,
}

// ── #190 / R7 — bases delete/restore/rename handlers ────────────────────────

/// Args for the three `base_record_{delete,soft_delete,restore}`
/// verbs. They share `{ path, record_id }` exactly; one type avoids
/// three identical mirrors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageBaseRecordIdArgs {
    /// Forge-relative path of the `.bases/` directory.
    pub path: String,
    /// Stable record identifier (per [`nexus_types::bases::BaseRecord`]).
    pub record_id: String,
}

/// Args for `base_property_delete` + `base_view_delete`. Both verbs
/// take `{ path, name }` and return [`StorageOk`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageBaseNamedArgs {
    /// Forge-relative path of the `.bases/` directory.
    pub path: String,
    /// Property name or view name (per the verb's contract).
    pub name: String,
}

/// Args for `base_property_rename`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageBasePropertyRenameArgs {
    /// Forge-relative path of the `.bases/` directory.
    pub path: String,
    /// Current property name.
    pub old_name: String,
    /// Replacement property name. Must not collide with another
    /// existing property in the same base.
    pub new_name: String,
}

// ── #190 / R7 — bases create/update/index/query (complex args) ──────────────
//
// These typed args envelopes keep the nested domain types
// (`BaseRecord`, `BaseView`, `BaseSchema`, frontmatter property
// `definition`) as `serde_json::Value` pass-throughs because the
// impl types (`nexus_types::bases::*`) don't derive `JsonSchema`
// / `TS`. The handlers still call `serde_json::from_value` on the
// inner shapes — the strict gate added here covers the outer
// `{ path, … }` envelope, which is where the audit's "typo silently
// accepted" hazard lives.

/// Args for `com.nexus.storage::base_record_create`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageBaseRecordCreateArgs {
    /// Forge-relative path of the `.bases/` directory.
    pub path: String,
    /// `nexus_types::bases::BaseRecord` serialised as JSON. The
    /// handler runs `serde_json::from_value::<BaseRecord>` on this
    /// field, so malformed inner shapes still surface as parse
    /// errors — just inside the handler, not at the envelope.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub record: serde_json::Value,
}

/// Args for `com.nexus.storage::base_record_update`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageBaseRecordUpdateArgs {
    /// Forge-relative path of the `.bases/` directory.
    pub path: String,
    /// Stable record identifier (per `BaseRecord::id`).
    pub record_id: String,
    /// Field-name → new-value map. Values pass through verbatim.
    #[cfg_attr(feature = "ts-export", ts(type = "Record<string, unknown>"))]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// Args for `com.nexus.storage::base_property_create`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageBasePropertyCreateArgs {
    /// Forge-relative path of the `.bases/` directory.
    pub path: String,
    /// New property name. Must not collide with an existing property.
    pub name: String,
    /// Property definition (type + per-type metadata). Pass-through
    /// for the property-definition shape the storage engine parses.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub definition: serde_json::Value,
}

/// Args for `com.nexus.storage::base_property_update`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageBasePropertyUpdateArgs {
    /// Forge-relative path of the `.bases/` directory.
    pub path: String,
    /// Property name to update.
    pub name: String,
    /// Replacement definition.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub definition: serde_json::Value,
    /// When `true`, rewrites every record's value for this property
    /// through the new definition's coercion path.
    #[serde(default)]
    pub migrate_values: bool,
}

/// Args for `com.nexus.storage::base_view_create` and
/// `com.nexus.storage::base_view_update`. The verbs share `{ path,
/// view }` exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageBaseViewArgs {
    /// Forge-relative path of the `.bases/` directory.
    pub path: String,
    /// `nexus_types::bases::BaseView` serialised as JSON.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub view: serde_json::Value,
}

/// Args for `com.nexus.storage::base_create`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageBaseCreateArgs {
    /// Forge-relative path of the new `.bases/` directory.
    pub path: String,
    /// `nexus_types::bases::BaseSchema` serialised as JSON.
    #[cfg_attr(feature = "ts-export", ts(type = "unknown"))]
    pub schema: serde_json::Value,
    /// Seed `BaseRecord`s to insert at creation time. Defaults to
    /// the empty list.
    #[serde(default)]
    #[cfg_attr(feature = "ts-export", ts(type = "Array<unknown>"))]
    pub seed_records: Vec<serde_json::Value>,
}

/// Args for `com.nexus.storage::base_query`. Filters and sorts are
/// DSL strings parsed by `crate::bases::query::parse_filter` /
/// `parse_sort`; the handler surfaces parse errors per-clause.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageBaseQueryArgs {
    /// Forge-relative path of the `.bases/` directory.
    pub path: String,
    /// Filter-DSL strings. Empty == no filters.
    #[serde(default)]
    pub filters: Vec<String>,
    /// Sort-DSL strings. Empty == no sorts (engine-default order).
    #[serde(default)]
    pub sorts: Vec<String>,
    /// Max rows. `None` == engine default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Skip count. `None` == 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
}

/// Reply for `com.nexus.storage::base_index`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageBaseIndexResult {
    /// Primary key the storage engine assigned to the indexed base.
    pub base_id: i64,
}

// ── #190 / R7 — vector store handlers ────────────────────────────────────────

/// Mirror of [`crate::vectorstore::ChunkEmbedding`] — kept in sync
/// manually so the impl type stays free of the optional `ts-rs` /
/// `schemars` derives. Compared via `cargo test -p nexus-bootstrap
/// --test ipc_schema_emit`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageChunkEmbedding {
    /// Path of the source file (forge-relative).
    pub file_path: String,
    /// Identifier of the originating block.
    pub block_id: u64,
    /// Textual content of the chunk.
    pub chunk_text: String,
    /// Dense vector representation of the chunk.
    pub embedding: Vec<f32>,
}

/// Args for `com.nexus.storage::vector_insert`. Replaces all chunks
/// for `file_path` atomically.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageVectorInsertArgs {
    /// Collection the vectors belong to (e.g. `notes`, `memory`). Defaults to
    /// `notes` so existing callers that omit it keep their current behaviour.
    #[serde(default = "default_vector_namespace")]
    pub namespace: String,
    /// Forge-relative path of the source file. Used as the dedup key
    /// (scoped to `namespace`).
    pub file_path: String,
    /// Replacement chunk set. Empty array clears all chunks for the
    /// file without inserting new ones.
    pub chunks: Vec<StorageChunkEmbedding>,
}

/// Args for `com.nexus.storage::vector_query`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageVectorQueryArgs {
    /// Collection to search (e.g. `notes`, `memory`). Defaults to `notes`.
    #[serde(default = "default_vector_namespace")]
    pub namespace: String,
    /// Query embedding (same dimensionality as the stored vectors).
    pub embedding: Vec<f32>,
    /// Maximum number of matches to return. Defaults to 5 when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Default vector-store namespace for IPC args that omit one. `notes` keeps
/// every pre-namespacing caller (the AI/RAG indexer) operating on the same
/// collection it always has.
fn default_vector_namespace() -> String {
    "notes".to_string()
}

/// Args for `com.nexus.storage::vector_delete_by_file`. Deletes all chunks for
/// `path` within `namespace`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageVectorDeleteArgs {
    /// Collection to delete from (e.g. `notes`, `memory`). Defaults to `notes`.
    #[serde(default = "default_vector_namespace")]
    pub namespace: String,
    /// Forge-relative path whose chunks are removed.
    pub path: String,
}

/// Args for `com.nexus.storage::vectorstore_count`. Counts the chunks in one
/// collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageVectorCountArgs {
    /// Collection to count (e.g. `notes`, `memory`). Defaults to `notes`.
    #[serde(default = "default_vector_namespace")]
    pub namespace: String,
}

/// One match row in a `vector_query` reply. Mirror of
/// [`crate::vectorstore::ChunkMatch`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageVectorMatch {
    /// Path of the source file (forge-relative).
    pub file_path: String,
    /// Identifier of the originating block.
    pub block_id: u64,
    /// Textual content of the matched chunk.
    pub chunk_text: String,
    /// Cosine similarity score. Higher is more relevant.
    pub score: f32,
}

/// Reply for `com.nexus.storage::vectorstore_count`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageVectorstoreCountResult {
    /// Total number of chunk-embedding rows in the store.
    pub count: u64,
}

// ── #190 / R7 — write_frontmatter ────────────────────────────────────────────

/// Args for `com.nexus.storage::write_frontmatter`. Mirrors the
/// hand-rolled shape the dispatch previously parsed by hand.
/// `value: None` deletes the key (no-op when absent); a present
/// `Some(_)` writes the literal string. Non-scalar `value`s are
/// rejected at the typed-parse boundary — the prior implementation
/// rejected them inside the handler with a custom error message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageWriteFrontmatterArgs {
    /// Forge-relative path to the markdown file. Path confinement is
    /// enforced at the engine boundary.
    pub path: String,
    /// Frontmatter key to write / delete.
    pub key: String,
    /// New value for the key. `None` deletes the key (no-op if absent).
    #[serde(default)]
    pub value: Option<String>,
}

/// Canonical `{ "ok": true }` reply for storage verbs that don't
/// carry a meaningful response body (`write_frontmatter`,
/// `delete_file`, `write_vault_file`, …). Adopted opportunistically
/// per #190 so the schemars generator sees the same wire shape the
/// handlers emit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageOk {
    /// Always `true`. The kernel surfaces failure via
    /// `PluginError::ExecutionFailed`, not via this flag.
    pub ok: bool,
}

/// Parse a markdown source's YAML frontmatter into a
/// [`ReadFrontmatterResult`]. Exported for unit tests and for the
/// `read_frontmatter` IPC dispatch in [`crate::core_plugin`]. Files
/// without a leading `---` block (or with a malformed one) yield
/// the default empty result.
#[must_use]
pub fn frontmatter_from_source(content: &str) -> ReadFrontmatterResult {
    let after_open = if let Some(s) = content.strip_prefix("---\r\n") {
        s
    } else if let Some(s) = content.strip_prefix("---\n") {
        s
    } else {
        return ReadFrontmatterResult::default();
    };
    let close_pattern = "\n---";
    let Some(close_pos) = after_open.find(close_pattern) else {
        return ReadFrontmatterResult::default();
    };
    let yaml_src = &after_open[..close_pos];
    let Ok(yaml) = serde_yml::from_str::<serde_yml::Value>(yaml_src) else {
        return ReadFrontmatterResult::default();
    };
    let mut out = ReadFrontmatterResult::default();
    if let serde_yml::Value::Mapping(map) = yaml {
        for (k, v) in map {
            let key = match k {
                serde_yml::Value::String(s) => s,
                other => format!("{other:?}"),
            };
            let stringified = stringify_yaml_value(&v);
            if key == "status" {
                let trimmed = stringified.trim().to_string();
                if !trimmed.is_empty() {
                    out.status = Some(trimmed);
                }
                continue;
            }
            out.fields.insert(key, stringified);
        }
    }
    out
}

fn stringify_yaml_value(v: &serde_yml::Value) -> String {
    match v {
        serde_yml::Value::Null => String::new(),
        serde_yml::Value::Bool(b) => b.to_string(),
        serde_yml::Value::Number(n) => n.to_string(),
        serde_yml::Value::String(s) => s.clone(),
        serde_yml::Value::Sequence(seq) => seq
            .iter()
            .map(stringify_yaml_value)
            .collect::<Vec<_>>()
            .join(", "),
        serde_yml::Value::Mapping(_) | serde_yml::Value::Tagged(_) => format!("{v:?}"),
    }
}

// ── BL-114 — query_symbol ────────────────────────────────────────────────────

/// Args for `com.nexus.storage::query_symbol` (handler 63). Mirrors
/// [`crate::code_index::SymbolFilter`]. `name` and `path` AND-combine
/// when both present; an empty payload returns every indexed symbol
/// up to the default limit.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageQuerySymbolArgs {
    /// Exact identifier match. Case-sensitive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Exact forge-relative path match. Scopes results to one file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Maximum rows to return. Defaults to 200 when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// One symbol row in [`StorageQuerySymbolResult`]. Mirror of
/// [`crate::code_index::SymbolRecord`] — kept in sync manually.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageSymbolRow {
    /// Row id in the `code_symbols` table.
    pub id: i64,
    /// Forge-relative path of the source file.
    pub path: String,
    /// Language label (`"rust"` / `"typescript"` / `"python"` / `"go"` / …).
    pub language: String,
    /// Symbol kind (`"function"` / `"struct"` / `"class"` / `"impl"` / …).
    pub kind: String,
    /// Identifier as it appears in source.
    pub name: String,
    /// 1-based starting line.
    pub line_start: u32,
    /// 1-based ending line.
    pub line_end: u32,
    /// Row id of the enclosing symbol, or `null` for top-level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<i64>,
    /// Leading doc comment, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_comment: Option<String>,
}

/// Return type for `com.nexus.storage::query_symbol`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct StorageQuerySymbolResult {
    /// Matching rows ordered by `(path, line_start)` ascending.
    pub symbols: Vec<StorageSymbolRow>,
}

// ── BL-128 — entity_search / entity_get / entity_relations ───────────────────

/// Args for `com.nexus.storage::entity_search` (handler 64).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntitySearchArgs {
    /// Substring query against entity id / aliases / description.
    /// Empty string returns the lexicographically-first `limit`
    /// records — useful for "give me anything" agent prepends.
    #[serde(default)]
    pub query: String,
    /// Optional case-insensitive filter on `entity_type`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_type: Option<String>,
    /// Maximum hits to return. Defaults to 10 when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// One hit in [`EntitySearchResult::results`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntitySearchHitRow {
    /// Canonical entity id (file stem).
    pub id: String,
    /// `entity_type` declared in frontmatter.
    pub entity_type: String,
    /// One-line description (frontmatter `description:` or first
    /// body paragraph, capped at 240 chars).
    pub description: String,
    /// Forge-relative path of the entity markdown file.
    pub relpath: String,
    /// Score from [`crate::entity_index::EntityIndex::search`].
    pub score: i32,
}

/// Return type for `com.nexus.storage::entity_search`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntitySearchResult {
    /// Hits ordered by descending score then ascending id.
    pub results: Vec<EntitySearchHitRow>,
}

/// Args for `com.nexus.storage::entity_get` (handler 65).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityGetArgs {
    /// Canonical id or one of the entity's aliases.
    pub id: String,
}

/// One outgoing relation declared on an [`EntityRecordRow`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityRelationRow {
    /// Target entity id or alias as declared on disk.
    pub target: String,
    /// Free-form relation kind.
    #[serde(rename = "type")]
    pub kind: String,
    /// Confidence in `[0.0, 1.0]`. Defaults to `1.0` on disk.
    pub confidence: f32,
}

/// Full entity payload returned by `entity_get`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityRecordRow {
    /// Canonical entity id (file stem).
    pub id: String,
    /// `entity_type` declared in frontmatter.
    pub entity_type: String,
    /// Aliases declared in frontmatter (after empty-string filtering).
    pub aliases: Vec<String>,
    /// One-line description (frontmatter or fallback first paragraph).
    pub description: String,
    /// Outgoing relations declared on this entity.
    pub relations: Vec<EntityRelationRow>,
    /// Forge-relative path of the entity markdown file.
    pub relpath: String,
}

/// Return type for `com.nexus.storage::entity_get`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityGetResult {
    /// The entity, or `null` when no id / alias matched. Always
    /// present on the wire (no `skip_serializing_if`) so consumers
    /// can distinguish "not found" from a malformed response.
    pub entity: Option<EntityRecordRow>,
}

/// Args for `com.nexus.storage::entity_relations` (handler 66).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityRelationsArgs {
    /// Canonical id or alias.
    pub id: String,
    /// One of `"outgoing"` / `"incoming"` / `"both"`. Defaults to
    /// `"both"` when absent or unrecognised.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
}

/// One row in [`EntityRelationsResult::relations`]. Aliased targets
/// are resolved to their canonical id before this row is emitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityRelationsResultRow {
    /// Source entity id.
    pub from: String,
    /// Target entity id (alias-resolved).
    pub to: String,
    /// Free-form relation kind.
    #[serde(rename = "type")]
    pub kind: String,
    /// Confidence in `[0.0, 1.0]`.
    pub confidence: f32,
}

/// Return type for `com.nexus.storage::entity_relations`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityRelationsResult {
    /// Rows ordered by (from, to, kind) ascending.
    pub relations: Vec<EntityRelationsResultRow>,
}

// ── BL-128 close — entity_upsert / entity_find_duplicates ────────────────────

/// One outgoing relation entry inside [`EntityUpsertArgs::relations`].
/// Relation kinds are normalised through
/// `crate::entity_index::normalize_relation_type` before being written
/// to disk, so callers can submit free-form LLM output without
/// preprocessing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityUpsertRelationRow {
    /// Target entity id (or alias — preserved verbatim).
    pub target: String,
    /// Free-form relation kind. Normalised before persistence.
    #[serde(rename = "type")]
    pub kind: String,
    /// Confidence in `[0.0, 1.0]`. Absent ⇒ `1.0` on disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

/// Args for `com.nexus.storage::entity_upsert` (handler 67).
///
/// `id` becomes the markdown file stem under `<forge>/entities/`.
/// All other fields map directly to the on-disk YAML keys recognised
/// by the thin-slice parser. Existing files are overwritten via the
/// atomic-write path (temp-fsync-rename) so a concurrent read
/// observes either the old or the new content, never a torn write.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityUpsertArgs {
    /// Canonical id — becomes the markdown file stem.
    pub id: String,
    /// `entity_type:` frontmatter key.
    pub entity_type: String,
    /// `aliases:` frontmatter key. Empty list omits the field on disk.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// `description:` frontmatter key. Empty string omits the field.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// `relations:` frontmatter list. Empty list omits the field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<EntityUpsertRelationRow>,
}

/// Return type for `com.nexus.storage::entity_upsert`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityUpsertResult {
    /// Forge-relative path of the entity markdown file that was
    /// written (`entities/<id>.md`).
    pub relpath: String,
    /// `true` when an existing file was replaced, `false` for a fresh
    /// create. The atomic-write path makes both cases observable
    /// atomically — this flag just lets a UI distinguish them.
    pub replaced: bool,
}

/// Args for `com.nexus.storage::entity_find_duplicates` (handler 68).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityFindDuplicatesArgs {
    /// Minimum Jaccard similarity in `[0.0, 1.0]` for a pair to be
    /// reported. Defaults to `0.92` when absent — matches Thoth's
    /// Dream-Cycle review threshold.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f32>,
}

/// One pair in [`EntityFindDuplicatesResult::pairs`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityDuplicatePairRow {
    /// Lexicographically-smaller entity id.
    pub a: String,
    /// Lexicographically-greater entity id.
    pub b: String,
    /// Jaccard token similarity in `[0.0, 1.0]`.
    pub similarity: f32,
}

/// Return type for `com.nexus.storage::entity_find_duplicates`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityFindDuplicatesResult {
    /// Pairs ordered by descending similarity then ascending `(a, b)`.
    pub pairs: Vec<EntityDuplicatePairRow>,
}

// ── BL-129 — entity_merge ─────────────────────────────────────────────────────

/// Args for `com.nexus.storage::entity_merge` (handler 70).
///
/// Merges the entity identified by `drop` into the entity identified
/// by `keep`. The surviving entity inherits:
///   * `keep`'s canonical id + `entity_type` + `relpath`,
///   * the union of both `aliases` lists (de-duplicated, plus `drop`'s
///     canonical id is added as a new alias to preserve back-references),
///   * the longer of the two descriptions,
///   * the union of both `relations` lists, de-duplicated on
///     `(target, kind)` with the maximum confidence kept on conflict.
///
/// `drop`'s markdown file is then deleted. Outgoing references in
/// other entities that pointed to `drop` are NOT rewritten — the
/// alias just appended to `keep` ensures they still resolve through
/// the alias-lookup path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityMergeArgs {
    /// Canonical id of the entity that survives the merge.
    pub keep: String,
    /// Canonical id of the entity that is merged into `keep` and
    /// then deleted. Must differ from `keep`.
    pub drop: String,
}

/// Return type for `com.nexus.storage::entity_merge`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityMergeResult {
    /// Canonical id of the surviving entity (echoes `keep`).
    pub kept: String,
    /// Canonical id of the entity that was deleted (echoes `drop`).
    pub dropped: String,
    /// Aliases added to `kept` (not previously present on it). Includes
    /// `drop`'s canonical id when it was not already an alias.
    pub aliases_added: u32,
    /// Relations added to `kept` (deduplicated on `(target, kind)`).
    pub relations_added: u32,
}

// ── BL-129 follow-up — list_draft_relations ─────────────────────────────────

/// Args for `com.nexus.storage::list_draft_relations` (handler 71).
///
/// Enumerates every outgoing relation across `<forge>/entities/*.md`
/// whose `confidence` is at or below `threshold`. Drives the
/// Dream-Cycle inbox: the LLM `infer_entity_relations` handler writes
/// proposals at `confidence: 0.5`, so callers default `threshold` to
/// `0.5` to capture exactly that set.
///
/// The handler is read-only — it never mutates entity files. Approve
/// or skip actions flow through the existing `entity_get` +
/// `entity_upsert` handlers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ListDraftRelationsArgs {
    /// Inclusive upper bound for relation confidence in `[0.0, 1.0]`.
    /// Defaults to `0.5` (Dream-Cycle proposal value) when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f32>,
    /// Maximum rows to return. Defaults to `200` when absent. The
    /// handler emits a `truncated: true` flag so the inbox UI can hint
    /// that more proposals exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// One row in [`ListDraftRelationsResult::relations`]. The
/// `(from, target, type)` triple is sufficient for the inbox to
/// re-fetch the source entity via `entity_get` and resubmit the
/// mutated relation list to `entity_upsert`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct DraftRelationRow {
    /// Canonical id of the source entity that declares the relation.
    pub from: String,
    /// Target as it appears in the source file (may be an alias).
    pub target: String,
    /// Relation kind (canonical — `render_entity_markdown` normalises
    /// at write time, so values reflected here are the canonical form).
    #[serde(rename = "type")]
    pub kind: String,
    /// Confidence in `[0.0, 1.0]`.
    pub confidence: f32,
    /// Forge-relative path of the source entity's markdown file.
    pub relpath: String,
}

/// Return type for `com.nexus.storage::list_draft_relations`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ListDraftRelationsResult {
    /// Draft relations sorted by ascending confidence, then
    /// ascending `(from, target, kind)` for stable display.
    pub relations: Vec<DraftRelationRow>,
    /// Total number of relations at-or-below threshold across the
    /// entire forge, even when the response is capped by `limit`.
    pub total: u32,
    /// `true` when `relations.len() < total`.
    pub truncated: bool,
}

// ── BL-129 — entity_decay_relations ──────────────────────────────────────────

/// Args for `com.nexus.storage::entity_decay_relations` (handler 69).
///
/// Each entity file under `<forge>/entities/` is read, its relation
/// confidences are multiplied by `factor` and clamped to `floor`, and
/// the file is atomically rewritten when any relation changes. Already-
/// at-floor relations are skipped so successive cycles converge.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityDecayRelationsArgs {
    /// Multiplicative decay factor in `(0.0, 1.0]`. Defaults to
    /// `0.95` — matches Thoth's Dream-Cycle decay rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factor: Option<f32>,
    /// Lower bound for relation confidence after decay. Defaults to
    /// `0.10`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor: Option<f32>,
    /// When `true`, compute counts but do not write any file. Useful
    /// for `nexus graph dream-cycle run --phase decay --dry-run` and
    /// for the shell to surface a preview before the user commits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
}

/// Return type for `com.nexus.storage::entity_decay_relations`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct EntityDecayRelationsResult {
    /// Total entity files scanned (parsed successfully).
    pub entities_scanned: u32,
    /// Entity files that had at least one relation modified and
    /// (unless `dry_run`) rewritten on disk.
    pub entities_updated: u32,
    /// Relations whose confidence was strictly reduced this pass.
    pub relations_decayed: u32,
    /// Relations whose post-decay value landed exactly on `floor`
    /// this pass. Pre-existing at-floor relations are excluded.
    pub relations_at_floor: u32,
    /// Reflects the request: when `true`, no files were written.
    pub dry_run: bool,
}

#[cfg(test)]
mod read_frontmatter_tests {
    use super::*;

    #[test]
    fn returns_default_when_no_frontmatter() {
        let out = frontmatter_from_source("# heading\n\nbody text\n");
        assert!(out.status.is_none());
        assert!(out.fields.is_empty());
    }

    #[test]
    fn parses_status_and_other_fields() {
        let src = "---\nstatus: info\ntitle: Hello\ntags:\n  - rust\n  - shell\n---\n\nbody\n";
        let out = frontmatter_from_source(src);
        assert_eq!(out.status.as_deref(), Some("info"));
        assert_eq!(out.fields.get("title").map(String::as_str), Some("Hello"));
        assert_eq!(
            out.fields.get("tags").map(String::as_str),
            Some("rust, shell"),
        );
    }

    #[test]
    fn empty_status_treated_as_absent() {
        let src = "---\nstatus:\n---\nbody\n";
        let out = frontmatter_from_source(src);
        assert!(out.status.is_none());
    }

    #[test]
    fn unterminated_frontmatter_returns_default() {
        let src = "---\nstatus: info\nbody without closing fence\n";
        let out = frontmatter_from_source(src);
        assert!(out.status.is_none());
    }

    #[test]
    fn invalid_yaml_returns_default() {
        let src = "---\nkey: : :\n---\nbody\n";
        let out = frontmatter_from_source(src);
        assert!(out.status.is_none());
    }
}
