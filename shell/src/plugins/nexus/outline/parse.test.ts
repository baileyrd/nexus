// Unit tests for parse.ts — markdown-string parser plus the Phase 7
// BlockTree walker. Uses node:test to stay aligned with editor tests.
//
// Run with: node --experimental-strip-types --test \
//   src/plugins/nexus/outline/parse.test.ts

import type { BlockTree, Block } from '../editor/types.ts'
import { parseHeadings, treeToHeadings } from './parse.ts'

const nodeTest: string = 'node:test'
const nodeAssert: string = 'node:assert/strict'
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const { test } = (await import(nodeTest)) as any
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const assert = ((await import(nodeAssert)) as any).default

// ── fixtures ────────────────────────────────────────────────────────────────

function makeBlock(id: string, partial: Partial<Block>): Block {
  return {
    id,
    ty: { kind: 'paragraph' },
    content: '',
    annotations: [],
    properties: {},
    parent_id: null,
    children: [],
    index_in_parent: 0,
    created_at: 0,
    updated_at: 0,
    is_deleted: false,
    ...partial,
  }
}

function buildTree(blocks: Block[]): BlockTree {
  const byId: Record<string, Block> = {}
  const rootIds: string[] = []
  for (const b of blocks) {
    byId[b.id] = b
    rootIds.push(b.id)
  }
  return { blocks: byId, root_blocks: rootIds, metadata: {} }
}

// ── parseHeadings (unchanged behaviour, sanity) ──────────────────────────────

test('parseHeadings extracts ATX headings with 1-based lines', () => {
  const md = '# First\n\npara\n## Second\n### Third\n'
  const hs = parseHeadings(md)
  assert.equal(hs.length, 3)
  assert.equal(hs[0].text, 'First')
  assert.equal(hs[0].level, 1)
  assert.equal(hs[0].line, 1)
  assert.equal(hs[1].line, 4)
  assert.equal(hs[2].line, 5)
})

// ── treeToHeadings ───────────────────────────────────────────────────────────

test('treeToHeadings walks root_blocks and collects heading blocks', () => {
  const tree = buildTree([
    makeBlock('a', { ty: { kind: 'heading', level: 1 }, content: 'Title' }),
    makeBlock('b', { ty: { kind: 'paragraph' }, content: 'body' }),
    makeBlock('c', { ty: { kind: 'heading', level: 2 }, content: 'Sub' }),
    makeBlock('d', { ty: { kind: 'heading', level: 3 }, content: 'Deep' }),
  ])
  const hs = treeToHeadings(tree)
  assert.equal(hs.length, 3)
  assert.deepEqual(
    hs.map((h) => [h.text, h.level, h.index]),
    [
      ['Title', 1, 0],
      ['Sub', 2, 1],
      ['Deep', 3, 2],
    ],
  )
  // Ids are <level>-<slug>-<index>.
  assert.equal(hs[0].id, '1-title-0')
  assert.equal(hs[1].id, '2-sub-1')
  assert.equal(hs[2].id, '3-deep-2')
  // No line hints → line=0 (source-mode scroll is a no-op, preview
  // mode uses index).
  assert.equal(hs[0].line, 0)
})

test('treeToHeadings skips deleted, empty, and non-heading blocks', () => {
  const tree = buildTree([
    makeBlock('a', { ty: { kind: 'paragraph' }, content: 'not a heading' }),
    makeBlock('b', {
      ty: { kind: 'heading', level: 1 },
      content: 'Deleted',
      is_deleted: true,
    }),
    makeBlock('c', { ty: { kind: 'heading', level: 2 }, content: '   ' }),
    makeBlock('d', { ty: { kind: 'heading', level: 1 }, content: 'Real' }),
  ])
  const hs = treeToHeadings(tree)
  assert.equal(hs.length, 1)
  assert.equal(hs[0].text, 'Real')
  assert.equal(hs[0].index, 0)
})

test('treeToHeadings picks up line hints positionally', () => {
  const tree = buildTree([
    makeBlock('a', { ty: { kind: 'heading', level: 1 }, content: 'One' }),
    makeBlock('b', { ty: { kind: 'heading', level: 2 }, content: 'Two' }),
  ])
  const hs = treeToHeadings(tree, [1, 7])
  assert.equal(hs[0].line, 1)
  assert.equal(hs[1].line, 7)
})

test('treeToHeadings returns [] for empty / missing trees', () => {
  assert.deepEqual(treeToHeadings(null), [])
  assert.deepEqual(treeToHeadings(undefined), [])
  assert.deepEqual(treeToHeadings({ blocks: {}, root_blocks: [], metadata: {} }), [])
})

test('treeToHeadings clamps bad level values to 1', () => {
  const tree = buildTree([
    makeBlock('a', { ty: { kind: 'heading', level: 0 }, content: 'Bad' }),
    makeBlock('b', {
      ty: { kind: 'heading', level: 42 as unknown as number },
      content: 'Also bad',
    }),
  ])
  const hs = treeToHeadings(tree)
  assert.equal(hs[0].level, 1)
  assert.equal(hs[1].level, 1)
})
