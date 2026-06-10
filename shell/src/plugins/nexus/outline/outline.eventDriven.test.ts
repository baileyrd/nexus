// Phase 7 integration-style test: a synthetic editor-change event fed
// through SessionManager reaches a subscriber that calls getTree and
// pushes headings into the outline store.
//
// We don't activate the whole plugin (that requires the full host
// plumbing). Instead we wire just the pieces Phase 7 adds:
//   1. a SessionManager bound to a mock KernelAPI that captures the
//      `api.on` handler (so we can fire a fake `changed` event),
//   2. an onChanged listener that mirrors `outline/index.ts`'s
//      recompute: call client.getTree + treeToHeadings + store.setHeadings.
//
// Run with: node --experimental-strip-types --test \
//   src/plugins/nexus/outline/outline.eventDriven.test.ts

import type { KernelAPI } from '../../../types/plugin.ts'
import type {
  EditorChangedPayload,
  EditorSnapshot,
  Block,
  BlockTree,
} from '../editor/types.ts'
import { makeEditorClient } from '../editor/kernelClient.ts'
import { makeSessionManager } from '../editor/sessionManager.ts'
import { useEditorStore } from '../editor/editorStore.ts'
import { useOutlineStore } from './outlineStore.ts'
import { treeToHeadings } from './parse.ts'

import { test } from 'node:test'
import assert from 'node:assert/strict'

// ── fixtures ────────────────────────────────────────────────────────────────

function makeBlock(id: string, partial: Partial<Block>): Block {
  return {
    id,
    ty: { kind: 'paragraph' },
    content: '',
    annotations: [],
    properties: {},
    parent_id: null,
    children: [],
    index_in_parent: 0,
    created_at: 0,
    updated_at: 0,
    is_deleted: false,
    ...partial,
  }
}

function buildTree(blocks: Block[]): BlockTree {
  const byId: Record<string, Block> = {}
  const rootIds: string[] = []
  for (const b of blocks) {
    byId[b.id] = b
    rootIds.push(b.id)
  }
  return { blocks: byId, root_blocks: rootIds, metadata: {} }
}

function emptySnapshot(relpath: string, revision = 0): EditorSnapshot {
  return {
    relpath,
    tree: { blocks: {}, root_blocks: [], metadata: {} },
    undoPosition: null,
    undoLen: 0,
    canUndo: false,
    canRedo: false,
    revision,
  }
}

interface CapturedHandler {
  topic: string
  handler: (topic: string, payload: unknown) => void
}

interface MockCtx {
  api: KernelAPI
  captured: CapturedHandler[]
  /** Snapshot the mock returns from `get_tree`. Mutable by the test. */
  setTreeResponse(snap: EditorSnapshot): void
}

function makeMockApi(initialOpenSnapshot: EditorSnapshot): MockCtx {
  const captured: CapturedHandler[] = []
  let treeResponse: EditorSnapshot = initialOpenSnapshot
  const api: KernelAPI = {
    async invoke<T = unknown>(
      _pluginId: string,
      commandId: string,
      _args?: unknown,
    ): Promise<T> {
      if (commandId === 'open') return initialOpenSnapshot as T
      if (commandId === 'get_tree') return treeResponse as T
      if (commandId === 'get_markdown') return '' as T
      return {} as T
    },
    async on<T = unknown>(
      topicPrefix: string,
      handler: (topic: string, payload: T) => void,
    ): Promise<() => void> {
      captured.push({
        topic: topicPrefix,
        // Erase the subscription's payload generic for storage; the test
        // replays concrete payloads through it.
        handler: handler as (topic: string, payload: unknown) => void,
      })
      return () => {}
    },
    async available(): Promise<boolean> {
      return true
    },
  }
  return {
    api,
    captured,
    setTreeResponse(snap) {
      treeResponse = snap
    },
  }
}

// ── tests ────────────────────────────────────────────────────────────────────

test('synthetic changed event → getTree → outline store updates', async () => {
  const relpath = 'notes/outline-test.md'

  // Reset stores so prior tests don't bleed through.
  useOutlineStore.getState().clear()
  useEditorStore.getState().clear()

  const ctx = makeMockApi(emptySnapshot(relpath, 1))
  const client = makeEditorClient(ctx.api)
  const mgr = makeSessionManager(client, ctx.api)

  // Acquire: triggers `api.on(...)` so we capture the handler that
  // the SessionManager will dispatch `changed` events through.
  await mgr.acquire(relpath)
  // Wait for the subscribing promise to resolve so `captured` is
  // populated.
  await Promise.resolve()
  await Promise.resolve()

  assert.equal(ctx.captured.length, 1, 'acquire wired exactly one subscription')
  const capture = ctx.captured[0]
  assert.ok(capture.topic.endsWith(relpath))

  // Mirror the outline plugin's onChanged recompute — but synchronous
  // so the test doesn't have to dance around rAF scheduling.
  const recompute = async () => {
    const snap = await client.getTree(relpath)
    const headings = treeToHeadings(snap.tree)
    useOutlineStore.getState().setHeadings(headings)
  }
  const unsub = mgr.onChanged((payload: EditorChangedPayload) => {
    if (payload.relpath !== relpath) return
    void recompute()
  })

  // First wire: the tree starts empty, so headings should be empty.
  await recompute()
  assert.equal(useOutlineStore.getState().headings.length, 0)

  // Now arrange for getTree to return a tree with headings, and
  // fire a synthetic `changed` event through the SessionManager's
  // subscription handler (i.e. what the Rust plugin would publish).
  ctx.setTreeResponse({
    ...emptySnapshot(relpath, 2),
    revision: 2,
    tree: buildTree([
      makeBlock('h1', { ty: { kind: 'heading', level: 1 }, content: 'Alpha' }),
      makeBlock('p1', { ty: { kind: 'paragraph' }, content: 'body' }),
      makeBlock('h2', { ty: { kind: 'heading', level: 2 }, content: 'Beta' }),
    ]),
  })

  const payload: EditorChangedPayload = {
    relpath,
    revision: 2,
    transaction_id: null, // non-local → no echo suppression
  }
  capture.handler(`com.nexus.editor.changed.${relpath}`, payload)

  // The onChanged listener runs synchronously but `recompute` is
  // async — drain microtasks so the getTree promise resolves and
  // the store write lands.
  await Promise.resolve()
  await Promise.resolve()

  const headings = useOutlineStore.getState().headings
  assert.equal(headings.length, 2)
  assert.equal(headings[0].text, 'Alpha')
  assert.equal(headings[0].level, 1)
  assert.equal(headings[1].text, 'Beta')
  assert.equal(headings[1].level, 2)

  unsub()
  await mgr.release(relpath)

  // Cleanup: leave stores in a predictable state for the next test.
  useOutlineStore.getState().clear()
  useEditorStore.getState().clear()
})

test('changed event for a different relpath is ignored by the filter', async () => {
  const relpath = 'notes/a.md'
  const other = 'notes/b.md'

  useOutlineStore.getState().clear()
  useEditorStore.getState().clear()

  const ctx = makeMockApi(emptySnapshot(relpath, 1))
  const client = makeEditorClient(ctx.api)
  const mgr = makeSessionManager(client, ctx.api)
  await mgr.acquire(relpath)
  await Promise.resolve()
  await Promise.resolve()

  let recomputes = 0
  const unsub = mgr.onChanged((payload: EditorChangedPayload) => {
    if (payload.relpath !== relpath) return
    recomputes++
  })

  const capture = ctx.captured[0]
  // Simulate a change event for the OTHER file — SessionManager
  // still delivers it to every listener, but our filter should
  // drop it. (In practice the Rust forwarder subscribes per-relpath
  // so cross-file deliveries shouldn't happen at all, but we guard
  // defensively.)
  capture.handler(`com.nexus.editor.changed.${other}`, {
    relpath: other,
    revision: 5,
    transaction_id: null,
  })

  assert.equal(recomputes, 0, 'filter drops events for non-active relpath')

  capture.handler(`com.nexus.editor.changed.${relpath}`, {
    relpath,
    revision: 2,
    transaction_id: null,
  })
  assert.equal(recomputes, 1)

  unsub()
  await mgr.release(relpath)
  useOutlineStore.getState().clear()
  useEditorStore.getState().clear()
})
