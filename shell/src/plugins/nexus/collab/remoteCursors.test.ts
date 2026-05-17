// BL-143 Phase 2.2 — remoteCursors pure-layer tests.
//
// Only the pure projection + color hash are tested here; the CM6
// ViewPlugin / Decoration glue is exercised end-to-end in the shell.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  buildRemoteCursorRanges,
  colorForUserId,
} from './remoteCursors.ts'
import type { CollabPeer } from './collabStore.ts'

function peer(
  user_id: string,
  display_name: string,
  cursor?: { relpath: string; offset?: number; selection_end?: number },
): CollabPeer {
  return { user_id, display_name, cursor, last_seen_ms: 0 }
}

test('only peers focused on the editor relpath produce ranges', () => {
  const peers: Record<string, CollabPeer> = {
    a: peer('a', 'Alice', { relpath: 'x.md', offset: 5 }),
    b: peer('b', 'Bob',   { relpath: 'y.md', offset: 7 }),
    c: peer('c', 'Carol'), // no cursor
  }
  const ranges = buildRemoteCursorRanges('x.md', peers, 100)
  assert.equal(ranges.length, 1)
  assert.equal(ranges[0]!.user_id, 'a')
  assert.equal(ranges[0]!.offset, 5)
  assert.equal(ranges[0]!.selection_end, undefined)
})

test('peers without offset are skipped (Phase 1.3 frame)', () => {
  const peers: Record<string, CollabPeer> = {
    a: peer('a', 'Alice', { relpath: 'x.md' }), // no offset
  }
  assert.deepEqual(buildRemoteCursorRanges('x.md', peers, 100), [])
})

test('selection_end equal to offset collapses to caret', () => {
  const peers: Record<string, CollabPeer> = {
    a: peer('a', 'Alice', { relpath: 'x.md', offset: 5, selection_end: 5 }),
  }
  const ranges = buildRemoteCursorRanges('x.md', peers, 100)
  assert.equal(ranges[0]!.selection_end, undefined)
})

test('out-of-range offsets clamp to doc length', () => {
  const peers: Record<string, CollabPeer> = {
    a: peer('a', 'Alice', { relpath: 'x.md', offset: 999, selection_end: 1500 }),
  }
  const ranges = buildRemoteCursorRanges('x.md', peers, 100)
  assert.equal(ranges[0]!.offset, 100)
  assert.equal(ranges[0]!.selection_end, 100)
})

test('ranges are sorted by offset then user_id', () => {
  const peers: Record<string, CollabPeer> = {
    z: peer('z', 'Zach',  { relpath: 'x.md', offset: 20 }),
    a: peer('a', 'Alice', { relpath: 'x.md', offset: 5 }),
    m: peer('m', 'Mike',  { relpath: 'x.md', offset: 5 }),
  }
  const ranges = buildRemoteCursorRanges('x.md', peers, 100)
  assert.deepEqual(ranges.map((r) => r.user_id), ['a', 'm', 'z'])
})

test('colorForUserId is stable and within #RRGGBB', () => {
  const a = colorForUserId('alice')
  const b = colorForUserId('alice')
  assert.equal(a, b, 'same id → same color')
  assert.match(a, /^#[0-9a-f]{6}$/, 'six-digit hex')
})

test('colorForUserId distinguishes different ids', () => {
  // Not strictly required (8 buckets means collisions exist), but
  // these two ids must land in different buckets for the demo to
  // look right.
  assert.notEqual(colorForUserId('alice'), colorForUserId('bob'))
})
