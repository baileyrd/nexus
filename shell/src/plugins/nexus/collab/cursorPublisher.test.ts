// BL-143 Phase 2.2 — cursorPublisher core tests.
//
// The CM6 wrapper is exercised end-to-end inside the shell; the core's
// debounce / dedup / disable-on-error logic is what these tests pin.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createCursorPublisherCore } from './cursorPublisher.ts'

/// Manual scheduler: stash callbacks keyed by a numeric token; tests
/// call `runAll(ms)` to advance time deterministically.
function makeScheduler() {
  let token = 0
  const pending = new Map<number, { cb: () => void; ms: number }>()
  return {
    schedule(cb: () => void, ms: number) {
      token += 1
      pending.set(token, { cb, ms })
      return token
    },
    clear(t: unknown) {
      pending.delete(t as number)
    },
    fireAll() {
      for (const [, entry] of pending) entry.cb()
      pending.clear()
    },
    /** Number of currently-scheduled callbacks. */
    size(): number {
      return pending.size
    },
  }
}

function setup(overrides: Partial<{
  relpath: string
  invoke: (a: string, b: string, c: unknown) => Promise<unknown>
}> = {}) {
  const calls: Array<{ plugin: string; cmd: string; args: unknown }> = []
  const sched = makeScheduler()
  const defaultInvoke = async (plugin: string, cmd: string, args: unknown) => {
    calls.push({ plugin, cmd, args })
  }
  const invoke = overrides.invoke
    ? async (plugin: string, cmd: string, args: unknown) => {
        calls.push({ plugin, cmd, args })
        return overrides.invoke!(plugin, cmd, args)
      }
    : defaultInvoke
  const core = createCursorPublisherCore({
    relpath: overrides.relpath ?? 'notes/a.md',
    invoke,
    debounceMs: 150,
    schedule: sched.schedule,
    clear: sched.clear,
  })
  return { core, calls, sched }
}

test('observe → debounce → invoke fires once per window', () => {
  const { core, calls, sched } = setup()
  core.observe({ offset: 5 })
  core.observe({ offset: 6 })
  core.observe({ offset: 7 })
  assert.equal(calls.length, 0, 'nothing fires before the debounce flushes')
  assert.equal(sched.size(), 1, 'only one pending timeout')
  sched.fireAll()
  assert.equal(calls.length, 1)
  assert.deepEqual(calls[0]!.args, { cursor: { relpath: 'notes/a.md', offset: 7 } })
})

test('selection range adds selection_end', () => {
  const { core, calls, sched } = setup()
  core.observe({ offset: 10, selectionEnd: 25 })
  sched.fireAll()
  assert.deepEqual(calls[0]!.args, {
    cursor: { relpath: 'notes/a.md', offset: 10, selection_end: 25 },
  })
})

test('selection_end equal to offset is treated as a caret (omitted)', () => {
  const { core, calls, sched } = setup()
  core.observe({ offset: 10, selectionEnd: 10 })
  sched.fireAll()
  assert.deepEqual(calls[0]!.args, {
    cursor: { relpath: 'notes/a.md', offset: 10 },
  })
})

test('repeat observation of the same caret is deduped', () => {
  const { core, calls, sched } = setup()
  core.observe({ offset: 5 })
  sched.fireAll()
  core.observe({ offset: 5 })
  sched.fireAll()
  assert.equal(calls.length, 1, 'second flush sees no movement → suppressed')
})

test('untitled relpath skips publishing entirely', () => {
  const { core, calls, sched } = setup({ relpath: 'untitled:scratch' })
  core.observe({ offset: 5 })
  sched.fireAll()
  assert.equal(calls.length, 0)
  assert.equal(core.isDisabled(), true)
})

test('empty relpath skips publishing entirely', () => {
  const { core, calls, sched } = setup({ relpath: '' })
  core.observe({ offset: 5 })
  sched.fireAll()
  assert.equal(calls.length, 0)
})

test('handler returning { published: false } disables further calls', async () => {
  let flushDone: () => void = () => {}
  const flushed = new Promise<void>((r) => { flushDone = r })
  const { core, calls, sched } = setup({
    invoke: async () => {
      flushDone()
      // Collab unconfigured: handler succeeds with a no-op reply.
      return { published: false }
    },
  })
  core.observe({ offset: 5 })
  sched.fireAll()
  await flushed
  // Let the resolved `.then` (which sets `disabled`) run. A macrotask
  // drains the queue regardless of how many microtask hops it takes.
  await new Promise((r) => setTimeout(r, 0))
  assert.equal(calls.length, 1)
  assert.equal(core.isDisabled(), true)

  core.observe({ offset: 12 })
  sched.fireAll()
  assert.equal(calls.length, 1, 'no more invokes after disable')
})

test('legacy "collab not configured" error still disables further calls', async () => {
  let flushDone: () => void = () => {}
  const flushed = new Promise<void>((r) => { flushDone = r })
  const { core, calls, sched } = setup({
    invoke: async () => {
      flushDone()
      throw new Error('ExecutionFailed: publish_presence: collab not configured')
    },
  })
  core.observe({ offset: 5 })
  sched.fireAll()
  await flushed
  // Drain the microtask queue fully: the rejection routes through
  // `.then(...).catch(...)`, one hop more than a resolved reply, so a
  // fixed couple of `Promise.resolve()` awaits can race it. A macrotask
  // guarantees both the .then pass-through and the .catch have run.
  await new Promise((r) => setTimeout(r, 0))
  assert.equal(calls.length, 1)
  assert.equal(core.isDisabled(), true)

  core.observe({ offset: 12 })
  sched.fireAll()
  assert.equal(calls.length, 1, 'no more invokes after disable')
})

test('destroy cancels a pending flush', () => {
  const { core, calls, sched } = setup()
  core.observe({ offset: 5 })
  assert.equal(sched.size(), 1)
  core.destroy()
  assert.equal(sched.size(), 0)
  sched.fireAll()
  assert.equal(calls.length, 0)
})
