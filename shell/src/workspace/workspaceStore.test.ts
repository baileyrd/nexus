// Unit tests for workspaceStore (Phase 3).
// Run with: node --experimental-strip-types --test src/workspace/workspaceStore.test.ts
//
// String-indirected node:test imports keep tsc happy without @types/node,
// matching the pattern in ViewRegistry.test.ts / Leaf.test.ts.

import type {
  Leaf,
  Tabs,
  View,
  WorkspaceJSON,
} from './types.ts'
import { viewRegistry } from './ViewRegistry.ts'
import { ViewBase } from './View.ts'
import { workspace } from './workspaceStore.ts'

const nodeTest: string = 'node:test'
const nodeAssert: string = 'node:assert/strict'
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const { test } = (await import(nodeTest)) as any
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const assert = ((await import(nodeAssert)) as any).default

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
