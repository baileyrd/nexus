/**
 * BL-067 phase 2b — `workspace.moveLeafToDock` unit tests.
 *
 * Authored under `tests/` rather than alongside the sibling
 * `src/workspace/workspaceStore.test.ts` because the default
 * `pnpm test` glob (`tests/*.test.ts`) only sees this directory.
 * Same posture as `tests/set-split-sizes.test.ts`.
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'

import type { Tabs } from '../src/workspace/types.ts'
import { workspace } from '../src/workspace/workspaceStore.ts'

function freshLayout(): void {
  workspace.resetToDefault()
}

function firstTabs(node: import('../src/workspace/types.ts').WorkspaceParent): Tabs {
  if (node.kind === 'tabs') return node
  if (node.kind === 'split') {
    for (const c of node.children) {
      try {
        return firstTabs(c)
      } catch {
        // continue scanning
      }
    }
  }
  if (node.kind === 'root' || node.kind === 'floating') {
    return firstTabs(node.child)
  }
  throw new Error(`no Tabs in ${node.kind}`)
}

test('moveLeafToDock relocates a leaf from rootSplit to leftSplit', async () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leaf = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(leaf)
  await leaf.setViewState({ type: 'foo' })

  workspace.moveLeafToDock(leaf, 'left')

  const leftTabs = firstTabs(workspace.leftSplit)
  assert.ok(
    leftTabs.leaves.some((l) => l.id === leaf.id),
    'leaf must live in leftSplit first Tabs',
  )
  assert.equal(rootTabs.leaves.includes(leaf), false, 'leaf removed from source')
  assert.equal(leaf.parent, leftTabs, 'parent pointer rewritten')
  assert.equal(
    leftTabs.activeIndex,
    leftTabs.leaves.length - 1,
    'activeIndex points at moved leaf',
  )
})

test('moveLeafToDock no-ops when leaf is already in the destination dock', async () => {
  freshLayout()
  const leftTabs = firstTabs(workspace.leftSplit)
  const leaf = workspace.createLeaf(leftTabs)
  leftTabs.leaves.push(leaf)
  await leaf.setViewState({ type: 'foo' })
  const beforeLen = leftTabs.leaves.length
  let layoutChanges = 0
  const off = workspace.on('layout-change', () => {
    layoutChanges += 1
  })

  workspace.moveLeafToDock(leaf, 'left')

  off()
  assert.equal(leftTabs.leaves.length, beforeLen, 'no-op did not duplicate the leaf')
  assert.equal(leaf.parent, leftTabs, 'parent pointer unchanged')
  assert.equal(layoutChanges, 0, 'no-op skipped layout-change emission')
})

test('moveLeafToDock to "main" routes to the root split', async () => {
  freshLayout()
  const rightTabs = firstTabs(workspace.rightSplit)
  const leaf = workspace.createLeaf(rightTabs)
  rightTabs.leaves.push(leaf)
  await leaf.setViewState({ type: 'foo' })

  workspace.moveLeafToDock(leaf, 'main')

  const rootTabs = firstTabs(workspace.rootSplit)
  assert.ok(
    rootTabs.leaves.some((l) => l.id === leaf.id),
    'leaf must live in the root split',
  )
  assert.equal(rightTabs.leaves.includes(leaf), false, 'leaf removed from right dock')
})

test('moveLeafToDock auto-expands a collapsed destination sidedock', async () => {
  freshLayout()
  workspace.leftSplit.collapsed = true
  const rootTabs = firstTabs(workspace.rootSplit)
  const leaf = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(leaf)
  await leaf.setViewState({ type: 'foo' })

  workspace.moveLeafToDock(leaf, 'left')

  assert.equal(workspace.leftSplit.collapsed, false, 'collapsed dock expanded after a move-in')
})

test('moveLeafToDock preserves activeLeaf when the moved leaf was active', async () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leaf = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(leaf)
  await leaf.setViewState({ type: 'foo' })
  workspace.setActiveLeaf(leaf)
  assert.equal(workspace.activeLeafId, leaf.id)

  workspace.moveLeafToDock(leaf, 'right')

  assert.equal(workspace.activeLeafId, leaf.id, 'active leaf still active after move')
})

test('moveLeafToDock clamps source activeIndex when removing the active tab', async () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const a = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(a)
  await a.setViewState({ type: 'a' })
  const b = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(b)
  await b.setViewState({ type: 'b' })
  rootTabs.activeIndex = rootTabs.leaves.length - 1 // points at b

  workspace.moveLeafToDock(b, 'left')

  assert.equal(rootTabs.leaves.includes(b), false)
  assert.ok(
    rootTabs.activeIndex < rootTabs.leaves.length,
    'activeIndex must stay inside leaves[]',
  )
})

test('moveLeafToDock leaves view alive (does not detach)', async () => {
  freshLayout()
  const rootTabs = firstTabs(workspace.rootSplit)
  const leaf = workspace.createLeaf(rootTabs)
  rootTabs.leaves.push(leaf)
  await leaf.setViewState({ type: 'foo' })
  assert.ok(leaf.view, 'sanity: view created by setViewState')
  const viewRef = leaf.view

  workspace.moveLeafToDock(leaf, 'right')

  assert.equal(leaf.view, viewRef, 'view instance survives the move (no detach)')
})
