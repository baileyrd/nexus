// Unit tests for SessionManager's refcount semantics.
//
// Run with: node --experimental-strip-types --test \
//   src/plugins/nexus/editor/sessionManager.test.ts

import type { KernelAPI } from '../../../types/plugin.ts'
import type { EditorChangedPayload, EditorSnapshot } from './types.ts'
import { makeEditorClient } from './kernelClient.ts'
import { makeSessionManager } from './sessionManager.ts'
import { useEditorStore } from './editorStore.ts'

const nodeTest: string = 'node:test'
const nodeAssert: string = 'node:assert/strict'
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const { test } = (await import(nodeTest)) as any
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const assert = ((await import(nodeAssert)) as any).default

// ── fixtures ─────────────────────────────────────────────────────────────────

interface InvokeCall {
  pluginId: string
  commandId: string
  args: unknown
}

function emptySnapshot(relpath: string): EditorSnapshot {
  return {
    relpath,
    tree: { blocks: {}, root_blocks: [], metadata: {} },
    undoPosition: null,
    undoLen: 0,
    canUndo: false,
    canRedo: false,
    revision: 0,
  }
}

function makeMockApi(): { api: KernelAPI; calls: InvokeCall[] } {
  const calls: InvokeCall[] = []
  const api: KernelAPI = {
    async invoke<T = unknown>(
      pluginId: string,
      commandId: string,
      args?: unknown,
    ): Promise<T> {
      calls.push({ pluginId, commandId, args })
      // `open` returns an EditorSnapshot; `close` returns `{}`.
      if (commandId === 'open') {
        const relpath =
          (args as { relpath?: string } | undefined)?.relpath ?? ''
        return emptySnapshot(relpath) as T
      }
      return {} as T
    },
    async on<T = unknown>(
      _topicPrefix: string,
      _handler: (topic: string, payload: T) => void,
    ): Promise<() => void> {
      return () => {}
    },
    async available(): Promise<boolean> {
      return true
    },
  }
  return { api, calls }
}

// ── tests ────────────────────────────────────────────────────────────────────

test('acquire twice opens the session exactly once and caches the snapshot', async () => {
  const { api, calls } = makeMockApi()
  const mgr = makeSessionManager(makeEditorClient(api))

  const snap1 = await mgr.acquire('notes/a.md')
  const snap2 = await mgr.acquire('notes/a.md')

  const opens = calls.filter((c) => c.commandId === 'open')
  assert.equal(opens.length, 1, 'open called once across two acquires')
  assert.equal(mgr.refcount('notes/a.md'), 2)
  assert.equal(snap1?.relpath, 'notes/a.md')
  // Cached snapshot is returned verbatim on the second acquire.
  assert.strictEqual(snap1, snap2)
})

test('release twice after two acquires closes exactly once', async () => {
  const { api, calls } = makeMockApi()
  const mgr = makeSessionManager(makeEditorClient(api))

  await mgr.acquire('notes/a.md')
  await mgr.acquire('notes/a.md')
  await mgr.release('notes/a.md')
  assert.equal(
    calls.filter((c) => c.commandId === 'close').length,
    0,
    'close must not fire until refcount hits zero',
  )
  assert.equal(mgr.refcount('notes/a.md'), 1)

  await mgr.release('notes/a.md')
  assert.equal(
    calls.filter((c) => c.commandId === 'close').length,
    1,
    'close fires on the 1→0 transition',
  )
  assert.equal(mgr.refcount('notes/a.md'), 0)
})

test('acquire after full release reopens the session', async () => {
  const { api, calls } = makeMockApi()
  const mgr = makeSessionManager(makeEditorClient(api))

  await mgr.acquire('notes/a.md')
  await mgr.release('notes/a.md')
  await mgr.acquire('notes/a.md')

  const opens = calls.filter((c) => c.commandId === 'open')
  const closes = calls.filter((c) => c.commandId === 'close')
  assert.equal(opens.length, 2)
  assert.equal(closes.length, 1)
  assert.equal(mgr.refcount('notes/a.md'), 1)
})

test('untitled / empty relpaths do not call the kernel and return null', async () => {
  const { api, calls } = makeMockApi()
  const mgr = makeSessionManager(makeEditorClient(api))

  const a = await mgr.acquire('untitled-1')
  const b = await mgr.acquire('')
  const c = await mgr.acquire(null)
  const d = await mgr.acquire(undefined)
  await mgr.release('untitled-1')
  await mgr.release('')

  assert.equal(a, null)
  assert.equal(b, null)
  assert.equal(c, null)
  assert.equal(d, null)
  assert.equal(calls.length, 0, 'kernel never invoked for untitled / empty paths')
})

test('release on an unknown relpath is a no-op', async () => {
  const { api, calls } = makeMockApi()
  const mgr = makeSessionManager(makeEditorClient(api))

  await mgr.release('never-acquired.md')
  assert.equal(calls.length, 0)
})

// ── Phase 4: subscription + echo suppression ──────────────────────────────────

interface SubscribeCall {
  topicPrefix: string
  handler: (topic: string, payload: unknown) => void
}

function makeMockApiWithSubscribe(): {
  api: KernelAPI
  subscribes: SubscribeCall[]
  unsubscribes: string[]
} {
  const subscribes: SubscribeCall[] = []
  const unsubscribes: string[] = []
  const api: KernelAPI = {
    async invoke<T = unknown>(
      _pluginId: string,
      commandId: string,
      args?: unknown,
    ): Promise<T> {
      if (commandId === 'open') {
        const relpath =
          (args as { relpath?: string } | undefined)?.relpath ?? ''
        return emptySnapshot(relpath) as T
      }
      return {} as T
    },
    async on<T = unknown>(
      topicPrefix: string,
      handler: (topic: string, payload: T) => void,
    ): Promise<() => void> {
      subscribes.push({
        topicPrefix,
        handler: handler as (topic: string, payload: unknown) => void,
      })
      return () => {
        unsubscribes.push(topicPrefix)
      }
    },
    async available(): Promise<boolean> {
      return true
    },
  }
  return { api, subscribes, unsubscribes }
}

/** Reset store revision / pending state between tests — Zustand
 *  singletons leak across test cases otherwise. */
function resetStoreRevisionState(): void {
  useEditorStore.setState({
    sessionRevision: new Map<string, number>(),
    pendingLocalRevisions: new Set<string>(),
  })
}

test('acquire subscribes to the per-relpath changed channel and updates store revision', async () => {
  resetStoreRevisionState()
  const { api, subscribes } = makeMockApiWithSubscribe()
  const mgr = makeSessionManager(makeEditorClient(api), api)

  await mgr.acquire('notes/a.md')
  // Wait a microtask so the subscribe promise chain installs the handle.
  await Promise.resolve()

  assert.equal(subscribes.length, 1, 'acquire opens one subscription')
  assert.equal(
    subscribes[0]!.topicPrefix,
    'com.nexus.editor.changed.notes/a.md',
    'subscription is keyed on the fully-qualified relpath',
  )

  // Seeded from the open snapshot.
  assert.equal(useEditorStore.getState().sessionRevision.get('notes/a.md'), 0)

  // Fire a synthetic event — should bump the store revision and not
  // touch pending (no pending id means no echo to suppress).
  const payload: EditorChangedPayload = {
    relpath: 'notes/a.md',
    revision: 3,
    transaction_id: null,
  }
  subscribes[0]!.handler('com.nexus.editor.changed.notes/a.md', payload)
  assert.equal(useEditorStore.getState().sessionRevision.get('notes/a.md'), 3)
})

test('changed events for a pending local transaction id are dropped (echo suppression)', async () => {
  resetStoreRevisionState()
  const { api, subscribes } = makeMockApiWithSubscribe()
  const mgr = makeSessionManager(makeEditorClient(api), api)

  await mgr.acquire('notes/b.md')
  await Promise.resolve()
  assert.equal(subscribes.length, 1)

  // Simulate a Phase-5 caller adding a pending transaction id before
  // dispatching `apply_transaction`.
  const txId = '11111111-2222-3333-4444-555555555555'
  useEditorStore.getState().addPendingLocalRevision(txId)

  const seen: EditorChangedPayload[] = []
  mgr.onChanged((p) => seen.push(p))

  // Fire the echo event.
  subscribes[0]!.handler('com.nexus.editor.changed.notes/b.md', {
    relpath: 'notes/b.md',
    revision: 7,
    transaction_id: txId,
  })

  // Store revision unchanged, pending set drained, no fan-out.
  assert.equal(
    useEditorStore.getState().sessionRevision.get('notes/b.md'),
    0,
    'echoed revision must NOT be written to the store',
  )
  assert.equal(
    useEditorStore.getState().pendingLocalRevisions.has(txId),
    false,
    'pending id is consumed on echo',
  )
  assert.equal(seen.length, 0, 'echoed events do not reach changed listeners')

  // A subsequent non-echo event does fan out.
  subscribes[0]!.handler('com.nexus.editor.changed.notes/b.md', {
    relpath: 'notes/b.md',
    revision: 8,
    transaction_id: null,
  })
  assert.equal(useEditorStore.getState().sessionRevision.get('notes/b.md'), 8)
  assert.equal(seen.length, 1)
  assert.equal(seen[0]!.revision, 8)
})

test('release unsubscribes on the 1→0 transition and clears the store revision', async () => {
  resetStoreRevisionState()
  const { api, subscribes, unsubscribes } = makeMockApiWithSubscribe()
  const mgr = makeSessionManager(makeEditorClient(api), api)

  await mgr.acquire('notes/c.md')
  await mgr.acquire('notes/c.md')
  await Promise.resolve()
  assert.equal(subscribes.length, 1)

  await mgr.release('notes/c.md')
  assert.equal(unsubscribes.length, 0, 'no unsubscribe while refcount > 0')
  assert.equal(useEditorStore.getState().sessionRevision.get('notes/c.md'), 0)

  await mgr.release('notes/c.md')
  assert.equal(unsubscribes.length, 1, 'unsubscribe fires on the 1→0 release')
  assert.equal(
    useEditorStore.getState().sessionRevision.has('notes/c.md'),
    false,
    'revision entry is cleared on release',
  )
})
