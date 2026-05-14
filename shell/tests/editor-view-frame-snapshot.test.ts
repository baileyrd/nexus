/**
 * BL-124 — `useFrameSnapshot` adoption in `EditorView`. Verifies the
 * narrowing behaviour at the FrameSnapshot layer rather than rendering
 * `EditorView` end-to-end through React (which would need to mount
 * CodeMirror, the runtime, etc. — too heavy for a unit test).
 *
 * The contract being pinned:
 *   1. Typing into tab A flushes the FrameSnapshot for tab A's
 *      controller (≤ 1 flush per keystroke, never more).
 *   2. Typing into tab A does NOT flush the FrameSnapshot for tab B's
 *      controller — a separately-mounted `EditorView` leaf bound to
 *      tab B sees zero re-renders from work on tab A.
 *   3. The active-tab object identity is preserved across the
 *      `editorStore.setContent` for other tabs — the underlying
 *      `tabs.map(t => t.relpath === r ? { ...t, content: v } : t)`
 *      already does this; we pin the invariant here so a future
 *      refactor doesn't quietly break the narrowing win.
 */
import { test } from 'node:test'
import assert from 'node:assert/strict'

import { FrameSnapshot, snap, type Scheduler } from '../src/stores/frameSnapshot'
import { useEditorStore } from '../src/plugins/nexus/editor/editorStore'

function resetStore(): void {
  useEditorStore.getState().clear()
}

function makeManualScheduler(): {
  scheduler: Scheduler
  runPending: () => void
  pending: () => boolean
} {
  let queued: (() => void) | null = null
  const scheduler: Scheduler = (cb) => {
    queued = cb
    return () => {
      if (queued === cb) queued = null
    }
  }
  return {
    scheduler,
    runPending: () => {
      const cb = queued
      queued = null
      cb?.()
    },
    pending: () => queued !== null,
  }
}

function leafEntries(relpath: string) {
  // Mirrors the BL-124 selectors in `EditorView`: per-relpath active
  // tab object + total tab count.
  return [
    snap(useEditorStore, (s) =>
      relpath ? s.tabs.find((t) => t.relpath === relpath) ?? null : null,
    ),
    snap(useEditorStore, (s) => s.tabs.length),
  ] as const
}

test('BL-124: typing into one tab flushes only that leaf\'s FrameSnapshot', () => {
  resetStore()
  // Seed two tabs via the store actions so the bridge-free
  // `setContent` path is what we exercise.
  useEditorStore.getState().openTab('notes/a.md', 'a.md')
  useEditorStore.getState().setTabContent('notes/a.md', 'aaa')
  useEditorStore.getState().openTab('notes/b.md', 'b.md')
  useEditorStore.getState().setTabContent('notes/b.md', 'bbb')

  const aSched = makeManualScheduler()
  const bSched = makeManualScheduler()
  const fsA = new FrameSnapshot(leafEntries('notes/a.md'), aSched.scheduler)
  const fsB = new FrameSnapshot(leafEntries('notes/b.md'), bSched.scheduler)
  let aFlushes = 0
  let bFlushes = 0
  const disposeA = fsA.start()
  const disposeB = fsB.start()
  const unsubA = fsA.subscribe(() => {
    aFlushes++
  })
  const unsubB = fsB.subscribe(() => {
    bFlushes++
  })

  try {
    // 10 keystrokes against tab A.
    for (let i = 0; i < 10; i++) {
      useEditorStore.getState().setContent('notes/a.md', `aaa${i}`)
      // Each keystroke schedules a flush on both controllers — the
      // Zustand notification fires every subscriber. Drain pending
      // flushes after each tick so we model rAF granularity.
      aSched.runPending()
      bSched.runPending()
    }

    assert.equal(aFlushes, 10, 'tab A flushes once per keystroke')
    assert.equal(
      bFlushes,
      0,
      'tab B has the same tab-count selector and a different tab object — its tuple is identity-stable, so no flush',
    )
  } finally {
    unsubA()
    unsubB()
    disposeA()
    disposeB()
  }
})

test('BL-124: setContent preserves identity for tabs other than the target', () => {
  resetStore()
  useEditorStore.getState().openTab('notes/a.md', 'a.md')
  useEditorStore.getState().setTabContent('notes/a.md', 'initial-a')
  useEditorStore.getState().openTab('notes/b.md', 'b.md')
  useEditorStore.getState().setTabContent('notes/b.md', 'initial-b')

  const tabB1 = useEditorStore.getState().tabs.find((t) => t.relpath === 'notes/b.md')
  useEditorStore.getState().setContent('notes/a.md', 'mutated-a')
  const tabB2 = useEditorStore.getState().tabs.find((t) => t.relpath === 'notes/b.md')

  assert.ok(tabB1, 'tab B exists before mutation')
  assert.ok(tabB2, 'tab B still exists after mutation')
  assert.equal(
    tabB1,
    tabB2,
    'setContent on tab A must NOT replace tab B\'s object — the per-relpath FrameSnapshot win depends on this',
  )
})

test('BL-124: setMode preserves identity for non-target tabs', () => {
  resetStore()
  useEditorStore.getState().openTab('notes/a.md', 'a.md')
  useEditorStore.getState().setTabContent('notes/a.md', 'aaa')
  useEditorStore.getState().openTab('notes/b.md', 'b.md')
  useEditorStore.getState().setTabContent('notes/b.md', 'bbb')

  const tabB1 = useEditorStore.getState().tabs.find((t) => t.relpath === 'notes/b.md')
  useEditorStore.getState().setMode('notes/a.md', 'source')
  const tabB2 = useEditorStore.getState().tabs.find((t) => t.relpath === 'notes/b.md')

  assert.equal(tabB1, tabB2, 'setMode on tab A must not touch tab B\'s identity')
})

test('BL-124: useFrameSnapshot rebuildKey re-binds when relpath changes for the same leaf', () => {
  // Sanity check on the BL-124 enhancement to `useFrameSnapshot`:
  // passing a new `rebuildKey` causes a fresh FrameSnapshot to be
  // constructed (the inner `useMemo` deps include the key). We can't
  // exercise the React hook directly without happy-dom; instead we
  // pin the FrameSnapshot factory behaviour the hook relies on —
  // distinct constructor calls produce distinct subscribers and
  // observe distinct relpaths.
  resetStore()
  useEditorStore.getState().openTab('notes/a.md', 'a.md')
  useEditorStore.getState().setTabContent('notes/a.md', 'aaa')
  useEditorStore.getState().openTab('notes/b.md', 'b.md')
  useEditorStore.getState().setTabContent('notes/b.md', 'bbb')

  const sched = makeManualScheduler()
  // First mount: bound to A.
  const fs1 = new FrameSnapshot(leafEntries('notes/a.md'), sched.scheduler)
  assert.equal(
    (fs1.current()[0] ?? null)?.content,
    'aaa',
    'fresh FrameSnapshot for "notes/a.md" reads tab A',
  )

  // Rebuild: simulate `useFrameSnapshot` re-running `useMemo` with a
  // new `rebuildKey`. Caller would dispose the old controller; new
  // one comes up against the new entries.
  const fs2 = new FrameSnapshot(leafEntries('notes/b.md'), sched.scheduler)
  assert.equal(
    (fs2.current()[0] ?? null)?.content,
    'bbb',
    'rebuild against "notes/b.md" reads tab B',
  )
})
