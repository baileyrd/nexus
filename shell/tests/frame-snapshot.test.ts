import { test } from 'node:test'
import assert from 'node:assert/strict'
import { create } from 'zustand'
import { FrameSnapshot, snap, type Scheduler } from '../src/stores/frameSnapshot'

interface CounterStore {
  n: number
  inc: () => void
  set: (n: number) => void
}

const makeStore = (initial = 0) =>
  create<CounterStore>((set) => ({
    n: initial,
    inc: () => set((s) => ({ n: s.n + 1 })),
    set: (n) => set({ n }),
  }))

/** Capture the most-recently-scheduled callback so the test can flush
 *  it deterministically. Mirrors a frame boundary without touching
 *  rAF / timers. */
function makeManualScheduler(): { scheduler: Scheduler; runPending: () => void; pending: () => boolean } {
  let queued: (() => void) | null = null
  const scheduler: Scheduler = (cb) => {
    queued = cb
    return () => {
      if (queued === cb) queued = null
    }
  }
  return {
    scheduler,
    runPending: () => {
      const cb = queued
      queued = null
      cb?.()
    },
    pending: () => queued != null,
  }
}

test('FrameSnapshot: initial read returns current selector values', () => {
  const a = makeStore(1)
  const b = makeStore(10)
  const fs = new FrameSnapshot([snap(a, (s) => s.n), snap(b, (s) => s.n)])
  assert.deepEqual(fs.current(), [1, 10])
})

test('FrameSnapshot: tuple identity stable within a frame even if stores mutate', () => {
  const a = makeStore(1)
  const b = makeStore(10)
  const { scheduler } = makeManualScheduler()
  const fs = new FrameSnapshot([snap(a, (s) => s.n), snap(b, (s) => s.n)], scheduler)
  const dispose = fs.start()
  const before = fs.current()
  a.getState().inc()
  b.getState().inc()
  // Frame boundary hasn't fired — the cached tuple is still the same
  // reference and still reads the pre-mutation values.
  assert.equal(fs.current(), before)
  assert.deepEqual(fs.current(), [1, 10])
  dispose()
})

test('FrameSnapshot: flush invalidates the tuple when any selector value changes', () => {
  const a = makeStore(1)
  const b = makeStore(10)
  const { scheduler, runPending, pending } = makeManualScheduler()
  const fs = new FrameSnapshot([snap(a, (s) => s.n), snap(b, (s) => s.n)], scheduler)
  const dispose = fs.start()
  const seen: number[] = []
  fs.subscribe(() => seen.push(seen.length))
  const before = fs.current()
  a.getState().inc()
  assert.ok(pending(), 'flush should be scheduled after any store mutation')
  runPending()
  assert.notEqual(fs.current(), before, 'tuple identity must change after a flush with diff')
  assert.deepEqual(fs.current(), [2, 10])
  assert.equal(seen.length, 1, 'listeners fire exactly once per flush')
  dispose()
})

test('FrameSnapshot: flush with no value changes does not notify listeners', () => {
  const a = makeStore(1)
  const { scheduler, runPending } = makeManualScheduler()
  const fs = new FrameSnapshot([snap(a, (s) => s.n)], scheduler)
  const dispose = fs.start()
  let calls = 0
  fs.subscribe(() => {
    calls++
  })
  const before = fs.current()
  a.getState().set(1) // same value — store still notifies, but selector reads unchanged
  runPending()
  assert.equal(fs.current(), before, 'no-op flush must preserve tuple identity')
  assert.equal(calls, 0, 'listeners do not fire when selector outputs are equal')
  dispose()
})

test('FrameSnapshot: multiple mutations within a frame coalesce into one flush', () => {
  const a = makeStore(1)
  const b = makeStore(10)
  const { scheduler, runPending } = makeManualScheduler()
  const fs = new FrameSnapshot([snap(a, (s) => s.n), snap(b, (s) => s.n)], scheduler)
  const dispose = fs.start()
  let calls = 0
  fs.subscribe(() => {
    calls++
  })
  a.getState().inc()
  a.getState().inc()
  b.getState().inc()
  b.getState().inc()
  runPending()
  assert.deepEqual(fs.current(), [3, 12])
  assert.equal(calls, 1, '4 mutations across 2 stores must collapse into 1 listener call')
  dispose()
})

test('FrameSnapshot: dispose cancels pending flush and detaches listeners', () => {
  const a = makeStore(1)
  const { scheduler, runPending, pending } = makeManualScheduler()
  const fs = new FrameSnapshot([snap(a, (s) => s.n)], scheduler)
  const dispose = fs.start()
  let calls = 0
  fs.subscribe(() => {
    calls++
  })
  a.getState().inc()
  assert.ok(pending())
  dispose()
  assert.ok(!pending(), 'dispose must cancel the queued frame')
  // Even if a stale callback somehow fires post-dispose, listeners
  // must not be notified — current() should still hold the old value.
  runPending() // no-op since dispose cleared it
  assert.equal(fs.current(), fs.current()) // identity preserved
  assert.equal(calls, 0)
  // Mutations after dispose are silently ignored.
  a.getState().inc()
  assert.ok(!pending(), 'no flush should be scheduled after dispose')
})

test('FrameSnapshot: start() twice without dispose throws', () => {
  const a = makeStore(0)
  const fs = new FrameSnapshot([snap(a, (s) => s.n)])
  const dispose = fs.start()
  assert.throws(() => fs.start(), /start\(\) called twice/)
  dispose()
})
