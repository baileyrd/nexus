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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
#[serde(deny_unknown_fields)]
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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
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
    ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/ipc/")
)]
#[serde(deny_unknown_fields)]
pub struct StorageQuerySymbolResult {
    /// Matching rows ordered by `(path, line_start)` ascending.
    pub symbols: Vec<StorageSymbolRow>,
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
