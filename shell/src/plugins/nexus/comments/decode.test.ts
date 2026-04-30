// Unit tests for the `com.nexus.comments` wire decoders.
//
// Run with: node --experimental-strip-types --test \
//   shell/src/plugins/nexus/comments/decode.test.ts

// Static imports — top-level await + dynamic import doesn't survive
// the tsx CJS transform that the test runner uses (see
// `editorStore.test.ts` for the same workaround).

import { decodeComment, decodeThread, decodeThreadList } from './decode.ts'

import { test } from 'node:test'
import assert from 'node:assert/strict'

const validComment = {
  id: '11111111-1111-4111-8111-111111111111',
  author: 'alice',
  body: 'first',
  mentions: ['bob'],
  created_at: '2026-04-30T00:00:00Z',
}

const validThread = {
  id: '22222222-2222-4222-8222-222222222222',
  block_id: '33333333-3333-4333-8333-333333333333',
  resolved: false,
  created_at: '2026-04-30T00:00:00Z',
  comments: [validComment],
}

test('decodeComment accepts the minimal valid shape', () => {
  const c = decodeComment(validComment)
  assert.ok(c)
  assert.equal(c!.id, validComment.id)
  assert.equal(c!.body, 'first')
  assert.deepEqual(c!.mentions, ['bob'])
  assert.equal(c!.author, 'alice')
})

test('decodeComment treats missing optionals as undefined', () => {
  const c = decodeComment({
    id: validComment.id,
    body: 'no author',
    created_at: validComment.created_at,
  })
  assert.ok(c)
  assert.equal(c!.author, undefined)
  assert.deepEqual(c!.mentions, [])
  assert.equal(c!.updated_at, undefined)
})

test('decodeComment rejects missing required fields', () => {
  assert.equal(decodeComment(null), null)
  assert.equal(decodeComment({ ...validComment, id: 42 }), null)
  assert.equal(decodeComment({ ...validComment, body: undefined }), null)
  assert.equal(decodeComment({ ...validComment, created_at: 123 }), null)
  // The decoder validates types only — kernel never emits an empty
  // `created_at` string but if it did, decode would still accept it.
  const cEmpty = decodeComment({ ...validComment, created_at: '' })
  assert.ok(cEmpty)
  assert.equal(cEmpty!.created_at, '')
})

test('decodeComment drops non-string mention entries', () => {
  const c = decodeComment({ ...validComment, mentions: ['ok', 42, null, 'also'] })
  assert.ok(c)
  assert.deepEqual(c!.mentions, ['ok', 'also'])
})

test('decodeThread requires at least one comment', () => {
  assert.equal(decodeThread({ ...validThread, comments: [] }), null)
  assert.equal(decodeThread({ ...validThread, comments: 'no' }), null)
})

test('decodeThread normalizes resolved to boolean false on missing', () => {
  const t = decodeThread({ ...validThread, resolved: undefined })
  assert.ok(t)
  assert.equal(t!.resolved, false)
})

test('decodeThread accepts the resolved branch', () => {
  const t = decodeThread({
    ...validThread,
    resolved: true,
    resolved_at: '2026-04-30T01:00:00Z',
    resolved_by: 'alice',
  })
  assert.ok(t)
  assert.equal(t!.resolved, true)
  assert.equal(t!.resolved_by, 'alice')
})

test('decodeThreadList drops malformed entries but preserves valid siblings', () => {
  const list = decodeThreadList([validThread, { junk: true }, validThread])
  assert.equal(list.length, 2)
  assert.equal(list[0].id, validThread.id)
})

test('decodeThreadList returns empty for non-array input', () => {
  assert.deepEqual(decodeThreadList(null), [])
  assert.deepEqual(decodeThreadList({ items: [] }), [])
})
