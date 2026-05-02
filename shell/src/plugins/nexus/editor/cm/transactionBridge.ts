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
  BlockId,
  EditorSnapshot,
  Operation,
  Transaction,
  TransactionMetadata,
  TransactionSource,
  UserAction,
} from '../types.ts'
import { useEditorStore } from '../editorStore.ts'

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
 * Translate a CM `ViewUpdate`'s `ChangeSet` into kernel [`Operation`]s
 * against a single root block.
 *
 * Strategy:
 *   - Walk `iterChanges`. If there's exactly ONE change segment and it
 *     is either pure insert (fromA == toA, inserted.length > 0) or
 *     pure delete (inserted.length == 0, fromA < toA), emit a matching
 *     `InsertText` / `DeleteText` op against `rootBlockId`. The `pos`
 *     is the CM offset (bytes-of-string-in-UTF-16, same as CM's doc
 *     indexing) — the kernel uses byte offsets; for ASCII-only edits
 *     these coincide, and the v1 coarse mapping accepts the drift for
 *     non-ASCII input. The `pre_annotations` field is `[]` — v1 doesn't
 *     carry annotations through CM yet.
 *   - For anything else (replace, multi-segment, or an empty change),
 *     fall back to a single `UpdateBlockContent` op carrying the whole
 *     new doc content. `old_content` is captured from the update's
 *     `startState`; the server verifies it matches the block's current
 *     content.
 *
 * Caller is responsible for providing a valid `rootBlockId` (typically
 * `snapshot.tree.root_blocks[0]`). Returns an empty op list when the
 * CM update has no actual doc change.
 */
export function changesToOps(
  update: ViewUpdate,
  rootBlockId: BlockId,
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
    if (isPureInsert) {
      return {
        ops: [
          {
            kind: 'insert_text',
            block_id: rootBlockId,
            pos: s.fromA,
            text: s.insertedStr,
            pre_annotations: [],
          },
        ],
        fallbackFullDoc: false,
      }
    }
    if (isPureDelete) {
      const deleted = update.startState.doc.sliceString(s.fromA, s.toA)
      return {
        ops: [
          {
            kind: 'delete_text',
            block_id: rootBlockId,
            pos: s.fromA,
            deleted_text: deleted,
            pre_annotations: [],
          },
        ],
        fallbackFullDoc: false,
      }
    }
  }

  // Fallback: one UpdateBlockContent carrying the whole doc.
  const oldContent = update.startState.doc.toString()
  const newContent = update.state.doc.toString()
  if (oldContent === newContent) {
    return { ops: [], fallbackFullDoc: false }
  }
  return {
    ops: [
      {
        kind: 'update_block_content',
        id: rootBlockId,
        old_content: oldContent,
        new_content: newContent,
        old_annotations: [],
        new_annotations: [],
      },
    ],
    fallbackFullDoc: true,
  }
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
    onError = defaultErrorReporter,
  } = opts

  const pending: ViewUpdate[] = []
  let rafHandle: number | null = null
  let flushing = false

  const reconcile = (
    view: BridgeViewLike,
    canonical: string,
    revision: number | null,
  ): void => {
    if (revision !== null) {
      useEditorStore.getState().setSessionRevision(relpath, revision)
    }
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
    void kernelClient
      .applyTransaction(relpath, tx)
      .then(async (snapshot) => {
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
          .then((canonical) => reconcile(view, canonical, null))
          .catch(() => {})
      })
  }

  const flush = (): void => {
    rafHandle = null
    if (pending.length === 0) return
    const batch = pending.splice(0, pending.length)

    const snapshot = getSnapshot()
    const rootId = snapshot?.tree.root_blocks[0]
    if (!rootId) return

    let ops: Operation[]
    if (batch.length === 1) {
      ops = changesToOps(batch[0]!, rootId).ops
    } else {
      const first = batch[0]!
      const last = batch[batch.length - 1]!
      const oldContent = first.startState.doc.toString()
      const newContent = last.state.doc.toString()
      if (oldContent === newContent) {
        ops = []
      } else {
        ops = [
          {
            kind: 'update_block_content',
            id: rootId,
            old_content: oldContent,
            new_content: newContent,
            old_annotations: [],
            new_annotations: [],
          },
        ]
      }
    }
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
