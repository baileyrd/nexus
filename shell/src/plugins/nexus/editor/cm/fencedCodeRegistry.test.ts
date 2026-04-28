// BL-008 — fenced-code-renderer registry unit tests.
//
// Run via the shell test runner: `pnpm --filter nexus-shell test`,
// picked up through `tests/fenced-code-registry.test.ts`.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { fencedCodeRegistry } from './fencedCodeRegistry.ts'

interface FakeElement {
  nodeType: number
  tag: string
}

function fakeElement(tag = 'div'): FakeElement {
  return { nodeType: 1, tag }
}

test('register: returns disposer that removes the renderer', () => {
  const el = fakeElement('a')
  const dispose = fencedCodeRegistry.register('test-foo', () => el as unknown as HTMLElement)
  assert.equal(fencedCodeRegistry.has('test-foo'), true)
  dispose()
  assert.equal(fencedCodeRegistry.has('test-foo'), false)
})

test('register: case-insensitive language matching', () => {
  const dispose = fencedCodeRegistry.register('Test-CASE', () => fakeElement() as unknown as HTMLElement)
  try {
    assert.equal(fencedCodeRegistry.has('test-case'), true)
    assert.equal(fencedCodeRegistry.has('TEST-CASE'), true)
  } finally {
    dispose()
  }
})

test('register: duplicate returns no-op disposer and warns', () => {
  const orig = console.warn
  let warnCount = 0
  console.warn = () => {
    warnCount++
  }
  try {
    const a = fencedCodeRegistry.register('test-dup', () => fakeElement('a') as unknown as HTMLElement)
    const b = fencedCodeRegistry.register('test-dup', () => fakeElement('b') as unknown as HTMLElement)
    assert.ok(warnCount >= 1, 'duplicate registration warns')
    b()
    assert.equal(fencedCodeRegistry.has('test-dup'), true, 'dup disposer is a no-op')
    a()
    assert.equal(fencedCodeRegistry.has('test-dup'), false)
  } finally {
    console.warn = orig
  }
})

test('register: empty/whitespace language emits warn and is no-op', () => {
  const orig = console.warn
  let warnCount = 0
  console.warn = () => {
    warnCount++
  }
  try {
    const dispose = fencedCodeRegistry.register('   ', () => fakeElement() as unknown as HTMLElement)
    assert.ok(warnCount >= 1)
    assert.equal(fencedCodeRegistry.has(''), false)
    dispose()
  } finally {
    console.warn = orig
  }
})

test('generation: advances on register and on dispose', () => {
  const before = fencedCodeRegistry.generation()
  const dispose = fencedCodeRegistry.register('test-gen', () => fakeElement() as unknown as HTMLElement)
  const afterReg = fencedCodeRegistry.generation()
  assert.ok(afterReg > before)
  dispose()
  const afterDispose = fencedCodeRegistry.generation()
  assert.ok(afterDispose > afterReg)
})

test('onChange: fires on register and unregister; disposer detaches listener', () => {
  let fires = 0
  const offChange = fencedCodeRegistry.onChange(() => {
    fires++
  })
  const dispose = fencedCodeRegistry.register('test-onchg', () => fakeElement() as unknown as HTMLElement)
  assert.equal(fires, 1)
  dispose()
  assert.equal(fires, 2)
  offChange()
  fencedCodeRegistry.register('test-onchg-2', () => fakeElement() as unknown as HTMLElement)()
  assert.equal(fires, 2, 'no further fires after offChange')
})

test('renderCached: synchronous renderer hits cache after first render', () => {
  let calls = 0
  const dispose = fencedCodeRegistry.register('test-sync', () => {
    calls++
    return fakeElement(`v${calls}`) as unknown as HTMLElement
  })
  try {
    const a = fencedCodeRegistry.renderCached('test-sync', 'foo')
    const b = fencedCodeRegistry.renderCached('test-sync', 'foo')
    assert.ok(a, 'first render returns element synchronously')
    assert.equal(a, b, 'second call returns the same cached element')
    assert.equal(calls, 1, 'renderer ran exactly once')
  } finally {
    dispose()
  }
})

test('renderCached: distinct sources both render', () => {
  let calls = 0
  const dispose = fencedCodeRegistry.register('test-distinct', (src) => {
    calls++
    return fakeElement(`v-${src}`) as unknown as HTMLElement
  })
  try {
    fencedCodeRegistry.renderCached('test-distinct', 'one')
    fencedCodeRegistry.renderCached('test-distinct', 'two')
    fencedCodeRegistry.renderCached('test-distinct', 'one')
    assert.equal(calls, 2, 'renderer called once per distinct source')
  } finally {
    dispose()
  }
})

test('renderCached: missing renderer returns null without error', () => {
  const result = fencedCodeRegistry.renderCached('not-registered', 'whatever')
  assert.equal(result, null)
})

test('renderCached: re-registration invalidates cached entries', () => {
  let calls = 0
  const renderer = () => {
    calls++
    return fakeElement() as unknown as HTMLElement
  }
  const a = fencedCodeRegistry.register('test-invalidate', renderer)
  fencedCodeRegistry.renderCached('test-invalidate', 'src')
  assert.equal(calls, 1)
  a()
  const b = fencedCodeRegistry.register('test-invalidate', renderer)
  fencedCodeRegistry.renderCached('test-invalidate', 'src')
  assert.equal(calls, 2, 'cache cleared after re-register')
  b()
})

test('renderCached: async renderer returns null then awaitPending resolves', async () => {
  const dispose = fencedCodeRegistry.register('test-async', async (src) => {
    await Promise.resolve()
    return fakeElement(`async-${src}`) as unknown as HTMLElement
  })
  try {
    const sync = fencedCodeRegistry.renderCached('test-async', 'x')
    assert.equal(sync, null, 'first call to async renderer returns null')
    const pending = fencedCodeRegistry.awaitPending('test-async', 'x')
    assert.ok(pending, 'pending promise is recorded')
    const result = await pending!
    assert.ok(!(result instanceof Error))
    const second = fencedCodeRegistry.renderCached('test-async', 'x')
    assert.ok(second, 'second call hits cache after async resolution')
    assert.equal(second, result)
  } finally {
    dispose()
  }
})

test('renderCached: async renderer rejection caches error', async () => {
  const dispose = fencedCodeRegistry.register('test-async-err', async () => {
    throw new Error('boom')
  })
  try {
    fencedCodeRegistry.renderCached('test-async-err', 'q')
    const pending = fencedCodeRegistry.awaitPending('test-async-err', 'q')
    const result = await pending!
    assert.ok(result instanceof Error)
    assert.match((result as Error).message, /boom/)
    const second = fencedCodeRegistry.renderCached('test-async-err', 'q')
    assert.equal(second, null, 'cached error keeps renderCached returning null')
  } finally {
    dispose()
  }
})

test('renderCached: synchronous renderer throw caches error', () => {
  const dispose = fencedCodeRegistry.register('test-sync-err', () => {
    throw new Error('crash')
  })
  try {
    const result = fencedCodeRegistry.renderCached('test-sync-err', 'q')
    assert.equal(result, null)
    const second = fencedCodeRegistry.renderCached('test-sync-err', 'q')
    assert.equal(second, null, 'subsequent calls hit cached error')
  } finally {
    dispose()
  }
})
