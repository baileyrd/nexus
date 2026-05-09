// BL-074 — apply-resolution helper unit tests.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { applyUseRemote, buildUseRemoteTransaction, uuidV4 } from './applyResolution'
import type { ConflictDetail } from './types'
import type { PluginAPI } from '../../../types/plugin'

interface InvokeCall {
  pluginId: string
  command: string
  args: unknown
}

function fakeApi(invokeImpl: (call: InvokeCall) => Promise<unknown>): {
  api: PluginAPI
  calls: InvokeCall[]
} {
  const calls: InvokeCall[] = []
  const api = {
    kernel: {
      invoke: async (pluginId: string, command: string, args: unknown) => {
        const call = { pluginId, command, args }
        calls.push(call)
        return invokeImpl(call)
      },
    },
  } as unknown as PluginAPI
  return { api, calls }
}

test('uuidV4 produces a v4-shaped string', () => {
  const id = uuidV4()
  // 8-4-4-4-12 hex with a `4` in the version slot and 8/9/a/b in the variant slot.
  assert.match(id, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
})

test('buildUseRemoteTransaction produces a wire-shape UpdateBlockContent op', () => {
  const tx = buildUseRemoteTransaction('block-1', 'old', 'new', 'tx-id', 1700000000000)
  assert.deepStrictEqual(tx, {
    id: 'tx-id',
    operations: [
      {
        kind: 'update_block_content',
        id: 'block-1',
        old_content: 'old',
        new_content: 'new',
        old_annotations: [],
        new_annotations: [],
      },
    ],
    created_at: 1700000000000,
    metadata: {
      user_action: { kind: 'paste' },
      source: 'user',
      ai_edit: false,
    },
  })
})

test('applyUseRemote dispatches apply_transaction for a concurrent_block_edit', async () => {
  const { api, calls } = fakeApi(async () => ({ relpath: 'notes.md' }))
  const detail: ConflictDetail = {
    kind: 'concurrent_block_edit',
    block_id: 'block-1',
    local: { site: 'a', lamport: 1 },
    remote: { site: 'b', lamport: 2 },
    local_content: 'local',
    remote_content: 'remote',
  }
  const err = await applyUseRemote(api, 'notes.md', detail)
  assert.strictEqual(err, null)
  assert.strictEqual(calls.length, 1)
  assert.strictEqual(calls[0]?.pluginId, 'com.nexus.editor')
  assert.strictEqual(calls[0]?.command, 'apply_transaction')
  const args = calls[0]?.args as {
    relpath: string
    transaction: {
      operations: Array<{ old_content: string; new_content: string; id: string }>
    }
  }
  assert.strictEqual(args.relpath, 'notes.md')
  assert.strictEqual(args.transaction.operations[0]?.old_content, 'local')
  assert.strictEqual(args.transaction.operations[0]?.new_content, 'remote')
  assert.strictEqual(args.transaction.operations[0]?.id, 'block-1')
})

test('applyUseRemote returns the IPC error message when the call rejects', async () => {
  const { api, calls } = fakeApi(async () => {
    throw new Error('IPC blew up')
  })
  const detail: ConflictDetail = {
    kind: 'concurrent_block_edit',
    block_id: 'block-1',
    local: { site: 'a', lamport: 1 },
    remote: { site: 'b', lamport: 2 },
    local_content: 'L',
    remote_content: 'R',
  }
  const err = await applyUseRemote(api, 'notes.md', detail)
  assert.strictEqual(err, 'IPC blew up')
  assert.strictEqual(calls.length, 1, 'we still tried')
})

test('applyUseRemote refuses structural_delete_edit without dispatching', async () => {
  const { api, calls } = fakeApi(async () => null)
  const detail: ConflictDetail = {
    kind: 'structural_delete_edit',
    block_id: 'block-1',
    delete: { site: 'a', lamport: 1 },
    edit: { site: 'b', lamport: 2 },
    local_content: 'edit-content',
    delete_origin: 'remote',
  }
  const err = await applyUseRemote(api, 'notes.md', detail)
  assert.strictEqual(err, 'Only concurrent block edits can be auto-resolved.')
  assert.strictEqual(calls.length, 0, 'no IPC dispatched for delete-edit')
})

test('applyUseRemote refuses concurrent_block_edit when content snapshots are missing', async () => {
  // The Rust side only populates content for `UpdateBlockContent` /
  // `InsertBlock` payloads. For `UpdateAnnotations` the snapshots are
  // None — the helper must surface a clear error rather than dispatch
  // an op with `undefined` operands.
  const { api, calls } = fakeApi(async () => null)
  const detail: ConflictDetail = {
    kind: 'concurrent_block_edit',
    block_id: 'block-1',
    local: { site: 'a', lamport: 1 },
    remote: { site: 'b', lamport: 2 },
    // Both fields intentionally absent.
  }
  const err = await applyUseRemote(api, 'notes.md', detail)
  assert.strictEqual(err, 'Conflict payload is missing the content snapshots needed to resolve.')
  assert.strictEqual(calls.length, 0)
})
