// BL-142 Phase 2b.2 — unit tests for the per-cell output buffer.

import { test, beforeEach } from 'node:test'
import assert from 'node:assert/strict'

import {
  _resetReplOutputStoreForTests,
  useReplOutputStore,
} from './replOutputStore.ts'

beforeEach(() => {
  _resetReplOutputStoreForTests()
})

test('append creates the buffer on first chunk', () => {
  useReplOutputStore.getState().append('s-1', 'hello ')
  const buf = useReplOutputStore.getState().buffers['s-1']
  assert.equal(buf.text, 'hello ')
  assert.equal(buf.startedAt, null, 'startedAt only set by clear()')
})

test('append concatenates chunks in arrival order', () => {
  const s = useReplOutputStore.getState()
  s.append('s-1', 'one ')
  s.append('s-1', 'two ')
  s.append('s-1', 'three')
  assert.equal(useReplOutputStore.getState().buffers['s-1'].text, 'one two three')
})

test('clear resets buffer text and sets startedAt', () => {
  const s = useReplOutputStore.getState()
  s.append('s-1', 'stale output')
  s.clear('s-1')
  const buf = useReplOutputStore.getState().buffers['s-1']
  assert.equal(buf.text, '')
  assert.ok(buf.startedAt !== null, 'startedAt must be a wall-clock millis value')
})

test('append after clear preserves startedAt', () => {
  const s = useReplOutputStore.getState()
  s.clear('s-1')
  const startedAt = useReplOutputStore.getState().buffers['s-1'].startedAt
  s.append('s-1', 'new output')
  assert.equal(
    useReplOutputStore.getState().buffers['s-1'].startedAt,
    startedAt,
    'append must not clobber startedAt',
  )
})

test('drop removes the buffer entry', () => {
  const s = useReplOutputStore.getState()
  s.append('s-1', 'x')
  s.append('s-2', 'y')
  s.drop('s-1')
  assert.equal(useReplOutputStore.getState().buffers['s-1'], undefined)
  assert.equal(useReplOutputStore.getState().buffers['s-2'].text, 'y')
})

test('buffers for different sessionIds are independent', () => {
  const s = useReplOutputStore.getState()
  s.append('s-1', 'one')
  s.append('s-2', 'two')
  assert.equal(useReplOutputStore.getState().buffers['s-1'].text, 'one')
  assert.equal(useReplOutputStore.getState().buffers['s-2'].text, 'two')
})
