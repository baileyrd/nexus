//! Save handlers and BL-141 excerpt-splice machinery: `save` (sync +
//! async), `plan_save`, `splice_excerpts`, `atomic_write`,
//! `apply_reflow_after_save`, plus the `pub(crate)` helpers
//! `relocate_excerpt_by_content` / `reflow_excerpt_ranges_for_source`
//! / `read_source_for_excerpts` used by the excerpts views handlers.
//!
//! Lifted from `core_plugin.rs` by SD-03 editor chunk 3
//! (2026-05-18 SOLID/DRY audit).

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use nexus_kernel::{Ipc as _, KernelPluginContext};
use nexus_plugins::PluginError;
use serde::Deserialize;
use serde_json::Value;

use crate::block::BlockType;
use crate::core_plugin::SessionMap;
use crate::markdown::MarkdownSerializer;

use super::shared::{
    acquire_session_entry, exec_err, relpath_arg, resolve_within, sessions_poisoned,
    STORAGE_IPC_TIMEOUT, STORAGE_PLUGIN_ID,
};

/// BL-141 Phase 2 — one excerpt to splice back into its source
/// file's line range. Built by `plan_save` for synthetic sessions;
/// applied by `splice_excerpts` after the source is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExcerptSplice {
    /// First line of the range to replace (1-based, inclusive).
    pub(crate) line_start: u32,
    /// Last line of the range to replace (1-based, inclusive).
    pub(crate) line_end: u32,
    /// New content (the user-edited multibuffer text for this
    /// excerpt). May contain embedded newlines.
    pub(crate) new_content: String,
}

/// BL-141 Phase 2 — what to do for a single `save` dispatch.
enum SavePlan {
    /// Regular session — write the canonical markdown serialization
    /// to `relpath`.
    Regular { markdown: String },
    /// Synthetic (multibuffer) session — for each `(source_relpath,
    /// splices)` pair, read the source file, apply every splice in
    /// reverse-line-order, write back.
    Splice {
        sources: Vec<(String, Vec<ExcerptSplice>)>,
    },
}

/// BL-141 Approach B step 4A — after a synthetic session's save
/// lands, walk its Excerpt blocks and update each
/// `(line_start, line_end)` to reflect the post-splice positions
/// computed by [`reflow_excerpt_ranges_for_source`]. Keeps the
/// invariant `slice_lines(source, line_start, line_end) ==
/// block.content` true after a save that grew or shrank the content
/// — so the next `refresh_excerpts` is a no-op for unchanged
/// blocks, and so the next save's splices target the right lines.
fn apply_reflow_after_save(sessions: &SessionMap, relpath: &str) -> Result<(), PluginError> {
    let entry = acquire_session_entry(sessions, relpath, "save:reflow")?;
    let mut guard = entry.lock().map_err(|_| sessions_poisoned())?;
    if !guard.is_synthetic {
        return Ok(());
    }
    // Group block ids by source relpath, capturing input order so we
    // can map the reflow result back.
    // Transient grouping map; a named type alias would obscure more than clarify.
    #[allow(clippy::type_complexity)]
    let mut groups: HashMap<String, Vec<(usize, uuid::Uuid, u32, u32, u32)>> = HashMap::new();
    for (input_idx, block_id) in guard.tree.root_blocks.clone().into_iter().enumerate() {
        let Some(block) = guard.tree.blocks.get(&block_id) else {
            continue;
        };
        let BlockType::Excerpt {
            source_relpath,
            line_start,
            line_end,
            ..
        } = &block.ty
        else {
            continue;
        };
        let new_count = u32::try_from(block.content.lines().count()).unwrap_or(u32::MAX);
        groups.entry(source_relpath.clone()).or_default().push((
            input_idx,
            block_id,
            *line_start,
            *line_end,
            new_count,
        ));
    }
    for entries in groups.into_values() {
        let inputs: Vec<(u32, u32, u32)> = entries
            .iter()
            .map(|(_, _, start, end, count)| (*start, *end, *count))
            .collect();
        let outs = reflow_excerpt_ranges_for_source(&inputs);
        for ((_, block_id, _, _, _), (new_start, new_end)) in entries.iter().zip(outs) {
            let Some(block) = guard.tree.blocks.get_mut(block_id) else {
                continue;
            };
            if let BlockType::Excerpt {
                line_start,
                line_end,
                ..
            } = &mut block.ty
            {
                *line_start = new_start;
                *line_end = new_end;
            }
        }
    }
    Ok(())
}

/// Plan the save under the session lock (so the IPC handler holds
/// the lock for the minimum time + the actual I/O can happen
/// without contending with concurrent dispatches).
///
/// Splits the path: regular sessions return their serialized
/// markdown directly; synthetic sessions group their Excerpt
/// blocks by source file so the caller can issue exactly one
/// read+write per source even when multiple excerpts hit the same
/// file.
fn plan_save(sessions: &SessionMap, relpath: &str) -> Result<SavePlan, PluginError> {
    let entry = acquire_session_entry(sessions, relpath, "save")?;
    let s = entry.lock().map_err(|_| sessions_poisoned())?;
    if !s.is_synthetic {
        return Ok(SavePlan::Regular {
            markdown: MarkdownSerializer::serialize(&s.tree),
        });
    }
    let mut by_source: HashMap<String, Vec<ExcerptSplice>> = HashMap::new();
    for block_id in &s.tree.root_blocks {
        let Some(block) = s.tree.blocks.get(block_id) else {
            continue;
        };
        if let BlockType::Excerpt {
            source_relpath,
            line_start,
            line_end,
            ..
        } = &block.ty
        {
            by_source
                .entry(source_relpath.clone())
                .or_default()
                .push(ExcerptSplice {
                    line_start: *line_start,
                    line_end: *line_end,
                    new_content: block.content.clone(),
                });
        }
    }
    // Stable ordering across saves: sort source relpaths alphabetically
    // so test fixtures + audit logs see a deterministic write order.
    let mut sources: Vec<(String, Vec<ExcerptSplice>)> = by_source.into_iter().collect();
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(SavePlan::Splice { sources })
}

/// BL-141 Approach B step 4B — content-anchored relocation.
///
/// Find `needle` as a contiguous sequence of source lines in
/// `source`. Returns `Some((line_start, line_end))` (1-based,
/// inclusive) only when **exactly one** match exists — ambiguous or
/// missing matches return `None` so the caller can decide whether
/// to fall back (typically: overwrite the snapshot with the slice
/// at the stored line range).
///
/// Used by `handle_refresh_excerpts` to recover from external
/// prepends / inserts above an excerpt's line range. If another
/// tab inserts lines at the top of the source, the excerpt's
/// stored `(line_start, line_end)` would otherwise read the wrong
/// content; content-search finds the same lines at their new
/// position and updates the anchors.
///
/// Uniqueness requirement: a 1-line excerpt with common content
/// (`}`, `import x`) would match many places; refusing to relocate
/// on multi-match keeps the heuristic from silently jumping to the
/// wrong site. Empty `needle` returns `None` — the caller should
/// short-circuit before reaching this function.
pub(crate) fn relocate_excerpt_by_content(source: &str, needle: &str) -> Option<(u32, u32)> {
    if needle.is_empty() {
        return None;
    }
    let needle_lines: Vec<&str> = needle.lines().collect();
    if needle_lines.is_empty() {
        return None;
    }
    let source_lines: Vec<&str> = source.lines().collect();
    if source_lines.len() < needle_lines.len() {
        return None;
    }

    let mut hits = Vec::new();
    for (idx, window) in source_lines.windows(needle_lines.len()).enumerate() {
        if window == needle_lines.as_slice() {
            hits.push(idx);
            if hits.len() > 1 {
                // Bail early — ambiguous.
                return None;
            }
        }
    }
    if hits.len() != 1 {
        return None;
    }
    let start_0 = hits[0];
    let line_start = u32::try_from(start_0).ok()?.checked_add(1)?;
    let line_end = line_start
        .checked_add(u32::try_from(needle_lines.len()).ok()?)?
        .checked_sub(1)?;
    Some((line_start, line_end))
}

/// BL-141 Approach B step 4A — compute the post-save `(line_start,
/// line_end)` positions for every Excerpt in a single source file.
///
/// Inputs carry the original range + the post-edit line count of the
/// excerpt's content (typically `content.lines().count()`). Outputs
/// land in INPUT ORDER so callers can zip them back against their
/// own list. Internally the function sorts by `line_start` ascending
/// so the running delta accumulates the same way `splice_excerpts`
/// shifts source lines — earlier (smaller-line-start) excerpts grow
/// or shrink the source, and later excerpts' positions move by the
/// running net delta.
///
/// Empty post-edit content (the user deleted everything inside the
/// excerpt) collapses to a degenerate range `(start, start - 1)` —
/// `slice_lines` returns empty for `start > end`, so a subsequent
/// `refresh_excerpts` reads back as empty without crashing.
///
/// Pure (no I/O, no session lock). Exposed crate-internally so the
/// post-save hook can wire it up; exercised by `reflow_tests`.
pub(crate) fn reflow_excerpt_ranges_for_source(excerpts: &[(u32, u32, u32)]) -> Vec<(u32, u32)> {
    // Pair each excerpt with its input index so we can restore the
    // caller's ordering at the end.
    let mut by_start: Vec<(usize, u32, u32, u32)> = excerpts
        .iter()
        .enumerate()
        .map(|(i, &(start, end, new_count))| (i, start, end, new_count))
        .collect();
    by_start.sort_by_key(|t| t.1);

    let mut out: Vec<(u32, u32)> = vec![(0, 0); excerpts.len()];
    let mut delta: i64 = 0;
    for (idx, original_start, original_end, new_count) in by_start {
        // Shift the start by the running delta from earlier excerpts.
        let shifted_start = i64::from(original_start) + delta;
        let new_start = u32::try_from(shifted_start.max(1)).unwrap_or(u32::MAX);
        let new_end = if new_count == 0 {
            new_start.saturating_sub(1)
        } else {
            new_start.saturating_add(new_count.saturating_sub(1))
        };
        out[idx] = (new_start, new_end);

        // Accumulate the delta this excerpt contributed.
        let old_span = i64::from(original_end) - i64::from(original_start) + 1;
        delta += i64::from(new_count) - old_span;
    }
    out
}

/// Splice every entry in `splices` into `old`. Splices are applied
/// in reverse-line-order so earlier splices don't shift the line
/// numbers of later ones. Out-of-range entries (line_start past
/// `old`'s end) are skipped defensively rather than panicking — a
/// stale multibuffer (e.g. one whose source was edited externally
/// since the excerpt was captured) shouldn't crash the save.
///
/// Preserves the trailing newline if `old` ended with one, matching
/// the canonical `MarkdownSerializer::serialize` convention.
fn splice_excerpts(old: &str, mut splices: Vec<ExcerptSplice>) -> String {
    // Sort by line_start descending — splice from the END of the
    // file toward the start so a splice that grows or shrinks one
    // range never invalidates the line numbers of a not-yet-applied
    // splice further up the file.
    splices.sort_by_key(|b| std::cmp::Reverse(b.line_start));
    let trailing_nl = old.ends_with('\n');
    let mut lines: Vec<String> = old.lines().map(String::from).collect();
    for sp in splices {
        let start_idx = (sp.line_start.saturating_sub(1)) as usize;
        if start_idx > lines.len() {
            // Excerpt range starts past EOF — skip; preserving the
            // existing source content is better than appending the
            // edit into an arbitrary spot.
            continue;
        }
        let end_idx_exclusive = std::cmp::min(sp.line_end as usize, lines.len());
        let new_lines: Vec<String> = sp.new_content.lines().map(String::from).collect();
        lines.splice(start_idx..end_idx_exclusive, new_lines);
    }
    let mut out = lines.join("\n");
    if trailing_nl {
        out.push('\n');
    }
    out
}

pub(crate) fn save_sync(
    forge_root: &Path,
    sessions: &SessionMap,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "save")?;
    let plan = plan_save(sessions, &relpath)?;
    let is_splice = matches!(plan, SavePlan::Splice { .. });
    match plan {
        SavePlan::Regular { markdown } => {
            let abs =
                resolve_within(forge_root, &relpath).map_err(|e| exec_err(format!("save: {e}")))?;
            atomic_write(&abs, &markdown)
                .map_err(|e| exec_err(format!("save: write '{}': {e}", abs.display())))?;
        }
        SavePlan::Splice { sources } => {
            for (source_relpath, splices) in sources {
                let abs = resolve_within(forge_root, &source_relpath)
                    .map_err(|e| exec_err(format!("save: {e}")))?;
                let old = fs::read_to_string(&abs)
                    .map_err(|e| exec_err(format!("save: read source '{}': {e}", abs.display())))?;
                let new = splice_excerpts(&old, splices);
                atomic_write(&abs, &new).map_err(|e| {
                    exec_err(format!("save: write source '{}': {e}", abs.display()))
                })?;
            }
        }
    }
    if is_splice {
        apply_reflow_after_save(sessions, &relpath)?;
    }
    Ok(serde_json::json!({}))
}

pub(crate) async fn save_async(
    forge_root: &Path,
    sessions: Arc<SessionMap>,
    ctx: Option<Arc<KernelPluginContext>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "save")?;
    let plan = plan_save(&sessions, &relpath)?;
    let is_splice = matches!(plan, SavePlan::Splice { .. });

    if let Some(ctx) = ctx.as_deref() {
        // Canonical path: storage's `write_file` does temp + fsync +
        // rename and updates the SQLite index atomically with the disk
        // write so a later `open` sees consistent state.
        match plan {
            SavePlan::Regular { markdown } => {
                ctx.ipc_call(
                    STORAGE_PLUGIN_ID,
                    "write_file",
                    serde_json::json!({ "path": relpath, "bytes": markdown.as_bytes() }),
                    STORAGE_IPC_TIMEOUT,
                )
                .await
                .map_err(|e| exec_err(format!("save: storage.write_file: {e}")))?;
            }
            SavePlan::Splice { sources } => {
                for (source_relpath, splices) in sources {
                    let old =
                        read_source_for_excerpts(forge_root, Some(ctx), &source_relpath).await?;
                    let new = splice_excerpts(&old, splices);
                    ctx.ipc_call(
                        STORAGE_PLUGIN_ID,
                        "write_file",
                        serde_json::json!({
                            "path": source_relpath,
                            "bytes": new.as_bytes(),
                        }),
                        STORAGE_IPC_TIMEOUT,
                    )
                    .await
                    .map_err(|e| {
                        exec_err(format!("save: storage.write_file '{source_relpath}': {e}"))
                    })?;
                }
            }
        }
        if is_splice {
            apply_reflow_after_save(&sessions, &relpath)?;
        }
        Ok(serde_json::json!({}))
    } else {
        // Fallback for context-less unit tests — mirror the sync path
        // (direct local-fs writes).
        match plan {
            SavePlan::Regular { markdown } => {
                let abs = resolve_within(forge_root, &relpath)
                    .map_err(|e| exec_err(format!("save: {e}")))?;
                atomic_write(&abs, &markdown)
                    .map_err(|e| exec_err(format!("save: write '{}': {e}", abs.display())))?;
            }
            SavePlan::Splice { sources } => {
                for (source_relpath, splices) in sources {
                    let abs = resolve_within(forge_root, &source_relpath)
                        .map_err(|e| exec_err(format!("save: {e}")))?;
                    let old = fs::read_to_string(&abs).map_err(|e| {
                        exec_err(format!("save: read source '{}': {e}", abs.display()))
                    })?;
                    let new = splice_excerpts(&old, splices);
                    atomic_write(&abs, &new).map_err(|e| {
                        exec_err(format!("save: write source '{}': {e}", abs.display()))
                    })?;
                }
            }
        }
        if is_splice {
            apply_reflow_after_save(&sessions, &relpath)?;
        }
        Ok(serde_json::json!({}))
    }
}

/// Per-source-file read used by `open_excerpts` /
/// `refresh_excerpts` / `save` (splice mode). Mirrors the
/// `read_file` shape of `open_async` / `resolve_block_link_async`.
pub(crate) async fn read_source_for_excerpts(
    forge_root: &Path,
    ctx: Option<&KernelPluginContext>,
    relpath: &str,
) -> Result<String, PluginError> {
    if let Some(ctx) = ctx {
        #[derive(Deserialize)]
        struct Resp {
            bytes: Option<Vec<u8>>,
        }
        let value = ctx
            .ipc_call(
                STORAGE_PLUGIN_ID,
                "read_file",
                serde_json::json!({ "path": relpath }),
                STORAGE_IPC_TIMEOUT,
            )
            .await
            .map_err(|e| exec_err(format!("open_excerpts: storage.read_file '{relpath}': {e}")))?;
        let resp: Resp = serde_json::from_value(value).map_err(|e| {
            exec_err(format!(
                "open_excerpts: storage.read_file decode '{relpath}': {e}"
            ))
        })?;
        let bytes = resp
            .bytes
            .ok_or_else(|| exec_err(format!("open_excerpts: file not found: '{relpath}'")))?;
        String::from_utf8(bytes)
            .map_err(|_| exec_err(format!("open_excerpts: '{relpath}' is not UTF-8")))
    } else {
        let abs = resolve_within(forge_root, relpath)
            .map_err(|e| exec_err(format!("open_excerpts: {e}")))?;
        fs::read_to_string(&abs)
            .map_err(|e| exec_err(format!("open_excerpts: read '{}': {e}", abs.display())))
    }
}

/// Write `contents` to `path` via a sibling `.tmp` + fsync + rename.
///
/// Only used when the plugin is driven without a [`KernelPluginContext`]
/// (unit tests); production saves route through `com.nexus.storage` via
/// [`save_async`] and get its fuller atomic-write guarantees.
/// Even here we fsync the temp file (so a crash between write and
/// rename never leaves a half-flushed file visible via the rename) —
/// the pre-refactor version skipped the fsync entirely.
///
/// Parent-directory fsync is best-effort: `File::sync_all` on a
/// directory is a no-op on Windows but persists the rename on POSIX.
fn atomic_write(path: &Path, contents: &str) -> Result<(), String> {
    use std::io::Write as _;

    let parent = path
        .parent()
        .ok_or_else(|| format!("no parent dir for '{}'", path.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("no filename in '{}'", path.display()))?;
    let tmp = parent.join(format!(".{}.tmp", file_name.to_string_lossy()));

    // Write + flush + fsync the temp file.
    {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        f.write_all(contents.as_bytes())
            .map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }

    // Atomic rename into place.
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;

    // Best-effort directory fsync so the rename itself is durable.
    // Silently ignore failures — Windows returns an error when opening
    // a directory for writing, and on POSIX the worst case is that the
    // rename is replayed by the filesystem journal anyway.
    if let Ok(dir) = fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod splice_tests {
    use super::{splice_excerpts, ExcerptSplice};

    fn sp(line_start: u32, line_end: u32, new_content: &str) -> ExcerptSplice {
        ExcerptSplice {
            line_start,
            line_end,
            new_content: new_content.to_string(),
        }
    }

    #[test]
    fn splice_single_range_preserves_surrounding_lines() {
        let old = "L1\nL2\nL3\nL4\nL5\n";
        let got = splice_excerpts(old, vec![sp(2, 4, "X\nY")]);
        assert_eq!(got, "L1\nX\nY\nL5\n");
    }

    #[test]
    fn splice_preserves_no_trailing_newline_when_source_had_none() {
        let old = "L1\nL2\nL3";
        let got = splice_excerpts(old, vec![sp(2, 2, "MID")]);
        assert_eq!(got, "L1\nMID\nL3");
    }

    #[test]
    fn splice_handles_growing_replacement() {
        let old = "A\nB\nC\n";
        let got = splice_excerpts(old, vec![sp(2, 2, "B1\nB2\nB3")]);
        assert_eq!(got, "A\nB1\nB2\nB3\nC\n");
    }

    #[test]
    fn splice_handles_shrinking_replacement() {
        let old = "A\nB\nC\nD\nE\n";
        let got = splice_excerpts(old, vec![sp(2, 4, "B+D")]);
        assert_eq!(got, "A\nB+D\nE\n");
    }

    #[test]
    fn splice_processes_multiple_ranges_in_reverse_order() {
        // Two non-overlapping splices. The first one (lines 1-2) is
        // a 2→1 line shrink; the second (lines 4-5) replaces with
        // a 2→3 line grow. If we applied them in input order the
        // first splice would shift the second's target range and
        // we'd splice the wrong lines. Reverse-order processing
        // dodges this entirely.
        let old = "A\nB\nC\nD\nE\n";
        let got = splice_excerpts(old, vec![sp(1, 2, "AB"), sp(4, 5, "DE1\nDE2\nDE3")]);
        assert_eq!(got, "AB\nC\nDE1\nDE2\nDE3\n");
    }

    #[test]
    fn splice_out_of_range_start_is_skipped_defensively() {
        // The excerpt range starts past EOF — e.g. a stale
        // multibuffer whose source was truncated externally. Better
        // to preserve the existing source than to append the edit
        // into an arbitrary spot.
        let old = "A\nB\n";
        let got = splice_excerpts(old, vec![sp(10, 12, "ignored")]);
        assert_eq!(got, "A\nB\n");
    }

    #[test]
    fn splice_clamps_end_at_eof() {
        let old = "A\nB\nC\n";
        let got = splice_excerpts(old, vec![sp(2, 99, "X")]);
        assert_eq!(got, "A\nX\n");
    }
}

#[cfg(test)]
mod relocate_tests {
    use super::relocate_excerpt_by_content;

    #[test]
    fn relocate_finds_unique_multi_line_match_at_top() {
        let src = "Original line A\nOriginal line B\nOriginal line C\nother\n";
        let needle = "Original line A\nOriginal line B\nOriginal line C";
        assert_eq!(relocate_excerpt_by_content(src, needle), Some((1, 3)));
    }

    #[test]
    fn relocate_finds_unique_match_after_external_prepend() {
        let src = "NEW header 1\nNEW header 2\nOriginal A\nOriginal B\nOriginal C\nfooter\n";
        let needle = "Original A\nOriginal B\nOriginal C";
        // Match moves from lines 1..=3 to lines 3..=5.
        assert_eq!(relocate_excerpt_by_content(src, needle), Some((3, 5)));
    }

    #[test]
    fn relocate_returns_none_for_ambiguous_match() {
        // Same 2-line sequence appears twice — refuse to guess.
        let src = "X\nY\nzzz\nX\nY\n";
        let needle = "X\nY";
        assert_eq!(relocate_excerpt_by_content(src, needle), None);
    }

    #[test]
    fn relocate_returns_none_when_needle_missing() {
        let src = "alpha\nbeta\ngamma\n";
        let needle = "not present";
        assert_eq!(relocate_excerpt_by_content(src, needle), None);
    }

    #[test]
    fn relocate_returns_none_for_empty_needle() {
        let src = "anything\n";
        assert_eq!(relocate_excerpt_by_content(src, ""), None);
    }

    #[test]
    fn relocate_handles_single_line_needle_when_unique() {
        let src = "alpha\nbeta unique line\ngamma\n";
        let needle = "beta unique line";
        assert_eq!(relocate_excerpt_by_content(src, needle), Some((2, 2)));
    }

    #[test]
    fn relocate_returns_none_when_needle_longer_than_source() {
        let src = "x\n";
        let needle = "x\ny\nz";
        assert_eq!(relocate_excerpt_by_content(src, needle), None);
    }
}

#[cfg(test)]
mod reflow_tests {
    use super::reflow_excerpt_ranges_for_source;

    #[test]
    fn reflow_unchanged_excerpts_yields_identity() {
        // (line_start, line_end, new_count). Same line counts as
        // before → no drift.
        let got = reflow_excerpt_ranges_for_source(&[(2, 4, 3), (8, 10, 3)]);
        assert_eq!(got, vec![(2, 4), (8, 10)]);
    }

    #[test]
    fn reflow_growing_earlier_excerpt_shifts_later_ones() {
        // E1 was 3 lines (2..=4), now 5 lines.
        // E2 was 3 lines (8..=10), unchanged in count → shifts by +2.
        let got = reflow_excerpt_ranges_for_source(&[(2, 4, 5), (8, 10, 3)]);
        assert_eq!(got, vec![(2, 6), (10, 12)]);
    }

    #[test]
    fn reflow_shrinking_earlier_excerpt_shifts_later_ones_negative() {
        // E1 was 3 lines (2..=4), now 1 line. Delta -2.
        // E2 was 1 line (8..=8), unchanged in count → shifts by -2.
        let got = reflow_excerpt_ranges_for_source(&[(2, 4, 1), (8, 8, 1)]);
        assert_eq!(got, vec![(2, 2), (6, 6)]);
    }

    #[test]
    fn reflow_preserves_input_order_when_inputs_are_unsorted() {
        // Same physical positions as test 2 but inputs out of order.
        let got = reflow_excerpt_ranges_for_source(&[(8, 10, 3), (2, 4, 5)]);
        assert_eq!(got, vec![(10, 12), (2, 6)]);
    }

    #[test]
    fn reflow_empty_content_yields_degenerate_range() {
        // Empty content (0 lines) ⇒ (start, start-1).
        let got = reflow_excerpt_ranges_for_source(&[(5, 7, 0)]);
        assert_eq!(got, vec![(5, 4)]);
    }

    #[test]
    fn reflow_handles_three_excerpts_with_mixed_deltas() {
        // E1 (2..=4): 3 → 5  (delta +2)
        // E2 (8..=10): 3 → 3 (delta 0; shifted by +2)
        // E3 (15..=15): 1 → 0 (delta -1; shifted by +2)
        let got = reflow_excerpt_ranges_for_source(&[(2, 4, 5), (8, 10, 3), (15, 15, 0)]);
        assert_eq!(got, vec![(2, 6), (10, 12), (17, 16)]);
    }
}
