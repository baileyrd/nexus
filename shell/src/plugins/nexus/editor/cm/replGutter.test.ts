// BL-142 Phase 2b.2 — pure-factor tests for the Run gutter. The
// DOM/click/visual layer needs a live Tauri window to verify; the
// underlying state derivation + equality check are testable here.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { stateFromDoc, stateIsSame } from './replGutter.ts'

test('stateFromDoc maps fence-open lines to blocks', () => {
  const doc = '# h\n\n```python repl\nprint(1)\n```\n\nafter\n'
  const state = stateFromDoc(doc)
  assert.equal(state.blocks.size, 1)
  const block = state.blocks.get(3)
  assert.equal(block?.language, 'python')
  assert.equal(block?.bodyStart, 4)
})

test('stateFromDoc returns an empty map for docs with no REPL fences', () => {
  const state = stateFromDoc('# h\n\n```rust\nfn main() {}\n```\n')
  assert.equal(state.blocks.size, 0)
})

test('stateFromDoc finds multiple REPL blocks at their respective open lines', () => {
  const doc =
    '```python repl\nx = 1\n```\n\n```node repl\nconsole.log(1)\n```\n'
  const state = stateFromDoc(doc)
  assert.equal(state.blocks.size, 2)
  assert.ok(state.blocks.has(1))
  assert.ok(state.blocks.has(5))
})

test('stateIsSame returns true for identical line sets', () => {
  const a = stateFromDoc('```python repl\nprint(1)\n```\n')
  const b = stateFromDoc('```python repl\nprint(2)\n```\n')
  // Same open-line layout even though body differs — the gutter
  // doesn't redraw on body edits inside an existing block.
  assert.equal(stateIsSame(a, b), true)
})

test('stateIsSame returns false when a new block is added', () => {
  const a = stateFromDoc('```python repl\nx\n```\n')
  const b = stateFromDoc(
    '```python repl\nx\n```\n\n```node repl\ny\n```\n',
  )
  assert.equal(stateIsSame(a, b), false)
})

test('stateIsSame returns false when a block moves to a different line', () => {
  const a = stateFromDoc('```python repl\nx\n```\n')
  const b = stateFromDoc('intro\n\n```python repl\nx\n```\n')
  assert.equal(stateIsSame(a, b), false)
})
