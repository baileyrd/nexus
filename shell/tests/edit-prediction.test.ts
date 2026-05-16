// BL-139 — unit tests for the CodeMirror edit-prediction extension.
//
// Mirrors the BL-034 ghost-completion test layout: drive the
// StateField transactions directly so we don't need a real DOM
// EditorView (node:test can't host one). The Tab/Esc keymap is
// exercised via the exported run handlers — those just need a view
// stub with state + dispatch.

import { EditorState, EditorSelection } from '@codemirror/state'
import { __test__ } from '../src/plugins/nexus/editor/cm/editPrediction.ts'

import { test, mock } from 'node:test'
import assert from 'node:assert/strict'

const {
  setPrediction,
  predictionField,
  acceptPrediction,
  dismissPrediction,
  tailByBytes,
  headByBytes,
  Debouncer,
} = __test__

function makeState(doc: string, headPos: number = doc.length): EditorState {
  return EditorState.create({
    doc,
    selection: EditorSelection.cursor(headPos),
    extensions: [predictionField],
  })
}

// ── State machine: render contract ────────────────────────────────────────

test('predictionField starts empty', () => {
  const state = makeState('let x = ')
  assert.equal(state.field(predictionField), null)
})

test('ghost-text render: setPrediction effect populates the field', () => {
  const state = makeState('let x = ')
  const tr = state.update({
    effects: setPrediction.of({ pos: 8, text: '42;', requestId: 1 }),
  })
  const sug = tr.state.field(predictionField)
  assert.ok(sug)
  assert.equal(sug?.text, '42;')
  assert.equal(sug?.requestId, 1)
})

// ── State machine: cancel-on-edit / cancel-on-move ────────────────────────

test('in-flight cancel: a doc change clears the existing prediction', () => {
  let state = makeState('let x = ')
  state = state.update({
    effects: setPrediction.of({ pos: 8, text: '42', requestId: 1 }),
  }).state
  // Type a character — the field's update predicate clears non-effect
  // doc changes. This is the same mechanism that invalidates stale
  // in-flight requests when the caret moves between dispatch and
  // resolution.
  state = state.update({ changes: { from: 8, to: 8, insert: 'X' } }).state
  assert.equal(
    state.field(predictionField),
    null,
    'doc change must clear stale prediction',
  )
})

test('cancel-on-move: a selection move clears the prediction', () => {
  let state = makeState('let x = ')
  state = state.update({
    effects: setPrediction.of({ pos: 8, text: 'tail', requestId: 2 }),
  }).state
  state = state.update({ selection: EditorSelection.cursor(0) }).state
  assert.equal(state.field(predictionField), null)
})

// ── Accept / dismiss (Tab / Escape) ───────────────────────────────────────

interface ViewStub {
  state: EditorState
  dispatch(spec: Parameters<EditorState['update']>[0]): void
}

function makeViewStub(state: EditorState): ViewStub {
  let current = state
  return {
    get state() {
      return current
    },
    dispatch(spec) {
      current = current.update(spec).state
    },
  }
}

test('Tab accept: acceptPrediction inserts text and clears the field', () => {
  const initial = makeState('let x = ', 8)
  const seeded = initial.update({
    effects: setPrediction.of({ pos: 8, text: '42;', requestId: 1 }),
  }).state
  const view = makeViewStub(seeded)
  const ok = acceptPrediction(view as unknown as Parameters<typeof acceptPrediction>[0])
  assert.equal(ok, true)
  assert.equal(view.state.doc.toString(), 'let x = 42;')
  assert.equal(view.state.selection.main.head, 11, 'caret advances past the inserted text')
  assert.equal(view.state.field(predictionField), null, 'field cleared after accept')
})

test('Tab accept: returns false when no prediction is live (lets Tab fall through)', () => {
  const view = makeViewStub(makeState('let x = ', 8))
  const ok = acceptPrediction(view as unknown as Parameters<typeof acceptPrediction>[0])
  assert.equal(ok, false, 'no prediction → Tab must fall through to default')
  assert.equal(view.state.doc.toString(), 'let x = ', 'doc unchanged')
})

test('Tab accept: aborts when caret has moved away from the prediction pos', () => {
  let state = makeState('let x = ', 8)
  state = state.update({
    effects: setPrediction.of({ pos: 8, text: '42;', requestId: 1 }),
  }).state
  // Mover caret — note the setPrediction effect is BEFORE the move,
  // so the field is set. The selection-move invalidates it via the
  // field predicate, but we test the explicit guard too.
  const view = makeViewStub(state)
  // Force-set a prediction back at pos=8 alongside a selection at pos=0
  view.dispatch({
    effects: setPrediction.of({ pos: 8, text: 'late', requestId: 2 }),
    selection: EditorSelection.cursor(0),
  })
  const ok = acceptPrediction(view as unknown as Parameters<typeof acceptPrediction>[0])
  assert.equal(ok, false, 'caret no longer at prediction pos → refuse')
  assert.equal(view.state.doc.toString(), 'let x = ')
})

test('Escape clear: dismissPrediction wipes the field and returns true', () => {
  let state = makeState('let x = ', 8)
  state = state.update({
    effects: setPrediction.of({ pos: 8, text: 'something', requestId: 9 }),
  }).state
  const view = makeViewStub(state)
  const ok = dismissPrediction(view as unknown as Parameters<typeof dismissPrediction>[0])
  assert.equal(ok, true)
  assert.equal(view.state.field(predictionField), null)
})

test('Escape clear: returns false when no prediction (lets Esc bubble)', () => {
  const view = makeViewStub(makeState('hi', 2))
  const ok = dismissPrediction(view as unknown as Parameters<typeof dismissPrediction>[0])
  assert.equal(ok, false)
})

// ── Disabled guard ────────────────────────────────────────────────────────

test('disabled guard: explicit setPrediction(null) clears the field', () => {
  // The fetcher early-returns on `!settings.enabled` so no setPrediction
  // ever fires; this test pins the explicit-null behaviour, which is
  // also what `dismissPrediction` and the `setPrediction.of(null)`
  // post-accept hop produce.
  let state = makeState('hello ', 6)
  state = state.update({
    effects: setPrediction.of({ pos: 6, text: 'world', requestId: 1 }),
  }).state
  state = state.update({ effects: setPrediction.of(null) }).state
  assert.equal(state.field(predictionField), null)
})

// ── Debounce coalescing ───────────────────────────────────────────────────

test('debounce coalescing: rapid schedules collapse into one fire', () => {
  mock.timers.enable({ apis: ['setTimeout'] })
  try {
    const d = new Debouncer()
    let calls = 0
    d.schedule(150, () => calls++)
    mock.timers.tick(50)
    d.schedule(150, () => calls++) // resets the timer
    mock.timers.tick(50)
    d.schedule(150, () => calls++) // resets again
    mock.timers.tick(140)
    assert.equal(calls, 0, 'no fire before the most-recent timer expires')
    mock.timers.tick(20)
    assert.equal(calls, 1, 'exactly one fire after the final delay elapses')
  } finally {
    mock.timers.reset()
  }
})

test('debounce cancel: cancel() drops a pending fire', () => {
  mock.timers.enable({ apis: ['setTimeout'] })
  try {
    const d = new Debouncer()
    let calls = 0
    d.schedule(150, () => calls++)
    mock.timers.tick(100)
    d.cancel()
    mock.timers.tick(500)
    assert.equal(calls, 0, 'cancel must drop the pending callback')
  } finally {
    mock.timers.reset()
  }
})

// ── Byte-budget helpers ────────────────────────────────────────────────────

test('tailByBytes returns last N bytes of an ASCII string', () => {
  assert.equal(tailByBytes('abcdefgh', 3), 'fgh')
  assert.equal(tailByBytes('abc', 10), 'abc')
  assert.equal(tailByBytes('', 5), '')
  assert.equal(tailByBytes('abc', 0), '')
})

test('tailByBytes respects UTF-8 byte budget without splitting codepoints', () => {
  // Each emoji is 4 bytes in UTF-8. Budget 5 → fits one emoji only.
  const out = tailByBytes('a🚀🚀', 5)
  const bytes = new TextEncoder().encode(out).length
  assert.ok(bytes <= 5, `expected ≤5 bytes, got ${bytes}`)
  // Result must still be a valid UTF-8 string (no replacement chars).
  assert.ok(!out.includes('�'))
})

test('headByBytes returns first N bytes', () => {
  assert.equal(headByBytes('abcdefgh', 3), 'abc')
  assert.equal(headByBytes('abc', 10), 'abc')
  assert.equal(headByBytes('', 5), '')
  assert.equal(headByBytes('abc', 0), '')
})

test('headByBytes respects UTF-8 byte budget without splitting codepoints', () => {
  const out = headByBytes('🚀🚀a', 5)
  const bytes = new TextEncoder().encode(out).length
  assert.ok(bytes <= 5, `expected ≤5 bytes, got ${bytes}`)
  assert.ok(!out.includes('�'))
})
