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

import { test } from 'node:test'
import assert from 'node:assert/strict'

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

test('changesToOps: insert in a block with inline formatting bails (no ops emitted)', () => {
  // Source: `# *Hi*`. Heading block content (per parser) is "Hi" — the
  // `*` marks are stripped into an Italic annotation. The source slice
  // at content start..end = "*Hi*" which differs from block.content =
  // "Hi", so resolveBlockPos returns null. The previous coarse
  // fallback (UpdateBlockContent with the whole doc) corrupted the
  // kernel; we now skip the op and let a later reconcile catch CM up.
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
  assert.equal(fallbackFullDoc, true, 'translation bailed')
  assert.equal(ops.length, 0, 'no op emitted — the broken fallback was removed')
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
  assert.equal(ops.length, 0)
})

test('changesToOps: replacement falls back (no ops emitted)', () => {
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
  assert.equal(ops.length, 0, 'replacement (delete + insert) is not a pure insert or delete')
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

// Build a ViewUpdate-shaped object from a CM transaction's real
// ChangeSet (not a synthetic whole-doc replace). The bridge core tests
// need this so `changesToOps` sees the precise per-keystroke change
// segments that translation depends on.
function realUpdate(
  startState: EditorState,
  tr: ReturnType<EditorState['update']>,
  view: BridgeViewLike,
): import('@codemirror/view').ViewUpdate {
  return {
    docChanged: startState.doc.toString() !== tr.state.doc.toString(),
    changes: tr.changes,
    startState,
    state: tr.state,
    view: view as unknown as import('@codemirror/view').EditorView,
  } as unknown as import('@codemirror/view').ViewUpdate
}

// ── Bridge core: five keystrokes batch into one transaction ─────────────────

test('bridge core: five keystrokes batch into ONE transaction with composed op', async () => {
  resetStore()
  const received: Transaction[] = []
  const { api } = makeMockApi({
    apply_transaction: (args) => {
      received.push((args as { transaction: Transaction }).transaction)
      // BL-123: InsertText is text-only → slim response.
      return { kind: 'slim', revision: 1 }
    },
    get_markdown: () => 'abcdef',
  })
  const client = new EditorKernelClient(api)
  const snapshot = snapshotForParagraph('a')
  const core = createBridgeCore({
    relpath: 'notes/a.md',
    kernelClient: client,
    getSnapshot: () => snapshot,
  })

  const view = makeStubView('a')
  // Start with non-empty paragraph (Lezer doesn't emit a Paragraph
  // node for an empty doc, so the translator can't pair anything to
  // root_blocks[0] in that state). Type five chars at the end.
  let st = mdState('a')
  for (const ch of 'bcdef') {
    const tr = st.update({ changes: { from: st.doc.length, to: st.doc.length, insert: ch } })
    core.push(realUpdate(st, tr, view))
    st = tr.state
  }

  core.flushSync()
  await Promise.resolve()
  await Promise.resolve()
  await new Promise((r) => setTimeout(r, 0))
  await Promise.resolve()

  assert.equal(received.length, 1, 'five keystrokes coalesce into one transaction')
  const tx = received[0]!
  assert.equal(tx.operations.length, 1, 'composed change set yields one op')
  const op = tx.operations[0]!
  assert.equal(op.kind, 'insert_text', 'translated to InsertText against the paragraph block')
  if (op.kind === 'insert_text') {
    assert.equal(op.block_id, ROOT_ID)
    assert.equal(op.pos, 1)
    assert.equal(op.text, 'bcdef')
  }

  assert.equal(
    useEditorStore.getState().pendingLocalRevisions.has(tx.id),
    true,
    'pending revision id is held until the echo consumes it',
  )
  const consumed = useEditorStore.getState().consumePendingLocalRevision(tx.id)
  assert.equal(consumed, true, 'a matching echo is dropped by the pending set')
})

// ── Text-only ops skip the getMarkdown reconcile round-trip ──────────────────

test('bridge core: text-only ops skip getMarkdown to halve per-keystroke IPC cost', async () => {
  resetStore()
  let getMarkdownCalls = 0
  const { api } = makeMockApi({
    // BL-123: InsertText is text-only → slim response.
    apply_transaction: () => ({ kind: 'slim', revision: 7 }),
    get_markdown: () => {
      getMarkdownCalls++
      return 'oldX'
    },
  })
  const client = new EditorKernelClient(api)
  const snapshot = snapshotForParagraph('old')
  const core = createBridgeCore({
    relpath: 'notes/b.md',
    kernelClient: client,
    getSnapshot: () => snapshot,
  })
  const view = makeStubView('oldX')

  const start = mdState('old')
  const tr = start.update({ changes: { from: 3, to: 3, insert: 'X' } })
  core.push(realUpdate(start, tr, view))
  core.flushSync()

  await Promise.resolve()
  await Promise.resolve()
  await new Promise((r) => setTimeout(r, 0))
  await Promise.resolve()

  assert.equal(getMarkdownCalls, 0, 'no getMarkdown round-trip for a pure InsertText op')
  assert.equal(view.dispatched.length, 0, 'no doc-replace dispatch')
  assert.equal(
    useEditorStore.getState().sessionRevision.get('notes/b.md'),
    7,
    'session revision still synced from the apply response',
  )
})

// ── BL-123: bridge coherency on both response shapes ────────────────────────

test('bridge core: slim response (text-only op) does NOT call setSnapshot', async () => {
  // BL-123: text-only ops (InsertText / DeleteText) get a slim
  // response carrying just the new revision. The bridge skips
  // setSnapshot in that case — the cached snapshot stays at its
  // pre-tx contents, but block IDs and structure are unchanged for
  // text-only ops so downstream consumers (drag-bridge, comments)
  // still see correct structure. Stale text content doesn't matter
  // because those consumers either re-read the tree on demand or use
  // `getTree` directly. Drops the snapshot-clone cost from the
  // kernel's hot path (BL-122 baseline: 39 → 24190 µs p50 across
  // 10 / 5000-block docs).
  resetStore()
  let setSnapshotCalls = 0
  const initialSnap = snapshotForParagraph('hello')
  let cached = initialSnap
  const { api } = makeMockApi({
    apply_transaction: () => ({ kind: 'slim', revision: 9 }),
    get_markdown: () => 'hello!',
  })
  const client = new EditorKernelClient(api)
  const view = makeStubView('hello')
  const core = createBridgeCore({
    relpath: 'notes/s-slim.md',
    kernelClient: client,
    getSnapshot: () => cached,
    setSnapshot: (snap) => {
      setSnapshotCalls++
      cached = snap
    },
  })

  const start = mdState('hello')
  const tr = start.update({ changes: { from: 5, to: 5, insert: '!' } })
  core.push(realUpdate(start, tr, view))
  core.flushSync()

  await Promise.resolve()
  await Promise.resolve()
  await new Promise((r) => setTimeout(r, 0))
  await Promise.resolve()

  assert.equal(setSnapshotCalls, 0, 'slim path skips setSnapshot')
  assert.equal(
    cached,
    initialSnap,
    'cached snapshot still references the pre-tx object',
  )
  assert.equal(
    useEditorStore.getState().sessionRevision.get('notes/s-slim.md'),
    9,
    'slim revision propagates to the session-revision store',
  )
})

test('bridge core: full response (structural op) calls setSnapshot with the post-apply tree', async () => {
  // BL-123: structural ops (everything other than InsertText /
  // DeleteText) still get a full response carrying the post-apply
  // snapshot, and the bridge still pushes that snapshot back to
  // sessionManager so external consumers see fresh data. This test
  // wires a UpdateBlockContent-style structural op (the same op kind
  // the CRDT-conflict resolver dispatches) and asserts the full
  // path's behaviour is unchanged from pre-BL-123.
  resetStore()
  let setSnapshotCalls = 0
  const postApplySnap = snapshotForParagraph('hello!')
  const { api } = makeMockApi({
    apply_transaction: () => ({ kind: 'full', ...postApplySnap }),
    get_markdown: () => 'hello!',
  })
  const client = new EditorKernelClient(api)
  let cached = snapshotForParagraph('hello')
  const view = makeStubView('hello')
  const core = createBridgeCore({
    relpath: 'notes/s-full.md',
    kernelClient: client,
    getSnapshot: () => cached,
    setSnapshot: (snap) => {
      setSnapshotCalls++
      cached = snap
    },
  })

  // Drive a transaction directly through the dispatch chain instead
  // of the CM translator — the translator only emits text-only ops
  // (insert_text / delete_text / update_annotations), so a structural
  // op can't be triggered by a single keystroke alone.
  await client.applyTransaction('notes/s-full.md', {
    id: '00000000-0000-4000-8000-0000000000ff',
    operations: [
      {
        kind: 'update_block_content',
        id: ROOT_ID,
        old_content: 'hello',
        new_content: 'hello!',
        old_annotations: [],
        new_annotations: [],
      },
    ],
    created_at: 0,
    metadata: {
      user_action: { kind: 'paste' },
      source: 'user',
      ai_edit: false,
    },
  })
  // Drive a no-op CM keystroke through the bridge so its dispatch
  // chain runs and we can verify the bridge handles full responses
  // on the same code path.
  const start = mdState('hello')
  const tr = start.update({ changes: { from: 5, to: 5, insert: '!' } })
  core.push(realUpdate(start, tr, view))
  core.flushSync()

  await Promise.resolve()
  await Promise.resolve()
  await new Promise((r) => setTimeout(r, 0))
  await Promise.resolve()

  assert.ok(setSnapshotCalls >= 1, 'full path calls setSnapshot')
  assert.equal(
    cached.tree.blocks[ROOT_ID]?.content,
    'hello!',
    'cached snapshot reflects the post-apply block content',
  )
  assert.ok(
    !('kind' in cached),
    'setSnapshot receives a clean EditorSnapshot — discriminator stripped',
  )
})

// ── In-flight gate sequences keystrokes correctly ────────────────────────────

test('bridge core: a keystroke arriving during an in-flight apply waits + drains after ack', async () => {
  resetStore()
  let applyCount = 0
  let snap = snapshotForParagraph('old')
  let kernelRevision = snap.revision
  const { api } = makeMockApi({
    apply_transaction: (args) => {
      applyCount++
      // Advance the kernel-side snapshot to mirror what the real
      // kernel would do — the second keystroke's translation needs a
      // post-apply view of the block content. Even though BL-123
      // returns slim for the InsertText path, the mock still keeps
      // its own local mutation of `snap` so its `getSnapshot` reflects
      // the post-op state for the bridge's lazy mirror init.
      const op = (args as { transaction: Transaction }).transaction.operations[0]
      if (op?.kind === 'insert_text') {
        const block = snap.tree.blocks[op.block_id]!
        const next = block.content.slice(0, op.pos) + op.text + block.content.slice(op.pos)
        snap = {
          ...snap,
          tree: {
            ...snap.tree,
            blocks: {
              ...snap.tree.blocks,
              [op.block_id]: { ...block, content: next },
            },
          },
        }
      }
      kernelRevision += 1
      // BL-123: InsertText is text-only → slim response.
      return { kind: 'slim', revision: kernelRevision }
    },
  })
  const client = new EditorKernelClient(api)
  const view = makeStubView('oldX')
  const core = createBridgeCore({
    relpath: 'notes/d.md',
    kernelClient: client,
    getSnapshot: () => snap,
    setSnapshot: (s) => {
      snap = s
    },
  })

  // First keystroke: doc goes "old" → "oldX". Flush sends a transaction.
  let st = mdState('old')
  let tr = st.update({ changes: { from: 3, to: 3, insert: 'X' } })
  core.push(realUpdate(st, tr, view))
  st = tr.state
  core.flushSync()

  // While the first apply is still in flight, the user types another
  // character. The bridge advances its optimistic mirror by op1
  // immediately, so the second flush translates against post-op state
  // without waiting for the kernel ack.
  view.setDoc('oldXY')
  tr = st.update({ changes: { from: 4, to: 4, insert: 'Y' } })
  core.push(realUpdate(st, tr, view))
  st = tr.state

  // Drain the first apply's promise chain. The post-apply handler
  // drains the queued second update synchronously. The wait is
  // bounded by an applyCount check rather than a fixed microtask
  // burst so the test doesn't flake on slower CI runners (GHA's
  // ubuntu-latest has been observed taking >4 microtasks between
  // ack and queued-flush; see #244 follow-up). A 2-second deadline
  // is generous — under load the queue normally drains in <5 ms.
  const deadline = Date.now() + 2000
  while (applyCount < 2 && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 0))
  }

  assert.equal(applyCount, 2, 'both keystrokes were applied to the kernel')
  assert.equal(view.state.doc.toString(), 'oldXY', 'CM doc preserved through the round-trip')
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
