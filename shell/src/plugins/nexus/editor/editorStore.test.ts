// Unit tests for the Phase 6 revision-based `isDirty` semantics.
//
// The pre-Phase-6 editor computed dirtiness from a content diff
// (`content !== savedContent`). Now that the kernel owns the block
// tree, dirtiness is driven by the kernel revision: a local edit
// advances `sessionRevision[relpath]`, a successful save snapshots
// the current `sessionRevision` into `savedRevision[relpath]`, and
// `isDirty` compares the two. Untitled tabs (no kernel session) keep
// the legacy content-diff behaviour.
//
// Run via the shell test runner: `pnpm --filter nexus-shell test`
// (picked up through the `tests/editor-store.test.ts` re-export shim).
// Static imports — top-level await + dynamic import doesn't survive
// the tsx CJS transform that the test runner uses.

import type { TransactionId } from './types.ts'
import { isDirty, useEditorStore, type EditorTab } from './editorStore.ts'

import { test } from 'node:test'
import assert from 'node:assert/strict'

function resetStore(): void {
  useEditorStore.setState({
    tabs: [],
    activeRelpath: null,
    sessionRevision: new Map<string, number>(),
    savedRevision: new Map<string, number>(),
    pendingLocalRevisions: new Set<TransactionId>(),
  })
}

function tabFor(relpath: string): EditorTab {
  const t = useEditorStore.getState().tabs.find((x) => x.relpath === relpath)
  if (!t) throw new Error(`no tab for ${relpath}`)
  return t
}

// ── Revision-based dirty tracking (the new contract) ────────────────────────

test('isDirty: freshly-seeded session tab is clean', () => {
  resetStore()
  const s = useEditorStore.getState()
  s.openTab('notes/a.md', 'a.md')
  s.setTabContent('notes/a.md', 'hello')
  // Mimic sessionManager.acquire: seed sessionRevision then snapshot
  // savedRevision from it.
  s.setSessionRevision('notes/a.md', 0)
  s.markSavedRevision('notes/a.md')

  assert.equal(isDirty(tabFor('notes/a.md')), false)
})

test('isDirty: local transaction advances sessionRevision → dirty', () => {
  resetStore()
  const s = useEditorStore.getState()
  s.openTab('notes/a.md', 'a.md')
  s.setTabContent('notes/a.md', 'hello')
  s.setSessionRevision('notes/a.md', 0)
  s.markSavedRevision('notes/a.md')

  // The transaction bridge calls setSessionRevision with the post-
  // apply revision. That's the only thing that flips the tab dirty
  // under the new contract.
  useEditorStore.getState().setSessionRevision('notes/a.md', 1)

  assert.equal(isDirty(tabFor('notes/a.md')), true)
})

test('isDirty: save → markSaved snaps savedRevision = sessionRevision, tab clean', () => {
  resetStore()
  const s = useEditorStore.getState()
  s.openTab('notes/a.md', 'a.md')
  s.setTabContent('notes/a.md', 'hello')
  s.setSessionRevision('notes/a.md', 0)
  s.markSavedRevision('notes/a.md')

  // Local edit (bridge):
  useEditorStore.getState().setSessionRevision('notes/a.md', 3)
  assert.equal(isDirty(tabFor('notes/a.md')), true)

  // Successful save:
  useEditorStore.getState().markSaved('notes/a.md')
  assert.equal(
    useEditorStore.getState().savedRevision.get('notes/a.md'),
    3,
    'markSaved snapshots the current sessionRevision',
  )
  assert.equal(isDirty(tabFor('notes/a.md')), false)

  // Further edit redirties:
  useEditorStore.getState().setSessionRevision('notes/a.md', 4)
  assert.equal(isDirty(tabFor('notes/a.md')), true)
})

// ── Invariant-violation safety net ─────────────────────────────────────────

test('isDirty: sessionRevision present without savedRevision warns + treats as clean', () => {
  resetStore()
  const s = useEditorStore.getState()
  s.openTab('notes/a.md', 'a.md')
  // Deliberately skip markSavedRevision() to simulate a missed acquire
  // seed (the latent invariant violation flagged in the editor phase
  // audit). isDirty should fall back to "clean" rather than assume
  // dirty (which would false-flag every newly-opened tab) — but it
  // must console.warn so the broken invariant surfaces in dev.
  s.setSessionRevision('notes/a.md', 0)

  const warnings: string[] = []
  const originalWarn = console.warn
  console.warn = (msg: unknown) => {
    if (typeof msg === 'string') warnings.push(msg)
  }
  try {
    assert.equal(isDirty(tabFor('notes/a.md')), false, 'fallback is clean, not dirty')
  } finally {
    console.warn = originalWarn
  }

  assert.equal(warnings.length, 1, 'exactly one warning emitted')
  assert.match(warnings[0]!, /invariant violation/, 'warning identifies the broken invariant')
  assert.match(warnings[0]!, /notes\/a\.md/, 'warning includes the offending relpath')
})

// ── Untitled tabs keep the content-diff fallback ────────────────────────────

test('isDirty: untitled tab with no session uses content-vs-savedContent fallback', () => {
  resetStore()
  const s = useEditorStore.getState()
  s.openUntitled('untitled-1', 'untitled-1')
  // No sessionRevision entry for untitled tabs.
  assert.equal(isDirty(tabFor('untitled-1')), false)

  s.setContent('untitled-1', 'draft')
  assert.equal(isDirty(tabFor('untitled-1')), true)

  s.markSaved('untitled-1')
  assert.equal(isDirty(tabFor('untitled-1')), false)
})

// ── Close / clear tidy up the revision maps ─────────────────────────────────

test('closeTab drops the saved-revision entry so a reopen starts fresh', () => {
  resetStore()
  const s = useEditorStore.getState()
  s.openTab('notes/a.md', 'a.md')
  s.setSessionRevision('notes/a.md', 5)
  s.markSavedRevision('notes/a.md')

  useEditorStore.getState().closeTab('notes/a.md')
  assert.equal(
    useEditorStore.getState().savedRevision.has('notes/a.md'),
    false,
    'savedRevision entry purged on close',
  )
})

test('renameTab moves revision entries from old relpath to new', () => {
  resetStore()
  const s = useEditorStore.getState()
  s.openUntitled('untitled-1', 'untitled-1')
  s.setContent('untitled-1', 'seed')
  // After the untitled → named transition we get a session, so pretend
  // sessionManager.acquire populated the maps under the new relpath.
  s.setSessionRevision('untitled-1', 0)
  s.markSavedRevision('untitled-1')

  useEditorStore.getState().renameTab('untitled-1', 'notes/b.md', 'b.md')
  const st = useEditorStore.getState()
  assert.equal(st.tabs[0]?.relpath, 'notes/b.md')
  assert.equal(st.tabs[0]?.name, 'b.md')
  assert.equal(st.sessionRevision.has('untitled-1'), false)
  assert.equal(st.sessionRevision.get('notes/b.md'), 0)
  assert.equal(st.savedRevision.get('notes/b.md'), 0)
  assert.equal(st.activeRelpath, 'notes/b.md')
})

// ── clear() resets everything (used on workspace:closed) ────────────────────

test('clear() wipes all tabs and both revision maps', () => {
  resetStore()
  const s = useEditorStore.getState()
  s.openTab('notes/a.md', 'a.md')
  s.setSessionRevision('notes/a.md', 2)
  s.markSavedRevision('notes/a.md')

  useEditorStore.getState().clear()
  const st = useEditorStore.getState()
  assert.equal(st.tabs.length, 0)
  assert.equal(st.activeRelpath, null)
  assert.equal(st.sessionRevision.size, 0)
  assert.equal(st.savedRevision.size, 0)
})

// ── #405: per-tab cursor / scroll / mode restore ────────────────────────────

test('openTab with no restore arg defaults to live mode, no position', () => {
  resetStore()
  useEditorStore.getState().openTab('notes/a.md', 'a.md')
  const tab = tabFor('notes/a.md')
  assert.equal(tab.mode, 'live')
  assert.equal(tab.cursorOffset, undefined)
  assert.equal(tab.scrollTop, undefined)
})

test('openTab seeds mode/cursorOffset/scrollTop from a restore object', () => {
  resetStore()
  useEditorStore
    .getState()
    .openTab('notes/a.md', 'a.md', { mode: 'source', cursorOffset: 42, scrollTop: 120 })
  const tab = tabFor('notes/a.md')
  assert.equal(tab.mode, 'source')
  assert.equal(tab.cursorOffset, 42)
  assert.equal(tab.scrollTop, 120)
})

test('openTab on an already-open tab ignores the restore arg (no refetch/reset)', () => {
  resetStore()
  const s = useEditorStore.getState()
  s.openTab('notes/a.md', 'a.md')
  s.setMode('notes/a.md', 'preview')
  s.setViewPosition('notes/a.md', 7, 30)

  const isNew = useEditorStore
    .getState()
    .openTab('notes/a.md', 'a.md', { mode: 'source', cursorOffset: 0, scrollTop: 0 })

  assert.equal(isNew, false)
  const tab = tabFor('notes/a.md')
  assert.equal(tab.mode, 'preview')
  assert.equal(tab.cursorOffset, 7)
  assert.equal(tab.scrollTop, 30)
})

test('setViewPosition updates cursorOffset/scrollTop for the matching tab only', () => {
  resetStore()
  const s = useEditorStore.getState()
  s.openTab('notes/a.md', 'a.md')
  s.openTab('notes/b.md', 'b.md')

  useEditorStore.getState().setViewPosition('notes/a.md', 15, 200)

  assert.equal(tabFor('notes/a.md').cursorOffset, 15)
  assert.equal(tabFor('notes/a.md').scrollTop, 200)
  assert.equal(tabFor('notes/b.md').cursorOffset, undefined)
  assert.equal(tabFor('notes/b.md').scrollTop, undefined)
})

test('setViewPosition on an unknown relpath is a silent no-op', () => {
  resetStore()
  useEditorStore.getState().openTab('notes/a.md', 'a.md')
  useEditorStore.getState().setViewPosition('notes/missing.md', 1, 1)
  assert.equal(useEditorStore.getState().tabs.length, 1)
})
