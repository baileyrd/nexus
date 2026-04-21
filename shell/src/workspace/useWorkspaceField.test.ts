// Unit test for the `useWorkspaceField` hook's subscription contract.
// Run with: node --experimental-strip-types --test src/workspace/useWorkspaceField.test.ts
//
// We can't render React hooks without jsdom + a renderer in this repo,
// so this test exercises the underlying event wiring the hook depends
// on: every sidedock mutation must emit `layout-change` so the hook's
// internal `useLayoutVersion` reducer can force a re-read. A broken
// emit would leave `useWorkspaceField` stuck on a stale value.

import { workspace } from './workspaceStore.ts'
import { useWorkspaceField } from './useWorkspaceField.ts'

const nodeTest: string = 'node:test'
const nodeAssert: string = 'node:assert/strict'
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const { test } = (await import(nodeTest)) as any
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const assert = ((await import(nodeAssert)) as any).default

test('useWorkspaceField is exported', () => {
  assert.equal(typeof useWorkspaceField, 'function')
})

test('layout-change fires on setSidedockCollapsed', () => {
  workspace.resetToDefault()
  workspace.setSidedockCollapsed('left', false)

  let hits = 0
  const off = workspace.on('layout-change', () => {
    hits++
  })

  workspace.setSidedockCollapsed('left', true)
  workspace.setSidedockCollapsed('left', false)

  off()
  assert.equal(hits, 2, 'two collapse transitions should emit two events')
})

test('layout-change fires on setSidedockSize', () => {
  workspace.resetToDefault()
  workspace.setSidedockSize('left', 300)

  let hits = 0
  const off = workspace.on('layout-change', () => {
    hits++
  })

  workspace.setSidedockSize('left', 400)
  workspace.setSidedockSize('left', 400) // same size → no emit (store dedupes)
  workspace.setSidedockSize('left', 500)

  off()
  assert.equal(hits, 2, 'duplicate size writes should dedupe to two emits')
})
