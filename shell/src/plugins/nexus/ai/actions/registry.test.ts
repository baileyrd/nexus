// shell/src/plugins/nexus/ai/actions/registry.test.ts
//
// BL-035 — unit coverage for the AI action registry. Mirrors the
// `contextContributors.test.ts` style: register / dispose / surface
// filter / error containment.
//
// Run:
//   node --import tsx --test \
//     shell/src/plugins/nexus/ai/actions/registry.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { AiAction } from '@nexus/extension-api'
import { aiActionRegistry } from './registry.ts'

function reset(): void {
  aiActionRegistry._resetForTests()
}

function makeAction(overrides: Partial<AiAction> = {}): AiAction {
  return {
    id: 'test.action',
    label: 'Test',
    surfaces: ['editor.selection'],
    run: async () => {},
    ...overrides,
  }
}

test('register / list: returns actions in registration order', () => {
  reset()
  aiActionRegistry.register(makeAction({ id: 'a' }))
  aiActionRegistry.register(makeAction({ id: 'b' }))
  const out = aiActionRegistry.list()
  assert.equal(out.length, 2)
  assert.equal(out[0].id, 'a')
  assert.equal(out[1].id, 'b')
})

test('register: empty id is rejected with a warn-and-skip', () => {
  reset()
  const origWarn = console.warn
  console.warn = () => {}
  try {
    const dispose = aiActionRegistry.register(makeAction({ id: '   ' }))
    assert.equal(aiActionRegistry.list().length, 0)
    dispose() // must not throw
  } finally {
    console.warn = origWarn
  }
})

test('actionsForSurface: filters by surface tag', () => {
  reset()
  aiActionRegistry.register(
    makeAction({ id: 'sel-only', surfaces: ['editor.selection'] }),
  )
  aiActionRegistry.register(makeAction({ id: 'block-only', surfaces: ['block'] }))
  aiActionRegistry.register(
    makeAction({ id: 'both', surfaces: ['editor.selection', 'block'] }),
  )
  const sel = aiActionRegistry.actionsForSurface('editor.selection')
  assert.deepEqual(sel.map((a) => a.id), ['sel-only', 'both'])
  const blk = aiActionRegistry.actionsForSurface('block')
  assert.deepEqual(blk.map((a) => a.id), ['block-only', 'both'])
  const canvas = aiActionRegistry.actionsForSurface('canvas.node')
  assert.equal(canvas.length, 0)
})

test('disposer removes only the targeted registration', () => {
  reset()
  const dispose1 = aiActionRegistry.register(makeAction({ id: 'a' }))
  aiActionRegistry.register(makeAction({ id: 'b' }))
  dispose1()
  const remaining = aiActionRegistry.list()
  assert.equal(remaining.length, 1)
  assert.equal(remaining[0].id, 'b')
})

test('disposer is idempotent', () => {
  reset()
  const dispose = aiActionRegistry.register(makeAction({ id: 'a' }))
  dispose()
  dispose()
  aiActionRegistry.register(makeAction({ id: 'b' }))
  assert.equal(aiActionRegistry.list().length, 1)
})

test('invoke: a throwing action does not poison the registry', async () => {
  reset()
  const origWarn = console.warn
  console.warn = () => {}
  try {
    const bad = makeAction({
      id: 'bad',
      run: () => {
        throw new Error('boom')
      },
    })
    aiActionRegistry.register(bad)
    const ok = await aiActionRegistry.invoke(bad, {
      surface: 'editor.selection',
      relpath: 'a.md',
      selection: 'x',
      selectionRange: { from: 0, to: 1 },
    })
    assert.equal(ok, false)
    // Subsequent register / list still works — registry is intact.
    aiActionRegistry.register(makeAction({ id: 'after' }))
    assert.equal(aiActionRegistry.list().length, 2)
  } finally {
    console.warn = origWarn
  }
})

test('invoke: async action that resolves successfully returns true', async () => {
  reset()
  let captured = ''
  const action = makeAction({
    run: async (ctx) => {
      if (ctx.surface === 'editor.selection') captured = ctx.selection
    },
  })
  const ok = await aiActionRegistry.invoke(action, {
    surface: 'editor.selection',
    relpath: 'a.md',
    selection: 'hello',
    selectionRange: { from: 0, to: 5 },
  })
  assert.equal(ok, true)
  assert.equal(captured, 'hello')
})
