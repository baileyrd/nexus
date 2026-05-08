// shell/src/plugins/nexus/status/statusStore.test.ts
//
// BL-053 Phase 4 — statusStore + fetchStatus tests against a fake
// `KernelAPI`. Pins the cache contract (read-through, in-flight
// coalescing, FIFO eviction at 256 entries, error-as-null) and the
// `invalidate` semantics.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import type { KernelAPI } from '../../../types/plugin.ts'
import {
  fetchStatus,
  getCachedStatus,
  useStatusStore,
} from './statusStore.ts'

interface FakeCall {
  pluginId: string
  commandId: string
  args: unknown
}

interface FakeKernel extends KernelAPI {
  calls: FakeCall[]
  responses: Map<string, unknown>
  errors: Map<string, string>
}

function makeFakeKernel(): FakeKernel {
  const calls: FakeCall[] = []
  const responses = new Map<string, unknown>()
  const errors = new Map<string, string>()
  const k: FakeKernel = {
    calls,
    responses,
    errors,
    async invoke<T>(pluginId: string, commandId: string, args?: unknown): Promise<T> {
      calls.push({ pluginId, commandId, args: args ?? {} })
      const key = (args as { path?: string })?.path ?? ''
      if (errors.has(key)) throw new Error(errors.get(key))
      return ((responses.get(key) ?? { status: null, fields: {} }) as T)
    },
    async on(): Promise<() => void> {
      return () => {}
    },
    async available(): Promise<boolean> {
      return true
    },
  }
  return k
}

test('fetchStatus caches the result and short-circuits on subsequent calls', async () => {
  useStatusStore.getState().clear()
  const k = makeFakeKernel()
  k.responses.set('a/b.md', { status: 'info', fields: {} })
  const first = await fetchStatus(k, 'a/b.md')
  assert.equal(first, 'info')
  assert.equal(getCachedStatus('a/b.md'), 'info')
  // Second fetch uses the cache — no new IPC.
  const second = await fetchStatus(k, 'a/b.md')
  assert.equal(second, 'info')
  assert.equal(k.calls.length, 1)
})

test('fetchStatus coalesces concurrent in-flight requests', async () => {
  useStatusStore.getState().clear()
  const k = makeFakeKernel()
  k.responses.set('coalesce.md', { status: 'ok', fields: {} })
  const [a, b, c] = await Promise.all([
    fetchStatus(k, 'coalesce.md'),
    fetchStatus(k, 'coalesce.md'),
    fetchStatus(k, 'coalesce.md'),
  ])
  assert.equal(a, 'ok')
  assert.equal(b, 'ok')
  assert.equal(c, 'ok')
  // Only one IPC call should have fired.
  assert.equal(k.calls.length, 1)
})

test('fetchStatus caches null on error so we don\'t re-fetch transient failures', async () => {
  useStatusStore.getState().clear()
  const k = makeFakeKernel()
  k.errors.set('broken.md', 'PermissionDenied')
  const out = await fetchStatus(k, 'broken.md')
  assert.equal(out, null)
  assert.equal(getCachedStatus('broken.md'), null)
  // Second call short-circuits — error was cached as `null`.
  const out2 = await fetchStatus(k, 'broken.md')
  assert.equal(out2, null)
  assert.equal(k.calls.length, 1)
})

test('invalidate drops the entry so the next fetch retries', async () => {
  useStatusStore.getState().clear()
  const k = makeFakeKernel()
  k.responses.set('p.md', { status: 'warn', fields: {} })
  await fetchStatus(k, 'p.md')
  assert.equal(getCachedStatus('p.md'), 'warn')
  useStatusStore.getState().invalidate('p.md')
  assert.equal(getCachedStatus('p.md'), undefined)
  // Update the response and re-fetch — should see the new value.
  k.responses.set('p.md', { status: 'risk', fields: {} })
  const refetched = await fetchStatus(k, 'p.md')
  assert.equal(refetched, 'risk')
})

test('FIFO eviction at the 256-entry limit', async () => {
  useStatusStore.getState().clear()
  const k = makeFakeKernel()
  // Seed 260 entries. The first 4 should evict.
  for (let i = 0; i < 260; i++) {
    k.responses.set(`f${i}.md`, { status: 'info', fields: {} })
    await fetchStatus(k, `f${i}.md`)
  }
  assert.equal(useStatusStore.getState().cache.size, 256)
  for (let i = 0; i < 4; i++) {
    assert.equal(getCachedStatus(`f${i}.md`), undefined, `f${i}.md evicted`)
  }
  // The most recent entry survives.
  assert.equal(getCachedStatus('f259.md'), 'info')
})

test('fetchStatus stores `null` when the reply has no status', async () => {
  useStatusStore.getState().clear()
  const k = makeFakeKernel()
  k.responses.set('nostatus.md', { status: null, fields: { title: 'Hello' } })
  const out = await fetchStatus(k, 'nostatus.md')
  assert.equal(out, null)
  // Cached `null` is distinct from "unfetched" (`undefined`).
  assert.equal(getCachedStatus('nostatus.md'), null)
})
