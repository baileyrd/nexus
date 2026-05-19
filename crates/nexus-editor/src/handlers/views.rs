//! View / lookup handlers: `execute_database_view`,
//! `resolve_block_link` (sync + async), `open_excerpts`,
//! `refresh_excerpts`. Includes the `pub(crate)` `excerpt_sources`
//! helper used by shell subscribers + the BL-141 multibuffer
//! support code (`merge_excerpt_requests`, `slice_lines`).
//!
//! Lifted from `core_plugin.rs` by SD-03 editor chunk 4
//! (2026-05-18 SOLID/DRY audit).

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use nexus_kernel::{Ipc as _, EventBus, KernelPluginContext};
use nexus_plugins::PluginError;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::block::{Block, BlockType};
use crate::core_plugin::{ExcerptRequest, Session, SessionMap};
use crate::markdown::{MarkdownParser, ParseOptions};
use crate::tree::BlockTree;
use crate::undo_tree::UndoTree;

use super::shared::{
    acquire_session_entry, exec_err, get_session_entry, insert_session_entry, publish_changed,
    relpath_arg, resolve_within, sessions_poisoned, snapshot_of, snapshot_to_value,
    DATABASE_PLUGIN_ID, MULTIBUFFER_RELPATH_PREFIX, STORAGE_IPC_TIMEOUT, STORAGE_PLUGIN_ID,
};

// ── execute_database_view ────────────────────────────────────────────────────

/// Execute an inline `[[{db:query}]]` view: load the base, translate
/// the editor-side [`crate::DatabaseViewConfig`] to a structured
/// [`nexus_types::bases::BaseView`], and run it through
/// `com.nexus.database::apply_view`.
///
/// Requires a wired [`KernelPluginContext`] — there is no fallback
/// path because both lookups are kernel-mediated. Returns
/// [`crate::database_view::ExecuteDatabaseViewResponse`] as JSON.
pub(crate) async fn execute_database_view(
    ctx: Option<Arc<KernelPluginContext>>,
    args: &Value,
) -> Result<Value, PluginError> {
    #[derive(Deserialize)]
    struct LoadedBase {
        schema: nexus_types::bases::BaseSchema,
        records: Vec<nexus_types::bases::BaseRecord>,
    }

    let parsed: crate::database_view::ExecuteDatabaseViewArgs = serde_json::from_value(args.clone())
        .map_err(|e| exec_err(format!("execute_database_view: invalid args: {e}")))?;

    let ctx = ctx.ok_or_else(|| {
        exec_err(
            "execute_database_view: no kernel context wired (this handler \
             cannot run in context-less unit tests)"
                .to_string(),
        )
    })?;

    // 1. Load the base through storage.
    let base_value = ctx
        .ipc_call(
            STORAGE_PLUGIN_ID,
            "base_load",
            serde_json::json!({ "path": parsed.database_path }),
            STORAGE_IPC_TIMEOUT,
        )
        .await
        .map_err(|e| exec_err(format!("execute_database_view: storage.base_load: {e}")))?;

    // The `base_load` handler returns a [`nexus_types::bases::Base`] —
    // we only need its schema + records here.
    let LoadedBase { schema, records } = serde_json::from_value(base_value).map_err(|e| {
        exec_err(format!(
            "execute_database_view: decode base_load response: {e}"
        ))
    })?;

    // 2. Translate config → structured view.
    let view = crate::database_view::config_to_view(&parsed.database_path, &parsed.view_config)
        .map_err(|e| exec_err(format!("execute_database_view: {e}")))?;

    // 3. Apply via the database plugin.
    let applied = ctx
        .ipc_call(
            DATABASE_PLUGIN_ID,
            "apply_view",
            serde_json::json!({
                "records": records,
                "schema": schema,
                "view": view,
            }),
            STORAGE_IPC_TIMEOUT,
        )
        .await
        .map_err(|e| exec_err(format!("execute_database_view: database.apply_view: {e}")))?;

    serde_json::to_value(crate::database_view::ExecuteDatabaseViewResponse { applied, schema })
        .map_err(|e| exec_err(format!("execute_database_view: serialize response: {e}")))
}

// ── resolve_block_link ───────────────────────────────────────────────────────

/// Resolve `block_id` against the in-memory session for `relpath`
/// when one is open, returning the lookup result with the root
/// ancestor's index in `tree.root_blocks`. Returns `Ok(None)` when
/// no session exists for `relpath`; the caller falls back to a
/// fresh parse.
///
/// BL-073: when the resolved block has no [`crate::Block::stable_id`]
/// yet, auto-stamp it via [`BlockTree::rekey`] so the next save
/// persists a `<!-- ^<uuid> -->` marker. The new stable id is what
/// the lookup returns. The second tuple element is `Some(revision)`
/// if a stamp happened (caller publishes a `changed` event), `None`
/// otherwise. The filesystem-fallback path used for closed sessions
/// deliberately does **not** auto-stamp — silently mutating the
/// on-disk file from a read-shaped IPC call would be a surprise.
fn resolve_in_session(
    sessions: &SessionMap,
    relpath: &str,
    block_id: uuid::Uuid,
) -> Result<Option<(Value, Option<u64>)>, PluginError> {
    let Some(entry) = get_session_entry(sessions, relpath)? else {
        return Ok(None);
    };
    let mut s = entry.lock().map_err(|_| sessions_poisoned())?;
    let needs_stamp = matches!(
        s.tree.get(block_id),
        Some(block) if block.stable_id.is_none()
    );
    if needs_stamp {
        let new_id = uuid::Uuid::new_v4();
        s.tree
            .rekey(block_id, new_id)
            .map_err(|e| exec_err(format!("resolve_block_link: auto-stamp rekey: {e}")))?;
        s.revision = s.revision.saturating_add(1);
        let value = resolve_in_tree(&s.tree, new_id);
        return Ok(Some((value, Some(s.revision))));
    }
    Ok(Some((resolve_in_tree(&s.tree, block_id), None)))
}

/// Walk `tree.root_blocks` to find which root ancestor contains
/// `block_id`, returning the lookup payload as JSON. Pure — does
/// not consult any session map.
pub(crate) fn resolve_in_tree(tree: &BlockTree, block_id: uuid::Uuid) -> Value {
    let Some(block) = tree.get(block_id) else {
        return serde_json::json!({
            "found": false,
            "block": null,
            "root_index": null,
        });
    };

    // Walk parents up to a root block.
    let mut cursor = block;
    while let Some(parent_id) = cursor.parent_id {
        match tree.get(parent_id) {
            Some(parent) => cursor = parent,
            None => break,
        }
    }
    let root_index = tree.root_blocks.iter().position(|id| *id == cursor.id);

    serde_json::json!({
        "found": true,
        "block": block,
        "root_index": root_index,
    })
}

fn parse_resolve_args(args: &Value) -> Result<(String, uuid::Uuid), PluginError> {
    let relpath = args
        .get("file_relpath")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| exec_err("resolve_block_link: missing 'file_relpath' string".to_string()))?;
    let block_id_str = args
        .get("block_id")
        .and_then(Value::as_str)
        .ok_or_else(|| exec_err("resolve_block_link: missing 'block_id' string".to_string()))?;
    let block_id = uuid::Uuid::parse_str(block_id_str)
        .map_err(|e| exec_err(format!("resolve_block_link: invalid 'block_id': {e}")))?;
    Ok((relpath, block_id))
}

pub(crate) fn resolve_block_link_sync(
    forge_root: &Path,
    sessions: &SessionMap,
    args: &Value,
) -> Result<Value, PluginError> {
    let (relpath, block_id) = parse_resolve_args(args)?;

    if let Some((value, _stamp_revision)) = resolve_in_session(sessions, &relpath, block_id)? {
        // The sync entry point is unit-test-only (no kernel context →
        // no event bus). The async path below publishes a changed
        // event when an auto-stamp happens.
        return Ok(value);
    }

    // No open session — read + parse transiently. Same fs fallback
    // path as `open_sync` (production traffic goes through the async
    // path via the kernel runtime).
    let abs = resolve_within(forge_root, &relpath)
        .map_err(|e| exec_err(format!("resolve_block_link: {e}")))?;
    let source = fs::read_to_string(&abs)
        .map_err(|e| exec_err(format!("resolve_block_link: read '{}': {e}", abs.display())))?;
    let parser = MarkdownParser::new(ParseOptions {
        file_path: relpath.clone(),
        ..ParseOptions::default()
    });
    let tree = parser
        .parse(&source)
        .map_err(|e| exec_err(format!("resolve_block_link: parse '{relpath}': {e}")))?;
    Ok(resolve_in_tree(&tree, block_id))
}

pub(crate) async fn resolve_block_link_async(
    forge_root: &Path,
    sessions: Arc<SessionMap>,
    ctx: Option<Arc<KernelPluginContext>>,
    event_bus: Option<&Arc<EventBus>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let (relpath, block_id) = parse_resolve_args(args)?;

    if let Some((value, stamp_revision)) = resolve_in_session(&sessions, &relpath, block_id)? {
        if let Some(revision) = stamp_revision {
            publish_changed(event_bus, &relpath, revision, None);
        }
        return Ok(value);
    }

    let source = if let Some(ctx) = ctx.as_deref() {
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
            .map_err(|e| exec_err(format!("resolve_block_link: storage.read_file: {e}")))?;
        let resp: Resp = serde_json::from_value(value)
            .map_err(|e| exec_err(format!("resolve_block_link: storage.read_file decode: {e}")))?;
        let bytes = resp
            .bytes
            .ok_or_else(|| exec_err(format!("resolve_block_link: file not found: '{relpath}'")))?;
        String::from_utf8(bytes)
            .map_err(|_| exec_err(format!("resolve_block_link: '{relpath}' is not UTF-8")))?
    } else {
        let abs = resolve_within(forge_root, &relpath)
            .map_err(|e| exec_err(format!("resolve_block_link: {e}")))?;
        fs::read_to_string(&abs)
            .map_err(|e| exec_err(format!("resolve_block_link: read '{}': {e}", abs.display())))?
    };

    let parser = MarkdownParser::new(ParseOptions {
        file_path: relpath.clone(),
        ..ParseOptions::default()
    });
    let tree = parser
        .parse(&source)
        .map_err(|e| exec_err(format!("resolve_block_link: parse '{relpath}': {e}")))?;
    Ok(resolve_in_tree(&tree, block_id))
}

// ── BL-141 — open_excerpts / refresh_excerpts ───────────────────────────────

/// Implementation of `HANDLER_OPEN_EXCERPTS`. Assembles a synthetic
/// read-only session whose root blocks are
/// [`crate::BlockType::Excerpt`] entries, one per merged input item.
///
/// Source files are read via `com.nexus.storage::read_file` (with a
/// local-fs fallback for context-less unit tests). Per-source files
/// are read once even if the input lists multiple ranges from the
/// same path.
pub(crate) async fn open_excerpts(
    forge_root: &Path,
    sessions: Arc<SessionMap>,
    ctx: Option<Arc<KernelPluginContext>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let items_value = args
        .get("items")
        .ok_or_else(|| exec_err("open_excerpts: missing 'items'".to_string()))?
        .clone();
    let raw_items: Vec<ExcerptRequest> = serde_json::from_value(items_value)
        .map_err(|e| exec_err(format!("open_excerpts: invalid 'items': {e}")))?;
    if raw_items.is_empty() {
        return Err(exec_err(
            "open_excerpts: 'items' must be non-empty".to_string(),
        ));
    }

    // Validate each item's line range up front; cheaper to fail fast
    // than to pay for storage reads on a doomed call.
    for (idx, item) in raw_items.iter().enumerate() {
        if item.line_start == 0 || item.line_end == 0 {
            return Err(exec_err(format!(
                "open_excerpts: item {idx} has zero line number (1-based)"
            )));
        }
        if item.line_start > item.line_end {
            return Err(exec_err(format!(
                "open_excerpts: item {idx} has line_start ({}) > line_end ({})",
                item.line_start, item.line_end
            )));
        }
    }

    // Walk in input order; for each item merge with any previously-kept
    // entry from the same source file whose range touches or overlaps.
    // Preserves first-seen order across the assembled view, which
    // matters for diagnostics / find-refs lists that flow in a
    // meaningful sequence (e.g. file → file → file).
    let merged = merge_excerpt_requests(raw_items);

    // Read every unique source file exactly once. Per-file failures
    // abort the call — a partial multibuffer is more confusing than a
    // clear error.
    let mut sources: HashMap<String, String> = HashMap::new();
    for item in &merged {
        if sources.contains_key(&item.relpath) {
            continue;
        }
        let source = super::save::read_source_for_excerpts(forge_root, ctx.as_deref(), &item.relpath)
            .await?;
        sources.insert(item.relpath.clone(), source);
    }

    // Build the synthetic block tree.
    let mut tree = BlockTree::default();
    for item in merged {
        let source = sources.get(&item.relpath).expect("just inserted");
        let snippet = slice_lines(source, item.line_start, item.line_end);
        let block = Block::new(BlockType::Excerpt {
            source_relpath: item.relpath.clone(),
            line_start: item.line_start,
            line_end: item.line_end,
            label: item.label.clone(),
        })
        .with_content(snippet);
        let next_idx = tree.root_blocks.len();
        tree.insert(block, None, next_idx)
            .map_err(|e| exec_err(format!("open_excerpts: tree insert: {e}")))?;
    }

    let synthetic_relpath = format!("{MULTIBUFFER_RELPATH_PREFIX}{}", Uuid::new_v4());
    let session = Session {
        tree,
        undo: UndoTree::new(),
        relpath: synthetic_relpath.clone(),
        revision: 0,
        is_synthetic: true,
    };

    let entry = insert_session_entry(&sessions, synthetic_relpath, session)?;
    let s = entry.lock().map_err(|_| sessions_poisoned())?;
    snapshot_to_value(&snapshot_of(&s), "open_excerpts")
}

/// Implementation of `HANDLER_REFRESH_EXCERPTS`. Re-reads every
/// source file referenced by a synthetic session's Excerpt blocks
/// and replaces each block's content with the source's current
/// slice. In-place mutation preserves block ids so any cursor state
/// the shell is tracking against this multibuffer stays valid.
///
/// Errors:
/// - `relpath` not in the session map.
/// - Session isn't synthetic (no excerpts to refresh).
/// - Source read fails.
pub(crate) async fn refresh_excerpts(
    forge_root: &Path,
    sessions: Arc<SessionMap>,
    ctx: Option<Arc<KernelPluginContext>>,
    event_bus: Option<&Arc<EventBus>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "refresh_excerpts")?;
    let entry = acquire_session_entry(&sessions, &relpath, "refresh_excerpts")?;

    // Collect the unique source relpaths without holding the lock
    // across the awaits. The lock is briefly re-acquired below to
    // splice the new snippets into the tree.
    let sources_to_read: Vec<String> = {
        let guard = entry.lock().map_err(|_| sessions_poisoned())?;
        if !guard.is_synthetic {
            return Err(exec_err(format!(
                "refresh_excerpts: session '{relpath}' is not a multibuffer"
            )));
        }
        excerpt_sources(&guard.tree)
    };
    if sources_to_read.is_empty() {
        // No-op refresh: still bump revision so callers can observe
        // completion via the event bus.
        let mut guard = entry.lock().map_err(|_| sessions_poisoned())?;
        guard.revision = guard.revision.saturating_add(1);
        let rev = guard.revision;
        let value = snapshot_to_value(&snapshot_of(&guard), "refresh_excerpts")?;
        drop(guard);
        publish_changed(event_bus, &relpath, rev, None);
        return Ok(value);
    }

    let mut fresh: HashMap<String, String> = HashMap::with_capacity(sources_to_read.len());
    for src in &sources_to_read {
        let text = super::save::read_source_for_excerpts(forge_root, ctx.as_deref(), src).await?;
        fresh.insert(src.clone(), text);
    }

    let (value, revision) = {
        let mut guard = entry.lock().map_err(|_| sessions_poisoned())?;
        let s: &mut Session = &mut guard;
        for id in s.tree.root_blocks.clone() {
            let Some(block) = s.tree.get_mut(id) else {
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
            let Some(source) = fresh.get(source_relpath) else {
                continue;
            };
            let snippet = slice_lines(source, *line_start, *line_end);
            if block.content == snippet {
                // Source at stored range still matches the snapshot —
                // no edit landed in this excerpt's window. Nothing to
                // do.
                continue;
            }
            // BL-141 Approach B step 4B — content-anchored relocation.
            // Try to find the snapshot's current text as a unique
            // contiguous line sequence in the new source. If found,
            // update the anchors and keep the snapshot intact (the
            // user is still looking at the same lines, just at a new
            // line number). If ambiguous / missing, fall back to
            // slice-and-overwrite — the user sees the latest source
            // content at the original line numbers, same as step 3's
            // baseline behaviour.
            let relocated = if !block.content.is_empty() {
                super::save::relocate_excerpt_by_content(source, &block.content)
            } else {
                None
            };
            if let Some((new_start, new_end)) = relocated {
                if let BlockType::Excerpt {
                    line_start: ls,
                    line_end: le,
                    ..
                } = &mut block.ty
                {
                    *ls = new_start;
                    *le = new_end;
                }
                // content already matches — skip overwrite.
            } else {
                block.content = snippet;
            }
        }
        s.revision = s.revision.saturating_add(1);
        let rev = s.revision;
        let value = snapshot_to_value(&snapshot_of(s), "refresh_excerpts")?;
        (value, rev)
    };
    publish_changed(event_bus, &relpath, revision, None);
    Ok(value)
}

/// Unique source relpaths referenced by every `Excerpt` block in
/// `tree`, in first-appearance order across `root_blocks`. Pure —
/// exported (crate-internal) so the shell-side subscriber can ask
/// "which sources does this multibuffer cover?" without re-walking
/// the tree itself.
pub(crate) fn excerpt_sources(tree: &BlockTree) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for id in &tree.root_blocks {
        let Some(block) = tree.blocks.get(id) else {
            continue;
        };
        if let BlockType::Excerpt { source_relpath, .. } = &block.ty {
            if seen.insert(source_relpath.as_str()) {
                out.push(source_relpath.clone());
            }
        }
    }
    out
}

/// Merge per-source-file overlapping or adjacent ranges in
/// first-appearance order. Two ranges `(a_s..=a_e)` and `(b_s..=b_e)`
/// merge when `b_s <= a_e + 1` (i.e. touching counts as overlapping).
/// Labels from later-merged-in items are dropped — the first item's
/// label wins, matching the first-appearance semantic.
fn merge_excerpt_requests(items: Vec<ExcerptRequest>) -> Vec<ExcerptRequest> {
    let mut merged: Vec<ExcerptRequest> = Vec::new();
    for item in items {
        let mut consumed = false;
        for existing in &mut merged {
            if existing.relpath != item.relpath {
                continue;
            }
            let touches = item.line_start <= existing.line_end.saturating_add(1)
                && existing.line_start <= item.line_end.saturating_add(1);
            if touches {
                existing.line_start = existing.line_start.min(item.line_start);
                existing.line_end = existing.line_end.max(item.line_end);
                consumed = true;
                break;
            }
        }
        if !consumed {
            merged.push(item);
        }
    }
    merged
}

/// Extract the inclusive 1-based line range `[start, end]` from
/// `source`, joining with `\n`. Out-of-range start clamps to an
/// empty string; out-of-range end clamps to the source's last line.
fn slice_lines(source: &str, start: u32, end: u32) -> String {
    let total_usize = source.lines().count();
    let total = u32::try_from(total_usize).unwrap_or(u32::MAX);
    if start > total {
        return String::new();
    }
    let start_idx = (start - 1) as usize;
    let end_idx = (end.min(total) - 1) as usize;
    source
        .lines()
        .skip(start_idx)
        .take(end_idx - start_idx + 1)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod excerpt_sources_tests {
    use super::excerpt_sources;
    use crate::block::{Block, BlockType};
    use crate::tree::BlockTree;

    fn excerpt(source_relpath: &str, line_start: u32, line_end: u32) -> Block {
        Block::new(BlockType::Excerpt {
            source_relpath: source_relpath.to_string(),
            line_start,
            line_end,
            label: None,
        })
    }

    #[test]
    fn excerpt_sources_empty_tree_returns_empty() {
        let tree = BlockTree::default();
        assert!(excerpt_sources(&tree).is_empty());
    }

    #[test]
    fn excerpt_sources_dedupes_in_first_appearance_order() {
        let mut tree = BlockTree::default();
        for (i, b) in [
            excerpt("a.md", 1, 5),
            excerpt("b.md", 1, 5),
            excerpt("a.md", 10, 15),
            excerpt("c.md", 1, 5),
            excerpt("b.md", 20, 30),
        ]
        .into_iter()
        .enumerate()
        {
            tree.insert(b, None, i).unwrap();
        }
        assert_eq!(excerpt_sources(&tree), vec!["a.md", "b.md", "c.md"]);
    }

    #[test]
    fn excerpt_sources_skips_non_excerpt_root_blocks() {
        let mut tree = BlockTree::default();
        tree.insert(Block::new(BlockType::Paragraph).with_content("nope"), None, 0)
            .unwrap();
        tree.insert(excerpt("only.md", 1, 5), None, 1).unwrap();
        assert_eq!(excerpt_sources(&tree), vec!["only.md"]);
    }
}
