//! Note + frontmatter handlers: `note_append`, `read_frontmatter`,
//! `write_frontmatter`, `note_find_duplicates`, `note_create_unique`,
//! `note_random`, `note_merge`, `note_create_from_title`.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{
    NoteExactDuplicateGroup, NoteFindDuplicatesArgs, NoteFindDuplicatesResult,
    NoteNearDuplicatePair, StorageNoteAppendArgs, StorageNoteCreateFromTitleArgs,
    StorageNoteCreateFromTitleResult, StorageNoteCreateUniqueArgs, StorageNoteCreateUniqueResult,
    StorageNoteMergeArgs, StorageNoteMergeResult, StorageNoteRandomArgs, StorageNoteRandomResult,
    StorageOk, StorageReadFrontmatterArgs, StorageWriteFrontmatterArgs,
};
use crate::unique_note::{UniqueNoteOptions, DEFAULT_ID_FORMAT, DEFAULT_SEPARATOR};
use crate::DeleteDestination;
use crate::{FileFilter, StorageEngine};

use super::shared::{exec_err, parse_args, to_value};

pub(crate) fn note_append(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via typed `StorageNoteAppendArgs`
    // (`deny_unknown_fields`). `FileMetadata`'s wire shape already
    // matches `StorageNoteAppendResult` field-for-field, so the
    // existing `to_value(&meta, …)` reply path is already typed.
    let StorageNoteAppendArgs { path, snippet } = parse_args(args, "note_append")?;
    // Path confinement is enforced by `read_file` and `write_file` via
    // `resolve_within` — absolute paths and `..` traversal are rejected
    // at the engine boundary (see issue #72). The `read_file` call
    // below surfaces the rejection before any disk I/O happens.
    //
    // Read existing content; treat a missing file as empty.
    let existing = match engine.read_file(&path) {
        Ok(bytes) => bytes,
        Err(crate::StorageError::FileNotFound(_)) => Vec::new(),
        Err(e) => return Err(exec_err(format!("note_append '{path}' read: {e}"))),
    };
    let existing_text = std::str::from_utf8(&existing).map_err(|e| {
        exec_err(format!(
            "note_append '{path}': existing file is not valid UTF-8: {e}"
        ))
    })?;
    let combined = build_appended(existing_text, &snippet);
    let meta = engine
        .write_file(&path, combined.as_bytes())
        .map_err(|e| exec_err(format!("note_append '{path}' write: {e}")))?;
    to_value(&meta, "note_append")
}

/// Build the post-append text for `note_append`. Centralised so the
/// unit test can pin the separator + trailing-newline contract without
/// going through the full dispatch pipeline.
///
/// Contract:
///   * Empty existing → returns `"{snippet}\n"` (no leading blank line).
///   * Non-empty existing that already ends with a blank-line gap is
///     left as-is; otherwise exactly one `\n\n` separator is inserted.
///   * Output always ends with a single `\n` so subsequent appends keep
///     the same shape.
pub(crate) fn build_appended(existing: &str, snippet: &str) -> String {
    let snippet_trimmed_end = snippet.trim_end_matches('\n');
    if existing.is_empty() {
        return format!("{snippet_trimmed_end}\n");
    }
    // Strip any trailing newlines from the existing buffer; we re-insert
    // exactly two so the snippet is preceded by one blank line regardless
    // of how the previous write ended.
    let base = existing.trim_end_matches('\n');
    format!("{base}\n\n{snippet_trimmed_end}\n")
}

/// BL-053 Phase 4 — read a markdown file's YAML frontmatter and return
/// it as a flat string-valued map. Lists collapse to comma-joined
/// strings; nested objects render via debug. Missing files /
/// unreadable bytes / non-markdown all return
/// `{ status: null, fields: {} }` so callers can branch on `status`
/// without a separate existence check.
pub(crate) fn read_frontmatter(forge_root: &Path, args: &Value) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via typed `StorageReadFrontmatterArgs`.
    let StorageReadFrontmatterArgs { path } = parse_args(args, "read_frontmatter")?;
    let result = read_frontmatter_for_path(forge_root, &path);
    to_value(&result, "read_frontmatter")
}

fn read_frontmatter_for_path(forge_root: &Path, path: &str) -> crate::ipc::ReadFrontmatterResult {
    let abs = forge_root.join(path);
    let Ok(content) = std::fs::read_to_string(&abs) else {
        return crate::ipc::ReadFrontmatterResult::default();
    };
    crate::ipc::frontmatter_from_source(&content)
}

pub(crate) fn write_frontmatter(
    engine: &StorageEngine,
    forge_root: &Path,
    args: &Value,
) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via typed `StorageWriteFrontmatterArgs`
    // (`deny_unknown_fields`). `value: None` deletes the key; any non-
    // string `value` is rejected at the typed-parse boundary rather
    // than inside the handler. The prior hand-rolled lookup silently
    // accepted unknown fields and reshaped malformed `value`s into a
    // custom error string; both paths now route through the standard
    // strictness gate.
    let StorageWriteFrontmatterArgs { path, key, value } = parse_args(args, "write_frontmatter")?;
    let current = std::fs::read_to_string(forge_root.join(&path))
        .map_err(|e| exec_err(format!("write_frontmatter '{path}' key='{key}' read: {e}")))?;
    let next = crate::core_plugin::apply_frontmatter_edit(&current, &key, value.as_deref());
    engine
        .write_file(&path, next.as_bytes())
        .map_err(|e| exec_err(format!("write_frontmatter '{path}' key='{key}' write: {e}")))?;
    to_value(&StorageOk { ok: true }, "write_frontmatter")
}

/// C23 (#376) — the note-level counterpart to `entity_find_duplicates`.
/// Exact duplicates come from a `content_hash` collision over indexed
/// markdown files (cheap — the index already carries `idx_files_hash`);
/// near-duplicates score cosine similarity over mean-pooled per-file
/// vectors from the `notes` embedding namespace, mirroring the O(n²)
/// pairwise-compare shape `EntityIndex::find_duplicates` already uses —
/// appropriate for personal-knowledge-base sizes.
pub(crate) fn find_duplicates(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let parsed: NoteFindDuplicatesArgs = parse_args(args, "note_find_duplicates")?;
    let near_threshold = parsed.near_threshold.unwrap_or(0.97).clamp(0.0, 1.0);

    let filter = FileFilter {
        prefix: None,
        file_type: Some("markdown".to_string()),
        include_deleted: false,
    };
    let files = engine
        .query_files(&filter)
        .map_err(|e| exec_err(format!("note_find_duplicates: {e}")))?;
    let mut by_hash: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for f in &files {
        by_hash
            .entry(f.content_hash.clone())
            .or_default()
            .push(f.path.clone());
    }
    let mut exact: Vec<NoteExactDuplicateGroup> = by_hash
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .map(|(content_hash, mut paths)| {
            paths.sort();
            NoteExactDuplicateGroup {
                content_hash,
                paths,
            }
        })
        .collect();
    exact.sort_by(|a, b| a.paths.first().cmp(&b.paths.first()));

    let vectors = engine
        .vector_mean_by_file("notes")
        .map_err(|e| exec_err(format!("note_find_duplicates: {e}")))?;
    let mut near = Vec::new();
    for i in 0..vectors.len() {
        for j in (i + 1)..vectors.len() {
            let (path_a, emb_a) = &vectors[i];
            let (path_b, emb_b) = &vectors[j];
            let sim = crate::vectorstore::cosine_similarity(emb_a, emb_b);
            if sim >= near_threshold {
                let (a, b) = if path_a <= path_b {
                    (path_a.clone(), path_b.clone())
                } else {
                    (path_b.clone(), path_a.clone())
                };
                near.push(NoteNearDuplicatePair {
                    a,
                    b,
                    similarity: sim,
                });
            }
        }
    }
    near.sort_by(|x, y| {
        y.similarity
            .partial_cmp(&x.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| x.a.cmp(&y.a))
            .then_with(|| x.b.cmp(&y.b))
    });

    to_value(
        &NoteFindDuplicatesResult { exact, near },
        "note_find_duplicates",
    )
}

/// RFC 0009 — `note_create_unique`. Zettelkasten-style note whose filename
/// is a chrono-formatted timestamp id plus the sanitized title. Naming
/// options default engine-side so a caller that sends only `title` gets
/// the canonical `%Y%m%d%H%M%S {title}.md` shape.
pub(crate) fn create_unique(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StorageNoteCreateUniqueArgs {
        title,
        id_format,
        separator,
        folder,
    } = parse_args(args, "note_create_unique")?;
    let options = UniqueNoteOptions {
        id_format: id_format
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_ID_FORMAT.to_string()),
        separator: separator.unwrap_or_else(|| DEFAULT_SEPARATOR.to_string()),
        folder: folder.filter(|s| !s.trim().is_empty()),
    };
    let path = engine
        .create_unique_note(&options, &title)
        .map_err(|e| exec_err(format!("note_create_unique: {e}")))?;
    to_value(
        &StorageNoteCreateUniqueResult { path },
        "note_create_unique",
    )
}

/// RFC 0009 — `note_random`. Uniform draw over indexed markdown files,
/// minus `exclude`, optionally under `prefix`. `path: null` when the
/// forge has nothing eligible.
pub(crate) fn random(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StorageNoteRandomArgs { exclude, prefix } = parse_args(args, "note_random")?;
    let path = engine
        .random_note_path(exclude.as_deref(), prefix.as_deref())
        .map_err(|e| exec_err(format!("note_random: {e}")))?;
    to_value(&StorageNoteRandomResult { path }, "note_random")
}

/// RFC 0009 — `note_merge`. Append `source` into `target`, redirect
/// inbound links, delete the source to `destination` (`forge` default).
pub(crate) fn merge(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let StorageNoteMergeArgs {
        source,
        target,
        update_links,
        destination,
    } = parse_args(args, "note_merge")?;
    let dest = parse_destination(destination.as_deref(), "note_merge")?;
    let outcome = engine
        .merge_notes(&source, &target, update_links, dest)
        .map_err(|e| exec_err(format!("note_merge '{source}' -> '{target}': {e}")))?;
    to_value(
        &StorageNoteMergeResult {
            target,
            files_rewritten: outcome.files_rewritten,
            links_updated: outcome.links_updated,
            trash_id: outcome.trash_id,
        },
        "note_merge",
    )
}

/// RFC 0009 — `note_create_from_title`. Sanitised `{title}.md` under
/// `folder` with `content` as the body; refuses to overwrite.
pub(crate) fn create_from_title(
    engine: &StorageEngine,
    args: &Value,
) -> Result<Value, PluginError> {
    let StorageNoteCreateFromTitleArgs {
        title,
        content,
        folder,
    } = parse_args(args, "note_create_from_title")?;
    let path = engine
        .create_note_from_title(
            &title,
            folder.as_deref().filter(|f| !f.trim().is_empty()),
            content.as_deref().unwrap_or(""),
        )
        .map_err(|e| exec_err(format!("note_create_from_title: {e}")))?;
    to_value(
        &StorageNoteCreateFromTitleResult { path },
        "note_create_from_title",
    )
}

/// Map the wire `destination` string shared by `trash_entry` and
/// `note_merge` onto [`DeleteDestination`]; `note_merge` additionally
/// accepts `"permanent"`.
fn parse_destination(raw: Option<&str>, handler: &str) -> Result<DeleteDestination, PluginError> {
    match raw {
        None | Some("forge") => Ok(DeleteDestination::ForgeTrash),
        Some("system") => Ok(DeleteDestination::SystemTrash),
        Some("permanent") => Ok(DeleteDestination::Permanent),
        Some(other) => Err(exec_err(format!(
            "{handler}: unknown destination '{other}' (expected 'forge', 'system', or 'permanent')"
        ))),
    }
}
