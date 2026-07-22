//! Note + frontmatter handlers: `note_append`, `read_frontmatter`,
//! `write_frontmatter`, `note_find_duplicates`.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::ipc::{
    NoteExactDuplicateGroup, NoteFindDuplicatesArgs, NoteFindDuplicatesResult,
    NoteNearDuplicatePair, StorageNoteAppendArgs, StorageOk, StorageReadFrontmatterArgs,
    StorageWriteFrontmatterArgs,
};
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
            NoteExactDuplicateGroup { content_hash, paths }
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
                near.push(NoteNearDuplicatePair { a, b, similarity: sim });
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
