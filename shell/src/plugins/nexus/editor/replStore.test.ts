// BL-142 Phase 2a — unit tests for the per-tab REPL session store.
// Uses a hand-rolled ReplClient stub instead of mocking the
// underlying KernelAPI so the tests exercise the store's state
// machine without re-testing client transport plumbing (covered
// by replClient.test.ts).

import { test, beforeEach } from 'node:test'
import assert from 'node:assert/strict'

import {
  _resetReplStoreForTests,
  useReplStore,
  type ReplSessionStatus,
} from './replStore.ts'
import type { ReplClient, ReplStartArgs } from './replClient.ts'

interface StubCall {
  kind: 'start' | 'eval' | 'stop'
  payload: unknown
}

interface StubBehaviour {
  startResult?: { id: string; lang: string }
  startError?: string
  evalError?: string
  stopError?: string
}

function stubClient(behaviour: StubBehaviour = {}): {
  client: ReplClient
  calls: StubCall[]
} {
  const calls: StubCall[] = []
  let nextId = 1
  const client = {
    async start(args: ReplStartArgs) {
      calls.push({ kind: 'start', payload: args })
      if (behaviour.startError) throw new Error(behaviour.startError)
      return (
        behaviour.startResult ?? {
          id: `session-${nextId++}`,
          lang: args.lang,
        }
      )
    },
    async eval(id: string, code: string) {
      calls.push({ kind: 'eval', payload: { id, code } })
      if (behaviour.evalError) throw new Error(behaviour.evalError)
    },
    async stop(id: string) {
      calls.push({ kind: 'stop', payload: { id } })
      if (behaviour.stopError) throw new Error(behaviour.stopError)
    },
    async list() {
      return []
    },
  } as ReplClient
  return { client, calls }
}

const KERNELS_PY = '{"python":"python3 -i"}'

beforeEach(() => {
  _resetReplStoreForTests()
})

test('ensureSession spawns a kernel on first call for a (relpath, lang) pair', async () => {
  const { client, calls } = stubClient()
  const id = await useReplStore
    .getState()
    .ensureSession(client, KERNELS_PY, 'a.md', 'python')

  assert.equal(id, 'session-1')
  assert.equal(calls.length, 1)
  assert.equal(calls[0].kind, 'start')
  assert.deepEqual(calls[0].payload, {
    lang: 'python',
    program: 'python3',
    args: ['-i'],
  })

  const entry = useReplStore.getState().sessions['a.md::python']
  assert.equal(entry.status, 'ready')
  assert.equal(entry.sessionId, 'session-1')
})

test('ensureSession reuses an existing session on subsequent calls', async () => {
  const { client, calls } = stubClient()
  const store = useReplStore.getState()

  const first = await store.ensureSession(client, KERNELS_PY, 'a.md', 'python')
  const second = await store.ensureSession(client, KERNELS_PY, 'a.md', 'python')

  assert.equal(first, second)
  assert.equal(
    calls.filter((c) => c.kind === 'start').length,
    1,
    'kernel should be spawned at most once per (relpath, lang)',
  )
})

test('ensureSession returns null when no kernel is configured for lang', async () => {
  const { client, calls } = stubClient()
  const id = await useReplStore
    .getState()
    .ensureSession(client, KERNELS_PY, 'a.md', 'ruby')

  assert.equal(id, null)
  assert.equal(calls.length, 0, 'no spawn attempt without a configured kernel')
  assert.equal(
    useReplStore.getState().sessions['a.md::ruby'],
    undefined,
    'no session entry is written for an unsupported lang',
  )
})

test('ensureSession records error status when client.start throws', async () => {
  const { client } = stubClient({ startError: 'process.spawn denied' })
  const id = await useReplStore
    .getState()
    .ensureSession(client, KERNELS_PY, 'a.md', 'python')

  assert.equal(id, null)
  const entry = useReplStore.getState().sessions['a.md::python']
  assert.equal<ReplSessionStatus>(entry.status, 'error')
  assert.match(entry.error ?? '', /process\.spawn denied/)
})

test('different relpaths get different sessions for the same lang', async () => {
  const { client, calls } = stubClient()
  const store = useReplStore.getState()

  const idA = await store.ensureSession(client, KERNELS_PY, 'a.md', 'python')
  const idB = await store.ensureSession(client, KERNELS_PY, 'b.md', 'python')

  assert.notEqual(idA, idB)
  assert.equal(calls.filter((c) => c.kind === 'start').length, 2)
})

test('evalCode spawns + evals in a single call', async () => {
  const { client, calls } = stubClient()
  const ok = await useReplStore
    .getState()
    .evalCode(client, KERNELS_PY, 'a.md', 'python', 'print(2+2)\n')

  assert.equal(ok, true)
  assert.equal(calls.length, 2)
  assert.equal(calls[0].kind, 'start')
  assert.equal(calls[1].kind, 'eval')
  assert.deepEqual(calls[1].payload, {
    id: 'session-1',
    code: 'print(2+2)\n',
  })
})

test('evalCode reuses the session across multiple evals', async () => {
  const { client, calls } = stubClient()
  const store = useReplStore.getState()

  await store.evalCode(client, KERNELS_PY, 'a.md', 'python', 'x = 1\n')
  await store.evalCode(client, KERNELS_PY, 'a.md', 'python', 'print(x)\n')

  assert.equal(calls.filter((c) => c.kind === 'start').length, 1)
  assert.equal(calls.filter((c) => c.kind === 'eval').length, 2)
})

test('evalCode returns false when no kernel is configured', async () => {
  const { client, calls } = stubClient()
  const ok = await useReplStore
    .getState()
    .evalCode(client, KERNELS_PY, 'a.md', 'ruby', 'puts 1\n')

  assert.equal(ok, false)
  assert.equal(calls.length, 0)
})

test('stopForTab tears down every session tagged to the relpath', async () => {
  const { client, calls } = stubClient()
  const store = useReplStore.getState()

  // Two REPLs on a.md (different langs), one on b.md
  const kernels = '{"python":"python3 -i","node":"node -i"}'
  await store.ensureSession(client, kernels, 'a.md', 'python')
  await store.ensureSession(client, kernels, 'a.md', 'node')
  await store.ensureSession(client, kernels, 'b.md', 'python')

  await store.stopForTab(client, 'a.md')

  const stops = calls.filter((c) => c.kind === 'stop')
  assert.equal(stops.length, 2, 'both a.md sessions should be stopped')
  assert.equal(
    useReplStore.getState().sessions['a.md::python'],
    undefined,
  )
  assert.equal(useReplStore.getState().sessions['a.md::node'], undefined)
  assert.notEqual(
    useReplStore.getState().sessions['b.md::python'],
    undefined,
    'unrelated sessions on other tabs survive',
  )
})

test('stopForTab tolerates client.stop errors and still drops the store entry', async () => {
  const { client } = stubClient({ stopError: 'router stopped' })
  const store = useReplStore.getState()

  await store.ensureSession(client, KERNELS_PY, 'a.md', 'python')
  await store.stopForTab(client, 'a.md')

  assert.equal(
    useReplStore.getState().sessions['a.md::python'],
    undefined,
    'store entry is cleared even when the backend stop errored',
  )
})

test('stopAll clears every active session', async () => {
  const { client, calls } = stubClient()
  const store = useReplStore.getState()
  const kernels = '{"python":"python3 -i","node":"node -i"}'

  await store.ensureSession(client, kernels, 'a.md', 'python')
  await store.ensureSession(client, kernels, 'b.md', 'node')

  await store.stopAll(client)

  assert.equal(calls.filter((c) => c.kind === 'stop').length, 2)
  assert.deepEqual(useReplStore.getState().sessions, {})
})
