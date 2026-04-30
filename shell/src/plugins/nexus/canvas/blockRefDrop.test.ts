// Pure-logic tests for the BL-048 canvas-side drop helpers.
// Re-exported via `shell/tests/block-ref-drop.test.ts`.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  BLOCK_REF_MIME,
  serializeBlockRef,
} from '../editor/blockRefDrag.ts'
import {
  buildBlockRefDropNode,
  hasBlockRefPayload,
  readBlockRefPayload,
} from './blockRefDrop.ts'

const A_UUID = 'd8e9f0a1-2b3c-4d5e-9f01-abcdef012345'

interface FakeDataTransfer {
  types: string[]
  data: Record<string, string>
  getData(t: string): string
}

function makeEvent(types: string[], data: Record<string, string>): {
  dataTransfer: FakeDataTransfer
} {
  const dt: FakeDataTransfer = {
    types,
    data,
    getData(type: string) {
      return data[type] ?? ''
    },
  }
  return { dataTransfer: dt }
}

test('hasBlockRefPayload: returns true only when our MIME is in the types list', () => {
  const yes = makeEvent([BLOCK_REF_MIME, 'text/plain'], {})
  assert.equal(hasBlockRefPayload(yes as unknown as DragEvent), true)
  const no = makeEvent(['text/plain'], {})
  assert.equal(hasBlockRefPayload(no as unknown as DragEvent), false)
  // No dataTransfer at all (e.g. a synthetic test event).
  assert.equal(
    hasBlockRefPayload({ dataTransfer: null } as unknown as DragEvent),
    false,
  )
})

test('readBlockRefPayload: parses the MIME and returns null for unrelated drops', () => {
  const ok = makeEvent(
    [BLOCK_REF_MIME],
    { [BLOCK_REF_MIME]: serializeBlockRef({ relpath: 'A.md', blockId: A_UUID }) },
  )
  const parsed = readBlockRefPayload(ok as unknown as DragEvent)
  assert.equal(parsed?.relpath, 'A.md')
  assert.equal(parsed?.blockId, A_UUID)

  const noPayload = makeEvent(['text/plain'], { 'text/plain': 'hello' })
  assert.equal(readBlockRefPayload(noPayload as unknown as DragEvent), null)

  const malformed = makeEvent(
    [BLOCK_REF_MIME],
    { [BLOCK_REF_MIME]: 'not-json' },
  )
  assert.equal(readBlockRefPayload(malformed as unknown as DragEvent), null)
})

test('buildBlockRefDropNode: text body is the BL-049 link form, anchored on the drop', () => {
  const node = buildBlockRefDropNode(
    { relpath: 'Notes/A.md', blockId: A_UUID, label: 'first' },
    { x: 200, y: 100 },
  )
  assert.equal(node.type, 'text')
  assert.equal(node.text, `[[Notes/A.md#^${A_UUID}|first]]`)
  // Default text-node size is 250 × 60; the node centres on the
  // drop point (rounded to integers).
  assert.equal(node.x, 200 - Math.round(250 / 2))
  assert.equal(node.y, 100 - Math.round(60 / 2))
  assert.equal(node.width, 250)
  assert.equal(node.height, 60)
  assert.equal(node.label, 'first')
  // Two consecutive builds get distinct ids.
  const node2 = buildBlockRefDropNode(
    { relpath: 'A.md', blockId: A_UUID },
    { x: 0, y: 0 },
  )
  assert.notEqual(node.id, node2.id)
})

test('buildBlockRefDropNode: omits label when payload lacks one', () => {
  const node = buildBlockRefDropNode(
    { relpath: 'A.md', blockId: A_UUID },
    { x: 0, y: 0 },
  )
  assert.equal(node.text, `[[A.md#^${A_UUID}]]`)
  assert.equal(node.label, undefined)
})
