// Unit tests for workspaceStore (Phase 3).
// Run with: node --experimental-strip-types --test src/workspace/workspaceStore.test.ts
//
// Static node:test imports (matching ViewRegistry.test.ts / Leaf.test.ts).
// The top-level `await import(...)` indirection this file used to carry
// is rejected by esbuild's CJS transform, which kept the wrapper at
// tests/workspace-workspaceStore.test.ts from ever loading it.

import type {
  Leaf,
  Tabs,
  View,
  WorkspaceJSON,
} from './types.ts'
import { viewRegistry } from './ViewRegistry.ts'
import { ViewBase } from './View.ts'
import { workspace } from './workspaceStore.ts'

import { test } from 'node:test'
import assert from 'node:assert/strict'

// --- helpers -------------------------------------------------------------

function freshLayout(): void {
  workspace.resetToDefault()
}

function firstTabs(node: import('./types.ts').WorkspaceParent): Tabs {
  if (node.kind === 'tabs') return node
  if (node.kind === 'split') return firstTabs(node.children[0]!)
  const withChild = node as { child?: import('./types.ts').WorkspaceParent }
  if (withChild.child) return firstTabs(withChild.child)
  throw new Error('no tabs found')
}

class FooView extends ViewBase {
  readonly viewType = 'foo'
  private _s: unknown = {}
  override getState(): unknown {
    return this._s
  }
  override setState(s: unknown): void {
    this._s = s
  }
}

// A view whose setState awaits a microtask, so we can assert ordering of
// layout-ready vs. setViewState completion during hydrate.
class SlowView extends ViewBase {
  readonly viewType = 'slow'
  private _s: unknown = {}
  static resolvedCount = 0
  override getState(): unknown {
    return this._s
  }
  override async setState(s: unknown): Promise<void> {
    await Promise.resolve() // hop a microtask
    this._s = s
    SlowView.resolvedCount++
  }
}

// Register fake view creators once at module load; disposers aren't needed
// because each test uses a fresh default layout.
viewRegistry.register('foo', (l: Leaf): View => new FooView(l))
viewRegistry.register('slow', (l: Leaf): View => new SlowView(l))

// --- tests ---------------------------------------------------------------

test('hydrate round-trip preserves ids and tree shape', async () => {
  const fixture: WorkspaceJSON = {
    main: {
      kind: 'split',
      id: 'main-split',
      direction: 'horizontal',
      children: [
        {
          kind: 'tabs',
          id: 'main-tabs',
          activeIndex: 0,
          leaves: [
            { kind: 'leaf', id: 'leaf-main-1', viewState: { type: 'empty', state: {} } },
          ],
        },
      ],
    },
    left: {
      kind: 'split',
      id: 'left-dock',
      direction: 'vertical',
      side: 'left',
      collapsed: false,
      size: 300,
      children: [
        {
          kind: 'tabs',
          id: 'left-tabs',
          activeIndex: 0,
          leaves: [
            { kind: 'leaf', id: 'leaf-left-1', viewState: { type: 'empty', state: {} } },
          ],
        },
      ],
    },
    right: {
      kind: 'split',
      id: 'right-dock',
      direction: 'vertical',
      side: 'right',
      collapsed: false,
      size: 300,
      children: [
        {
          kind: 'tabs',
          id: 'right-tabs',
          activeIndex: 0,
          leaves: [
            { kind: 'leaf', id: 'leaf-right-1', viewState: { type: 'empty', state: {} } },
          ],
        },
      ],
    },
    bottom: {
      kind: 'split',
      id: 'bottom-dock',
      direction: 'horizontal',
      side: 'bottom',
      collapsed: true,
      size: 240,
      children: [
        {
          kind: 'tabs',
          id: 'bottom-tabs',
          activeIndex: 0,
          leaves: [
            { kind: 'leaf', id: 'leaf-bottom-1', viewState: { type: 'empty', state: {} } },
          ],
        },
      ],
    },
    active: 'leaf-main-1',
    lastOpenFiles: [],
  }

  await workspace.hydrate(fixture)
  const out = workspace.serialize()
  assert.deepEqual(out, fixture, 'serialize(hydrate(json)) must round-trip')
})

test('ensureLeafOfType does not move an existing leaf', async () => {
  freshLayout()
  // Create a leaf of type 'foo' in the rootSplit's first Tabs.
  const rootTabs = firstTabs(workspace.rootSplit)
  const leaf = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(leaf)
  await leaf.setViewState({ type: 'foo' })

  const result = await workspace.ensureLeafOfType('foo', 'left')
  assert.equal(result.id, leaf.id, 'returned leaf must be the existing one')
  assert.equal(result.parent, rootTabs, 'parent must still be rootTabs (no move)')
})

test('ensureLeafOfType creates in the named side if none exists', async () => {
  freshLayout()
  const leaf = await workspace.ensureLeafOfType('foo', 'right')
  const rightTabs = firstTabs(workspace.rightSplit)
  assert.ok(
    rightTabs.leaves.some(l => l.id === leaf.id),
    'new leaf must live in rightSplit first Tabs',
  )
  assert.equal(leaf.view?.viewType, 'foo')
})

test('ensureLeafOfType("terminal", "bottom") creates a leaf inside bottomSplit', async () => {
  freshLayout()
  const leaf = await workspace.ensureLeafOfType('foo', 'bottom')
  const bottomTabs = firstTabs(workspace.bottomSplit)
  assert.ok(
    bottomTabs.leaves.some(l => l.id === leaf.id),
    'new leaf must live in bottomSplit first Tabs',
  )
  assert.equal(leaf.view?.viewType, 'foo')
  assert.equal(workspace.bottomSplit.side, 'bottom')
})

test('hydrate without a `bottom` field creates a default collapsed bottom split', async () => {
  // Simulate a workspace.json written before the bottom drawer landed:
  // the persisted shape has no `bottom` key. Hydrate must tolerate it.
  const legacy: WorkspaceJSON = {
    main: {
      kind: 'split',
      id: 'main-split',
      direction: 'horizontal',
      children: [
        {
          kind: 'tabs',
          id: 'main-tabs',
          activeIndex: 0,
          leaves: [
            { kind: 'leaf', id: 'leaf-main-1', viewState: { type: 'empty', state: {} } },
          ],
        },
      ],
    },
    left: {
      kind: 'split',
      id: 'left-dock',
      direction: 'vertical',
      side: 'left',
      collapsed: false,
      size: 300,
      children: [
        {
          kind: 'tabs',
          id: 'left-tabs',
          activeIndex: 0,
          leaves: [
            { kind: 'leaf', id: 'leaf-left-1', viewState: { type: 'empty', state: {} } },
          ],
        },
      ],
    },
    right: {
      kind: 'split',
      id: 'right-dock',
      direction: 'vertical',
      side: 'right',
      collapsed: false,
      size: 300,
      children: [
        {
          kind: 'tabs',
          id: 'right-tabs',
          activeIndex: 0,
          leaves: [
            { kind: 'leaf', id: 'leaf-right-1', viewState: { type: 'empty', state: {} } },
          ],
        },
      ],
    },
    active: null,
    lastOpenFiles: [],
  }

  await workspace.hydrate(legacy)
  assert.ok(workspace.bottomSplit, 'bottomSplit must be present after hydrate')
  assert.equal(workspace.bottomSplit.side, 'bottom')
  assert.equal(workspace.bottomSplit.collapsed, true, 'default bottom is collapsed')
  assert.equal(workspace.bottomSplit.size, 240)
})

test('revealLeaf expands collapsed dock + sets activeIndex + activeLeafId', async () => {
  freshLayout()
  // Collapse the left dock and seed two leaves in left Tabs.
  workspace.leftSplit.collapsed = true
  const leftTabs = firstTabs(workspace.leftSplit)
  // leftTabs already has one seed leaf from resetToDefault; add a second.
  const second = workspace.createLeaf(leftTabs)
  leftTabs.leaves.push(second)
  leftTabs.activeIndex = 0

  workspace.revealLeaf(second)
  assert.equal(workspace.leftSplit.collapsed, false, 'dock must be expanded')
  assert.equal(leftTabs.activeIndex, 1, 'activeIndex must point at revealed leaf')
  assert.equal(workspace.activeLeafId, second.id, 'activeLeafId must match')
})

test('detachLeaf removes from parent, map, and clears activeLeafId', async () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const seedLeaf = rootTabs.leaves[0]!
  workspace.setActiveLeaf(seedLeaf)
  assert.equal(workspace.activeLeafId, seedLeaf.id)

  await workspace.detachLeaf(seedLeaf)
  assert.equal(rootTabs.leaves.includes(seedLeaf), false, 'removed from parent Tabs')
  assert.equal(workspace.leaves.has(seedLeaf.id), false, 'removed from leaves map')
  assert.equal(workspace.activeLeafId, null, 'activeLeafId cleared')
  assert.equal(seedLeaf.view, null, 'leaf.detach() ran (view is null)')
})

test('active-leaf-change bridging: leaf-emitted event updates activeLeafId', async () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leaf = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(leaf)

  const observed: string[] = []
  const off = workspace.on('active-leaf-change', (payload) => {
    const p = payload as { leaf: Leaf }
    observed.push(p.leaf.id)
  })
  try {
    // A setViewState with active:true causes Leaf.ts to emit active-leaf-change.
    // The store's bridge should set activeLeafId and fan out exactly once.
    await leaf.setViewState({ type: 'foo', active: true })
    assert.equal(workspace.activeLeafId, leaf.id, 'bridge set activeLeafId')
    assert.equal(observed.length, 1, 'subscribers see event exactly once')
    assert.equal(observed[0], leaf.id)
  } finally {
    off()
  }
})

test('layout-ready fires AFTER all setViewState calls complete during hydrate', async () => {
  SlowView.resolvedCount = 0
  let resolvedAtLayoutReady = -1
  const off = workspace.on('layout-ready', () => {
    resolvedAtLayoutReady = SlowView.resolvedCount
  })

  const fixture: WorkspaceJSON = {
    main: {
      kind: 'split',
      id: 'ms',
      direction: 'horizontal',
      children: [
        {
          kind: 'tabs',
          id: 'mt',
          activeIndex: 0,
          leaves: [
            { kind: 'leaf', id: 'l-slow-1', viewState: { type: 'slow', state: { n: 1 } } },
            { kind: 'leaf', id: 'l-slow-2', viewState: { type: 'slow', state: { n: 2 } } },
          ],
        },
      ],
    },
    left: {
      kind: 'split', id: 'ld', direction: 'vertical', side: 'left', collapsed: false, size: 300,
      children: [
        { kind: 'tabs', id: 'lt', activeIndex: 0, leaves: [
          { kind: 'leaf', id: 'l-slow-3', viewState: { type: 'slow', state: { n: 3 } } },
        ] },
      ],
    },
    right: {
      kind: 'split', id: 'rd', direction: 'vertical', side: 'right', collapsed: false, size: 300,
      children: [
        { kind: 'tabs', id: 'rt', activeIndex: 0, leaves: [] },
      ],
    },
    active: null,
    lastOpenFiles: [],
  }

  try {
    await workspace.hydrate(fixture)
    assert.equal(
      resolvedAtLayoutReady,
      3,
      'layout-ready must fire only after all 3 slow setState promises have resolved',
    )
    assert.equal(SlowView.resolvedCount, 3)
  } finally {
    off()
  }
})

// --- BL-029 popout / floating window tests -------------------------------

test('popoutLeaf moves leaf out of its parent Tabs into a fresh FloatingWindow', async () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leaf = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(leaf)
  await leaf.setViewState({ type: 'foo' })

  const before = rootTabs.leaves.length
  const fwId = workspace.popoutLeaf(leaf.id)

  assert.equal(rootTabs.leaves.length, before - 1, 'leaf removed from parent tabs')
  assert.equal(workspace.floating.length, 1, 'one floating window appended')
  const fw = workspace.floating[0]!
  assert.equal(fw.id, fwId)
  assert.equal(fw.kind, 'floating')
  assert.equal(fw.child.kind, 'tabs')
  const fwTabs = fw.child as Tabs
  assert.equal(fwTabs.leaves.length, 1)
  assert.equal(fwTabs.leaves[0]!.id, leaf.id)
  assert.equal(leaf.parent, fwTabs, 'leaf.parent reparented to FW tabs')
})

test('popoutLeaf is idempotent for an already popped-out leaf', async () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leaf = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(leaf)
  await leaf.setViewState({ type: 'foo' })

  const fwId1 = workspace.popoutLeaf(leaf.id)
  const fwId2 = workspace.popoutLeaf(leaf.id)
  assert.equal(fwId1, fwId2, 'second popout returns the same FW id')
  assert.equal(workspace.floating.length, 1, 'no duplicate FW created')
})

test('popoutLeaf with bounds attaches bounds to the FW', async () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leaf = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(leaf)
  await leaf.setViewState({ type: 'foo' })

  const fwId = workspace.popoutLeaf(leaf.id, { x: 100, y: 200, w: 800, h: 600 })
  const fw = workspace.findFloatingWindow(fwId)!
  assert.deepEqual(fw.bounds, { x: 100, y: 200, w: 800, h: 600 })
})

test('popoutLeaf throws on unknown leaf id', async () => {
  freshLayout()
  assert.throws(
    () => workspace.popoutLeaf('definitely-not-a-leaf-id'),
    /unknown leaf id/,
  )
})

test('setFloatingWindowBounds updates bounds and emits layout-change', async () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leaf = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(leaf)
  await leaf.setViewState({ type: 'foo' })

  const fwId = workspace.popoutLeaf(leaf.id)
  let layoutChanges = 0
  const off = workspace.on('layout-change', () => {
    layoutChanges++
  })
  try {
    workspace.setFloatingWindowBounds(fwId, { x: 1, y: 2, w: 300, h: 400 })
    assert.equal(layoutChanges, 1, 'first bounds change fires layout-change')
    workspace.setFloatingWindowBounds(fwId, { x: 1, y: 2, w: 300, h: 400 })
    assert.equal(layoutChanges, 1, 'identical bounds re-write is a no-op')
    workspace.setFloatingWindowBounds(fwId, { x: 5, y: 6, w: 300, h: 400 })
    assert.equal(layoutChanges, 2, 'changed bounds fires layout-change')

    const fw = workspace.findFloatingWindow(fwId)!
    assert.deepEqual(fw.bounds, { x: 5, y: 6, w: 300, h: 400 })
  } finally {
    off()
  }
})

test('setFloatingWindowBounds is a no-op for unknown id', () => {
  freshLayout()
  // Just shouldn't throw.
  workspace.setFloatingWindowBounds('does-not-exist', { x: 0, y: 0, w: 1, h: 1 })
})

test('closeFloatingWindow removes FW + disposes leaves', async () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leaf = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(leaf)
  await leaf.setViewState({ type: 'foo' })

  const fwId = workspace.popoutLeaf(leaf.id)
  assert.ok(workspace.leaves.has(leaf.id), 'leaf still tracked pre-close')
  await workspace.closeFloatingWindow(fwId)
  assert.equal(workspace.floating.length, 0)
  assert.equal(workspace.leaves.has(leaf.id), false, 'leaf removed from registry')
})

test('closeFloatingWindow clears activeLeafId when active leaf was inside', async () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leaf = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(leaf)
  await leaf.setViewState({ type: 'foo', active: true })
  workspace.setActiveLeaf(leaf)
  assert.equal(workspace.activeLeafId, leaf.id)

  const fwId = workspace.popoutLeaf(leaf.id)
  await workspace.closeFloatingWindow(fwId)
  assert.equal(workspace.activeLeafId, null, 'active id cleared')
})

test('closeFloatingWindow on unknown id is a silent no-op', async () => {
  freshLayout()
  await workspace.closeFloatingWindow('does-not-exist')
  assert.equal(workspace.floating.length, 0)
})

test('serialize includes floating[] only when non-empty', async () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leaf = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(leaf)
  await leaf.setViewState({ type: 'foo' })

  const before = workspace.serialize()
  assert.equal(
    'floating' in before,
    false,
    'empty floating[] omitted from JSON for backwards-compat',
  )

  const fwId = workspace.popoutLeaf(leaf.id, { x: 10, y: 20, w: 800, h: 600 })
  const after = workspace.serialize()
  assert.ok(after.floating, 'floating field present after popout')
  assert.equal(after.floating!.length, 1)
  assert.equal(after.floating![0]!.kind, 'floating')
  assert.equal(after.floating![0]!.id, fwId)
  assert.deepEqual(after.floating![0]!.bounds, { x: 10, y: 20, w: 800, h: 600 })
})

test('hydrate restores floating[] entries with bounds', async () => {
  const fixture: WorkspaceJSON = {
    main: {
      kind: 'split',
      id: 'main-split',
      direction: 'horizontal',
      children: [{ kind: 'tabs', id: 'main-tabs', activeIndex: 0, leaves: [] }],
    },
    left: {
      kind: 'split',
      id: 'left-dock',
      direction: 'vertical',
      side: 'left',
      collapsed: false,
      size: 300,
      children: [{ kind: 'tabs', id: 'left-tabs', activeIndex: 0, leaves: [] }],
    },
    right: {
      kind: 'split',
      id: 'right-dock',
      direction: 'vertical',
      side: 'right',
      collapsed: false,
      size: 300,
      children: [{ kind: 'tabs', id: 'right-tabs', activeIndex: 0, leaves: [] }],
    },
    floating: [
      {
        kind: 'floating',
        id: 'fw-1',
        bounds: { x: 100, y: 200, w: 600, h: 400 },
        child: {
          kind: 'tabs',
          id: 'fw-tabs',
          activeIndex: 0,
          leaves: [
            { kind: 'leaf', id: 'fw-leaf-1', viewState: { type: 'foo', state: {} } },
          ],
        },
      },
    ],
    active: null,
    lastOpenFiles: [],
  }

  await workspace.hydrate(fixture)
  assert.equal(workspace.floating.length, 1)
  const fw = workspace.floating[0]!
  assert.equal(fw.id, 'fw-1')
  assert.deepEqual(fw.bounds, { x: 100, y: 200, w: 600, h: 400 })
  const fwTabs = fw.child as Tabs
  assert.equal(fwTabs.leaves.length, 1)
  assert.equal(fwTabs.leaves[0]!.id, 'fw-leaf-1')
  assert.equal(fwTabs.leaves[0]!.view?.viewType, 'foo')

  const out = workspace.serialize()
  assert.ok(out.floating, 'serialize round-trips floating[]')
  assert.equal(out.floating!.length, 1)
  assert.equal(out.floating![0]!.id, 'fw-1')
})

// --- P4-07 splitLeaf -----------------------------------------------------

test('splitLeaf horizontal: wraps a same-direction sibling onto the rootSplit', async () => {
  freshLayout()
  const tabs = firstTabs(workspace.rootSplit)
  const a = workspace.createLeaf(tabs)
  tabs.leaves.push(a)
  await a.setViewState({ type: 'foo' })

  // rootSplit defaults to horizontal in resetToDefault, so the new
  // sibling Tabs lands as the next child of the same Split — no extra
  // wrapping level.
  const beforeChildCount = workspace.rootSplit.children.length
  const newLeaf = workspace.splitLeaf(a.id, workspace.rootSplit.direction)

  assert.notEqual(newLeaf.id, a.id)
  assert.equal(workspace.activeLeafId, newLeaf.id, 'new leaf becomes active')
  assert.equal(
    workspace.rootSplit.children.length,
    beforeChildCount + 1,
    'one new sibling Tabs in the same Split',
  )
})

test('splitLeaf orthogonal: wraps parent Tabs in a new Split of requested direction', async () => {
  freshLayout()
  const tabs = firstTabs(workspace.rootSplit)
  const a = workspace.createLeaf(tabs)
  tabs.leaves.push(a)
  await a.setViewState({ type: 'foo' })

  const opposite =
    workspace.rootSplit.direction === 'horizontal' ? 'vertical' : 'horizontal'
  const newLeaf = workspace.splitLeaf(a.id, opposite)

  // The parent Tabs should now sit inside a fresh wrapping Split that
  // is itself a child of rootSplit.
  const wrappers = workspace.rootSplit.children.filter(
    (c) => c.kind === 'split' && (c as { direction?: string }).direction === opposite,
  )
  assert.ok(
    wrappers.length >= 1,
    'a new orthogonal Split should appear as a rootSplit child',
  )
  assert.equal(workspace.activeLeafId, newLeaf.id)
})

test('splitLeaf throws for unknown leaf id', () => {
  freshLayout()
  assert.throws(() => workspace.splitLeaf('does-not-exist', 'horizontal'), {
    message: /unknown leaf id/,
  })
})
