// C35 (#388) — "forget this memory" surface: decode the `id` field the
// dashboard needs to target update/delete, and dispatch the right IPC verb
// with the right args. Modal-interaction wiring (which pick option the user
// chose) mirrors files/index.ts's confirm-delete flow, which the codebase
// leaves untested; these are the argument-shaping units that flow does call.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { decodeMemories, forgetMemory, reclassifyMemory, updateMemoryContent } from './index'
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

test('decodeMemories carries the id field through alongside existing fields', () => {
  const rows = decodeMemories([
    { id: 'abc-123', content: 'hello', category: 'general' },
    { content: 'no id here' },
    { id: 42, content: 'non-string id is dropped' },
  ])
  assert.strictEqual(rows.length, 3)
  assert.strictEqual(rows[0]?.id, 'abc-123')
  assert.strictEqual(rows[0]?.content, 'hello')
  assert.strictEqual(rows[1]?.id, undefined)
  assert.strictEqual(rows[2]?.id, undefined)
})

test('decodeMemories tolerates non-array input', () => {
  assert.deepStrictEqual(decodeMemories(null), [])
  assert.deepStrictEqual(decodeMemories({ not: 'an array' }), [])
})

test('forgetMemory dispatches delete with the memory id and resolves null on success', async () => {
  const { api, calls } = fakeApi(async () => ({ deleted: true }))
  const err = await forgetMemory(api, 'mem-1')
  assert.strictEqual(err, null)
  assert.strictEqual(calls.length, 1)
  assert.strictEqual(calls[0]?.pluginId, 'com.nexus.memory')
  assert.strictEqual(calls[0]?.command, 'delete')
  assert.deepStrictEqual(calls[0]?.args, { id: 'mem-1' })
})

test('forgetMemory surfaces a message when the store reports deleted: false', async () => {
  const { api } = fakeApi(async () => ({ deleted: false }))
  const err = await forgetMemory(api, 'mem-missing')
  assert.strictEqual(err, 'Memory was already gone.')
})

test('forgetMemory returns the IPC error message when the call rejects', async () => {
  const { api } = fakeApi(async () => {
    throw new Error('ipc down')
  })
  const err = await forgetMemory(api, 'mem-1')
  assert.strictEqual(err, 'Error: ipc down')
})

test('updateMemoryContent dispatches update with id + content and resolves null on success', async () => {
  const { api, calls } = fakeApi(async () => ({ updated: true }))
  const err = await updateMemoryContent(api, 'mem-1', 'new content')
  assert.strictEqual(err, null)
  assert.strictEqual(calls.length, 1)
  assert.strictEqual(calls[0]?.command, 'update')
  assert.deepStrictEqual(calls[0]?.args, { id: 'mem-1', content: 'new content' })
})

test('updateMemoryContent surfaces a message when the store reports updated: false', async () => {
  const { api } = fakeApi(async () => ({ updated: false }))
  const err = await updateMemoryContent(api, 'mem-missing', 'x')
  assert.strictEqual(err, 'Memory was not found.')
})

test('updateMemoryContent returns the IPC error message when the call rejects', async () => {
  const { api } = fakeApi(async () => {
    throw new Error('ipc down')
  })
  const err = await updateMemoryContent(api, 'mem-1', 'x')
  assert.strictEqual(err, 'Error: ipc down')
})

// C41 / #394 — reclassify: same argument-shaping unit as forget/edit above.

test('reclassifyMemory dispatches update with id + memory_type and resolves null on success', async () => {
  const { api, calls } = fakeApi(async () => ({ updated: true }))
  const err = await reclassifyMemory(api, 'mem-1', 'semantic')
  assert.strictEqual(err, null)
  assert.strictEqual(calls.length, 1)
  assert.strictEqual(calls[0]?.pluginId, 'com.nexus.memory')
  assert.strictEqual(calls[0]?.command, 'update')
  assert.deepStrictEqual(calls[0]?.args, { id: 'mem-1', memory_type: 'semantic' })
})

test('reclassifyMemory surfaces a message when the store reports updated: false', async () => {
  const { api } = fakeApi(async () => ({ updated: false }))
  const err = await reclassifyMemory(api, 'mem-missing', 'procedural')
  assert.strictEqual(err, 'Memory was not found.')
})

test('reclassifyMemory returns the IPC error message when the call rejects', async () => {
  const { api } = fakeApi(async () => {
    throw new Error('ipc down')
  })
  const err = await reclassifyMemory(api, 'mem-1', 'episodic')
  assert.strictEqual(err, 'Error: ipc down')
})
