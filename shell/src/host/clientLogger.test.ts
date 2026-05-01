// SH-017 — clientLogger unit tests.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { clientLogger } from './clientLogger'

test('clientLogger: info writes to ring buffer', () => {
  clientLogger.clear()
  clientLogger.info('[test] hello', 42)
  const entries = clientLogger.getEntries()
  assert.equal(entries.length, 1)
  assert.equal(entries[0]!.level, 'info')
  assert.equal(entries[0]!.message, '[test] hello')
  assert.deepEqual(entries[0]!.args, [42])
})

test('clientLogger: error level is recorded correctly', () => {
  clientLogger.clear()
  clientLogger.error('[test] boom', new Error('fail'))
  const entries = clientLogger.getEntries()
  assert.equal(entries[0]!.level, 'error')
})

test('clientLogger: ring buffer caps at 200 entries', () => {
  clientLogger.clear()
  for (let i = 0; i < 210; i++) {
    clientLogger.debug(`msg ${i}`)
  }
  const entries = clientLogger.getEntries()
  assert.equal(entries.length, 200, 'ring buffer must not exceed 200 entries')
  // Oldest entries should have been dropped; last entry is msg 209.
  assert.ok(entries[entries.length - 1]!.message.includes('209'))
})

test('clientLogger: clear() empties the buffer', () => {
  clientLogger.info('x')
  clientLogger.clear()
  assert.equal(clientLogger.getEntries().length, 0)
})
