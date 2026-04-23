// WI-11 §4.3 closer: debounced + single-flight queue around
// `canvas_patch`.
//
// Pre-WI-11 the shell fired one IPC per `commit()` call and
// discarded the promise (`void client.patch(...).catch(...)`). The
// audit at docs/wi11-canvas-status.md identified two consequences:
//
//   1. Keyboard-driven rapid edits (Inspector typing, repeated
//      single-key delete, double-click create text) hit the kernel
//      synchronously, one round-trip per edit. Drag gestures already
//      coalesced via the `pointerup` flush — but every other path
//      was un-batched.
//   2. Two `commit()`s issued in quick succession could land in
//      either order on the kernel side because each was a fresh
//      fire-and-forget invocation. Combined with the kernel's
//      lock-free read-modify-write in `patch_canvas` (§3 of the
//      audit), a slow first patch could overwrite a faster second
//      one — apparent state regression.
//
// The queue addresses both shell-side. Kernel-side concerns
// (cross-tab races, no revision/etag) are out of v1 scope and
// documented in the audit.
//
// Design contract:
//
//   • `enqueue(ops)` appends `ops` to the pending batch and starts
//     (or extends) a trailing-edge debounce timer. Multiple
//     `enqueue` calls within `debounceMs` produce a single IPC
//     call carrying the concatenated op list.
//
//   • `flushNow()` cancels the debounce timer and immediately
//     starts a flush of whatever is pending. `pointerup` calls
//     this so a drag-end always lands before the next user action
//     and the existing structural drag-coalescing guarantee is
//     preserved. Returns the flush promise so callers can `await`
//     it (`dispose()` does, the test harness does).
//
//   • Single-flight: at most one IPC call is in flight per queue
//     at any time. If a debounce fires while an earlier flush is
//     still pending, the new flush awaits the earlier one before
//     issuing its own IPC. This serialises the kernel's
//     read-modify-write per-canvas from the shell's side and
//     eliminates intra-tab reordering.
//
//   • Errors from the underlying `patch` call are routed through
//     the supplied `onError` callback. The queue keeps running
//     after an error so a transient kernel hiccup doesn't wedge
//     the canvas.
//
//   • `dispose()` flushes any pending patches synchronously
//     (start the IPC), then awaits both that flush and any
//     prior in-flight call. Callers (CanvasView's unmount effect
//     and `pagehide` listener) await the returned promise so
//     teardown doesn't drop edits.
//
// The queue is intentionally per-canvas (one instance per relpath):
// concatenating patches from independent canvases would be
// nonsense, and per-canvas single-flight is exactly the
// granularity the kernel needs.

import type { CanvasPatchOp } from './kernelClient'

/** Default trailing-edge debounce window in ms. Picked at 250 ms
 *  to match the audit's recommendation (§3, "0.5d to add a 250 ms
 *  trailing-edge debounce"). Inspector typing at ~5 keys/sec
 *  collapses to one IPC per word; pointer-driven edits land within
 *  one frame of the gesture finishing because `pointerup` calls
 *  `flushNow()`. */
export const DEFAULT_PATCH_DEBOUNCE_MS = 250

export interface PatchQueueOptions {
  /** Underlying IPC call. Receives the concatenated batch in
   *  arrival order. The queue treats the returned promise as the
   *  sole signal of completion — resolve = success, reject =
   *  surface to `onError`. */
  patch: (ops: CanvasPatchOp[]) => Promise<unknown>
  /** Surface for transport / kernel errors. Receives the
   *  rejection reason and the batch that failed. The queue
   *  intentionally does not retry — the doc state is already
   *  optimistic, the user will see the next save attempt
   *  succeed or fail on its own merits. */
  onError?: (err: unknown, batch: CanvasPatchOp[]) => void
  /** Override the debounce window. Tests use 0 for synchronous
   *  flushes. Production callers should use the default. */
  debounceMs?: number
}

export interface PatchQueue {
  /** Append `ops` to the pending batch. Empty arrays are no-ops
   *  (matches the existing `commit()` early-return for
   *  `forward.length === 0`). */
  enqueue(ops: CanvasPatchOp[]): void
  /** Cancel the debounce timer and flush whatever is pending
   *  immediately. Resolves when the IPC call (and any prior
   *  in-flight call) completes. Safe to call when nothing is
   *  pending — resolves immediately in that case (after any
   *  in-flight). */
  flushNow(): Promise<void>
  /** Flush + wait for everything to drain. Idempotent. After
   *  `dispose()` returns, no more IPC calls will be issued by
   *  this queue even if someone holds a stale reference and
   *  calls `enqueue`. */
  dispose(): Promise<void>
  /** Test-only: number of patches currently queued (not yet
   *  in flight). */
  pendingCount(): number
  /** Test-only: whether a flush IPC is currently in flight. */
  inFlight(): boolean
}

export function createPatchQueue(options: PatchQueueOptions): PatchQueue {
  const debounceMs = options.debounceMs ?? DEFAULT_PATCH_DEBOUNCE_MS

  let pending: CanvasPatchOp[] = []
  let timer: ReturnType<typeof setTimeout> | null = null
  let inFlightPromise: Promise<void> | null = null
  let disposed = false

  const cancelTimer = () => {
    if (timer != null) {
      clearTimeout(timer)
      timer = null
    }
  }

  /** Drain `pending` into a fresh array and start the IPC.
   *  Chains onto any prior `inFlightPromise` so the kernel sees
   *  one in-flight `canvas_patch` per canvas at a time. */
  const startFlush = (): Promise<void> => {
    cancelTimer()
    if (pending.length === 0) {
      // Still wait on a prior flight if there is one — callers
      // (`flushNow` / `dispose`) want a "everything written"
      // signal, not just "this batch written".
      return inFlightPromise ?? Promise.resolve()
    }
    const batch = pending
    pending = []
    const prior = inFlightPromise ?? Promise.resolve()
    const next: Promise<void> = prior
      .catch(() => {
        // Prior flush failure was already surfaced via `onError`
        // when the prior flush rejected. Swallow here so the
        // chain continues — the next batch is the user's latest
        // intent and deserves its own attempt.
      })
      .then(() => options.patch(batch))
      .then(
        () => {
          // Successful patch. Clear the in-flight reference iff
          // it's still us (a later flush may have overwritten it
          // already, in which case it owns the chain now).
          if (inFlightPromise === next) inFlightPromise = null
        },
        (err) => {
          if (inFlightPromise === next) inFlightPromise = null
          options.onError?.(err, batch)
        },
      )
    inFlightPromise = next
    return next
  }

  return {
    enqueue(ops) {
      if (disposed) return
      if (ops.length === 0) return
      // Push, not splice: arrival order is the kernel-side apply
      // order, which matches the existing `applyPatchOps`
      // semantics in canvasStore.ts and the kernel's
      // `apply_patch` loop.
      for (const op of ops) pending.push(op)
      if (timer != null) return
      timer = setTimeout(() => {
        timer = null
        void startFlush()
      }, debounceMs)
    },
    flushNow() {
      if (disposed) return inFlightPromise ?? Promise.resolve()
      return startFlush()
    },
    async dispose() {
      if (disposed) {
        // Already disposing — wait on the in-flight (if any) and
        // bail. Don't restart the chain.
        if (inFlightPromise) await inFlightPromise.catch(() => {})
        return
      }
      disposed = true
      // Drain whatever the user typed last.
      const final = startFlush()
      await final.catch(() => {})
    },
    pendingCount() {
      return pending.length
    },
    inFlight() {
      return inFlightPromise != null
    },
  }
}
