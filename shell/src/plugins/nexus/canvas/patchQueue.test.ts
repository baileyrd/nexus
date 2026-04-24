// Unit tests for the WI-11 patch queue. The queue's only external
// dependency is the supplied `patch` callback, so these tests stub
// it directly with a recorder rather than building a full mock
// kernel — the kernel client wrapper (`kernelClient.ts::patch`)
// is itself a one-line `kernel.invoke` call and was already in
// scope before WI-11.
//
// Three behavioural contracts the audit at docs/wi11-canvas-status.md
// flagged as missing:
//
//   1. Debounce — N enqueue() calls within the debounce window
//      collapse into one `patch()` invocation carrying the
//      concatenated batch.
//   2. Single-flight — while a flush is in flight, the next
//      enqueue+timer doesn't issue a parallel IPC; it queues
//      behind the in-flight call.
//   3. flushNow() — bypasses the debounce timer so `pointerup`
//      drag-end can immediately persist (preserves the structural
//      drag-coalescing guarantee from the pre-WI-11 code).
//
// Plus error-path: a rejecting `patch` surfaces via the `onError`
// callback and the queue keeps running for the next batch.
//
// Run via the shell test runner: `pnpm --filter nexus-shell test`
// (picked up through the `tests/canvas-store.test.ts` re-export
// shim).

import type { CanvasPatchOp } from './kernelClient.ts'
import { createPatchQueue } from './patchQueue.ts'

import { test } from 'node:test'
import assert from 'node:assert/strict'

// ── Helpers ─────────────────────────────────────────────────────────────────

interface Recorded {
  /** Each entry is one IPC call's worth of ops. The outer array
   *  is the call sequence; the inner array is what was sent in
   *  that single `patch()` invocation. */
  calls: CanvasPatchOp[][]
}

function makeRecorder(): {
  patch: (ops: CanvasPatchOp[]) => Promise<unknown>
  recorded: Recorded
} {
  const recorded: Recorded = { calls: [] }
  return {
    patch: async (ops) => {
      // Snapshot the batch — caller may reuse the array reference
      // (we don't, but defensive copies make assertions easier).
      recorded.calls.push([...ops])
    },
    recorded,
  }
}

/** Patch factory whose returned promise the test controls. Use for
 *  single-flight tests where we need to hold one IPC open while
 *  exercising the queue. */
function makeControlledRecorder(): {
  patch: (ops: CanvasPatchOp[]) => Promise<unknown>
  recorded: Recorded
  /** Resolve the Nth in-flight call (0-indexed) with success. */
  resolve: (idx: number) => void
  /** Reject the Nth in-flight call with the given error. */
  reject: (idx: number, err: unknown) => void
} {
  const recorded: Recorded = { calls: [] }
  const resolvers: Array<{
    resolve: (v: unknown) => void
    reject: (e: unknown) => void
  }> = []
  return {
    patch: (ops) => {
      recorded.calls.push([...ops])
      return new Promise((resolve, reject) => {
        resolvers.push({ resolve, reject })
      })
    },
    recorded,
    resolve: (idx) => resolvers[idx].resolve(undefined),
    reject: (idx, err) => resolvers[idx].reject(err),
  }
}

function move(id: string, x: number, y: number): CanvasPatchOp {
  return { op: 'node_move', id, x, y }
}

const tick = (ms: number) => new Promise<void>((r) => setTimeout(r, ms))

// ── Debounce ────────────────────────────────────────────────────────────────

test('patchQueue: multiple enqueue() within debounce window collapse to one IPC', async () => {
  const { patch, recorded } = makeRecorder()
  const q = createPatchQueue({ patch, debounceMs: 20 })

  q.enqueue([move('a', 1, 1)])
  q.enqueue([move('a', 2, 2)])
  q.enqueue([move('b', 3, 3)])

  // Before the timer fires: nothing has hit the IPC.
  assert.equal(recorded.calls.length, 0)
  assert.equal(q.pendingCount(), 3)

  // After the timer fires: exactly one IPC carrying all three ops
  // in arrival order (matches the kernel's serial apply_patch loop).
  await tick(40)
  assert.equal(recorded.calls.length, 1)
  assert.deepEqual(recorded.calls[0], [
    move('a', 1, 1),
    move('a', 2, 2),
    move('b', 3, 3),
  ])
  assert.equal(q.pendingCount(), 0)

  await q.dispose()
})

test('patchQueue: empty op array is a no-op (no IPC, no timer)', async () => {
  const { patch, recorded } = makeRecorder()
  const q = createPatchQueue({ patch, debounceMs: 10 })

  q.enqueue([])
  assert.equal(q.pendingCount(), 0)

  await tick(30)
  assert.equal(recorded.calls.length, 0)

  await q.dispose()
})

// ── flushNow (drag-end coalescing preservation) ────────────────────────────

test('patchQueue: flushNow() bypasses the debounce timer', async () => {
  const { patch, recorded } = makeRecorder()
  // 5s debounce so we'd notice if the test relied on it.
  const q = createPatchQueue({ patch, debounceMs: 5000 })

  q.enqueue([move('a', 1, 1)])
  assert.equal(recorded.calls.length, 0)

  await q.flushNow()
  assert.equal(recorded.calls.length, 1)
  assert.deepEqual(recorded.calls[0], [move('a', 1, 1)])

  await q.dispose()
})

test('patchQueue: flushNow() with nothing pending resolves cleanly', async () => {
  const { patch, recorded } = makeRecorder()
  const q = createPatchQueue({ patch, debounceMs: 10 })

  await q.flushNow()
  assert.equal(recorded.calls.length, 0)

  await q.dispose()
})

// ── Single-flight ──────────────────────────────────────────────────────────

test('patchQueue: single-flight — second flush waits for the first', async () => {
  const ctl = makeControlledRecorder()
  const q = createPatchQueue({ patch: ctl.patch, debounceMs: 5 })

  // Start flight 1 and let the timer fire.
  q.enqueue([move('a', 1, 1)])
  await tick(20)
  assert.equal(ctl.recorded.calls.length, 1, 'first flush in flight')
  assert.equal(q.inFlight(), true)

  // Second batch arrives while the first is still in flight.
  q.enqueue([move('b', 2, 2)])
  await tick(20)
  // The timer for batch 2 has fired, but the IPC must NOT have
  // been issued yet — we're still holding flight 1 open.
  assert.equal(ctl.recorded.calls.length, 1, 'second flush queued behind first')

  // Resolve flight 1 → the chained flight 2 starts.
  ctl.resolve(0)
  await tick(20)
  assert.equal(ctl.recorded.calls.length, 2, 'second flush issued after first resolved')
  assert.deepEqual(ctl.recorded.calls[1], [move('b', 2, 2)])

  ctl.resolve(1)
  await q.dispose()
})

test('patchQueue: enqueues during in-flight collapse into one follow-up IPC', async () => {
  const ctl = makeControlledRecorder()
  const q = createPatchQueue({ patch: ctl.patch, debounceMs: 5 })

  q.enqueue([move('a', 1, 1)])
  await tick(20) // flight 1 in-flight

  // While flight 1 is open, three more enqueues arrive.
  q.enqueue([move('b', 1, 1)])
  q.enqueue([move('c', 1, 1)])
  q.enqueue([move('d', 1, 1)])
  await tick(20) // their debounce fires; flight 2 is queued behind 1

  ctl.resolve(0)
  await tick(20)

  // Flight 2 carries all three batched ops in one IPC.
  assert.equal(ctl.recorded.calls.length, 2)
  assert.deepEqual(ctl.recorded.calls[1], [
    move('b', 1, 1),
    move('c', 1, 1),
    move('d', 1, 1),
  ])

  ctl.resolve(1)
  await q.dispose()
})

// ── Error path ─────────────────────────────────────────────────────────────

test('patchQueue: rejecting patch routes to onError with the failed batch', async () => {
  const ctl = makeControlledRecorder()
  const errors: Array<{ err: unknown; batch: CanvasPatchOp[] }> = []
  const q = createPatchQueue({
    patch: ctl.patch,
    debounceMs: 5,
    onError: (err, batch) => errors.push({ err, batch }),
  })

  q.enqueue([move('a', 1, 1)])
  await tick(20)
  ctl.reject(0, new Error('boom'))
  await tick(20)

  assert.equal(errors.length, 1)
  assert.equal((errors[0].err as Error).message, 'boom')
  assert.deepEqual(errors[0].batch, [move('a', 1, 1)])

  // Queue keeps running — a fresh batch should still flush.
  q.enqueue([move('b', 2, 2)])
  await tick(20)
  assert.equal(ctl.recorded.calls.length, 2)
  ctl.resolve(1)

  await q.dispose()
})

// ── Dispose ────────────────────────────────────────────────────────────────

test('patchQueue: dispose() drains pending patches before resolving', async () => {
  const { patch, recorded } = makeRecorder()
  const q = createPatchQueue({ patch, debounceMs: 5000 })

  q.enqueue([move('a', 1, 1)])
  q.enqueue([move('b', 2, 2)])
  // Pre-dispose the timer hasn't fired (5s window), so nothing's
  // gone out yet.
  assert.equal(recorded.calls.length, 0)

  await q.dispose()

  // dispose() flushed the pending batch as one IPC.
  assert.equal(recorded.calls.length, 1)
  assert.deepEqual(recorded.calls[0], [move('a', 1, 1), move('b', 2, 2)])
})

test('patchQueue: enqueue after dispose is dropped', async () => {
  const { patch, recorded } = makeRecorder()
  const q = createPatchQueue({ patch, debounceMs: 5 })

  await q.dispose()
  q.enqueue([move('a', 1, 1)])
  await tick(20)
  assert.equal(recorded.calls.length, 0)
})

// ── Drag-end coalescing scenario (integration-shape) ───────────────────────

test('patchQueue: drag-coalescing — pointermove batch + pointerup flush = one IPC', async () => {
  // Simulates the CanvasView path: the existing drag handler
  // accumulates moves in local doc state and emits a single
  // `node_move` op on `pointerup`, then calls `flushNow()`.
  // Even with debounce in the way, the user-visible behaviour
  // must still be "one IPC per drag gesture".
  const { patch, recorded } = makeRecorder()
  const q = createPatchQueue({ patch, debounceMs: 250 })

  // pointerdown + N pointermoves: no enqueue (drag handler
  // updates local doc only).
  // pointerup: one enqueue with the final delta.
  q.enqueue([move('node-1', 100, 200)])
  // pointerup handler immediately flushes.
  await q.flushNow()

  assert.equal(recorded.calls.length, 1)
  assert.deepEqual(recorded.calls[0], [move('node-1', 100, 200)])

  await q.dispose()
})
