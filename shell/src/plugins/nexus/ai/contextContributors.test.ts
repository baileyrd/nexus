// shell/src/plugins/nexus/ai/contextContributors.test.ts
//
// BL-032 — unit coverage for the context-contributor registry + the
// `assemblePrompt` helper. The registry is the load-bearing surface
// the overlay leans on; everything else is a UI shell.
//
// Run:
//   node --import tsx --test \
//     shell/src/plugins/nexus/ai/contextContributors.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  contextContributors,
  assemblePrompt,
  type ContextContribution,
} from './contextContributors.ts'

function reset(): void {
  contextContributors._resetForTests()
}

test('register / collect: returns contributions in registration order', async () => {
  reset()
  contextContributors.register('editor', () => ({
    surfaceId: 'editor',
    chips: [{ id: 'a', label: 'A', kind: 'file' }],
    promptBlock: 'BLOCK_A',
  }))
  contextContributors.register('canvas', () => ({
    surfaceId: 'canvas',
    chips: [{ id: 'b', label: 'B', kind: 'node' }],
    promptBlock: 'BLOCK_B',
  }))

  const out = await contextContributors.collect()
  assert.equal(out.length, 2)
  assert.equal(out[0].surfaceId, 'editor')
  assert.equal(out[1].surfaceId, 'canvas')
})

test('collect: drops null returns', async () => {
  reset()
  contextContributors.register('editor', () => null)
  contextContributors.register('bases', () => ({
    surfaceId: 'bases',
    chips: [{ id: 'r', label: 'Row', kind: 'row' }],
  }))
  const out = await contextContributors.collect()
  assert.equal(out.length, 1)
  assert.equal(out[0].surfaceId, 'bases')
})

test('collect: tolerates async contributors', async () => {
  reset()
  contextContributors.register('editor', async () => {
    await new Promise((r) => setTimeout(r, 1))
    return {
      surfaceId: 'editor',
      chips: [{ id: 'late', label: 'late', kind: 'file' }],
    } as ContextContribution
  })
  const out = await contextContributors.collect()
  assert.equal(out.length, 1)
  assert.equal(out[0].chips[0].id, 'late')
})

test('collect: a throwing contributor does not poison the batch', async () => {
  reset()
  // Silence the registry's console.warn in the throwing path so the
  // test output stays clean.
  const origWarn = console.warn
  console.warn = () => {}
  try {
    contextContributors.register('editor', () => {
      throw new Error('boom')
    })
    contextContributors.register('bases', () => ({
      surfaceId: 'bases',
      chips: [{ id: 'survives', label: 'survives', kind: 'row' }],
    }))
    const out = await contextContributors.collect()
    assert.equal(out.length, 1)
    assert.equal(out[0].surfaceId, 'bases')
  } finally {
    console.warn = origWarn
  }
})

test('disposer removes only the targeted registration', async () => {
  reset()
  const dispose1 = contextContributors.register('editor', () => ({
    surfaceId: 'editor',
    chips: [{ id: '1', label: '1', kind: 'file' }],
  }))
  contextContributors.register('canvas', () => ({
    surfaceId: 'canvas',
    chips: [{ id: '2', label: '2', kind: 'node' }],
  }))
  dispose1()
  const out = await contextContributors.collect()
  assert.equal(out.length, 1)
  assert.equal(out[0].surfaceId, 'canvas')
})

test('disposer is idempotent', async () => {
  reset()
  const dispose = contextContributors.register('editor', () => ({
    surfaceId: 'editor',
    chips: [],
  }))
  dispose()
  dispose() // second call must not throw nor remove anything else
  contextContributors.register('canvas', () => ({
    surfaceId: 'canvas',
    chips: [],
  }))
  const out = await contextContributors.collect()
  assert.equal(out.length, 1)
})

test('register: empty/whitespace surfaceId is rejected', async () => {
  reset()
  const origWarn = console.warn
  console.warn = () => {}
  try {
    const dispose = contextContributors.register('   ', () => ({
      surfaceId: 'x',
      chips: [],
    }))
    // Disposer is a no-op when registration was rejected; calling it
    // must not throw.
    dispose()
    const out = await contextContributors.collect()
    assert.equal(out.length, 0)
  } finally {
    console.warn = origWarn
  }
})

// ── assemblePrompt ────────────────────────────────────────────────────────

test('assemblePrompt: no contributions ⇒ assembled equals trimmed user prompt', () => {
  const out = assemblePrompt('  hello world  ', [])
  assert.equal(out.userPrompt, 'hello world')
  assert.equal(out.assembled, 'hello world')
  assert.deepEqual(out.chips, [])
})

test('assemblePrompt: concatenates non-empty blocks then a Question section', () => {
  const out = assemblePrompt('what does foo do?', [
    {
      surfaceId: 'editor',
      chips: [{ id: 'f', label: 'foo.md', kind: 'file' }],
      promptBlock: '### Current file\n\nfoo body',
    },
    {
      surfaceId: 'editor',
      chips: [{ id: 's', label: 'sel', kind: 'selection' }],
      promptBlock: '### Selection\n\nfoo()',
    },
  ])
  assert.equal(
    out.assembled,
    '### Current file\n\nfoo body\n\n### Selection\n\nfoo()\n\n## Question\nwhat does foo do?',
  )
  assert.equal(out.chips.length, 2)
  assert.equal(out.chips[0].id, 'f')
  assert.equal(out.chips[1].id, 's')
})

test('assemblePrompt: blank/whitespace promptBlocks are filtered out', () => {
  const out = assemblePrompt('q', [
    { surfaceId: 'editor', chips: [], promptBlock: '   ' },
    { surfaceId: 'editor', chips: [], promptBlock: '' },
    { surfaceId: 'bases', chips: [], promptBlock: 'real block' },
  ])
  assert.equal(out.assembled, 'real block\n\n## Question\nq')
})

// ── BL-033 — chip removal threading ───────────────────────────────────────

test('assemblePrompt: removed chip drops only its fragment when chipPromptBlocks is set', () => {
  const out = assemblePrompt(
    'q',
    [
      {
        surfaceId: 'editor',
        chips: [
          { id: 'f', label: 'foo.md', kind: 'file' },
          { id: 's', label: 'sel', kind: 'selection' },
        ],
        promptBlock: 'FILE_BLOCK\n\nSEL_BLOCK',
        chipPromptBlocks: { f: 'FILE_BLOCK', s: 'SEL_BLOCK' },
      },
    ],
    new Set(['s']),
  )
  // Only the file fragment survives; the visible chip rail loses 's'.
  assert.equal(out.assembled, 'FILE_BLOCK\n\n## Question\nq')
  assert.equal(out.chips.length, 1)
  assert.equal(out.chips[0].id, 'f')
})

test('assemblePrompt: without chipPromptBlocks, removing all chips drops the whole surface', () => {
  const out = assemblePrompt(
    'q',
    [
      {
        surfaceId: 'editor',
        chips: [{ id: 'f', label: 'foo.md', kind: 'file' }],
        promptBlock: 'FILE_BLOCK',
      },
      {
        surfaceId: 'bases',
        chips: [{ id: 'r', label: 'row', kind: 'row' }],
        promptBlock: 'ROW_BLOCK',
      },
    ],
    new Set(['f']),
  )
  assert.equal(out.assembled, 'ROW_BLOCK\n\n## Question\nq')
  assert.equal(out.chips.length, 1)
  assert.equal(out.chips[0].id, 'r')
})

test('assemblePrompt: without chipPromptBlocks, partial removal keeps the surface', () => {
  // Coarse mode falls back to the surface's joined block — when at
  // least one chip survives the whole block stays.
  const out = assemblePrompt(
    'q',
    [
      {
        surfaceId: 'editor',
        chips: [
          { id: 'f', label: 'foo.md', kind: 'file' },
          { id: 's', label: 'sel', kind: 'selection' },
        ],
        promptBlock: 'COMBINED',
      },
    ],
    new Set(['s']),
  )
  assert.equal(out.assembled, 'COMBINED\n\n## Question\nq')
  assert.equal(out.chips.length, 1)
})

test('assemblePrompt: a contributor that contributes only chips still surfaces them', () => {
  const out = assemblePrompt('q', [
    {
      surfaceId: 'editor',
      chips: [{ id: 'f', label: 'foo.md', kind: 'file' }],
      // no promptBlock
    },
  ])
  assert.equal(out.assembled, 'q')
  assert.equal(out.chips.length, 1)
})
