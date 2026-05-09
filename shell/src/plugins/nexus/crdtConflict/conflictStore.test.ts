// BL-074 — resolver modal store unit tests.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  _resetConflictStoreForTests,
  useConflictStore,
} from './conflictStore'
import type { ConflictDetail } from './types'

function fixtureConcurrent(blockId: string): ConflictDetail {
  return {
    kind: 'concurrent_block_edit',
    block_id: blockId,
    local: { site: 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', lamport: 1 },
    remote: { site: 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', lamport: 2 },
    local_content: 'L',
    remote_content: 'R',
  }
}

function fixtureStructural(blockId: string): ConflictDetail {
  return {
    kind: 'structural_delete_edit',
    block_id: blockId,
    delete: { site: 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', lamport: 1 },
    edit: { site: 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', lamport: 2 },
    local_content: 'kept-edit',
    delete_origin: 'remote',
  }
}

test('enqueue sets `current` when idle', () => {
  _resetConflictStoreForTests()
  useConflictStore.getState().enqueue('notes.md', [fixtureConcurrent('block-1')])
  const s = useConflictStore.getState()
  assert.notStrictEqual(s.current, null)
  assert.strictEqual(s.current?.relpath, 'notes.md')
  assert.strictEqual(s.current?.rows.length, 1)
  assert.strictEqual(s.current?.rows[0]?.resolution, 'pending')
  assert.strictEqual(s.queue.length, 0)
})

test('enqueue queues subsequent requests behind `current`', () => {
  _resetConflictStoreForTests()
  const store = useConflictStore.getState()
  store.enqueue('a.md', [fixtureConcurrent('block-a')])
  store.enqueue('b.md', [fixtureStructural('block-b')])
  const s = useConflictStore.getState()
  assert.strictEqual(s.current?.relpath, 'a.md')
  assert.strictEqual(s.queue.length, 1)
  assert.strictEqual(s.queue[0]?.relpath, 'b.md')
})

test('enqueue with empty conflicts is a no-op', () => {
  _resetConflictStoreForTests()
  useConflictStore.getState().enqueue('notes.md', [])
  assert.strictEqual(useConflictStore.getState().current, null)
})

test('setResolution updates the row in place', () => {
  _resetConflictStoreForTests()
  useConflictStore.getState().enqueue('notes.md', [
    fixtureConcurrent('block-1'),
    fixtureStructural('block-2'),
  ])
  useConflictStore.getState().setResolution(0, 'used_remote')
  const s = useConflictStore.getState()
  assert.strictEqual(s.current?.rows[0]?.resolution, 'used_remote')
  assert.strictEqual(s.current?.rows[1]?.resolution, 'pending')
  assert.strictEqual(s.current?.rows[0]?.error, null)
})

test('setResolution carries error when supplied', () => {
  _resetConflictStoreForTests()
  useConflictStore.getState().enqueue('notes.md', [fixtureConcurrent('block-1')])
  useConflictStore.getState().setResolution(0, 'pending', 'IPC blew up')
  const s = useConflictStore.getState()
  assert.strictEqual(s.current?.rows[0]?.error, 'IPC blew up')
  assert.strictEqual(s.current?.rows[0]?.resolution, 'pending')
})

test('setResolution rejects out-of-range index without crashing', () => {
  _resetConflictStoreForTests()
  useConflictStore.getState().enqueue('notes.md', [fixtureConcurrent('block-1')])
  useConflictStore.getState().setResolution(5, 'used_remote')
  // Original row stays pending — the bad call is silently dropped.
  assert.strictEqual(useConflictStore.getState().current?.rows[0]?.resolution, 'pending')
})

test('dismissCurrent advances to the next queued request', () => {
  _resetConflictStoreForTests()
  const store = useConflictStore.getState()
  store.enqueue('a.md', [fixtureConcurrent('block-a')])
  store.enqueue('b.md', [fixtureStructural('block-b')])
  useConflictStore.getState().dismissCurrent()
  const s = useConflictStore.getState()
  assert.strictEqual(s.current?.relpath, 'b.md')
  assert.strictEqual(s.queue.length, 0)
})

test('dismissCurrent clears state when queue is empty', () => {
  _resetConflictStoreForTests()
  useConflictStore.getState().enqueue('a.md', [fixtureConcurrent('block-a')])
  useConflictStore.getState().dismissCurrent()
  assert.strictEqual(useConflictStore.getState().current, null)
  assert.strictEqual(useConflictStore.getState().queue.length, 0)
})

test('successive enqueues assign distinct ids', () => {
  _resetConflictStoreForTests()
  useConflictStore.getState().enqueue('a.md', [fixtureConcurrent('block-a')])
  const firstId = useConflictStore.getState().current?.id
  useConflictStore.getState().enqueue('b.md', [fixtureStructural('block-b')])
  const queuedId = useConflictStore.getState().queue[0]?.id
  assert.notStrictEqual(firstId, queuedId, 'queued request must have a fresh id')
  assert.ok(typeof firstId === 'number' && typeof queuedId === 'number')
})
