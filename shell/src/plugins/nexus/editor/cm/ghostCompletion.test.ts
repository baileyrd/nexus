// BL-034 — unit tests for the CodeMirror ghost completion extension.
//
// Tests cover the pure state-machine behaviour: setting a
// suggestion, edits / selection moves invalidating it, and the
// runId-based stale guard. The view-driven Tab/Esc handlers
// (acceptSuggestion / dismissSuggestion) need a real DOM
// EditorView, which `node:test` can't provide; they're exercised
// in the integration test that mounts CodeMirrorHost.
//
// Approach: operate on EditorState transactions directly. The field
// definition + invalidation predicate live in
// `__test__.suggestionField` / `__test__.setSuggestion`.

import { EditorState, EditorSelection } from '@codemirror/state'
import { __test__ } from './ghostCompletion.ts'

import { test } from 'node:test'
import assert from 'node:assert/strict'

const { setSuggestion, suggestionField } = __test__

function makeState(doc: string, headPos: number = doc.length): EditorState {
  return EditorState.create({
    doc,
    selection: EditorSelection.cursor(headPos),
    extensions: [suggestionField],
  })
}

test('suggestionField starts empty', () => {
  const state = makeState('hello')
  assert.equal(state.field(suggestionField), null)
})

test('setSuggestion effect populates the field', () => {
  const state = makeState('hello world ')
  const tr = state.update({
    effects: setSuggestion.of({ pos: 12, text: 'and goodbye', requestId: 1 }),
  })
  const sug = tr.state.field(suggestionField)
  assert.ok(sug)
  assert.equal(sug?.text, 'and goodbye')
  assert.equal(sug?.requestId, 1)
})

test('a doc change invalidates an existing suggestion', () => {
  let state = makeState('hello world ')
  state = state.update({
    effects: setSuggestion.of({ pos: 12, text: 'rest', requestId: 1 }),
  }).state
  // Type a character — the field's update predicate clears non-effect
  // doc changes.
  state = state.update({ changes: { from: 12, to: 12, insert: 'X' } }).state
  assert.equal(
    state.field(suggestionField),
    null,
    'doc change must clear stale suggestion',
  )
})

test('a selection move invalidates the suggestion', () => {
  let state = makeState('hello world ')
  state = state.update({
    effects: setSuggestion.of({ pos: 12, text: 'tail', requestId: 2 }),
  }).state
  state = state.update({ selection: EditorSelection.cursor(0) }).state
  assert.equal(state.field(suggestionField), null)
})

test('a setSuggestion alongside a selection change still wins (fresh request lands)', () => {
  // The fetcher dispatches setSuggestion in a separate transaction
  // from the selection that triggered it, but if a future code path
  // ever batches them we still want the explicit effect to take
  // precedence over the "selection invalidates" rule.
  let state = makeState('hello ', 6)
  state = state.update({
    effects: setSuggestion.of({ pos: 6, text: 'A', requestId: 1 }),
  }).state
  state = state.update({
    selection: EditorSelection.cursor(6),
    effects: setSuggestion.of({ pos: 6, text: 'B', requestId: 2 }),
  }).state
  const sug = state.field(suggestionField)
  assert.equal(sug?.text, 'B')
  assert.equal(sug?.requestId, 2)
})

test('an explicit setSuggestion(null) clears the field', () => {
  let state = makeState('hello ', 6)
  state = state.update({
    effects: setSuggestion.of({ pos: 6, text: 'world', requestId: 9 }),
  }).state
  state = state.update({ effects: setSuggestion.of(null) }).state
  assert.equal(state.field(suggestionField), null)
})
