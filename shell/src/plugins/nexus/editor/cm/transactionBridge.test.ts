// Unit tests for the Phase 5 transaction bridge.
//
// These avoid constructing a CM `EditorView` (which needs a DOM) by
// testing the view-independent `createBridgeCore` against synthetic
// `ViewUpdate`-shaped objects assembled from real `EditorState`
// transactions.
//
// Run with: node --experimental-strip-types --test \
//   src/plugins/nexus/editor/cm/transactionBridge.test.ts

import type { KernelAPI } from '../../../../types/plugin.ts'
import type {
  BlockId,
  EditorSnapshot,
  Operation,
  Transaction,
} from '../types.ts'
import { EditorKernelClient } from '../kernelClient.ts'
import { useEditorStore } from '../editorStore.ts'
import {
  changesToOps,
  createBridgeCore,
  makeTransaction,
  type BridgeViewLike,
} from './transactionBridge.ts'

import { EditorState } from '@codemirror/state'

const nodeTest: string = 'node:test'
const nodeAssert: string = 'node:assert/strict'
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const { test } = (await import(nodeTest)) as any
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const assert = ((await import(nodeAssert)) as any).default

// ── fixtures ─────────────────────────────────────────────────────────────────

const ROOT_ID: BlockId = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee'

function snapshotWithRoot(relpath: string, content = ''): EditorSnapshot {
  return {
    relpath,
    tree: {
      blocks: {
        [ROOT_ID]: {
          id: ROOT_ID,
          ty: { kind: 'paragraph' },
          content,
          annotations: [],
          properties: {},
          parent_id: null,
          children: [],
          index_in_parent: 0,
          created_at: 0,
          updated_at: 0,
          is_deleted: false,
        },
      },
      root_blocks: [ROOT_ID],
      metadata: {},
    },
    undoPosition: null,
    undoLen: 0,
    canUndo: false,
    canRedo: false,
    revision: 0,
  }
}

interface InvokeCall {
  commandId: string
  args: unknown
}

function makeMockApi(
  responses: Record<string, (args: unknown) => unknown>,
): { api: KernelAPI; calls: InvokeCall[] } {
  const calls: InvokeCall[] = []
  const api: KernelAPI = {
    async invoke<T = unknown>(
      _pluginId: string,
      commandId: string,
      args?: unknown,
    ): Promise<T> {
      calls.push({ commandId, args })
      const r = responses[commandId]
      if (!r) return {} as T
      return r(args) as T
    },
    async on(): Promise<() => void> {
      return () => {}
    },
    async available() {
      return true
    },
  }
  return { api, calls }
}

function resetStore(): void {
  useEditorStore.setState({
    sessionRevision: new Map(),
    pendingLocalRevisions: new Set(),
  })
}

/**
 * Build a `ViewUpdate`-shaped object from two `EditorState`s and a
 * `ChangeSet`. Matches the subset of fields the bridge reads
 * (`docChanged`, `changes`, `startState`, `state`, `view`).
 */
function makeFakeUpdate(
  startState: EditorState,
  endState: EditorState,
  view: BridgeViewLike,
): import('@codemirror/view').ViewUpdate {
  const changes = startState.changes(
    // Rebuild the diff between the two doc strings via a single-change
    // spec covering the whole doc. This matches the kind of change
    // CM itself would record for a doc-replace dispatch.
    {
      from: 0,
      to: startState.doc.length,
      insert: endState.doc.toString(),
    },
  )
  return {
    docChanged: startState.doc.toString() !== endState.doc.toString(),
    changes,
    startState,
    state: endState,
    view: view as unknown as import('@codemirror/view').EditorView,
  } as unknown as import('@codemirror/view').ViewUpdate
}

/**
 * Build a minimal stub view that records dispatched changes. Enough
 * surface for the bridge's reconciliation path.
 */
function makeStubView(initialDoc: string): BridgeViewLike & {
  dispatched: Array<{ from: number; to: number; insert: string }>
  setDoc(s: string): void
} {
  let doc = initialDoc
  const dispatched: Array<{ from: number; to: number; insert: string }> = []
  return {
    state: {
      doc: {
        toString: () => doc,
      },
    },
    dispatch(spec) {
      dispatched.push(spec.changes)
      const before = doc
      doc =
        before.slice(0, spec.changes.from) +
        spec.changes.insert +
        before.slice(spec.changes.to)
    },
    dispatched,
    setDoc(s: string) {
      doc = s
    },
  }
}

// ── changesToOps ─────────────────────────────────────────────────────────────

test('changesToOps: single insert produces one insert_text op', () => {
  const start = EditorState.create({ doc: '' })
  const tr = start.update({ changes: { from: 0, to: 0, insert: 'hello' } })
  const update = {
    docChanged: true,
    changes: tr.changes,
    startState: start,
    state: tr.state,
    view: null,
  } as unknown as import('@codemirror/view').ViewUpdate

  const { ops, fallbackFullDoc } = changesToOps(update, ROOT_ID)
  assert.equal(fallbackFullDoc, false)
  assert.equal(ops.length, 1)
  const op = ops[0]!
  assert.equal(op.kind, 'insert_text')
  if (op.kind === 'insert_text') {
    assert.equal(op.block_id, ROOT_ID)
    assert.equal(op.pos, 0)
    assert.equal(op.text, 'hello')
  }
})

test('changesToOps: single delete produces one delete_text op', () => {
  const start = EditorState.create({ doc: 'hello' })
  const tr = start.update({ changes: { from: 1, to: 4, insert: '' } })
  const update = {
    docChanged: true,
    changes: tr.changes,
    startState: start,
    state: tr.state,
    view: null,
  } as unknown as import('@codemirror/view').ViewUpdate

  const { ops, fallbackFullDoc } = changesToOps(update, ROOT_ID)
  assert.equal(fallbackFullDoc, false)
  assert.equal(ops.length, 1)
  const op = ops[0]!
  assert.equal(op.kind, 'delete_text')
  if (op.kind === 'delete_text') {
    assert.equal(op.block_id, ROOT_ID)
    assert.equal(op.pos, 1)
    assert.equal(op.deleted_text, 'ell')
  }
})

test('changesToOps: replacement falls back to update_block_content', () => {
  const start = EditorState.create({ doc: 'hello' })
  const tr = start.update({ changes: { from: 0, to: 5, insert: 'WORLD' } })
  const update = {
    docChanged: true,
    changes: tr.changes,
    startState: start,
    state: tr.state,
    view: null,
  } as unknown as import('@codemirror/view').ViewUpdate

  const { ops, fallbackFullDoc } = changesToOps(update, ROOT_ID)
  assert.equal(fallbackFullDoc, true)
  assert.equal(ops.length, 1)
  const op = ops[0]!
  assert.equal(op.kind, 'update_block_content')
  if (op.kind === 'update_block_content') {
    assert.equal(op.id, ROOT_ID)
    assert.equal(op.old_content, 'hello')
    assert.equal(op.new_content, 'WORLD')
  }
})

// ── makeTransaction ──────────────────────────────────────────────────────────

test('makeTransaction: stamps id + metadata from options', () => {
  const ops: Operation[] = [
    {
      kind: 'insert_text',
      block_id: ROOT_ID,
      pos: 0,
      text: 'a',
      pre_annotations: [],
    },
  ]
  const tx = makeTransaction(ops, { source: 'user' })
  assert.equal(typeof tx.id, 'string')
  assert.ok(tx.id.length >= 16)
  assert.equal(tx.operations.length, 1)
  assert.equal(tx.metadata.source, 'user')
  assert.equal(tx.metadata.ai_edit, false)
  assert.deepEqual(tx.metadata.user_action, { kind: 'keystroke' })
})

test('makeTransaction: ai source marks ai_edit true', () => {
  const tx = makeTransaction([], { source: 'ai' })
  assert.equal(tx.metadata.source, 'ai')
  assert.equal(tx.metadata.ai_edit, true)
})

// ── Bridge core: five keystrokes batch into one transaction ─────────────────

test('bridge core: five keystrokes batch into ONE transaction with pending-id tracking', async () => {
  resetStore()
  const received: Transaction[] = []
  const { api } = makeMockApi({
    apply_transaction: (args) => {
      received.push((args as { transaction: Transaction }).transaction)
      return snapshotWithRoot('notes/a.md', 'hello')
    },
    get_markdown: () => 'hello',
  })
  const client = new EditorKernelClient(api)
  const snapshot = snapshotWithRoot('notes/a.md', '')
  const core = createBridgeCore({
    relpath: 'notes/a.md',
    kernelClient: client,
    getSnapshot: () => snapshot,
  })

  const view = makeStubView('')
  // Simulate five sequential keystrokes as five separate ViewUpdates
  // on the same rAF tick. Each carries a proper startState→state pair
  // from a chain of EditorState transactions.
  let st = EditorState.create({ doc: '' })
  for (const ch of 'hello') {
    const tr = st.update({ changes: { from: st.doc.length, to: st.doc.length, insert: ch } })
    const update = makeFakeUpdate(st, tr.state, view)
    core.push(update)
    st = tr.state
  }

  core.flushSync()
  // Let the async apply + getMarkdown promise chain resolve.
  await Promise.resolve()
  await Promise.resolve()
  await new Promise((r) => setTimeout(r, 0))
  await Promise.resolve()

  assert.equal(received.length, 1, 'five keystrokes coalesce into one transaction')
  const tx = received[0]!
  assert.equal(tx.operations.length, 1, 'multi-update batch collapses to one op')
  assert.equal(tx.operations[0]!.kind, 'update_block_content')

  // Pending-id was inserted before dispatch and is still there until
  // the Phase-4 echo consumes it.
  assert.equal(
    useEditorStore.getState().pendingLocalRevisions.has(tx.id),
    true,
    'pending revision id is held until the echo consumes it',
  )
  const consumed = useEditorStore.getState().consumePendingLocalRevision(tx.id)
  assert.equal(consumed, true, 'a matching echo is dropped by the pending set')
})

// ── Reconciliation from getMarkdown ──────────────────────────────────────────

test('bridge core: reconciles CM doc via getMarkdown after apply_transaction', async () => {
  resetStore()
  const { api } = makeMockApi({
    apply_transaction: () => ({
      ...snapshotWithRoot('notes/b.md', 'unused'),
      revision: 7,
    }),
    get_markdown: () => 'REMOTE CANONICAL',
  })
  const client = new EditorKernelClient(api)
  const snapshot = snapshotWithRoot('notes/b.md', 'old')
  const core = createBridgeCore({
    relpath: 'notes/b.md',
    kernelClient: client,
    getSnapshot: () => snapshot,
  })
  const view = makeStubView('oldX')

  const start = EditorState.create({ doc: 'old' })
  const tr = start.update({ changes: { from: 3, to: 3, insert: 'X' } })
  core.push(makeFakeUpdate(start, tr.state, view))
  core.flushSync()

  await Promise.resolve()
  await Promise.resolve()
  await new Promise((r) => setTimeout(r, 0))
  await Promise.resolve()

  assert.equal(view.dispatched.length, 1, 'bridge issued a reconciliation dispatch')
  assert.equal(view.dispatched[0]!.insert, 'REMOTE CANONICAL')
  // Revision was bumped in the store.
  assert.equal(
    useEditorStore.getState().sessionRevision.get('notes/b.md'),
    7,
    'session revision is synced from the apply response',
  )
})

// ── Undo via Ctrl-Z (through the kernel client) ──────────────────────────────

test('kernel undo path: client.undo round-trips and the view reflects canonical markdown', async () => {
  resetStore()
  let undoCalled = false
  const { api } = makeMockApi({
    undo: () => {
      undoCalled = true
      return snapshotWithRoot('notes/c.md', 'after-undo')
    },
    get_markdown: () => 'after-undo',
  })
  const client = new EditorKernelClient(api)

  // Mirror the work the extension's keymap `run` does: call undo,
  // fetch canonical markdown, replace the view's doc.
  const view = makeStubView('before-undo')
  const snap = await client.undo('notes/c.md')
  const canonical = await client.getMarkdown('notes/c.md')
  const current = view.state.doc.toString()
  view.dispatch({ changes: { from: 0, to: current.length, insert: canonical } })

  assert.equal(undoCalled, true)
  assert.equal(snap.relpath, 'notes/c.md')
  assert.equal(view.state.doc.toString(), 'after-undo')
})
