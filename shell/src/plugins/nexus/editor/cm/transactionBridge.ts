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
  /** Receive a `reset` callback that, when invoked, clears the
   *  optimistic mirror and cancels every queued chain entry. The save
   *  flow calls this after `sync_content` pushes CM markdown into the
   *  kernel — the kernel's block IDs change in that path, so any
   *  in-flight op against the old IDs would be a doomed dispatch.
   *  Optional so headless tests can skip the wiring. */
  registerReset?: (reset: () => void) => void
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
    registerReset,
    onError = defaultErrorReporter,
  } = opts

  const pending: ViewUpdate[] = []
  let rafHandle: number | null = null
  let flushing = false
  // Optimistic mirror of the kernel-side block tree. Each flush
  // translates against the mirror, generates ops, and advances the
  // mirror by applying those ops locally — without waiting for the
  // kernel to ack. The kernel processes our `apply_transaction` calls
  // serially (Tauri serialises IPC), so when an ack arrives the mirror
  // already reflects the resulting state.
  //
  // Why a mirror at all: the sessionManager-owned snapshot only
  // advances at apply-success, which adds the kernel round-trip
  // latency (~30-60ms) to every keystroke before the *next* op can be
  // translated against fresh block content. A local mirror sidesteps
  // that wait — keystrokes that arrive during the round-trip translate
  // against the optimistic mirror and dispatch immediately.
  //
  // Initialised lazily from `getSnapshot()` on first flush; reset on
  // apply-error (kernel rejected, mirror is now ahead of reality).
  let mirror: EditorSnapshot | null = null

  // Serialise IPC dispatches through a JS-level promise chain. Multiple
  // flushes can fire back-to-back without waiting for prior acks; the
  // chain ensures the kernel sees `apply_transaction` calls in
  // generation order.
  let dispatchChain: Promise<void> = Promise.resolve()
  // Bumped whenever an apply fails. Each queued chain entry captures
  // the generation it was scheduled under; entries whose generation no
  // longer matches the current value skip their work. Without this, a
  // single rejection cascades through every subsequent queued op (each
  // computed against an optimistic-mirror state the kernel never
  // reached), so each one in turn rejects and triggers another
  // recovery — visible to the user as repeated flicker.
  let dispatchGeneration = 0

  registerReset?.(() => {
    // Clear the mirror so the next flush re-inits from the (now
    // freshly-set) sessionManager snapshot, and bump the generation
    // so any queued chain entry computed against the discarded
    // mirror short-circuits instead of issuing a dispatch with stale
    // block IDs or byte offsets.
    mirror = null
    dispatchGeneration++
  })

  const reconcile = (
    view: BridgeViewLike,
    canonical: string,
    revision: number | null,
  ): void => {
    if (revision !== null) {
      useEditorStore.getState().setSessionRevision(relpath, revision)
    }
    if (pending.length > 0) return
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
    // Text-only ops can't diverge from CM — the kernel just mutates
    // block content verbatim, no serializer normalisation. Skip the
    // post-apply `getMarkdown` round-trip for these; structural ops
    // (none currently generated by the translator, but kept for
    // forward-compat) would still reconcile if they appeared.
    const skipReconcile = tx.operations.every(
      (o) =>
        o.kind === 'insert_text' ||
        o.kind === 'delete_text' ||
        o.kind === 'update_annotations',
    )
    const myGen = dispatchGeneration
    dispatchChain = dispatchChain.then(async () => {
      if (myGen !== dispatchGeneration) {
        // A prior op in this chain failed; this op's byte offsets were
        // computed against an optimistic-mirror state the kernel never
        // reached. Sending it would just trigger another rejection and
        // another full-doc reconcile flicker. Drop it silently — CM
        // keeps the user's typing locally, and a future explicit
        // resync (save flow, focus change) reconciles content.
        useEditorStore.getState().consumePendingLocalRevision(tx.id)
        return
      }
      try {
        const response = await kernelClient.applyTransaction(relpath, tx)
        // BL-123: slim response (text-only ops) carries just the
        // revision — no snapshot to push back, no reconcile to run.
        // The sessionManager cache stays at its pre-tx contents; block
        // IDs and structure are unchanged for text-only ops, so the
        // downstream consumers (drag-bridge, comments, block-link
        // nav) keep working off correct structure. Stale text content
        // doesn't matter because those consumers either re-read the
        // tree on demand or use `getTree`/`stamp_block` directly.
        if (response.kind === 'slim') {
          useEditorStore.getState().setSessionRevision(relpath, response.revision)
          return
        }
        // Full response — same wire shape as pre-BL-123 plus the
        // `kind` discriminator. Strip the discriminator before
        // handing to setSnapshot so consumers see a clean
        // EditorSnapshot.
        const { kind: _kind, ...snapshot } = response
        // Push the authoritative snapshot back to sessionManager so
        // external consumers (drag-bridge, comments) see fresh data.
        // The bridge itself keeps using its own mirror.
        setSnapshot?.(snapshot)
        if (skipReconcile) {
          useEditorStore.getState().setSessionRevision(relpath, snapshot.revision)
          return
        }
        const canonical = await kernelClient.getMarkdown(relpath)
        reconcile(view, canonical, snapshot.revision)
      } catch (err) {
        useEditorStore.getState().consumePendingLocalRevision(tx.id)
        onError('editor bridge: apply_transaction failed', err)
        // Mirror is ahead of reality — kernel didn't apply this op.
        // Bump the generation so subsequent queued ops skip themselves
        // (they were computed against an unreachable state). Refetch
        // the authoritative tree so the next translation has correct
        // block contents.
        //
        // Deliberately *not* reconciling CM here. Replacing the doc
        // out from under the user causes visible flicker, and the
        // common cause of a failure (a block-merging keystroke we
        // didn't translate, e.g. backspace across a paragraph
        // boundary) leaves CM with the user's intended state — we'd
        // rather diverge silently than yank their edits back. A
        // higher-level resync (save / focus) can push CM content to
        // the kernel when needed.
        dispatchGeneration++
        try {
          const fresh = await kernelClient.getTree(relpath)
          mirror = fresh
        } catch {
          mirror = null
        }
      }
    })
  }

  const flush = (): void => {
    rafHandle = null
    if (pending.length === 0) return
    const batch = pending.splice(0, pending.length)

    // Lazy-init the mirror from the sessionManager snapshot on first
    // flush. Subsequent flushes reuse and advance the mirror without
    // touching the cache, so keystrokes don't wait for the kernel
    // round-trip to refresh block contents.
    if (!mirror) {
      mirror = getSnapshot()
      if (!mirror) return
    }
    const snapshot = mirror

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

    // Optimistically advance the mirror so the next flush sees the
    // post-op state, without waiting for the kernel ack.
    mirror = applyOpsToSnapshot(snapshot, ops)

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

/**
 * Apply text-only ops to a snapshot's block tree, returning a new
 * snapshot. Used by the bridge's optimistic mirror — each generated
 * `InsertText`/`DeleteText` op is reflected in the mirror immediately
 * so the next flush's translation sees the expected post-op block
 * contents (matching what the kernel will see once acks land).
 *
 * Wire ops carry UTF-8 byte offsets (`op.pos`); block content is a JS
 * UTF-16 string. The walker converts byte offset to char index so the
 * splice lands at the same logical position.
 */
function applyOpsToSnapshot(
  snap: EditorSnapshot,
  ops: ReadonlyArray<Operation>,
): EditorSnapshot {
  let blocks = snap.tree.blocks
  let dirty = false
  for (const op of ops) {
    if (op.kind === 'insert_text') {
      const block = blocks[op.block_id]
      if (!block) continue
      const charIdx = utf8ByteOffsetToCharIndex(block.content, op.pos)
      const next = block.content.slice(0, charIdx) + op.text + block.content.slice(charIdx)
      blocks = { ...blocks, [op.block_id]: { ...block, content: next } }
      dirty = true
      continue
    }
    if (op.kind === 'delete_text') {
      const block = blocks[op.block_id]
      if (!block) continue
      const charStart = utf8ByteOffsetToCharIndex(block.content, op.pos)
      // `deleted_text` is the original JS substring — its `length`
      // matches the char span to remove (the bridge captured it via
      // CM's `sliceString`, so it's already in JS UTF-16 units).
      const charEnd = charStart + op.deleted_text.length
      const next = block.content.slice(0, charStart) + block.content.slice(charEnd)
      blocks = { ...blocks, [op.block_id]: { ...block, content: next } }
      dirty = true
      continue
    }
    // Other op kinds (insert_block, delete_block, reparent,
    // update_block_content, update_annotations) aren't currently
    // generated by `changesToOps` — when they appear, extend this
    // walker. For now they're a silent skip; the mirror will drift
    // until the kernel ack triggers a `setSnapshot` refresh.
  }
  if (!dirty) return snap
  return { ...snap, tree: { ...snap.tree, blocks } }
}

/** UTF-8 byte offset → JS UTF-16 char index inside `s`. Mirrors the
 *  encoding the bridge sends to the kernel (which uses byte offsets in
 *  `block.content`) so the local mirror can splice at the matching
 *  char position. Walks the string once, accumulating bytes per char
 *  until `byteOffset` is reached. */
function utf8ByteOffsetToCharIndex(s: string, byteOffset: number): number {
  if (byteOffset <= 0) return 0
  let bytes = 0
  let chars = 0
  while (chars < s.length && bytes < byteOffset) {
    const code = s.charCodeAt(chars)
    if (code < 0x80) {
      bytes += 1
      chars += 1
    } else if (code < 0x800) {
      bytes += 2
      chars += 1
    } else if (code >= 0xd800 && code <= 0xdbff) {
      // High surrogate of a supplementary code point — paired with the
      // following low surrogate it's 4 UTF-8 bytes total.
      bytes += 4
      chars += 2
    } else {
      bytes += 3
      chars += 1
    }
  }
  return chars
}
