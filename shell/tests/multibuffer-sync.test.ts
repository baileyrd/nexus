// BL-141 Approach B step 3b — pure-helper tests for the multibuffer
// registry. The plugin's subscriber + IPC plumbing is exercised by
// integration tests; these pin the projection shape so the
// `changed`-event handler doesn't silently drop multibuffers or
// re-route to the wrong tab.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import type { EditorSnapshot, BlockId } from '../src/plugins/nexus/editor/types'
import {
  CHANGED_TOPIC_PREFIX,
  changedTopicRelpath,
  extractSources,
  isMultibufferRelpath,
  multibuffersWatchingSource,
  type MultibufferRegistry,
} from '../src/plugins/nexus/multibufferSync/multibufferRegistry'

function snapshotWith(blocks: Array<{ kind: string; source_relpath?: string }>): EditorSnapshot {
  const blockMap: Record<BlockId, never> = {} as Record<BlockId, never>
  const root: string[] = []
  blocks.forEach((b, i) => {
    const id = `b${i}`
    root.push(id)
    ;(blockMap as Record<string, unknown>)[id] = {
      id,
      ty: b,
      content: '',
      annotations: [],
      properties: {},
      parent_id: null,
      children: [],
      index_in_parent: i,
      created_at: 0,
      updated_at: 0,
      is_deleted: false,
    }
  })
  return {
    relpath: 'multibuffer://stub',
    tree: {
      blocks: blockMap,
      root_blocks: root,
    },
    undo_position: null,
    undo_len: 0,
    can_undo: false,
    can_redo: false,
    revision: 0,
  } as unknown as EditorSnapshot
}

// ── extractSources ───────────────────────────────────────────────────────────

test('extractSources collects unique Excerpt source_relpaths in order', () => {
  const snap = snapshotWith([
    { kind: 'excerpt', source_relpath: 'a.md' },
    { kind: 'excerpt', source_relpath: 'b.md' },
    { kind: 'excerpt', source_relpath: 'a.md' },
    { kind: 'excerpt', source_relpath: 'c.md' },
  ])
  assert.deepEqual(extractSources(snap), ['a.md', 'b.md', 'c.md'])
})

test('extractSources skips non-Excerpt blocks', () => {
  const snap = snapshotWith([
    { kind: 'paragraph' },
    { kind: 'excerpt', source_relpath: 'real.md' },
  ])
  assert.deepEqual(extractSources(snap), ['real.md'])
})

test('extractSources tolerates Excerpt blocks missing source_relpath', () => {
  const snap = snapshotWith([
    { kind: 'excerpt' },
    { kind: 'excerpt', source_relpath: 'ok.md' },
  ])
  assert.deepEqual(extractSources(snap), ['ok.md'])
})

test('extractSources returns empty for an empty tree', () => {
  const snap = snapshotWith([])
  assert.deepEqual(extractSources(snap), [])
})

// ── multibuffersWatchingSource ───────────────────────────────────────────────

test('multibuffersWatchingSource returns the relpaths whose sources contain the changed file', () => {
  const reg: MultibufferRegistry = new Map([
    ['multibuffer://1', { sources: new Set(['a.md', 'b.md']) }],
    ['multibuffer://2', { sources: new Set(['b.md', 'c.md']) }],
    ['multibuffer://3', { sources: new Set(['d.md']) }],
  ])
  assert.deepEqual(
    multibuffersWatchingSource(reg, 'b.md').sort(),
    ['multibuffer://1', 'multibuffer://2'],
  )
  assert.deepEqual(multibuffersWatchingSource(reg, 'd.md'), ['multibuffer://3'])
  assert.deepEqual(multibuffersWatchingSource(reg, 'zzz.md'), [])
})

test('multibuffersWatchingSource on empty registry returns empty', () => {
  assert.deepEqual(
    multibuffersWatchingSource(new Map(), 'whatever.md'),
    [],
  )
})

// ── changedTopicRelpath ──────────────────────────────────────────────────────

test('changedTopicRelpath strips the prefix and returns the suffix', () => {
  assert.equal(
    changedTopicRelpath(`${CHANGED_TOPIC_PREFIX}src/lib.md`),
    'src/lib.md',
  )
})

test('changedTopicRelpath returns null for non-changed topics', () => {
  assert.equal(changedTopicRelpath('com.nexus.lsp.notification'), null)
  assert.equal(changedTopicRelpath('unrelated'), null)
})

test('changedTopicRelpath returns null when suffix is empty', () => {
  assert.equal(changedTopicRelpath(CHANGED_TOPIC_PREFIX), null)
})

// ── isMultibufferRelpath ────────────────────────────────────────────────────

test('isMultibufferRelpath detects the multibuffer:// prefix', () => {
  assert.equal(isMultibufferRelpath('multibuffer://abc'), true)
  assert.equal(isMultibufferRelpath('src/lib.md'), false)
  assert.equal(isMultibufferRelpath(''), false)
})
