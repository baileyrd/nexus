// shell/src/registry/UriHandlerRegistry.test.ts
//
// WI-13 unit tests for UriHandlerRegistry. Mirrors the
// KeybindingRegistry.test.ts shape — sibling-of-implementation, picked
// up by the `tests/uri-handler-registry.test.ts` shim under the
// default `pnpm test` glob.
//
// Coverage:
//   - register stores a handler and returns an unsub
//   - the returned unsub is idempotent (call twice safe)
//   - dispatch routes by scheme (matches uri.protocol forms with the
//     trailing colon)
//   - dispatch returns false for an unknown scheme
//   - unregisterByPlugin sweeps every handler owned by that plugin
//   - same-scheme conflict between two plugins is rejected with a
//     warning + no-op disposable (first-match-wins, matches legacy
//     SI-2)
//   - same-plugin re-register replaces the handler (idempotent
//     hot-reload path)
//   - dispatch swallows handler errors (sync + async) so a buggy
//     plugin can't break the dispatch loop

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  UriHandlerRegistry,
  canonicalizeScheme,
} from './UriHandlerRegistry.ts'

test('canonicalizeScheme strips trailing colon and lowercases', () => {
  assert.equal(canonicalizeScheme('nexus'), 'nexus')
  assert.equal(canonicalizeScheme('Nexus'), 'nexus')
  assert.equal(canonicalizeScheme('NEXUS:'), 'nexus')
  assert.equal(canonicalizeScheme('  nexus  '), 'nexus')
  assert.equal(canonicalizeScheme(''), '')
})

test('register stores a handler and dispatch routes by scheme', () => {
  const reg = new UriHandlerRegistry()
  const calls: URL[] = []
  reg.register('nexus', 'plug.a', (url) => { calls.push(url) })

  assert.equal(reg.has('nexus'), true)
  assert.equal(reg.has('Nexus:'), true)

  const ok = reg.dispatch(new URL('nexus://note/some-path'))
  assert.equal(ok, true)
  assert.equal(calls.length, 1)
  assert.equal(calls[0].href, 'nexus://note/some-path')
})

test('register returns an idempotent unsub', () => {
  const reg = new UriHandlerRegistry()
  const unsub = reg.register('nexus', 'plug.a', () => {})

  assert.equal(reg.has('nexus'), true)
  unsub()
  assert.equal(reg.has('nexus'), false)
  // Calling again must be a no-op, not throw.
  unsub()
  assert.equal(reg.has('nexus'), false)
})

test('dispatch returns false for unknown scheme', () => {
  const reg = new UriHandlerRegistry()
  reg.register('nexus', 'plug.a', () => {})

  // No handler registered for `obsidian:`
  const ok = reg.dispatch(new URL('obsidian://vault/x'))
  assert.equal(ok, false)
})

test('unregisterByPlugin sweeps every handler owned by that plugin', () => {
  const reg = new UriHandlerRegistry()
  reg.register('nexus',  'plug.a', () => {})
  reg.register('forge',  'plug.a', () => {})
  reg.register('canvas', 'plug.b', () => {})

  reg.unregisterByPlugin('plug.a')

  assert.equal(reg.has('nexus'), false)
  assert.equal(reg.has('forge'), false)
  // Untouched — different owner.
  assert.equal(reg.has('canvas'), true)

  const remaining = reg.all()
  assert.equal(remaining.length, 1)
  assert.equal(remaining[0].pluginId, 'plug.b')
  assert.equal(remaining[0].scheme,   'canvas')
})

test('same-scheme conflict between two plugins: first-match-wins, second is rejected', () => {
  const reg = new UriHandlerRegistry()

  const aCalls: URL[] = []
  const bCalls: URL[] = []
  reg.register('nexus', 'plug.a', (url) => { aCalls.push(url) })

  // Silence the expected console.warn so the test output stays clean.
  // Capture the warning to verify the conflict path was hit.
  const warns: unknown[] = []
  const origWarn = console.warn
  console.warn = (...args: unknown[]) => { warns.push(args) }
  let unsubB: () => void
  try {
    unsubB = reg.register('nexus', 'plug.b', (url) => { bCalls.push(url) })
  } finally {
    console.warn = origWarn
  }

  // The warn must have fired and mentioned both plugin ids.
  assert.equal(warns.length, 1)
  const warnText = String((warns[0] as unknown[])[0])
  assert.match(warnText, /plug\.a/)
  assert.match(warnText, /plug\.b/)

  // Dispatch still hits plug.a's handler — plug.b never won the slot.
  reg.dispatch(new URL('nexus://x'))
  assert.equal(aCalls.length, 1)
  assert.equal(bCalls.length, 0)

  // The rejected unsub must be a no-op — calling it must NOT remove
  // plug.a's handler.
  unsubB!()
  assert.equal(reg.has('nexus'), true)
  reg.dispatch(new URL('nexus://y'))
  assert.equal(aCalls.length, 2)
})

test('same-plugin re-register replaces the existing handler (hot-reload path)', () => {
  const reg = new UriHandlerRegistry()
  const v1Calls: URL[] = []
  const v2Calls: URL[] = []

  reg.register('nexus', 'plug.a', (url) => { v1Calls.push(url) })
  // Same plugin re-registers — replacement, no warning.
  reg.register('nexus', 'plug.a', (url) => { v2Calls.push(url) })

  reg.dispatch(new URL('nexus://x'))
  assert.equal(v1Calls.length, 0)
  assert.equal(v2Calls.length, 1)
})

test('dispatch swallows synchronous handler errors and still returns true', () => {
  const reg = new UriHandlerRegistry()
  reg.register('nexus', 'plug.a', () => {
    throw new Error('boom')
  })

  // Silence the expected console.error.
  const origErr = console.error
  console.error = () => {}
  let result: boolean
  try {
    result = reg.dispatch(new URL('nexus://x'))
  } finally {
    console.error = origErr
  }
  assert.equal(result, true)
})

test('dispatch surfaces async handler rejections without throwing', async () => {
  const reg = new UriHandlerRegistry()
  reg.register('nexus', 'plug.a', async () => {
    throw new Error('async boom')
  })

  // Capture the console.error that the async catch path logs.
  const errs: unknown[] = []
  const origErr = console.error
  console.error = (...args: unknown[]) => { errs.push(args) }

  let result: boolean
  try {
    result = reg.dispatch(new URL('nexus://x'))
    // Flush microtasks so the .catch logger has a chance to run.
    await Promise.resolve()
    await Promise.resolve()
  } finally {
    console.error = origErr
  }

  assert.equal(result, true)
  assert.equal(errs.length, 1)
})

test('canonical scheme matching: dispatch via URL.protocol form works regardless of registered case', () => {
  const reg = new UriHandlerRegistry()
  let hit = 0
  reg.register('NEXUS', 'plug.a', () => { hit++ })

  reg.dispatch(new URL('nexus://x'))
  assert.equal(hit, 1)
})
