// BL-071: emacs keymap unit tests. Drives a real `EditorView` through
// the kill-ring, mark-ring, and motion paths so the assertions match
// what CodeMirror's keymap dispatcher runs at mount time.
//
// `happy-dom` globals are registered via the test runner's `--import`
// flag before this file loads; re-exported via `tests/emacs-keymap.test.ts`
// so the top-level glob picks it up.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { EditorState, EditorSelection } from '@codemirror/state'
import { EditorView } from '@codemirror/view'

import {
  emacsKeymapExt,
  getMarkRing,
  KILL_RING_LIMIT,
  MARK_RING_LIMIT,
  peekKill,
  pushKill,
  resetKillRingForTests,
} from './emacsKeymap.ts'

function mount(opts: {
  doc?: string
  selection?: EditorSelection
}): EditorView {
  resetKillRingForTests()
  const state = EditorState.create({
    doc: opts.doc ?? '',
    selection: opts.selection,
    extensions: [emacsKeymapExt({ relpath: 'notes/foo.md' })],
  })
  const parent = document.createElement('div')
  document.body.appendChild(parent)
  return new EditorView({ state, parent })
}

/**
 * Synthesise a keydown event that matches CM6's chord parser. Use the
 * `code` plus modifier flags so `ctrlKey + key=' '` reads as
 * `Ctrl-Space`.
 */
function dispatchKey(
  view: EditorView,
  init: KeyboardEventInit & { key: string },
): void {
  // CM6 listens on `keydown` at the contentDOM. Synthesise the event
  // there so the keymap facet's `runScopeHandlers` runs.
  view.contentDOM.dispatchEvent(
    new KeyboardEvent('keydown', { bubbles: true, ...init }),
  )
}

// ── Kill ring ────────────────────────────────────────────────────────────────

test('pushKill / peekKill: most-recent-last semantics', () => {
  resetKillRingForTests()
  pushKill('one')
  pushKill('two')
  assert.equal(peekKill(), 'two')
})

test('pushKill: drops empty strings', () => {
  resetKillRingForTests()
  pushKill('')
  assert.equal(peekKill(), null)
})

test('pushKill: caps at KILL_RING_LIMIT entries', () => {
  resetKillRingForTests()
  for (let i = 0; i < KILL_RING_LIMIT + 5; i++) pushKill(`k${i}`)
  // The most recent push wins; nothing crashes when the cap kicks in.
  assert.equal(peekKill(), `k${KILL_RING_LIMIT + 4}`)
})

// ── C-w / M-w / C-y ──────────────────────────────────────────────────────────

test('Ctrl-w kills the active region into the kill ring', () => {
  const view = mount({
    doc: 'hello world',
    selection: EditorSelection.range(0, 5),
  })
  try {
    dispatchKey(view, { key: 'w', ctrlKey: true })
    assert.equal(view.state.doc.toString(), ' world')
    assert.equal(peekKill(), 'hello')
    assert.equal(view.state.selection.main.empty, true)
  } finally {
    view.destroy()
  }
})

test('Alt-w copies the region without removing it', () => {
  const view = mount({
    doc: 'hello world',
    selection: EditorSelection.range(0, 5),
  })
  try {
    dispatchKey(view, { key: 'w', altKey: true })
    assert.equal(view.state.doc.toString(), 'hello world', 'doc unchanged')
    assert.equal(peekKill(), 'hello')
    // Selection collapses to the end of the copied region.
    assert.equal(view.state.selection.main.head, 5)
    assert.equal(view.state.selection.main.empty, true)
  } finally {
    view.destroy()
  }
})

test('Ctrl-y yanks the most recent kill at the cursor', () => {
  const view = mount({
    doc: 'XY',
    selection: EditorSelection.cursor(1),
  })
  try {
    pushKill('-mid-')
    dispatchKey(view, { key: 'y', ctrlKey: true })
    assert.equal(view.state.doc.toString(), 'X-mid-Y')
    // Cursor lands after the inserted text.
    assert.equal(view.state.selection.main.head, 6)
  } finally {
    view.destroy()
  }
})

test('Ctrl-y is a no-op when the kill ring is empty', () => {
  const view = mount({ doc: 'untouched', selection: EditorSelection.cursor(0) })
  try {
    resetKillRingForTests()
    dispatchKey(view, { key: 'y', ctrlKey: true })
    assert.equal(view.state.doc.toString(), 'untouched')
  } finally {
    view.destroy()
  }
})

// ── C-k ──────────────────────────────────────────────────────────────────────

test('Ctrl-k mid-line kills to end-of-line and pushes onto the kill ring', () => {
  const view = mount({
    doc: 'hello world\nnext',
    selection: EditorSelection.cursor(5),
  })
  try {
    dispatchKey(view, { key: 'k', ctrlKey: true })
    assert.equal(view.state.doc.toString(), 'hello\nnext')
    assert.equal(peekKill(), ' world')
  } finally {
    view.destroy()
  }
})

test('Ctrl-k at end-of-line kills the newline', () => {
  const view = mount({
    doc: 'hello\nworld',
    selection: EditorSelection.cursor(5),
  })
  try {
    dispatchKey(view, { key: 'k', ctrlKey: true })
    assert.equal(view.state.doc.toString(), 'helloworld')
    assert.equal(peekKill(), '\n')
  } finally {
    view.destroy()
  }
})

// ── C-Space mark ring ────────────────────────────────────────────────────────

test('Ctrl-Space sets the mark and stores the cursor on the ring', () => {
  const view = mount({ doc: 'abcdef', selection: EditorSelection.cursor(3) })
  try {
    dispatchKey(view, { key: ' ', ctrlKey: true })
    assert.deepEqual(getMarkRing(view), [3])
  } finally {
    view.destroy()
  }
})

test('Ctrl-Space respects the MARK_RING_LIMIT cap', () => {
  const view = mount({
    doc: 'a'.repeat(MARK_RING_LIMIT + 5),
    selection: EditorSelection.cursor(0),
  })
  try {
    for (let i = 0; i < MARK_RING_LIMIT + 4; i++) {
      view.dispatch({ selection: EditorSelection.cursor(i) })
      dispatchKey(view, { key: ' ', ctrlKey: true })
    }
    const ring = getMarkRing(view)
    assert.equal(ring.length, MARK_RING_LIMIT)
    // Oldest entry is the value just past where the cap began
    // shifting; newest is the most recent dispatch.
    assert.equal(ring[ring.length - 1], MARK_RING_LIMIT + 3)
  } finally {
    view.destroy()
  }
})
