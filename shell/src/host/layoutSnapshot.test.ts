// BL-067 Phase 0 — unit tests for the layout introspection API.
//
// Run with: node --experimental-strip-types --test \
//   src/host/layoutSnapshot.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { countLeavesInLayout, getLayoutSnapshot, bindPluginRegistry, globalSnapshot } from './layoutSnapshot.ts'
import { slotRegistry } from '../registry/SlotRegistry.ts'
import { viewRegistry } from '../workspace/ViewRegistry.ts'
import type { WorkspaceJSON, SerializedNode } from '../workspace/types.ts'
import type { PluginRegistry } from './PluginRegistry.ts'

function noopView() {
  return { viewType: 'noop', leaf: { id: 'x' } as never, getState: () => ({}), setState: () => {}, onOpen: () => {}, onClose: () => {} }
}

// ── countLeavesInLayout ─────────────────────────────────────────────────────

test('countLeavesInLayout: empty roots → 0', () => {
  const empty: SerializedNode = { kind: 'root', id: 'r', child: { kind: 'tabs', id: 't', leaves: [], activeIndex: 0 } }
  const json: WorkspaceJSON = {
    main: empty,
    left: empty,
    right: empty,
    bottom: empty,
    floating: [],
    active: null,
    lastOpenFiles: [],
  }
  assert.equal(countLeavesInLayout(json), 0)
})

test('countLeavesInLayout: walks tabs + splits + floating', () => {
  const leaf = (id: string): SerializedNode => ({
    kind: 'leaf',
    id,
    viewState: { type: 'empty', state: {} } as never,
  })
  const main: SerializedNode = {
    kind: 'root',
    id: 'r',
    child: {
      kind: 'split',
      id: 's1',
      direction: 'horizontal',
      children: [
        { kind: 'tabs', id: 't1', leaves: [leaf('a'), leaf('b')] as never, activeIndex: 0 },
        { kind: 'tabs', id: 't2', leaves: [leaf('c')] as never, activeIndex: 0 },
      ],
    },
  }
  const empty: SerializedNode = { kind: 'root', id: 'r', child: { kind: 'tabs', id: 't', leaves: [], activeIndex: 0 } }
  const json: WorkspaceJSON = {
    main,
    left: empty,
    right: empty,
    bottom: empty,
    floating: [
      {
        kind: 'floating',
        id: 'f1',
        child: { kind: 'tabs', id: 'tf', leaves: [leaf('d')] as never, activeIndex: 0 },
      },
    ],
    active: null,
    lastOpenFiles: [],
  }
  assert.equal(countLeavesInLayout(json), 4)
})

// ── getLayoutSnapshot ───────────────────────────────────────────────────────

test('getLayoutSnapshot: returns slots, viewTypes, extensions, layout, timestamp', () => {
  const before = Date.now()
  const snap = getLayoutSnapshot()
  const after = Date.now()

  // The built-in `empty` view-type is always registered at module load.
  assert.ok(snap.viewTypes.some((v) => v.type === 'empty'), 'empty view-type present')
  assert.ok(Array.isArray(snap.extensions))
  // Every slot key must be present even if empty so the builder can iterate.
  for (const key of ['overlay', 'titleBar', 'activityBar', 'statusBarLeft', 'statusBarRight', 'paneMode'] as const) {
    assert.ok(Array.isArray(snap.slots[key]), `slot ${key} present`)
  }
  assert.ok(snap.layout, 'layout shape present')
  assert.ok(snap.takenAtMs >= before && snap.takenAtMs <= after, 'timestamp in range')
})

test('getLayoutSnapshot: surfaces newly-registered view-types', () => {
  const dispose = viewRegistry.register('bl067.test.view', noopView as never)
  try {
    const snap = getLayoutSnapshot()
    assert.ok(snap.viewTypes.some((v) => v.type === 'bl067.test.view'))
  } finally {
    dispose()
  }
})

test('getLayoutSnapshot: with mock registry resolves pluginId', () => {
  const dispose = viewRegistry.register('bl067.owned.view', noopView as never)
  const mock = { ownerOfViewType: (t: string) => (t === 'bl067.owned.view' ? 'mock.plugin' : null) } as unknown as PluginRegistry
  try {
    const snap = getLayoutSnapshot(mock)
    const entry = snap.viewTypes.find((v) => v.type === 'bl067.owned.view')
    assert.equal(entry?.pluginId, 'mock.plugin')
    // Built-ins still resolve to null.
    const empty = snap.viewTypes.find((v) => v.type === 'empty')
    assert.equal(empty?.pluginId, null)
  } finally {
    dispose()
  }
})

test('getLayoutSnapshot: surfaces slot entries (id + pluginId + priority, no component)', () => {
  const entry = {
    id: 'bl067.test.entry',
    pluginId: 'bl067.test.plugin',
    component: () => null,
    priority: 42,
  }
  slotRegistry.register('statusBarLeft', entry)
  try {
    const snap = getLayoutSnapshot()
    const found = snap.slots.statusBarLeft.find((e) => e.id === 'bl067.test.entry')
    assert.ok(found, 'entry present in snapshot')
    assert.equal(found.pluginId, 'bl067.test.plugin')
    assert.equal(found.priority, 42)
    // Snapshot must not leak the React component reference.
    assert.equal((found as { component?: unknown }).component, undefined)
  } finally {
    slotRegistry.unregister('bl067.test.entry')
  }
})

// ── globalSnapshot / bindPluginRegistry ─────────────────────────────────────

test('globalSnapshot: uses bound registry for ownership resolution', () => {
  const disposeView = viewRegistry.register('bl067.global.view', noopView as never)
  const mock = { ownerOfViewType: (t: string) => (t === 'bl067.global.view' ? 'bound.plugin' : null) } as unknown as PluginRegistry
  bindPluginRegistry(mock)
  try {
    const snap = globalSnapshot()
    const entry = snap.viewTypes.find((v) => v.type === 'bl067.global.view')
    assert.equal(entry?.pluginId, 'bound.plugin')
  } finally {
    disposeView()
    // Restore module to no-registry state — passing null via cast since
    // setter is the only mutator.
    bindPluginRegistry(null as unknown as PluginRegistry)
  }
})
