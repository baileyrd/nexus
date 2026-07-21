// C60 (#413) — unit tests for the pure bus-event/active-file matcher
// behind the comments pane's live cross-window/cross-peer refresh.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { commentsEventTargetsActiveFile } from './index.ts'

test('returns false when there is no active file', () => {
  assert.equal(
    commentsEventTargetsActiveFile({ file_path: 'foo.md' }, null),
    false,
  )
})

test('returns false when the payload is not an object', () => {
  assert.equal(commentsEventTargetsActiveFile(null, 'foo.md'), false)
  assert.equal(commentsEventTargetsActiveFile('foo.md', 'foo.md'), false)
  assert.equal(commentsEventTargetsActiveFile(undefined, 'foo.md'), false)
})

test('returns false when file_path is missing or not a string', () => {
  assert.equal(commentsEventTargetsActiveFile({}, 'foo.md'), false)
  assert.equal(
    commentsEventTargetsActiveFile({ file_path: 42 }, 'foo.md'),
    false,
  )
})

test('returns false when file_path does not match the active file', () => {
  assert.equal(
    commentsEventTargetsActiveFile({ file_path: 'bar.md' }, 'foo.md'),
    false,
  )
})

test('returns true when file_path matches the active file', () => {
  assert.equal(
    commentsEventTargetsActiveFile({ file_path: 'foo.md' }, 'foo.md'),
    true,
  )
})
