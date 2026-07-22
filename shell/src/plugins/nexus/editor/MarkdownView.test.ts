// #405 — regression coverage for MarkdownView's getState()/setState()
// round trip: mode / cursorOffset / scrollTop must survive a
// serialize → hydrate cycle through `.forge/workspace.json` so a
// restored note reopens at its last mode, cursor, and scroll position
// instead of always resetting to `live` mode at the top of the file.
//
// Run via the shell test runner: `pnpm --filter nexus-shell test`
// (picked up through the `tests/markdown-view.test.ts` re-export shim).

import { test } from 'node:test'
import assert from 'node:assert/strict'

import type { ReactElement } from 'react'
import { MarkdownView } from './MarkdownView.tsx'
import { useEditorStore } from './editorStore.ts'
import type { Leaf } from '../../../workspace'
import type { TransactionId } from './types.ts'

type RenderFn = (relpath: string | undefined, leafId: string) => ReactElement

function resetStore(): void {
  useEditorStore.setState({
    tabs: [],
    activeRelpath: null,
    sessionRevision: new Map<string, number>(),
    savedRevision: new Map<string, number>(),
    pendingLocalRevisions: new Set<TransactionId>(),
  })
}

// getState()/setState() never touch DOM or the render fn — a minimal
// leaf stub with just an `id` is enough (matches how `onOpen`/`onClose`,
// which do need a real leaf, are simply never called in these tests).
function makeView(): MarkdownView {
  const leaf = { id: 'leaf-1' } as Leaf
  const render: RenderFn = () => null as unknown as ReactElement
  return new MarkdownView(leaf, render, null)
}

test('setState/getState round-trips a bare relpath (no tab in the store yet)', () => {
  resetStore()
  const view = makeView()
  view.setState({ relpath: 'notes/a.md' })
  // mode/cursorOffset/scrollTop are present-but-undefined rather than
  // omitted (same shape setState always builds) — harmless once this
  // round-trips through JSON.stringify (workspace.json persistence),
  // which drops undefined-valued keys, but deepEqual here is exact.
  assert.deepEqual(view.getState(), {
    relpath: 'notes/a.md',
    mode: undefined,
    cursorOffset: undefined,
    scrollTop: undefined,
  })
})

test('setState parses persisted mode/cursorOffset/scrollTop before any tab exists', () => {
  resetStore()
  const view = makeView()
  view.setState({ relpath: 'notes/a.md', mode: 'source', cursorOffset: 42, scrollTop: 120 })
  // No tab in the editor store for this relpath yet — getState() falls
  // back to what setState just parsed, so a save mid-restore (e.g. the
  // active-leaf-change seed racing serialize()) doesn't regress to a
  // blank ephemeral state.
  assert.deepEqual(view.getState(), {
    relpath: 'notes/a.md',
    mode: 'source',
    cursorOffset: 42,
    scrollTop: 120,
  })
})

test('setState drops a malformed mode / non-numeric position fields', () => {
  resetStore()
  const view = makeView()
  view.setState({ relpath: 'notes/a.md', mode: 'not-a-mode', cursorOffset: 'nope', scrollTop: null })
  assert.deepEqual(view.getState(), { relpath: 'notes/a.md', mode: undefined, cursorOffset: undefined, scrollTop: undefined })
})

test('setState with a non-object / missing relpath resets to empty state', () => {
  resetStore()
  const view = makeView()
  view.setState({ relpath: 'notes/a.md' })
  view.setState('garbage')
  assert.deepEqual(view.getState(), {})
})

test('getState prefers the live editor-store tab once one exists', () => {
  resetStore()
  const view = makeView()
  view.setState({ relpath: 'notes/a.md', mode: 'source', cursorOffset: 1, scrollTop: 1 })

  // Simulate the tab being created (loadFile → openTab) and then
  // CodeMirrorHost's position listener advancing the live position —
  // getState() must reflect the *live* store values, not the stale
  // ones setState() cached before the tab existed.
  useEditorStore.getState().openTab('notes/a.md', 'a.md', { mode: 'source', cursorOffset: 1, scrollTop: 1 })
  useEditorStore.getState().setMode('notes/a.md', 'preview')
  useEditorStore.getState().setViewPosition('notes/a.md', 99, 500)

  assert.deepEqual(view.getState(), {
    relpath: 'notes/a.md',
    mode: 'preview',
    cursorOffset: 99,
    scrollTop: 500,
  })
})

test('getState with no relpath set returns the raw (empty) state', () => {
  resetStore()
  const view = makeView()
  assert.deepEqual(view.getState(), {})
})
