// CodeMirror ↔ kernel transaction bridge (Phase 5).
//
// Routes every CM `docChanged` update through `com.nexus.editor::apply_transaction`
// so the Rust `BlockTree` owns authoritative document state.
//
// Heuristics:
//   - v1 coarse block identity: the whole doc maps to `tree.root_blocks[0]`.
//     The CM change-to-op mapping emits `InsertText` / `DeleteText` against
//     that root when the CM `ChangeSet` is a single contiguous insert or
//     a single contiguous delete. For anything more complex (replace,
//     multi-segment changes), the bridge falls back to a single
//     `UpdateBlockContent` carrying the whole new doc content. If even
//     that can't be mapped (no root block yet), the caller surfaces the
//     failure and relies on a `getMarkdown` reconciliation pass.
//   - Batching: every CM update that fires inside a single
//     `requestAnimationFrame` tick is coalesced into ONE transaction —
//     per resolved decision #3, the kernel's `UndoTree` groups at the
//     `Transaction` boundary, so rAF batching gives us "undo merges
//     rapid keystrokes" for free.
//   - Echo suppression: every outgoing Transaction carries a UUID.
//     Before dispatch, the bridge stashes the id in
//     `editorStore.pendingLocalRevisions`; the Phase 4 session-manager
//     event handler consumes it when the echo arrives.
//   - Reconciliation: the `applyTransaction` response always returns
//     the full `EditorSnapshot`. If the resulting canonical markdown
//     (fetched via `getMarkdown`) diverges from CM's current doc (e.g.
//     the serializer normalized whitespace), the bridge replaces the
//     whole CM doc via `dispatch({ changes })`. Cursor/selection are
//     best-effort — at worst they snap to doc end.

import { EditorView, ViewUpdate } from '@codemirror/view'
import type { Extension } from '@codemirror/state'

import type { EditorKernelClient } from '../kernelClient.ts'
import { clientLogger } from '../../../../clientLogger'
import type {
  EditorSnapshot,
  Operation,
  Transaction,
  TransactionMetadata,
  TransactionSource,
  UserAction,
} from '../types.ts'
import { useEditorStore } from '../editorStore.ts'
import { resolveBlockPos, resolveBlockRange } from './blockPosMap.ts'

/**
 * Optional surface for surfacing errors. The bridge gets a `KernelAPI`
 * indirectly via the `EditorKernelClient`; a richer `PluginAPI.notifications`
 * isn't reachable here, so we accept an opt-in error reporter and fall
 * back to `console.error`.
 */
export interface BridgeErrorReporter {
  (message: string, err: unknown): void
}

// ── Change-to-op mapping ─────────────────────────────────────────────────────

/**
 * Return value from [`changesToOps`]. `ops` is the op list to send; when
 * `ops` is empty the caller should skip dispatch (no-op change). The
 * `fallbackFullDoc` flag is informational — `true` means the mapping
 * couldn't be expressed as a single Insert/Delete against a block and
 * collapsed to `UpdateBlockContent` of the whole doc.
 */
export interface ChangeMapping {
  ops: Operation[]
  fallbackFullDoc: boolean
}

/**
 * Translate a CM `ViewUpdate`'s `ChangeSet` into kernel [`Operation`]s.
 *
 * Strategy:
 *   - Walk `iterChanges`. If there's exactly ONE segment that's a pure
 *     insert or a pure delete entirely within a single top-level
 *     Paragraph or ATXHeading block, resolve the CM offset to
 *     `(block_id, byte-offset-within-block)` via {@link resolveBlockPos}
 *     / {@link resolveBlockRange} and emit `InsertText`/`DeleteText`
 *     against the correct block. Inserts/deletes containing a newline
 *     bypass this path — newlines typically split or join blocks and
 *     can't be expressed as a text op against one block.
 *   - For anything else (multi-segment, cross-block, unresolvable
 *     position, inline-formatted block, list/blockquote/etc.), fall
 *     back to a single `UpdateBlockContent` op against the first root
 *     block carrying the whole new doc content. This fallback is
 *     known-imperfect — see the bridge module header for context — but
 *     covers cases the single-block translator deliberately punts on.
 *
 * `pre_annotations` is `[]` — the kernel doesn't consume it on the
 * forward apply (only used for inverse computation, which the kernel
 * derives itself from the post-state). Returns an empty op list when
 * the CM update has no actual doc change.
 */
export function changesToOps(
  update: ViewUpdate,
  snapshot: EditorSnapshot,
): ChangeMapping {
  if (!update.docChanged) return { ops: [], fallbackFullDoc: false }

  interface Seg {
    fromA: number
    toA: number
    fromB: number
    toB: number
    insertedStr: string
  }
  const segs: Seg[] = []
  update.changes.iterChanges((fromA, toA, fromB, toB, inserted) => {
    segs.push({
      fromA,
      toA,
      fromB,
      toB,
      insertedStr: inserted.toString(),
    })
  })

  if (segs.length === 1) {
    const s = segs[0]!
    const isPureInsert = s.fromA === s.toA && s.insertedStr.length > 0
    const isPureDelete = s.fromA < s.toA && s.insertedStr.length === 0
    if (isPureInsert && !s.insertedStr.includes('\n')) {
      const pos = resolveBlockPos(update.startState, snapshot, s.fromA)
      if (pos) {
        return {
          ops: [
            {
              kind: 'insert_text',
              block_id: pos.blockId,
              pos: pos.bytePos,
              text: s.insertedStr,
              pre_annotations: [],
            },
          ],
          fallbackFullDoc: false,
        }
      }
    }
    if (isPureDelete) {
      const deleted = update.startState.doc.sliceString(s.fromA, s.toA)
      if (!deleted.includes('\n')) {
        const range = resolveBlockRange(update.startState, snapshot, s.fromA, s.toA)
        if (range) {
          return {
            ops: [
              {
                kind: 'delete_text',
                block_id: range.blockId,
                pos: range.byteFrom,
                deleted_text: deleted,
                pre_annotations: [],
              },
            ],
            fallbackFullDoc: false,
          }
        }
      }
    }
  }

  // No single-block translation worked. The previous fallback —
  // `update_block_content` against `root_blocks[0]` with the whole
  // doc as `new_content` — actively corrupted the kernel by stuffing
  // the entire markdown into a single block's content string. Skip
  // the op instead; CM keeps the user's local state until the next
  // successful flush (or a reconcile from `getMarkdown`) brings CM
  // back in line with the kernel. Multi-block edits (Enter, paste)
  // need real `InsertBlock`/`DeleteBlock` op synthesis — tracked
  // separately.
  return { ops: [], fallbackFullDoc: true }
}

// ── Transaction assembly ─────────────────────────────────────────────────────

/** Options for [`makeTransaction`]. */
export interface MakeTransactionOptions {
  source: TransactionSource
  /** Defaults to `{ kind: 'keystroke' }` — the common case for CM edits. */
  userAction?: UserAction
}

/**
 * Build a fresh [`Transaction`] wrapping `ops`. Generates a v4-ish UUID
 * via `crypto.randomUUID` (available in Node 19+ and every browser we
 * target). `aiEdit` is derived from `source === 'ai'`.
 */
export function makeTransaction(
  ops: Operation[],
  opts: MakeTransactionOptions,
): Transaction {
  const metadata: TransactionMetadata = {
    user_action: opts.userAction ?? { kind: 'keystroke' },
    source: opts.source,
    ai_edit: opts.source === 'ai',
  }
  return {
    id: newTransactionId(),
    operations: ops,
    created_at: Date.now(),
    metadata,
  }
}

/**
 * UUID generator. Prefers the platform `crypto.randomUUID`; falls back
 * to a best-effort hex string when unavailable (ancient runtimes).
 */
function newTransactionId(): string {
  const g = globalThis as { crypto?: { randomUUID?: () => string } }
  if (g.crypto?.randomUUID) return g.crypto.randomUUID()
  // Fallback: not a real v4 UUID, but the kernel only uses it as an
  // opaque id — uniqueness is what matters.
  const rand = () =>
    Math.floor(Math.random() * 0x1_0000_0000)
      .toString(16)
      .padStart(8, '0')
  return `${rand()}-${rand().slice(0, 4)}-4${rand().slice(0, 3)}-8${rand().slice(0, 3)}-${rand()}${rand().slice(0, 4)}`
}

// ── CM extension factory ─────────────────────────────────────────────────────

export interface TransactionBridgeOptions {
  relpath: string
  kernelClient: EditorKernelClient
  /** Returns the current cached snapshot for this relpath. The bridge
   *  uses it to resolve `root_blocks[0]` when assembling ops. Typically
   *  `() => sessionManager.getSnapshot(relpath)` via a helper, or a
   *  closure that reads from wherever the snapshot lives. */
  getSnapshot: () => EditorSnapshot | null
  /** Replace the cached snapshot for this relpath. The bridge calls
   *  this after each successful `apply_transaction` so the next CM-
   *  offset translation sees the post-edit block tree. Typically
   *  `(snap) => sessionManager.setSnapshot(relpath, snap)`. Optional so
   *  existing tests that drive the bridge without a session manager
   *  keep working — when omitted, the snapshot stays stale and the
   *  block-pos translator will bail more aggressively. */
  setSnapshot?: (snapshot: EditorSnapshot) => void
  /** Report an error from the async dispatch path. Defaults to
   *  `console.error`. Plugin-layer callers typically wire this to
   *  `api.notifications.show({ type: 'error', message })`. */
  onError?: BridgeErrorReporter
}

/**
 * Minimal view surface the bridge needs for reconciliation dispatches.
 * Real callers pass a CM `EditorView`; tests pass a stub that records
 * the dispatch call.
 */
export interface BridgeViewLike {
  state: { doc: { toString(): string } }
  dispatch(spec: { changes: { from: number; to: number; insert: string } }): void
}

/**
 * View-independent core of the bridge. Drives the pending-batch
 * bookkeeping and the kernel round-trip without touching CM's DOM. The
 * `transactionBridge` CM extension is a thin wrapper over this that
 * feeds it `ViewUpdate` events from `EditorView.updateListener`.
 *
 * Exposed separately so headless unit tests can exercise the batching,
 * echo-suppression, and reconciliation paths without constructing a
 * real `EditorView` (which requires a DOM).
 */
export interface BridgeCore {
  /** Record an update; schedule a flush on the next tick. */
  push(update: ViewUpdate): void
  /** Force a synchronous flush — used by tests to avoid waiting on rAF. */
  flushSync(): void
}

export function createBridgeCore(opts: TransactionBridgeOptions): BridgeCore {
  const {
    relpath,
    kernelClient,
    getSnapshot,
    setSnapshot,
    onError = defaultErrorReporter,
  } = opts

  const pending: ViewUpdate[] = []
  let rafHandle: number | null = null
  let flushing = false
  // Tracks transactions whose `apply_transaction` round-trip hasn't
  // resolved yet. While > 0, the local CM doc is ahead of whatever
  // canonical we hold — replacing the doc would clobber chars typed
  // during the round-trip. Decremented just before reconcile runs so
  // the *last* in-flight transaction's reconcile is the one that
  // actually executes when the queue drains.
  let inFlight = 0

  const reconcile = (
    view: BridgeViewLike,
    canonical: string,
    revision: number | null,
  ): void => {
    if (revision !== null) {
      useEditorStore.getState().setSessionRevision(relpath, revision)
    }
    // Skip the full-doc replace if there's still work the kernel
    // hasn't seen. `pending.length > 0` means keystrokes queued for
    // the next rAF flush; `inFlight > 0` means earlier transactions
    // whose responses haven't landed yet. In either case `canonical`
    // is stale relative to CM and replacing would lose user typing.
    // A later reconcile (after pending drains and inFlight returns
    // to zero) will catch us up if the kernel normalized anything.
    if (pending.length > 0 || inFlight > 0) return
    const current = view.state.doc.toString()
    if (current === canonical) return
    flushing = true
    try {
      view.dispatch({
        changes: { from: 0, to: current.length, insert: canonical },
      })
    } finally {
      flushing = false
    }
  }

  const dispatchTransaction = (view: BridgeViewLike, tx: Transaction): void => {
    useEditorStore.getState().addPendingLocalRevision(tx.id)
    inFlight++
    // Text-only ops (InsertText/DeleteText/UpdateAnnotations) cannot
    // produce a canonical that diverges from CM — the kernel just
    // mutates a block's content string verbatim, no serializer
    // normalization happens. Skip the post-apply `getMarkdown` round-
    // trip for these, halving the IPC cost per keystroke. Structural
    // ops (insert_block, reparent, update_block_content, ...) can
    // change the serialized form and still need reconcile.
    const skipReconcile = tx.operations.every(
      (o) =>
        o.kind === 'insert_text' ||
        o.kind === 'delete_text' ||
        o.kind === 'update_annotations',
    )
    void kernelClient
      .applyTransaction(relpath, tx)
      .then(async (snapshot) => {
        // Refresh the cached snapshot so the next CM-offset translation
        // sees the post-edit block tree. Without this, a heading whose
        // content grew from "Hello" to "Hello!" would still resolve
        // against the open-time "Hello" — the source-vs-content equality
        // check would fail and the bridge would bail to the (now empty)
        // fallback, silently dropping the next keystroke.
        setSnapshot?.(snapshot)
        inFlight--
        // Drain accumulated pending updates synchronously rather than
        // scheduling a fresh rAF — going through requestAnimationFrame
        // here adds ~16ms of needless latency between every kernel ack
        // and the next batch dispatch, which the user perceives as
        // typing stutter when chars arrive faster than the round-trip.
        if (pending.length > 0) flush()
        if (skipReconcile) {
          useEditorStore.getState().setSessionRevision(relpath, snapshot.revision)
          return
        }
        let canonical: string
        try {
          canonical = await kernelClient.getMarkdown(relpath)
        } catch (err) {
          onError('editor bridge: getMarkdown failed after apply', err)
          return
        }
        reconcile(view, canonical, snapshot.revision)
      })
      .catch((err) => {
        useEditorStore.getState().consumePendingLocalRevision(tx.id)
        onError('editor bridge: apply_transaction failed', err)
        void kernelClient
          .getMarkdown(relpath)
          .then((canonical) => {
            inFlight--
            if (pending.length > 0) flush()
            reconcile(view, canonical, null)
          })
          .catch(() => {
            inFlight--
            if (pending.length > 0) flush()
          })
      })
  }

  const flush = (): void => {
    rafHandle = null
    if (pending.length === 0) return
    if (inFlight > 0) {
      // Snapshot won't be refreshed until the in-flight apply returns.
      // Translating against a stale snapshot would either misroute the
      // op or bail to the (now-empty) fallback and silently drop the
      // edit. The post-apply `.then` schedules another flush, which
      // drains accumulated pending updates in one composed batch.
      return
    }
    const batch = pending.splice(0, pending.length)

    const snapshot = getSnapshot()
    if (!snapshot) return

    // Collapse a multi-update batch into a single synthetic update by
    // composing its ChangeSets. This sidesteps the in-batch snapshot
    // staleness problem: each individual u[i] would otherwise need to
    // be translated against a mirror of the snapshot that already
    // reflects u[0..i-1]'s mutations. The composed change set runs
    // against the batch's *original* startState, which the cached
    // snapshot matches (the snapshot only advances at apply-success).
    let synthetic: ViewUpdate
    if (batch.length === 1) {
      synthetic = batch[0]!
    } else {
      const first = batch[0]!
      const last = batch[batch.length - 1]!
      let composed = first.changes
      for (let i = 1; i < batch.length; i++) {
        composed = composed.compose(batch[i]!.changes)
      }
      synthetic = {
        docChanged: true,
        changes: composed,
        startState: first.startState,
        state: last.state,
        view: last.view,
      } as ViewUpdate
    }
    const ops = changesToOps(synthetic, snapshot).ops
    if (ops.length === 0) return

    const tx = makeTransaction(ops, { source: 'user' })
    const view = batch[batch.length - 1]!.view as unknown as BridgeViewLike
    dispatchTransaction(view, tx)
  }

  const scheduleFlush = (): void => {
    if (rafHandle !== null) return
    const g = globalThis as { requestAnimationFrame?: (cb: () => void) => number }
    if (typeof g.requestAnimationFrame === 'function') {
      rafHandle = g.requestAnimationFrame(flush)
    } else {
      rafHandle = 1
      queueMicrotask(() => {
        rafHandle = null
        flush()
      })
    }
  }

  return {
    push(update) {
      if (!update.docChanged) return
      if (flushing) return
      pending.push(update)
      scheduleFlush()
    },
    flushSync() {
      rafHandle = null
      flush()
    },
  }
}

/**
 * CM extension that observes `docChanged` updates, batches them within
 * a single rAF tick, and dispatches one kernel transaction per tick.
 *
 * See module header for the full heuristic + reconciliation contract.
 */
export function transactionBridge(opts: TransactionBridgeOptions): Extension {
  const core = createBridgeCore(opts)
  return EditorView.updateListener.of((update) => {
    core.push(update)
  })
}

function defaultErrorReporter(message: string, err: unknown): void {
  clientLogger.error(`[nexus.editor] ${message}:`, err)
}
