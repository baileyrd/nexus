// Pure-logic tests for the BL-048 block-ref drag contract.
// Re-exported via `shell/tests/block-ref-drag.test.ts`.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  BLOCK_REF_MIME,
  blockRefToLink,
  parseBlockRef,
  serializeBlockRef,
} from './blockRefDrag.ts'

const A_UUID = 'd8e9f0a1-2b3c-4d5e-9f01-abcdef012345'

test('MIME constant matches the contract documented in the module header', () => {
  assert.equal(BLOCK_REF_MIME, 'application/x-nexus-block-ref')
})

test('serialize → parse round-trips happy-path payload', () => {
  const out = serializeBlockRef({
    relpath: 'Notes/A.md',
    blockId: A_UUID,
    label: 'first paragraph',
  })
  const parsed = parseBlockRef(out)
  assert.deepEqual(parsed, {
    relpath: 'Notes/A.md',
    blockId: A_UUID,
    label: 'first paragraph',
  })
})

test('serialize normalises blockId to lowercase + drops empty labels', () => {
  const out = serializeBlockRef({
    relpath: 'A.md',
    blockId: A_UUID.toUpperCase(),
    label: '   ',
  })
  const parsed = parseBlockRef(out)
  assert.equal(parsed?.blockId, A_UUID)
  assert.equal(parsed?.label, null)
})

test('serialize rejects empty path / non-UUID id', () => {
  assert.throws(() => serializeBlockRef({ relpath: '', blockId: A_UUID }))
  assert.throws(() => serializeBlockRef({ relpath: 'A.md', blockId: 'not-a-uuid' }))
})

test('parse returns null for malformed inputs', () => {
  assert.equal(parseBlockRef(null), null)
  assert.equal(parseBlockRef(undefined), null)
  assert.equal(parseBlockRef(''), null)
  assert.equal(parseBlockRef('not json'), null)
  assert.equal(parseBlockRef('null'), null)
  assert.equal(parseBlockRef('{"relpath":"A.md"}'), null) // missing blockId
  assert.equal(
    parseBlockRef('{"relpath":"A.md","blockId":"oops"}'),
    null,
  )
})

test('blockRefToLink emits the BL-049 link form, with label when present', () => {
  assert.equal(
    blockRefToLink({ relpath: 'A.md', blockId: A_UUID }),
    `[[A.md#^${A_UUID}]]`,
  )
  assert.equal(
    blockRefToLink({ relpath: 'A.md', blockId: A_UUID, label: 'see this' }),
    `[[A.md#^${A_UUID}|see this]]`,
  )
})
