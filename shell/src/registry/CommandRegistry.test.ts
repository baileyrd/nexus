// shell/src/registry/CommandRegistry.test.ts
//
// WI-35 — per-plugin crash quarantine for command handlers.
//
// Sibling-of-implementation; surfaced to the default `pnpm test` glob
// via `tests/command-registry.test.ts` (mirrors the UriHandlerRegistry
// + ExtensionHost shim pattern).
//
// Coverage (Q3 re-throw semantics):
//   - A handler that throws synchronously: execute() re-throws to the
//     caller; subsequent execute() calls for unrelated commands still
//     work; the registry emits `command:error` on the event bus.
//   - A handler that rejects asynchronously: same — the rejection
//     surfaces as an awaited error, not a swallowed one.
//   - Sibling handlers registered by other plugins stay callable after
//     a crash (the entry is not evicted).
//   - Unknown command: no handler wired → existing warn path preserved
//     (no throw, returns undefined) so the behaviour of manifest-only
//     stubs doesn't regress.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { CommandRegistry } from './CommandRegistry.ts'
import { eventBus } from '../host/EventBus.ts'

test('WI-35 — sync handler that throws re-throws to caller', async () => {
  const reg = new CommandRegistry()
  reg.register('p.bad', 'cmd.boom', () => {
    throw new Error('boom-sync')
  })
  await assert.rejects(() => reg.execute('cmd.boom'), /boom-sync/)
})

test('WI-35 — async handler that rejects re-throws to caller', async () => {
  const reg = new CommandRegistry()
  reg.register('p.bad', 'cmd.reject', async () => {
    throw new Error('boom-async')
  })
  await assert.rejects(() => reg.execute('cmd.reject'), /boom-async/)
})

test('WI-35 — a throwing command does not break sibling commands', async () => {
  const reg = new CommandRegistry()
  reg.register('p.bad', 'cmd.boom', () => { throw new Error('boom') })
  reg.register('p.good', 'cmd.ok', () => 42)
  await assert.rejects(() => reg.execute('cmd.boom'))
  // Registry state unchanged — bad entry still present, good one still callable.
  assert.equal(reg.has('cmd.boom'), true)
  const r = await reg.execute('cmd.ok')
  assert.equal(r, 42)
})

test('WI-35 — execute() re-calls after a throw still work (no poisoning)', async () => {
  const reg = new CommandRegistry()
  let n = 0
  reg.register('p.flaky', 'cmd.flaky', () => {
    n++
    if (n === 1) throw new Error('first-call-fails')
    return n
  })
  await assert.rejects(() => reg.execute('cmd.flaky'))
  const r = await reg.execute('cmd.flaky')
  assert.equal(r, 2)
})

test('WI-35 — throwing handler emits command:error on the event bus', async () => {
  const reg = new CommandRegistry()
  const seen: Array<{ commandId: string; pluginId?: string; error: string }> = []
  const unsub = eventBus.on<{ commandId: string; pluginId?: string; error: string }>(
    'command:error',
    (e) => { seen.push(e) },
  )
  try {
    reg.register('p.err', 'cmd.err', () => {
      throw new Error('surface-me')
    })
    await assert.rejects(() => reg.execute('cmd.err'))
    assert.equal(seen.length, 1)
    assert.equal(seen[0].commandId, 'cmd.err')
    assert.equal(seen[0].pluginId, 'p.err')
    assert.match(seen[0].error, /surface-me/)
  } finally {
    unsub()
  }
})

test('WI-35 — unknown command: warn + undefined, no throw (regression guard)', async () => {
  const reg = new CommandRegistry()
  // Manifest-only (no handler) entries should still no-op with a warn —
  // the crash-quarantine try/catch must not change that contract.
  const r = await reg.execute('cmd.unknown')
  assert.equal(r, undefined)
})
