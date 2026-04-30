// Pure-logic tests for the BL-051 multi-block-to-multi-cursor
// promotion. Re-exported via
// `shell/tests/multi-cursor-promote.test.ts`.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { EditorSelection, EditorState, Text } from '@codemirror/state'
import { EditorView } from '@codemirror/view'

import {
  blockOfLine,
  blocksInRange,
  cursorsFromBlocks,
  multiCursorPromoteExt,
  promoteBlockSelectionToMultiCursor,
} from './multiCursorPromote.ts'

// ── blockOfLine ─────────────────────────────────────────────────────────────

test('blockOfLine returns null on a blank line', () => {
  const doc = Text.of(['paragraph one', '', 'paragraph two', ''])
  assert.equal(blockOfLine(doc, 2), null)
})

test('blockOfLine returns the maximal run of non-blank lines', () => {
  const doc = Text.of(['line a', 'line b', 'line c', '', 'tail'])
  const block = blockOfLine(doc, 2)
  assert.ok(block)
  assert.equal(block!.topLineNo, 1)
  assert.equal(block!.bottomLineNo, 3)
})

// ── blocksInRange ───────────────────────────────────────────────────────────

test('blocksInRange returns one entry per spanned block, skipping blank gaps', () => {
  const doc = Text.of([
    'block A line 1',
    'block A line 2',
    '',
    'block B',
    '',
    'block C line 1',
    'block C line 2',
  ])
  // Selection spans line 1 → end of line 7.
  const blocks = blocksInRange(doc, 0, doc.length)
  assert.equal(blocks.length, 3)
  assert.equal(blocks[0].topLineNo, 1)
  assert.equal(blocks[0].bottomLineNo, 2)
  assert.equal(blocks[1].topLineNo, 4)
  assert.equal(blocks[1].bottomLineNo, 4)
  assert.equal(blocks[2].topLineNo, 6)
  assert.equal(blocks[2].bottomLineNo, 7)
})

test('blocksInRange returns one block when the selection is contained within it', () => {
  const doc = Text.of(['only block', 'second line', '', 'tail'])
  const blocks = blocksInRange(doc, 2, 5)
  assert.equal(blocks.length, 1)
  assert.equal(blocks[0].topLineNo, 1)
})

test('blocksInRange returns empty when the selection is entirely on blank lines', () => {
  const doc = Text.of(['hello', '', '', 'world'])
  // Range covers only the two blank lines (positions 6..7).
  const start = doc.line(2).from
  const end = doc.line(3).to
  assert.deepEqual(blocksInRange(doc, start, end), [])
})

// ── cursorsFromBlocks ───────────────────────────────────────────────────────

test('cursorsFromBlocks places one cursor per block at the anchor row+col', () => {
  const doc = Text.of([
    'aaa', // line 1, block A row 0
    'bbbb', // line 2, block A row 1   ← anchor (col 2)
    '', // line 3
    'cc', // line 4, block B row 0
    'dddd', // line 5, block B row 1
    '', // line 6
    'eeeee', // line 7, block C row 0
    'fffff', // line 8, block C row 1
  ])
  const blocks = blocksInRange(doc, 0, doc.length)
  // Anchor on line 2 col 2 (offset = 4 + 2 = 6 in the doc).
  const anchorPos = doc.line(2).from + 2
  const cursors = cursorsFromBlocks(doc, blocks, anchorPos)
  assert.equal(cursors.length, 3)
  // Block A row 1 col 2 → line 2, col 2.
  assert.equal(cursors[0], doc.line(2).from + 2)
  // Block B row 1 col 2 → line 5, col 2.
  assert.equal(cursors[1], doc.line(5).from + 2)
  // Block C row 1 col 2 → line 8, col 2.
  assert.equal(cursors[2], doc.line(8).from + 2)
})

test('cursorsFromBlocks clamps row when target block is shorter', () => {
  const doc = Text.of([
    'aaa', // line 1
    'bbb', // line 2
    'ccc', // line 3   ← anchor row 2 col 1 within block A
    '',
    'short', // line 5 — block B has only 1 row
  ])
  const blocks = blocksInRange(doc, 0, doc.length)
  const anchorPos = doc.line(3).from + 1
  const cursors = cursorsFromBlocks(doc, blocks, anchorPos)
  // Block B row index 2 doesn't exist → clamp to its last row (row 0).
  assert.equal(cursors[1], doc.line(5).from + 1)
})

test('cursorsFromBlocks clamps column when target row is shorter', () => {
  const doc = Text.of([
    'longest line here', // line 1   ← anchor col 12
    '',
    'tiny', // line 3 — col 12 doesn't fit
  ])
  const blocks = blocksInRange(doc, 0, doc.length)
  const anchorPos = doc.line(1).from + 12
  const cursors = cursorsFromBlocks(doc, blocks, anchorPos)
  // Cursor on line 3 clamps to line end (length 4).
  assert.equal(cursors[1], doc.line(3).from + 4)
})

test('cursorsFromBlocks falls back to block starts when anchor sits on a blank line', () => {
  const doc = Text.of(['hi', '', 'there'])
  const blocks = blocksInRange(doc, 0, doc.length)
  const blankPos = doc.line(2).from
  const cursors = cursorsFromBlocks(doc, blocks, blankPos)
  assert.deepEqual(cursors, [doc.line(1).from, doc.line(3).from])
})

// ── promoteBlockSelectionToMultiCursor ──────────────────────────────────────

function makeView(doc: string, anchor: number, head: number): EditorView {
  return new EditorView({
    state: EditorState.create({
      doc,
      selection: EditorSelection.range(anchor, head),
      extensions: [multiCursorPromoteExt()],
    }),
  })
}

test('promote: returns false on a collapsed selection (falls through)', () => {
  const view = makeView('hello\n\nworld\n', 0, 0)
  assert.equal(promoteBlockSelectionToMultiCursor(view), false)
  view.destroy()
})

test('promote: returns false when the selection covers only one block', () => {
  const view = makeView('foo bar baz\n', 0, 7)
  assert.equal(promoteBlockSelectionToMultiCursor(view), false)
  view.destroy()
})

test('promote: returns true and produces one cursor per spanned block', () => {
  const text = 'aaa\nbbbb\n\ncc\ndddd\n\neeeee\nfffff\n'
  // Anchor on line 1 col 0; head at end so the selection spans
  // every block.
  const view = makeView(text, 0, text.length)
  const ok = promoteBlockSelectionToMultiCursor(view)
  assert.equal(ok, true)
  const ranges = view.state.selection.ranges
  assert.equal(ranges.length, 3)
  for (const r of ranges) assert.equal(r.from, r.to, 'cursors are collapsed')
  // Each cursor lands at the start of its respective block (anchor
  // row 0 col 0).
  const lines = view.state.doc
  assert.equal(ranges[0].head, lines.line(1).from)
  assert.equal(ranges[1].head, lines.line(4).from)
  assert.equal(ranges[2].head, lines.line(7).from)
  view.destroy()
})

test('promote: preserves anchor row + col across blocks of varying heights', () => {
  const text = 'aaa\nbbb\n\ncc\ndddd\n'
  // Anchor on line 2 col 1 (within block A, row 1).
  const anchor = 4 + 1
  const head = text.length
  const view = makeView(text, anchor, head)
  const ok = promoteBlockSelectionToMultiCursor(view)
  assert.equal(ok, true)
  const ranges = view.state.selection.ranges
  assert.equal(ranges.length, 2)
  // Block A row 1 col 1 → line 2 col 1.
  assert.equal(ranges[0].head, view.state.doc.line(2).from + 1)
  // Block B row 1 col 1 → line 5 col 1 (block B has rows 1-2).
  assert.equal(ranges[1].head, view.state.doc.line(5).from + 1)
  view.destroy()
})

test('promote: main cursor index is the last range so typing applies bottom-up', () => {
  const text = 'aa\n\nbb\n\ncc\n'
  const view = makeView(text, 0, text.length)
  promoteBlockSelectionToMultiCursor(view)
  const sel = view.state.selection
  assert.equal(sel.mainIndex, sel.ranges.length - 1)
  view.destroy()
})
