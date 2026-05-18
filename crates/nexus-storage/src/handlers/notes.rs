//! Note + frontmatter handlers: `note_append`, `read_frontmatter`,
//! `write_frontmatter`.

use std::path::Path;

use nexus_plugins::PluginError;
use serde_json::Value;

use crate::StorageEngine;

use super::shared::{exec_err, path_arg, to_value};

pub(crate) fn note_append(engine: &StorageEngine, args: &Value) -> Result<Value, PluginError> {
    let path = path_arg(args, "note_append")?;
    let snippet = args
        .get("snippet")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("note_append: missing 'snippet' string".to_string()))?;
    // Path confinement is enforced by `read_file` and `write_file` via
    // `resolve_within` — absolute paths and `..` traversal are rejected
    // at the engine boundary (see issue #72). The `read_file` call
    // below surfaces the rejection before any disk I/O happens.
    //
    // Read existing content; treat a missing file as empty.
    let existing = match engine.read_file(&path) {
        Ok(bytes) => bytes,
        Err(crate::StorageError::FileNotFound(_)) => Vec::new(),
        Err(e) => return Err(exec_err(format!("note_append: read: {e}"))),
    };
    let existing_text = std::str::from_utf8(&existing)
        .map_err(|e| exec_err(format!("note_append: existing file is not valid UTF-8: {e}")))?;
    let combined = build_appended(existing_text, snippet);
    let meta = engine
        .write_file(&path, combined.as_bytes())
        .map_err(|e| exec_err(format!("note_append: write: {e}")))?;
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
    let path = path_arg(args, "read_frontmatter")?;
    let result = read_frontmatter_for_path(forge_root, &path);
    to_value(&result, "read_frontmatter")
}

fn read_frontmatter_for_path(
    forge_root: &Path,
    path: &str,
) -> crate::ipc::ReadFrontmatterResult {
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
    let path = path_arg(args, "write_frontmatter")?;
    let key = args
        .get("key")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| exec_err("write_frontmatter: missing 'key' string".to_string()))?
        .to_string();
    // `value: null` removes the key (no-op when absent). Any other
    // type is rejected — we only round-trip scalars through the
    // user-facing add-property flow.
    let value: Option<String> = match args.get("value") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(other) => {
            return Err(exec_err(format!(
                "write_frontmatter: 'value' must be string or null, got {other:?}"
            )))
        }
    };
    let current = std::fs::read_to_string(forge_root.join(&path))
        .map_err(|e| exec_err(format!("write_frontmatter: read: {e}")))?;
    let next = crate::core_plugin::apply_frontmatter_edit(&current, &key, value.as_deref());
    engine
        .write_file(&path, next.as_bytes())
        .map_err(|e| exec_err(format!("write_frontmatter: write: {e}")))?;
    Ok(serde_json::json!({ "ok": true }))
}
