// shell/src/plugins/nexus/editor/cm/gitGutter.test.ts
//
// BL-079 — unit tests for `buildLineMarkers` (the pure factor of
// the gutter extension). The DOM-mounting and IPC-driven branches
// are integration concerns; the routing matrix (added vs. modified
// vs. deletion-above) is the load-bearing logic.

import { describe, it } from 'node:test'
import assert from 'node:assert/strict'
import { buildLineMarkers } from './gitGutter.ts'

describe('buildLineMarkers', () => {
  it('returns empty when no hunks', () => {
    const m = buildLineMarkers([])
    assert.equal(m.size, 0)
  })

  it('marks pure additions with kind="added"', () => {
    // Hunk: 0 lines in old at line 1 → 2 new lines at lines 1-2.
    const m = buildLineMarkers([
      {
        old_start: 1,
        old_count: 0,
        new_start: 1,
        new_count: 2,
        lines: [
          { kind: 'Added', content: 'first new' },
          { kind: 'Added', content: 'second new' },
        ],
      },
    ])
    assert.equal(m.size, 2)
    assert.equal(m.get(1)?.kind, 'added')
    assert.equal(m.get(2)?.kind, 'added')
    assert.deepEqual(m.get(1)?.removed, [])
  })

  it('marks +/- pairs as modified with the original lines preserved', () => {
    // Hunk: one removed line replaced by one added line, same
    // location. Result: line 1 in the new file is "modified",
    // tooltip carries the removed content.
    const m = buildLineMarkers([
      {
        old_start: 1,
        old_count: 1,
        new_start: 1,
        new_count: 1,
        lines: [
          { kind: 'Removed', content: 'old version' },
          { kind: 'Added', content: 'new version' },
        ],
      },
    ])
    assert.equal(m.size, 1)
    const marker = m.get(1)
    assert.equal(marker?.kind, 'modified')
    assert.deepEqual(marker?.removed, ['old version'])
  })

  it('marks pure deletions on the line above the gap', () => {
    // Hunk: a context line, then a deletion. The context (new line
    // 1) gets the deletion-above marker; no lines correspond to
    // the removed content in the new file.
    const m = buildLineMarkers([
      {
        old_start: 1,
        old_count: 2,
        new_start: 1,
        new_count: 1,
        lines: [
          { kind: 'Context', content: 'kept' },
          { kind: 'Removed', content: 'gone' },
        ],
      },
    ])
    assert.equal(m.size, 1)
    const marker = m.get(1)
    assert.equal(marker?.kind, 'deletion-above')
    assert.deepEqual(marker?.removed, ['gone'])
  })

  it('marks deletion-only-at-hunk-end on the last observed new line', () => {
    // Hunk: one context line then a removed line at the end. No
    // following context to anchor against — fall back to the
    // last observed new line.
    const m = buildLineMarkers([
      {
        old_start: 5,
        old_count: 2,
        new_start: 5,
        new_count: 1,
        lines: [
          { kind: 'Context', content: 'kept' },
          { kind: 'Removed', content: 'tail' },
        ],
      },
    ])
    const marker = m.get(5)
    assert.equal(marker?.kind, 'deletion-above')
    assert.deepEqual(marker?.removed, ['tail'])
  })

  it('flushes pending Removed across multiple Removed lines', () => {
    // Three removed lines back-to-back, then a context. Single
    // deletion-above marker carrying all three removed lines.
    const m = buildLineMarkers([
      {
        old_start: 1,
        old_count: 4,
        new_start: 1,
        new_count: 1,
        lines: [
          { kind: 'Removed', content: 'a' },
          { kind: 'Removed', content: 'b' },
          { kind: 'Removed', content: 'c' },
          { kind: 'Context', content: 'kept' },
        ],
      },
    ])
    // pendingRemoved attaches to the line "before" the context.
    // No previous new line existed — fallback to new_start (1).
    const marker = m.get(1)
    assert.equal(marker?.kind, 'deletion-above')
    assert.deepEqual(marker?.removed, ['a', 'b', 'c'])
  })

  it('treats a multi-line +/- block as a single modification per added line', () => {
    // Two removed → two added, all at the same hunk location. The
    // first added line picks up both removed contents (everything
    // pending at that point); subsequent added lines are pure
    // adds.
    const m = buildLineMarkers([
      {
        old_start: 1,
        old_count: 2,
        new_start: 1,
        new_count: 2,
        lines: [
          { kind: 'Removed', content: 'old-a' },
          { kind: 'Removed', content: 'old-b' },
          { kind: 'Added', content: 'new-a' },
          { kind: 'Added', content: 'new-b' },
        ],
      },
    ])
    assert.equal(m.get(1)?.kind, 'modified')
    assert.deepEqual(m.get(1)?.removed, ['old-a', 'old-b'])
    assert.equal(m.get(2)?.kind, 'added')
    assert.deepEqual(m.get(2)?.removed, [])
  })

  it('handles multiple hunks independently', () => {
    const m = buildLineMarkers([
      {
        old_start: 1,
        old_count: 0,
        new_start: 1,
        new_count: 1,
        lines: [{ kind: 'Added', content: 'first' }],
      },
      {
        old_start: 10,
        old_count: 1,
        new_start: 11,
        new_count: 1,
        lines: [
          { kind: 'Removed', content: 'gone' },
          { kind: 'Added', content: 'replaced' },
        ],
      },
    ])
    assert.equal(m.get(1)?.kind, 'added')
    assert.equal(m.get(11)?.kind, 'modified')
    assert.deepEqual(m.get(11)?.removed, ['gone'])
  })
})
