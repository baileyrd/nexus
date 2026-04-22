// Unit tests for EditorKernelClient. Uses node:test (same pattern as the
// workspace tests) to avoid adding a dev dependency.
//
// Run with: node --experimental-strip-types --test \
//   src/plugins/nexus/editor/kernelClient.test.ts

import type { KernelAPI } from '../../../types/plugin.ts'
import type { EditorSnapshot, Transaction } from './types.ts'
import { EDITOR_PLUGIN_ID, makeEditorClient } from './kernelClient.ts'

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
  timeoutMs: number | undefined
}

function makeMockApi(returnValue: unknown): {
  api: KernelAPI
  calls: InvokeCall[]
} {
  const calls: InvokeCall[] = []
  const api: KernelAPI = {
    async invoke<T = unknown>(
      pluginId: string,
      commandId: string,
      args?: unknown,
      timeoutMs?: number,
    ): Promise<T> {
      calls.push({ pluginId, commandId, args, timeoutMs })
      return returnValue as T
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

function emptySnapshot(relpath: string): EditorSnapshot {
  return {
    relpath,
    tree: { blocks: {}, root_blocks: [], metadata: {} },
    undoPosition: 0,
    undoLen: 1,
    canUndo: true,
    canRedo: false,
    revision: 1,
  }
}

// ── tests ────────────────────────────────────────────────────────────────────

test('applyTransaction routes to the editor plugin with the right command + args shape', async () => {
  const relpath = 'notes/a.md'
  const expected = emptySnapshot(relpath)
  const { api, calls } = makeMockApi(expected)
  const client = makeEditorClient(api)

  const tx: Transaction = {
    id: '00000000-0000-4000-8000-000000000001',
    operations: [
      {
        kind: 'insert_text',
        block_id: '11111111-1111-4111-8111-111111111111',
        pos: 5,
        text: ' world',
        pre_annotations: [],
      },
    ],
    created_at: 1_700_000_000_000,
    metadata: {
      user_action: { kind: 'keystroke' },
      source: 'user',
      ai_edit: false,
    },
  }

  const snap = await client.applyTransaction(relpath, tx)

  // The mock received exactly one invocation with the expected shape.
  assert.equal(calls.length, 1)
  const call = calls[0]
  assert.equal(call.pluginId, EDITOR_PLUGIN_ID)
  assert.equal(call.commandId, 'apply_transaction')
  // args must be `{ relpath, transaction }` with the transaction threaded
  // through verbatim — the kernel deserializes it with serde, so the TS
  // wire shape (snake_case + `kind` discriminator) must match Rust.
  assert.deepEqual(call.args, { relpath, transaction: tx })

  // Snapshot is threaded through unchanged.
  assert.deepEqual(snap, expected)
})

test('getMarkdown routes to get_markdown and returns the raw string payload', async () => {
  const relpath = 'notes/c.md'
  const expected = '# Hello\n\nbody\n'
  const { api, calls } = makeMockApi(expected)
  const client = makeEditorClient(api)

  const md = await client.getMarkdown(relpath)

  assert.equal(calls.length, 1)
  const call = calls[0]
  assert.equal(call.pluginId, EDITOR_PLUGIN_ID)
  assert.equal(call.commandId, 'get_markdown')
  assert.deepEqual(call.args, { relpath })
  assert.equal(md, expected)
})

test('openSession / getTree / save / undo / redo / close use the documented command strings', async () => {
  const relpath = 'notes/b.md'
  const snap = emptySnapshot(relpath)
  const { api, calls } = makeMockApi(snap)
  const client = makeEditorClient(api)

  await client.openSession(relpath)
  await client.getTree(relpath)
  await client.undo(relpath)
  await client.redo(relpath)
  await client.saveSession(relpath)
  await client.closeSession(relpath)

  const cmds = calls.map((c) => c.commandId)
  assert.deepEqual(cmds, ['open', 'get_tree', 'undo', 'redo', 'save', 'close'])
  for (const call of calls) {
    assert.equal(call.pluginId, EDITOR_PLUGIN_ID)
    assert.deepEqual(call.args, { relpath })
  }
})
