// BL-077 follow-up — unit tests for the save-format-hook registry.
//
// Re-exported via `shell/tests/lsp-tails.test.ts` so the default
// `pnpm test` glob picks them up.

import { test, beforeEach } from 'node:test'
import assert from 'node:assert/strict'

import {
  _resetSaveFormatHooksForTests,
  _saveFormatHookCount,
  registerSaveFormatHook,
  runSaveFormatHook,
} from './saveFormatHooks.ts'

beforeEach(() => {
  _resetSaveFormatHooksForTests()
})

test('runSaveFormatHook resolves silently when no hook is registered', async () => {
  // No hook. No throw, no error reported. The save command's
  // pre-write step should be a no-op for tabs without LSP.
  let errorReported = false
  await runSaveFormatHook('foo.rs', () => {
    errorReported = true
  })
  assert.equal(errorReported, false)
})

test('registerSaveFormatHook + runSaveFormatHook fires the registered fn', async () => {
  let called = 0
  registerSaveFormatHook('foo.rs', async () => {
    called += 1
  })
  await runSaveFormatHook('foo.rs')
  assert.equal(called, 1)
})

test('runSaveFormatHook only fires the hook for the matching relpath', async () => {
  let called = 0
  registerSaveFormatHook('foo.rs', async () => {
    called += 1
  })
  await runSaveFormatHook('bar.rs')
  assert.equal(called, 0)
  await runSaveFormatHook('foo.rs')
  assert.equal(called, 1)
})

test('disposer returned by registerSaveFormatHook removes the hook', async () => {
  let called = 0
  const dispose = registerSaveFormatHook('foo.rs', async () => {
    called += 1
  })
  dispose()
  await runSaveFormatHook('foo.rs')
  assert.equal(called, 0)
  assert.equal(_saveFormatHookCount(), 0)
})

test('disposer no-ops when a later register has replaced the hook', async () => {
  let firstCalls = 0
  let secondCalls = 0
  const firstDispose = registerSaveFormatHook('foo.rs', async () => {
    firstCalls += 1
  })
  registerSaveFormatHook('foo.rs', async () => {
    secondCalls += 1
  })
  // First disposer fires *after* the second register — must NOT
  // remove the second hook (which is the live one). This matters
  // when an EditorView remounts before the previous one finished
  // tearing down: the order is `register#2` → `dispose#1`.
  firstDispose()
  await runSaveFormatHook('foo.rs')
  assert.equal(firstCalls, 0)
  assert.equal(secondCalls, 1)
})

test('runSaveFormatHook routes thrown errors through the onError callback', async () => {
  registerSaveFormatHook('foo.rs', async () => {
    throw new Error('format pipe broken')
  })
  let received: unknown = null
  await runSaveFormatHook('foo.rs', (err) => {
    received = err
  })
  assert.ok(received instanceof Error)
  assert.equal((received as Error).message, 'format pipe broken')
})

test('runSaveFormatHook swallows errors when no onError is supplied', async () => {
  registerSaveFormatHook('foo.rs', async () => {
    throw new Error('format pipe broken')
  })
  // The promise must still resolve — a save command without an
  // error-reporting bridge should still proceed to write the file
  // even when a misbehaving formatter throws.
  await runSaveFormatHook('foo.rs')
})
