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
