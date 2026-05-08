// BL-077 follow-up — unit tests for the reveal-line consumer
// helper.
//
// Re-exported via `shell/tests/lsp-tails.test.ts` so the default
// `pnpm test` glob picks them up.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'

import { lspPositionToCmOffset, revealLineInView } from './revealLine.ts'

// ── lspPositionToCmOffset ───────────────────────────────────────────────────

test('lspPositionToCmOffset: 0,0 maps to document start', () => {
  const state = EditorState.create({ doc: 'hello\nworld' })
  assert.equal(lspPositionToCmOffset(state.doc, 0, 0), 0)
})

test('lspPositionToCmOffset: line 1 col 0 maps past the first newline', () => {
  const state = EditorState.create({ doc: 'hello\nworld' })
  // CM is 1-indexed; LSP line 1 = CM line 2 = offset 6 ("hello\n").
  assert.equal(lspPositionToCmOffset(state.doc, 1, 0), 6)
})

test('lspPositionToCmOffset: character within the line', () => {
  const state = EditorState.create({ doc: 'hello\nworld' })
  // LSP line 1 char 3 = offset 6 + 3 = 9 ("hello\nwor").
  assert.equal(lspPositionToCmOffset(state.doc, 1, 3), 9)
})

test('lspPositionToCmOffset: negative line clamps to document start', () => {
  const state = EditorState.create({ doc: 'hello\nworld' })
  assert.equal(lspPositionToCmOffset(state.doc, -1, 0), 0)
})

test('lspPositionToCmOffset: line past the document clamps to end', () => {
  const state = EditorState.create({ doc: 'hello\nworld' })
  // Two lines; line 5 is well past the end.
  assert.equal(lspPositionToCmOffset(state.doc, 5, 0), state.doc.length)
})

test('lspPositionToCmOffset: character past line length clamps at line end', () => {
  const state = EditorState.create({ doc: 'hello\nworld' })
  // LSP line 0 ('hello') has length 5; char 99 clamps to 5.
  assert.equal(lspPositionToCmOffset(state.doc, 0, 99), 5)
})

test('lspPositionToCmOffset: empty document at origin returns 0', () => {
  const state = EditorState.create({ doc: '' })
  assert.equal(lspPositionToCmOffset(state.doc, 0, 0), 0)
})

// ── revealLineInView ────────────────────────────────────────────────────────

function makeView(doc: string): EditorView {
  const state = EditorState.create({ doc })
  return new EditorView({ state })
}

test('revealLineInView: places the cursor at the resolved offset', () => {
  const view = makeView('alpha\nbeta\ngamma')
  // LSP line 2, character 3 = offset of "alpha\nbeta\n" (11) + 3 = 14
  // "alpha\nbeta\ngam|ma".
  revealLineInView(view, 2, 3)
  const sel = view.state.selection.main
  assert.equal(sel.from, 14)
  assert.equal(sel.to, 14)
})

test('revealLineInView: clamps overshoot at end of line', () => {
  const view = makeView('alpha\nbeta\ngamma')
  revealLineInView(view, 0, 999)
  // End of line 0 — "alpha".
  const sel = view.state.selection.main
  assert.equal(sel.from, 5)
})

test('revealLineInView: returns true on success', () => {
  const view = makeView('alpha')
  assert.equal(revealLineInView(view, 0, 0), true)
})

test('revealLineInView: empty buffer at 0,0 still dispatches without throwing', () => {
  const view = makeView('')
  revealLineInView(view, 0, 0)
  assert.equal(view.state.selection.main.from, 0)
})
