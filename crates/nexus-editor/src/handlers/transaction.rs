//! Transaction handlers: `apply_transaction`, `undo`, `redo`. Includes
//! the BL-073 inbound-link auto-stamper and the BL-126 payload-size
//! pre-check.
//!
//! Lifted from `core_plugin.rs` by SD-03 editor chunk 3
//! (2026-05-18 SOLID/DRY audit).

use std::sync::Arc;

use nexus_kernel::EventBus;
use nexus_plugins::PluginError;
use serde_json::Value;

use crate::block::BlockType;
use crate::core_plugin::{ApplyTransactionResponse, OpObserver, Session, SessionMap};
use crate::tree::BlockTree;

use super::shared::{
    acquire_session_entry, exec_err, publish_changed, relpath_arg, sessions_poisoned, snapshot_of,
    snapshot_to_value,
};

/// BL-122 / BL-126 cap on the user-controlled payload bytes of a
/// single transaction.
const MAX_TRANSACTION_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn apply_transaction(
    sessions: &SessionMap,
    event_bus: Option<&Arc<EventBus>>,
    observer: Option<&Arc<dyn OpObserver>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "apply_transaction")?;
    let tx_value = args
        .get("transaction")
        .ok_or_else(|| exec_err("apply_transaction: missing 'transaction'".to_string()))?
        .clone();
    // Cap the transaction payload before applying. Issue #85's
    // original implementation re-serialized `tx_value` via
    // `serde_json::to_vec(&tx_value)` purely to count JSON bytes —
    // ~10–20% of the BL-122-measured small-tx latency was spent in
    // that throwaway serialize on the typing hot path. BL-126
    // replaces it with a structural sum (op-by-op string-length
    // tally on the typed `Transaction`); cheaper and bounded against
    // the same conceptual 16 MiB limit. A small headroom margin
    // accounts for JSON's per-string overhead — the limit is on
    // bytes of user-controlled payload, not exact JSON-serialized
    // bytes, so we under-bound rather than over-bound to keep the
    // safety property.
    let tx: crate::Transaction = serde_json::from_value(tx_value)
        .map_err(|e| exec_err(format!("apply_transaction: invalid transaction: {e}")))?;
    let tx_size = transaction_payload_size(&tx);
    if tx_size > MAX_TRANSACTION_BYTES {
        return Err(exec_err(format!(
            "apply_transaction: transaction payload is {tx_size} bytes; \
             max is {MAX_TRANSACTION_BYTES} bytes"
        )));
    }
    let tx_id = tx.id;
    let op_count = tx.operations.len();
    // BL-123: text-only ops (insert_text / delete_text) get a slim
    // response. UpdateAnnotations stays on the full path because the
    // bridge's optimistic mirror doesn't track annotations — the
    // snapshot is the only authoritative source for the post-apply
    // annotation list.
    let text_only = !tx.operations.is_empty()
        && tx.operations.iter().all(|op| {
            matches!(
                op,
                crate::Operation::InsertText { .. } | crate::Operation::DeleteText { .. }
            )
        });

    // BL-122 / BL-126: instrumentation span for the typing-latency
    // perf harness. Records per-call op count + transaction payload
    // bytes (structural sum of insert/delete/content strings), and
    // (after serialize) bytes-out. A subscriber installed at `info`
    // level captures wall-time via `span.enter()`/exit; with no
    // subscriber the span is a no-op pointer bump.
    let span = tracing::info_span!(
        "apply_transaction",
        op_count,
        text_only,
        payload_bytes = tx_size,
        bytes_out = tracing::field::Empty,
    );
    let _enter = span.enter();

    let entry = acquire_session_entry(sessions, &relpath, "apply_transaction")?;
    let (response, revision, applied_ops) = {
        let mut guard = entry.lock().map_err(|_| sessions_poisoned())?;
        let s: &mut Session = &mut guard;
        // BL-141 Phase 2 — multibuffer sessions accept per-character
        // text ops (`InsertText` / `DeleteText`) as well as the
        // Approach-A `UpdateBlockContent` commit op, all targeted at
        // Excerpt blocks. Structural ops (`InsertBlock` /
        // `DeleteBlock` / `ReparentBlock` / `UpdateAnnotations`) stay
        // rejected — they don't have a clean line-range splice mapping,
        // and they'd also corrupt the synthetic tree's
        // Excerpt-only invariant.
        //
        // Approach B step 2 (this gate-widening) treats each Excerpt's
        // `content` as the authoritative text the user is editing.
        // Save (handle_save) still walks every Excerpt and splices the
        // current content back into the source file via
        // `splice_excerpts`, so the on-disk round-trip is unchanged.
        // The user-visible win is per-keystroke editing instead of
        // "edit-in-textarea, dispatch one UpdateBlockContent on commit".
        //
        // Source-session dispatch (the original Approach B sketch) is
        // not needed at this layer: external-edit sync (step 3) and
        // line-range drift (step 4) operate by re-slicing source text,
        // not by replaying ops on the source `Session`.
        if s.is_synthetic {
            for op in &tx.operations {
                let id: &uuid::Uuid = match op {
                    crate::Operation::InsertText { block_id, .. }
                    | crate::Operation::DeleteText { block_id, .. } => block_id,
                    crate::Operation::UpdateBlockContent { id, .. } => id,
                    _ => {
                        return Err(exec_err(format!(
                            "apply_transaction: session '{relpath}' is a \
                             multibuffer; only InsertText / DeleteText / \
                             UpdateBlockContent ops on Excerpt blocks are \
                             accepted in Phase 2 (got a non-content op — \
                             InsertBlock / DeleteBlock / ReparentBlock / \
                             UpdateAnnotations have no line-range splice \
                             mapping and would corrupt the synthetic tree's \
                             Excerpt-only invariant)"
                        )));
                    }
                };
                let target = s.tree.blocks.get(id).ok_or_else(|| {
                    exec_err(format!(
                        "apply_transaction: multibuffer block {id} not found"
                    ))
                })?;
                if !matches!(target.ty, BlockType::Excerpt { .. }) {
                    return Err(exec_err(format!(
                        "apply_transaction: multibuffer block {id} is not \
                         an Excerpt block; only Excerpt blocks are editable \
                         in a synthetic session"
                    )));
                }
            }
        }
        // BL-073: capture the operation set before consuming `tx` so we
        // can scan new wikilink / block-ref annotations after apply and
        // auto-stamp inbound-link targets. The post-apply scan reads
        // the freshly-mutated block content (wikilink fragments live in
        // the source text, not the annotation payload), so it has to
        // run after `execute` returns. BL-074: same captured `ops` is
        // handed to the observer (out of band from the lock).
        let ops = tx.operations.clone();
        s.undo
            .execute(tx, &mut s.tree)
            .map_err(|e| exec_err(format!("apply_transaction: {e}")))?;
        auto_stamp_inbound_targets(&mut s.tree, &ops);
        s.revision = s.revision.saturating_add(1);
        let rev = s.revision;
        let response = if text_only {
            ApplyTransactionResponse::Slim { revision: rev }
        } else {
            ApplyTransactionResponse::Full(snapshot_of(s))
        };
        (response, rev, ops)
    };
    let value = serde_json::to_value(&response)
        .map_err(|e| exec_err(format!("apply_transaction: serialize response: {e}")))?;
    if let Ok(buf) = serde_json::to_vec(&value) {
        span.record("bytes_out", buf.len());
    }
    if let Some(obs) = observer {
        obs.on_apply_transaction(&relpath, &applied_ops);
    }
    publish_changed(event_bus, &relpath, revision, Some(tx_id));
    Ok(value)
}

/// BL-073 helper: stamp every block in `tree` that newly became the
/// target of an inbound `Wikilink` (with a `#^<uuid>` fragment) or
/// `BlockRef` annotation introduced by `ops`. Stamping rekeys the
/// target's positional id to a fresh v4 UUID and sets `stable_id` so
/// the next save persists a `<!-- ^<uuid> -->` marker. Idempotent —
/// blocks that already carry a `stable_id` are skipped, and any
/// stamping failure (block missing, rekey collision) is silent: the
/// transaction itself already committed and shouldn't be invalidated
/// by a metadata-only side effect.
fn auto_stamp_inbound_targets(tree: &mut BlockTree, ops: &[crate::Operation]) {
    use crate::Operation;

    let mut targets: Vec<uuid::Uuid> = Vec::new();
    for op in ops {
        let (host_id, old, new) = match op {
            Operation::UpdateAnnotations {
                block_id,
                old_annotations,
                new_annotations,
            } => (
                *block_id,
                old_annotations.as_slice(),
                new_annotations.as_slice(),
            ),
            Operation::UpdateBlockContent {
                id,
                old_annotations,
                new_annotations,
                ..
            } => (*id, old_annotations.as_slice(), new_annotations.as_slice()),
            _ => continue,
        };
        // Annotations carry their own equality, so the simplest "what's
        // new" view is set difference by structural equality. The
        // annotation count per block is small (<100 in realistic docs),
        // so the O(n*m) scan is fine.
        for ann in new {
            if old.iter().any(|prev| prev == ann) {
                continue;
            }
            if let Some(target) = inbound_target(tree, host_id, ann) {
                targets.push(target);
            }
        }
    }

    targets.sort_unstable();
    targets.dedup();
    for old_id in targets {
        let needs_stamp = matches!(tree.get(old_id), Some(b) if b.stable_id.is_none());
        if !needs_stamp {
            continue;
        }
        let new_id = uuid::Uuid::new_v4();
        // Best-effort: a rekey collision (impossibly rare with v4) or
        // a block that disappeared between scan and stamp is harmless
        // — the user's link still resolves on the next explicit
        // `stamp_block` or `resolve_block_link` call.
        let _ = tree.rekey(old_id, new_id);
    }
}

/// Resolve a single annotation to the in-tree block id it points at,
/// when the annotation is one of the inbound-link kinds that BL-073
/// auto-stamps. Returns the *current* (positional) id of the target
/// so the caller can pass it to [`BlockTree::rekey`].
///
/// `Wikilink`s carry only the file part of the path in their payload
/// (the fragment is dropped at parse time per
/// `markdown::inline::parse_wikilink_inner`), so we recover the
/// `#^<uuid>` fragment from the *content* of the host block — the
/// raw `[[file#^uuid]]` text lives there and the annotation's
/// `start`/`end` byte range pins it down.
fn inbound_target(
    tree: &BlockTree,
    host_id: uuid::Uuid,
    ann: &crate::Annotation,
) -> Option<uuid::Uuid> {
    use crate::AnnotationType;
    match &ann.ty {
        AnnotationType::BlockRef { block_id } => Some(*block_id),
        AnnotationType::Wikilink { .. } => {
            let host = tree.get(host_id)?;
            let bytes = host.content.as_bytes();
            if ann.end > bytes.len() || ann.start >= ann.end {
                return None;
            }
            let slice = std::str::from_utf8(&bytes[ann.start..ann.end]).ok()?;
            extract_wikilink_block_uuid(slice)
        }
        _ => None,
    }
}

/// Parse a `[[...]]` literal and return the block uuid encoded in its
/// `#^<uuid>` fragment, when present. Returns `None` for path-only
/// links, fragment-less links, heading-only fragments (`#section`,
/// not `#^uuid`), and any uuid parse failure. Mirrors
/// `markdown::inline::parse_wikilink_inner` but keeps only the
/// fragment branch we care about.
pub(crate) fn extract_wikilink_block_uuid(literal: &str) -> Option<uuid::Uuid> {
    let inner = literal.strip_prefix("[[")?.strip_suffix("]]")?;
    // Display-text suffix (`|display`) is stripped first so we don't
    // confuse a `#` inside the display text with the path fragment.
    let target = match inner.find('|') {
        Some(pipe) => &inner[..pipe],
        None => inner,
    };
    let hash = target.find('#')?;
    let fragment = &target[hash + 1..];
    let id_str = fragment.strip_prefix('^')?;
    uuid::Uuid::parse_str(id_str).ok()
}

pub(crate) fn undo(
    sessions: &SessionMap,
    event_bus: Option<&Arc<EventBus>>,
    observer: Option<&Arc<dyn OpObserver>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "undo")?;
    // Capture the transaction being reversed *before* the undo runs
    // so the observer can author inverse ops against the
    // pre-undo (post-tx) state of its own mirror tree.
    let entry = acquire_session_entry(sessions, &relpath, "undo")?;
    let (value, revision, captured) = {
        let mut guard = entry.lock().map_err(|_| sessions_poisoned())?;
        let s: &mut Session = &mut guard;
        let cur_tx = s
            .undo
            .current()
            .map(|idx| Arc::clone(&s.undo.transactions()[idx]));
        s.undo
            .undo(&mut s.tree)
            .map_err(|e| exec_err(format!("undo: {e}")))?;
        s.revision = s.revision.saturating_add(1);
        let rev = s.revision;
        let val = snapshot_to_value(&snapshot_of(s), "undo")?;
        let post_tree = s.tree.clone();
        (val, rev, cur_tx.map(|tx| (tx, post_tree)))
    };
    if let (Some(obs), Some((tx, post_tree))) = (observer, captured.as_ref()) {
        obs.on_undo_transaction(&relpath, tx, post_tree);
    }
    publish_changed(event_bus, &relpath, revision, None);
    Ok(value)
}

pub(crate) fn redo(
    sessions: &SessionMap,
    event_bus: Option<&Arc<EventBus>>,
    observer: Option<&Arc<dyn OpObserver>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let relpath = relpath_arg(args, "redo")?;
    let entry = acquire_session_entry(sessions, &relpath, "redo")?;
    let (value, revision, captured) = {
        let mut guard = entry.lock().map_err(|_| sessions_poisoned())?;
        let s: &mut Session = &mut guard;
        s.undo
            .redo(&mut s.tree)
            .map_err(|e| exec_err(format!("redo: {e}")))?;
        s.revision = s.revision.saturating_add(1);
        let rev = s.revision;
        let val = snapshot_to_value(&snapshot_of(s), "redo")?;
        // Post-redo, `current` points at the just-replayed tx.
        let replayed_tx = s
            .undo
            .current()
            .map(|idx| Arc::clone(&s.undo.transactions()[idx]));
        let post_tree = s.tree.clone();
        (val, rev, replayed_tx.map(|tx| (tx, post_tree)))
    };
    if let (Some(obs), Some((tx, post_tree))) = (observer, captured.as_ref()) {
        obs.on_redo_transaction(&relpath, tx, post_tree);
    }
    publish_changed(event_bus, &relpath, revision, None);
    Ok(value)
}

/// BL-126: structural sum of the user-controlled bytes carried by
/// `tx`. Replaces the pre-BL-126 `serde_json::to_vec(&tx_value).len()`
/// pre-check that paid for a throwaway full-tx serialize on the
/// typing hot path.
///
/// Counts the bytes of every string field that scales with payload
/// (`text` / `deleted_text` / block content / content-update old/new)
/// plus a fixed-cost approximation for each annotation. Constant-
/// cost fields (UUIDs, positions, structural metadata) are
/// excluded — they're bounded by op count, which the caller already
/// bounds implicitly through the same cap (a 16 MiB tx ceiling
/// translates to ≥ tens of millions of zero-payload ops, far past
/// the CRDT engine's intended operating range).
///
/// Annotations get a fixed 64-byte allowance per entry — the typed
/// `Annotation` is at most `{ start, end, ty }` where `ty` is a
/// small enum, and the empirical max annotation payload (a wikilink
/// with a long path) is ~200 bytes. 64 is a conservative average
/// that bounds the worst case to within ~3x the JSON-serialized
/// byte count, well under the 16 MiB safety ceiling.
pub(crate) fn transaction_payload_size(tx: &crate::Transaction) -> usize {
    tx.operations.iter().map(op_payload_size).sum()
}

fn op_payload_size(op: &crate::Operation) -> usize {
    use crate::Operation;
    const PER_ANNOTATION: usize = 64;
    let ann_cost = |xs: &[crate::Annotation]| xs.len() * PER_ANNOTATION;
    match op {
        Operation::InsertText {
            text,
            pre_annotations,
            ..
        } => text.len() + ann_cost(pre_annotations),
        Operation::DeleteText {
            deleted_text,
            pre_annotations,
            ..
        } => deleted_text.len() + ann_cost(pre_annotations),
        Operation::InsertBlock { block, .. } => block.content.len() + ann_cost(&block.annotations),
        Operation::DeleteBlock { old_block, .. } => {
            old_block.content.len() + ann_cost(&old_block.annotations)
        }
        Operation::ReparentBlock { .. } => 0,
        Operation::UpdateBlockContent {
            old_content,
            new_content,
            old_annotations,
            new_annotations,
            ..
        } => {
            old_content.len()
                + new_content.len()
                + ann_cost(old_annotations)
                + ann_cost(new_annotations)
        }
        Operation::UpdateAnnotations {
            old_annotations,
            new_annotations,
            ..
        } => ann_cost(old_annotations) + ann_cost(new_annotations),
    }
}
