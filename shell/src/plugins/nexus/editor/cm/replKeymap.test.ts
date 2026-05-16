// BL-142 Phase 2b.2 — pure-factor tests for the Shift-Enter
// cursor → block resolver. The actual keymap firing (browser
// keyboard events + CM6 priority) needs visual verification.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { blockForCursor } from './replKeymap.ts'

test('blockForCursor finds the REPL block at the cursor', () => {
  const doc = '# h\n\n```python repl\nprint(2+2)\n```\n'
  // Cursor on "print(2+2)" → offset of the first p (after the
  // fence open + newline)
  const cursor = doc.indexOf('print')
  const hit = blockForCursor(doc, cursor)
  assert.equal(hit?.block.language, 'python')
  assert.equal(hit?.code, 'print(2+2)\n')
})

test('blockForCursor returns null when cursor is outside any block', () => {
  const doc = '# heading\n\n```python repl\nprint(1)\n```\n\nafter\n'
  const cursorOutside = doc.indexOf('heading')
  assert.equal(blockForCursor(doc, cursorOutside), null)
})

test('blockForCursor accepts cursor on the opening fence line', () => {
  const doc = '```python repl\nprint(1)\n```\n'
  const cursorOnFence = doc.indexOf('```python')
  const hit = blockForCursor(doc, cursorOnFence)
  assert.equal(hit?.block.language, 'python')
  // Body extraction still works — Shift-Enter on the fence runs
  // the cell.
  assert.equal(hit?.code, 'print(1)\n')
})

test('blockForCursor accepts cursor on the closing fence line', () => {
  const doc = '```python repl\nprint(1)\n```\n'
  const cursorOnClose = doc.lastIndexOf('```')
  const hit = blockForCursor(doc, cursorOnClose)
  assert.equal(hit?.block.language, 'python')
})

test('blockForCursor picks the right block when multiple are present', () => {
  const doc =
    '```python repl\nprint(1)\n```\n\n```node repl\nconsole.log(1)\n```\n'
  const cursorInSecond = doc.indexOf('console.log')
  const hit = blockForCursor(doc, cursorInSecond)
  assert.equal(hit?.block.language, 'node')
  assert.equal(hit?.code, 'console.log(1)\n')
})

test('blockForCursor handles cursor at end of doc gracefully', () => {
  const doc = '```python repl\nprint(1)\n```\n'
  const hit = blockForCursor(doc, doc.length)
  // Cursor past the trailing newline — outside any block.
  assert.equal(hit, null)
})

test('blockForCursor returns null when cursorOffset is past end of doc', () => {
  const doc = '```python repl\nprint(1)\n```\n'
  // Defensively clamp — don't infinite-loop or crash on a
  // pathological offset.
  const hit = blockForCursor(doc, doc.length + 100)
  assert.equal(hit, null)
})
