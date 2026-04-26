/**
 * OI-14 — `api.editor.active()` / `api.editor.onChange()` projection helpers.
 *
 * The production implementation in PluginAPI.ts wires these into a real
 * `useEditorStore.subscribe` + `PluginRegistry.trackSubscription` chain.
 * That chain itself is already exercised by `subscription-cleanup.test.ts`
 * (registry sweep) and by the editor-store tests (revision semantics);
 * what's *new* in OI-14 is the two-line projection from the editor
 * store's wider state into the public `{ relpath, revision }` shape and
 * the dedupe predicate that prevents redundant `onChange` callbacks.
 *
 * These tests cover those two pure helpers, plus an integration check
 * that the dedupe predicate actually suppresses no-op store mutations
 * when consumed against the real `useEditorStore`.
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { computeActiveEditor, activeEditorEquals } from './activeEditor'
import { useEditorStore } from '../plugins/nexus/editor/editorStore'

test('computeActiveEditor returns null when no active relpath', () => {
  const out = computeActiveEditor({
    activeRelpath: null,
    sessionRevision: new Map(),
  })
  assert.equal(out, null)
})

test('computeActiveEditor reads revision from sessionRevision map', () => {
  const out = computeActiveEditor({
    activeRelpath: 'notes/foo.md',
    sessionRevision: new Map([['notes/foo.md', 7]]),
  })
  assert.deepEqual(out, { relpath: 'notes/foo.md', revision: 7 })
})

test('computeActiveEditor defaults revision to 0 when not yet tracked', () => {
  const out = computeActiveEditor({
    activeRelpath: 'notes/new.md',
    sessionRevision: new Map(),
  })
  assert.deepEqual(out, { relpath: 'notes/new.md', revision: 0 })
})

test('activeEditorEquals: both null', () => {
  assert.equal(activeEditorEquals(null, null), true)
})

test('activeEditorEquals: one null', () => {
  assert.equal(activeEditorEquals(null, { relpath: 'a.md', revision: 1 }), false)
  assert.equal(activeEditorEquals({ relpath: 'a.md', revision: 1 }, null), false)
})

test('activeEditorEquals: same relpath and revision', () => {
  assert.equal(
    activeEditorEquals(
      { relpath: 'a.md', revision: 3 },
      { relpath: 'a.md', revision: 3 },
    ),
    true,
  )
})

test('activeEditorEquals: revision advance', () => {
  assert.equal(
    activeEditorEquals(
      { relpath: 'a.md', revision: 3 },
      { relpath: 'a.md', revision: 4 },
    ),
    false,
  )
})

test('activeEditorEquals: relpath change', () => {
  assert.equal(
    activeEditorEquals(
      { relpath: 'a.md', revision: 1 },
      { relpath: 'b.md', revision: 1 },
    ),
    false,
  )
})

test('integration: dedupe over useEditorStore mutations only fires on real changes', () => {
  // Reset the store to a known starting point so this test is hermetic.
  useEditorStore.setState({
    tabs: [],
    activeRelpath: null,
    sessionRevision: new Map<string, number>(),
    savedRevision: new Map<string, number>(),
  })

  const seen: Array<ReturnType<typeof computeActiveEditor>> = []
  let last = computeActiveEditor(useEditorStore.getState())
  const unsub = useEditorStore.subscribe((state) => {
    const next = computeActiveEditor(state)
    if (activeEditorEquals(next, last)) return
    last = next
    seen.push(next)
  })

  // Activate a tab — should fire.
  useEditorStore.setState({ activeRelpath: 'notes/a.md' })
  // Bump an unrelated savedRevision — must NOT fire.
  useEditorStore.setState((s) => {
    const next = new Map(s.savedRevision)
    next.set('notes/a.md', 1)
    return { savedRevision: next }
  })
  // Advance sessionRevision for the active file — should fire.
  useEditorStore.setState((s) => {
    const next = new Map(s.sessionRevision)
    next.set('notes/a.md', 2)
    return { sessionRevision: next }
  })
  // Switch to a different active file — should fire.
  useEditorStore.setState({ activeRelpath: 'notes/b.md' })
  // Close all tabs — should fire.
  useEditorStore.setState({ activeRelpath: null })

  unsub()

  assert.deepEqual(seen, [
    { relpath: 'notes/a.md', revision: 0 },
    { relpath: 'notes/a.md', revision: 2 },
    { relpath: 'notes/b.md', revision: 0 },
    null,
  ])
})

test('integration: idempotent disposer pattern', () => {
  let disposed = 0
  const inner = () => {
    disposed++
  }
  let alreadyDisposed = false
  const unsub = () => {
    if (alreadyDisposed) return
    alreadyDisposed = true
    inner()
  }
  unsub()
  unsub()
  unsub()
  assert.equal(disposed, 1, 'inner disposer must be called exactly once')
})
