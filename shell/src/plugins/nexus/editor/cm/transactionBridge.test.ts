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

// The bridge translation now needs a real markdown EditorState so the
// Lezer parse tree can find top-level blocks. These tests stand up a
// state with `markdown()` and verify position translation lands on the
// right block with the right byte offset.

import { markdown } from '@codemirror/lang-markdown'

function mdState(doc: string): EditorState {
  return EditorState.create({ doc, extensions: [markdown()] })
}

function snapshotForParagraph(content: string): EditorSnapshot {
  return snapshotWithRoot('notes/a.md', content)
}

function snapshotWithBlocks(
  relpath: string,
  blocks: Array<{ id: BlockId; content: string; kind: string; level?: number }>,
): EditorSnapshot {
  const tree: EditorSnapshot['tree'] = {
    blocks: {},
    root_blocks: [],
    metadata: {},
  }
  for (const b of blocks) {
    const ty: { kind: string; level?: number } = { kind: b.kind }
    if (b.level !== undefined) ty.level = b.level
    tree.blocks[b.id] = {
      id: b.id,
      ty,
      content: b.content,
      annotations: [],
      properties: {},
      parent_id: null,
      children: [],
      index_in_parent: tree.root_blocks.length,
      created_at: 0,
      updated_at: 0,
      is_deleted: false,
    }
    tree.root_blocks.push(b.id)
  }
  return {
    relpath,
    tree,
    undoPosition: null,
    undoLen: 0,
    canUndo: false,
    canRedo: false,
    revision: 0,
  }
}

test('changesToOps: insert into a paragraph emits insert_text against that block', () => {
  const start = mdState('hello')
  const tr = start.update({ changes: { from: 5, to: 5, insert: '!' } })
  const update = {
    docChanged: true,
    changes: tr.changes,
    startState: start,
    state: tr.state,
    view: null,
  } as unknown as import('@codemirror/view').ViewUpdate

  const { ops, fallbackFullDoc } = changesToOps(update, snapshotForParagraph('hello'))
  assert.equal(fallbackFullDoc, false)
  assert.equal(ops.length, 1)
  const op = ops[0]!
  assert.equal(op.kind, 'insert_text')
  if (op.kind === 'insert_text') {
    assert.equal(op.block_id, ROOT_ID)
    assert.equal(op.pos, 5)
    assert.equal(op.text, '!')
  }
})

test('changesToOps: insert at end of an H1 heading translates to block-local byte offset', () => {
  // Doc: `# Hello`. CM offset at line end = 7. The heading block's
  // content is "Hello" (5 bytes). The op pos MUST be 5, not 7.
  const start = mdState('# Hello')
  const tr = start.update({ changes: { from: 7, to: 7, insert: '!' } })
  const update = {
    docChanged: true,
    changes: tr.changes,
    startState: start,
    state: tr.state,
    view: null,
  } as unknown as import('@codemirror/view').ViewUpdate

  const snap = snapshotWithBlocks('notes/h.md', [
    { id: ROOT_ID, content: 'Hello', kind: 'heading', level: 1 },
  ])
  const { ops, fallbackFullDoc } = changesToOps(update, snap)
  assert.equal(fallbackFullDoc, false)
  assert.equal(ops.length, 1)
  const op = ops[0]!
  assert.equal(op.kind, 'insert_text')
  if (op.kind === 'insert_text') {
    assert.equal(op.block_id, ROOT_ID)
    assert.equal(op.pos, 5, 'CM offset 7 maps to byte offset 5 inside the heading content')
    assert.equal(op.text, '!')
  }
})

test('changesToOps: non-ASCII content uses UTF-8 byte offsets, not JS char offsets', () => {
  // `# café` — heading content is "café" (4 JS chars, 5 UTF-8 bytes).
  // CM offset at line end = 6. Heading content start in source = 2.
  // Substring source[2..6] = "café" = 5 bytes. Op pos MUST be 5.
  const start = mdState('# café')
  const tr = start.update({ changes: { from: 6, to: 6, insert: '!' } })
  const update = {
    docChanged: true,
    changes: tr.changes,
    startState: start,
    state: tr.state,
    view: null,
  } as unknown as import('@codemirror/view').ViewUpdate

  const snap = snapshotWithBlocks('notes/h.md', [
    { id: ROOT_ID, content: 'café', kind: 'heading', level: 1 },
  ])
  const { ops } = changesToOps(update, snap)
  assert.equal(ops.length, 1)
  const op = ops[0]!
  assert.equal(op.kind, 'insert_text')
  if (op.kind === 'insert_text') {
    assert.equal(op.pos, 5, 'UTF-8 byte offset reflects é = 2 bytes')
  }
})

test('changesToOps: insert in the second top-level block targets root_blocks[1]', () => {
  const SECOND_ID = 'bbbbbbbb-cccc-4ddd-8eee-ffffffffffff'
  // Doc:
  //   # Title
  //
  //   body text
  const doc = '# Title\n\nbody text'
  const start = mdState(doc)
  // Insert at end of "body text" (offset 18)
  const tr = start.update({ changes: { from: 18, to: 18, insert: '!' } })
  const update = {
    docChanged: true,
    changes: tr.changes,
    startState: start,
    state: tr.state,
    view: null,
  } as unknown as import('@codemirror/view').ViewUpdate

  const snap = snapshotWithBlocks('notes/m.md', [
    { id: ROOT_ID, content: 'Title', kind: 'heading', level: 1 },
    { id: SECOND_ID, content: 'body text', kind: 'paragraph' },
  ])
  const { ops, fallbackFullDoc } = changesToOps(update, snap)
  assert.equal(fallbackFullDoc, false)
  const op = ops[0]!
  assert.equal(op.kind, 'insert_text')
  if (op.kind === 'insert_text') {
    assert.equal(op.block_id, SECOND_ID, 'op targets the paragraph block, not root_blocks[0]')
    assert.equal(op.pos, 9, 'CM offset 18 maps to byte 9 inside "body text"')
  }
})

test('changesToOps: delete inside a heading translates both ends to byte offsets', () => {
  const start = mdState('# Hello')
  // Delete "ell" from inside heading: CM offsets 3..6
  const tr = start.update({ changes: { from: 3, to: 6, insert: '' } })
  const update = {
    docChanged: true,
    changes: tr.changes,
    startState: start,
    state: tr.state,
    view: null,
  } as unknown as import('@codemirror/view').ViewUpdate

  const snap = snapshotWithBlocks('notes/h.md', [
    { id: ROOT_ID, content: 'Hello', kind: 'heading', level: 1 },
  ])
  const { ops, fallbackFullDoc } = changesToOps(update, snap)
  assert.equal(fallbackFullDoc, false)
  const op = ops[0]!
  assert.equal(op.kind, 'delete_text')
  if (op.kind === 'delete_text') {
    assert.equal(op.block_id, ROOT_ID)
    assert.equal(op.pos, 1, 'CM offset 3 -> byte 1 inside "Hello"')
    assert.equal(op.deleted_text, 'ell')
  }
})

test('changesToOps: insert in a block with inline formatting bails to the full-doc fallback', () => {
  // Source: `# *Hi*`. Heading block content (per parser) is "Hi" — the
  // `*` marks are stripped into an Italic annotation. The source slice
  // at content start..end = "*Hi*" which differs from block.content =
  // "Hi", so resolveBlockPos returns null and we fall back.
  const start = mdState('# *Hi*')
  const tr = start.update({ changes: { from: 6, to: 6, insert: '!' } })
  const update = {
    docChanged: true,
    changes: tr.changes,
    startState: start,
    state: tr.state,
    view: null,
  } as unknown as import('@codemirror/view').ViewUpdate

  const snap = snapshotWithBlocks('notes/h.md', [
    { id: ROOT_ID, content: 'Hi', kind: 'heading', level: 1 },
  ])
  const { ops, fallbackFullDoc } = changesToOps(update, snap)
  assert.equal(fallbackFullDoc, true)
  const op = ops[0]!
  assert.equal(op.kind, 'update_block_content')
})

test('changesToOps: inserted newline bypasses single-block translation', () => {
  const start = mdState('hello')
  const tr = start.update({ changes: { from: 5, to: 5, insert: '\n' } })
  const update = {
    docChanged: true,
    changes: tr.changes,
    startState: start,
    state: tr.state,
    view: null,
  } as unknown as import('@codemirror/view').ViewUpdate

  const { ops, fallbackFullDoc } = changesToOps(update, snapshotForParagraph('hello'))
  assert.equal(fallbackFullDoc, true, 'newline insertion falls back, since it would split the block')
  const op = ops[0]!
  assert.equal(op.kind, 'update_block_content')
})

test('changesToOps: replacement falls back to update_block_content', () => {
  const start = mdState('hello')
  const tr = start.update({ changes: { from: 0, to: 5, insert: 'WORLD' } })
  const update = {
    docChanged: true,
    changes: tr.changes,
    startState: start,
    state: tr.state,
    view: null,
  } as unknown as import('@codemirror/view').ViewUpdate

  const { ops, fallbackFullDoc } = changesToOps(update, snapshotForParagraph('hello'))
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

// ── Reconcile defers while typing is in flight ───────────────────────────────

test('bridge core: reconcile skips doc-replace while a follow-up keystroke is pending', async () => {
  resetStore()
  // Two responses queued by call order. Both `apply_transaction`s
  // return the *same* canonical the first time we ask, then a
  // catch-up canonical that matches the second update — so the first
  // reconcile, if it ran, would clobber the second keystroke.
  let applyCount = 0
  let getCount = 0
  const { api } = makeMockApi({
    apply_transaction: () => {
      applyCount++
      return snapshotWithRoot('notes/d.md', 'old')
    },
    get_markdown: () => {
      getCount++
      // First reconcile would see "old" (lost the user's typing).
      // Second reconcile sees the up-to-date doc.
      return getCount === 1 ? 'old' : 'oldXY'
    },
  })
  const client = new EditorKernelClient(api)
  const snapshot = snapshotWithRoot('notes/d.md', 'old')
  const core = createBridgeCore({
    relpath: 'notes/d.md',
    kernelClient: client,
    getSnapshot: () => snapshot,
  })
  const view = makeStubView('oldX')

  // First keystroke: doc goes "old" → "oldX". Flush sends a transaction.
  let st = EditorState.create({ doc: 'old' })
  let tr = st.update({ changes: { from: 3, to: 3, insert: 'X' } })
  core.push(makeFakeUpdate(st, tr.state, view))
  st = tr.state
  core.flushSync()

  // While the first apply is still in flight, the user types another
  // character. The view + state advance to "oldXY"; the bridge queues
  // another update.
  view.setDoc('oldXY')
  tr = st.update({ changes: { from: 4, to: 4, insert: 'Y' } })
  core.push(makeFakeUpdate(st, tr.state, view))
  st = tr.state

  // Drain the first apply's promise chain. The reconcile call inside
  // it must see `pending.length > 0` and skip the doc-replace.
  await Promise.resolve()
  await Promise.resolve()
  await new Promise((r) => setTimeout(r, 0))
  await Promise.resolve()

  assert.equal(
    view.dispatched.length,
    0,
    'first reconcile defers because a follow-up keystroke is pending',
  )
  assert.equal(view.state.doc.toString(), 'oldXY', 'CM doc still has the user typing intact')

  // Flush the queued second update; its reconcile lands cleanly.
  core.flushSync()
  await Promise.resolve()
  await Promise.resolve()
  await new Promise((r) => setTimeout(r, 0))
  await Promise.resolve()

  assert.equal(applyCount, 2, 'both keystrokes were applied to the kernel')
  assert.equal(
    view.dispatched.length,
    0,
    'second reconcile is a no-op because CM already matches canonical',
  )
  assert.equal(view.state.doc.toString(), 'oldXY')
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
