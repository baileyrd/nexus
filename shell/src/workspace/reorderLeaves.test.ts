// Tests for workspace.reorderLeaves (tab drag-reorder).
// Surfaced to the default pnpm test glob via tests/reorder-leaves.test.ts.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Tabs } from './types.ts'
import { workspace } from './workspaceStore.ts'

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

test('reorderLeaves moves a tab forward within the strip', () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leafA = workspace.createLeaf(rootTabs)
  const leafB = workspace.createLeaf(rootTabs)
  const leafC = workspace.createLeaf(rootTabs)
  rootTabs.leaves.length = 0
  rootTabs.leaves.push(leafA, leafB, leafC)
  rootTabs.activeIndex = 0

  workspace.reorderLeaves(rootTabs.id, 0, 2) // A → end: B, C, A
  assert.equal(rootTabs.leaves[0]!.id, leafB.id)
  assert.equal(rootTabs.leaves[1]!.id, leafC.id)
  assert.equal(rootTabs.leaves[2]!.id, leafA.id)
})

test('reorderLeaves moves a tab backward within the strip', () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leafA = workspace.createLeaf(rootTabs)
  const leafB = workspace.createLeaf(rootTabs)
  const leafC = workspace.createLeaf(rootTabs)
  rootTabs.leaves.length = 0
  rootTabs.leaves.push(leafA, leafB, leafC)
  rootTabs.activeIndex = 2

  workspace.reorderLeaves(rootTabs.id, 2, 0) // C → front: C, A, B
  assert.equal(rootTabs.leaves[0]!.id, leafC.id)
  assert.equal(rootTabs.leaves[1]!.id, leafA.id)
  assert.equal(rootTabs.leaves[2]!.id, leafB.id)
})

test('reorderLeaves preserves the active leaf identity after reorder', () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leafA = workspace.createLeaf(rootTabs)
  const leafB = workspace.createLeaf(rootTabs)
  const leafC = workspace.createLeaf(rootTabs)
  rootTabs.leaves.length = 0
  rootTabs.leaves.push(leafA, leafB, leafC)
  rootTabs.activeIndex = 1 // leafB is active

  workspace.reorderLeaves(rootTabs.id, 0, 2) // move A: B, C, A
  // leafB is now at index 0; activeIndex must follow it
  assert.equal(rootTabs.leaves[rootTabs.activeIndex]!.id, leafB.id)
})

test('reorderLeaves is a no-op when fromIndex === toIndex', () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leafA = workspace.createLeaf(rootTabs)
  const leafB = workspace.createLeaf(rootTabs)
  rootTabs.leaves.length = 0
  rootTabs.leaves.push(leafA, leafB)
  rootTabs.activeIndex = 0

  workspace.reorderLeaves(rootTabs.id, 0, 0)
  assert.equal(rootTabs.leaves[0]!.id, leafA.id)
  assert.equal(rootTabs.leaves[1]!.id, leafB.id)
})
