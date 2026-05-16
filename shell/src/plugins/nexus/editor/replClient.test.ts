// Unit tests for ReplClient. Mirrors the kernelClient.test.ts pattern
// — node:test + a hand-rolled mock KernelAPI; no Tauri required.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import type { KernelAPI } from '../../../types/plugin.ts'
import {
  makeReplClient,
  TERMINAL_PLUGIN_ID,
  type ReplStartResponse,
  type ReplInfo,
} from './replClient.ts'

interface InvokeCall {
  pluginId: string
  commandId: string
  args: unknown
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
    ): Promise<T> {
      calls.push({ pluginId, commandId, args })
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

test('ReplClient.start routes to com.nexus.terminal::repl_start with the supplied args', async () => {
  const response: ReplStartResponse = { id: 'session-1', lang: 'python' }
  const { api, calls } = makeMockApi(response)
  const client = makeReplClient(api)

  const got = await client.start({
    lang: 'python',
    program: 'python3',
    args: ['-iq'],
  })

  assert.equal(calls.length, 1)
  assert.equal(calls[0].pluginId, TERMINAL_PLUGIN_ID)
  assert.equal(calls[0].commandId, 'repl_start')
  assert.deepEqual(calls[0].args, {
    lang: 'python',
    program: 'python3',
    args: ['-iq'],
  })
  assert.deepEqual(got, response)
})

test('ReplClient.eval routes to repl_eval with id + code', async () => {
  const { api, calls } = makeMockApi(null)
  const client = makeReplClient(api)

  await client.eval('session-1', 'print(2+2)\n')

  assert.equal(calls.length, 1)
  assert.equal(calls[0].commandId, 'repl_eval')
  assert.deepEqual(calls[0].args, { id: 'session-1', code: 'print(2+2)\n' })
})

test('ReplClient.stop routes to repl_stop', async () => {
  const { api, calls } = makeMockApi(null)
  const client = makeReplClient(api)

  await client.stop('session-1')

  assert.equal(calls.length, 1)
  assert.equal(calls[0].commandId, 'repl_stop')
  assert.deepEqual(calls[0].args, { id: 'session-1' })
})

test('ReplClient.list returns the snapshot from repl_list', async () => {
  const snapshot: ReplInfo[] = [
    {
      id: 'a',
      lang: 'python',
      program: 'python3',
      args: ['-iq'],
      started_at_ms: 1_716_000_000_000,
    },
  ]
  const { api, calls } = makeMockApi(snapshot)
  const client = makeReplClient(api)

  const got = await client.list()

  assert.equal(calls.length, 1)
  assert.equal(calls[0].commandId, 'repl_list')
  assert.deepEqual(calls[0].args, {})
  assert.deepEqual(got, snapshot)
})

test('ReplClient.start forwards optional working_dir + env when supplied', async () => {
  const { api, calls } = makeMockApi({ id: 'x', lang: 'node' })
  const client = makeReplClient(api)

  await client.start({
    lang: 'node',
    program: 'node',
    args: ['--interactive'],
    working_dir: '/tmp/scratch',
    env: [['NODE_NO_WARNINGS', '1']],
  })

  assert.deepEqual(calls[0].args, {
    lang: 'node',
    program: 'node',
    args: ['--interactive'],
    working_dir: '/tmp/scratch',
    env: [['NODE_NO_WARNINGS', '1']],
  })
})
