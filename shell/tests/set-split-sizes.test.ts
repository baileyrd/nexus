/**
 * OI-02 — `workspace.setSplitSizes` unit tests.
 *
 * Authored as a standalone file under `tests/` (rather than appended
 * to `src/workspace/workspaceStore.test.ts`) because the sibling
 * store-test file uses top-level `await import(...)` tricks to dodge
 * `@types/node`, and the default `pnpm test` glob surfaces `.ts`
 * files in `tests/` via tsx's CJS transform — which rejects
 * top-level await. Static `node:test` imports here keep the runner
 * happy and land in the same reporter output.
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Tabs } from '../src/workspace/types.ts'
import { workspace } from '../src/workspace/workspaceStore.ts'

function freshLayout(): void {
  workspace.resetToDefault()
}

// Helper: give the root split a second child so `sizes` is meaningful.
function addSecondChild(): void {
  workspace.rootSplit.children.push({
    kind: 'tabs',
    id: 't-second',
    activeIndex: 0,
    leaves: [],
  } satisfies Tabs)
}

test('OI-02 — setSplitSizes writes sizes on the named split and fires layout-change', () => {
  freshLayout()
  addSecondChild()
  let events = 0
  const off = workspace.on('layout-change', () => {
    events++
  })
  try {
    workspace.setSplitSizes(workspace.rootSplit.id, [0.7, 0.3])
    assert.deepEqual(workspace.rootSplit.sizes, [0.7, 0.3])
    assert.equal(events >= 1, true, 'layout-change must fire on size write')
  } finally {
    off()
  }
})

test('OI-02 — setSplitSizes rejects arity mismatch (no write, no event)', () => {
  freshLayout()
  addSecondChild()
  workspace.setSplitSizes(workspace.rootSplit.id, [0.5, 0.5])
  const firstSizes = workspace.rootSplit.sizes
  let events = 0
  const off = workspace.on('layout-change', () => {
    events++
  })
  try {
    workspace.setSplitSizes(workspace.rootSplit.id, [0.3, 0.3, 0.4])
    assert.deepEqual(workspace.rootSplit.sizes, firstSizes)
    assert.equal(events, 0)
  } finally {
    off()
  }
})

test('OI-02 — setSplitSizes clamps small values to the min weight', () => {
  freshLayout()
  addSecondChild()
  workspace.setSplitSizes(workspace.rootSplit.id, [0.99, 0.01])
  const sizes = workspace.rootSplit.sizes
  assert.ok(sizes, 'sizes should be set')
  assert.equal(sizes!.length, 2)
  assert.ok(
    sizes![1] >= 0.1,
    `right weight ${sizes![1]} must be clamped to >= 0.1`,
  )
})

test('OI-02 — setSplitSizes on an unknown split id is a no-op', () => {
  freshLayout()
  let events = 0
  const off = workspace.on('layout-change', () => {
    events++
  })
  try {
    workspace.setSplitSizes('no-such-split', [0.5, 0.5])
    assert.equal(events, 0)
  } finally {
    off()
  }
})

test('OI-02 — setSplitSizes skips redundant writes (idempotent)', () => {
  freshLayout()
  addSecondChild()
  workspace.setSplitSizes(workspace.rootSplit.id, [0.6, 0.4])
  let events = 0
  const off = workspace.on('layout-change', () => {
    events++
  })
  try {
    workspace.setSplitSizes(workspace.rootSplit.id, [0.6, 0.4])
    assert.equal(events, 0, 'identical sizes must not refire layout-change')
  } finally {
    off()
  }
})
