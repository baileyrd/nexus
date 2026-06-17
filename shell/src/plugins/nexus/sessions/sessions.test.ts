// shell/src/plugins/nexus/sessions/sessions.test.ts
//
// Tests for the session-tree navigator (RFC 0008, Phase 5.4):
//   - decodeSessionNodes / buildForest / flattenForest (pure)
//   - decodeCheckpoints
//   - the runtime fork verbs over a stub kernel (command + args + store)

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  buildForest,
  decodeSessionNodes,
  flattenForest,
  type SessionNode,
} from './sessionTree.ts'
import { createSessionsRuntime } from './sessionsRuntime.ts'
import { decodeCheckpoints, useSessionsStore } from './sessionsStore.ts'

function node(id: string, parentId: string | null, startedAt: string): SessionNode {
  return {
    id,
    goal: `goal ${id}`,
    outcome: 'complete',
    startedAt,
    endedAt: startedAt,
    parentId,
    branchPoint: parentId ? 2 : null,
  }
}

// ── Pure forest logic ───────────────────────────────────────────────────────

test('decodeSessionNodes parses tree linkage and skips junk', () => {
  const raw = [
    { id: 'a', goal: 'g', outcome: 'complete', started_at: '1', ended_at: '2' },
    { id: 'b', parent_id: 'a', branch_point: 3, started_at: '4' },
    { id: 42 }, // bad id
    'nope', // not an object
  ]
  const nodes = decodeSessionNodes(raw)
  assert.equal(nodes.length, 2)
  assert.equal(nodes[0].parentId, null)
  assert.equal(nodes[1].parentId, 'a')
  assert.equal(nodes[1].branchPoint, 3)
})

test('decodeSessionNodes tolerates non-arrays', () => {
  assert.deepEqual(decodeSessionNodes(null), [])
  assert.deepEqual(decodeSessionNodes({}), [])
})

test('buildForest nests children under parents and assigns depth', () => {
  // root a → child b → grandchild c; plus a sibling branch d off a.
  const nodes = [
    node('a', null, '2026-01-01'),
    node('b', 'a', '2026-01-02'),
    node('c', 'b', '2026-01-03'),
    node('d', 'a', '2026-01-04'),
  ]
  const forest = buildForest(nodes)
  assert.equal(forest.length, 1)
  const a = forest[0]
  assert.equal(a.id, 'a')
  assert.equal(a.depth, 0)
  // children sorted oldest-first: b before d.
  assert.deepEqual(a.children.map((c) => c.id), ['b', 'd'])
  assert.equal(a.children[0].depth, 1)
  assert.equal(a.children[0].children[0].id, 'c')
  assert.equal(a.children[0].children[0].depth, 2)
})

test('buildForest treats an orphan (missing parent) as a root', () => {
  const forest = buildForest([node('orphan', 'gone', '2026-01-01')])
  assert.equal(forest.length, 1)
  assert.equal(forest[0].id, 'orphan')
  assert.equal(forest[0].depth, 0)
})

test('buildForest orders roots newest-first', () => {
  const forest = buildForest([
    node('old', null, '2026-01-01'),
    node('new', null, '2026-02-01'),
  ])
  assert.deepEqual(forest.map((r) => r.id), ['new', 'old'])
})

test('buildForest breaks a cycle instead of looping forever', () => {
  // a↔b cycle (corrupt data) — both surface as roots, no infinite loop.
  const a = node('a', 'b', '2026-01-01')
  const b = node('b', 'a', '2026-01-02')
  const forest = buildForest([a, b])
  const ids = flattenForest(forest).map((n) => n.id).sort()
  assert.deepEqual(ids, ['a', 'b'])
})

test('flattenForest yields pre-order with the root first', () => {
  const forest = buildForest([
    node('a', null, '2026-01-01'),
    node('b', 'a', '2026-01-02'),
    node('c', 'b', '2026-01-03'),
  ])
  assert.deepEqual(flattenForest(forest).map((n) => n.id), ['a', 'b', 'c'])
})

test('decodeCheckpoints parses and skips incomplete rows', () => {
  const cps = decodeCheckpoints([
    { name: 'cp1', session_id: 's1', round: 3, created_at: 't' },
    { name: 'no-session', round: 1 },
    { session_id: 'no-name' },
  ])
  assert.equal(cps.length, 1)
  assert.equal(cps[0].name, 'cp1')
  assert.equal(cps[0].round, 3)
})

// ── Runtime over a stub kernel ──────────────────────────────────────────────

interface Call {
  command: string
  args: unknown
}

test('refreshSessions populates the store forest from session_list', async () => {
  useSessionsStore.getState().reset()
  const calls: Call[] = []
  const runtime = createSessionsRuntime({
    kernel: {
      async invoke<T = unknown>(_p: string, command: string, args?: unknown): Promise<T> {
        calls.push({ command, args })
        if (command === 'session_list') {
          return [
            { id: 'root', goal: 'g', outcome: 'complete', started_at: '1', ended_at: '2' },
            { id: 'child', parent_id: 'root', branch_point: 2, started_at: '3' },
          ] as T
        }
        return null as T
      },
      async available() {
        return true
      },
    },
    notifications: { show() {} },
  })
  await runtime.refreshSessions()
  const nodes = useSessionsStore.getState().nodes
  assert.equal(nodes.length, 2)
  assert.equal(useSessionsStore.getState().loading, false)
  assert.equal(useSessionsStore.getState().error, null)
})

test('branch invokes session_branch with auto_approve and selects the child', async () => {
  useSessionsStore.getState().reset()
  const calls: Call[] = []
  const runtime = createSessionsRuntime({
    kernel: {
      async invoke<T = unknown>(_p: string, command: string, args?: unknown): Promise<T> {
        calls.push({ command, args })
        if (command === 'session_branch') {
          return { id: 'child-1', goal: 'g', rounds: [], outcome: 'complete' } as T
        }
        if (command === 'session_list') return [] as T
        return null as T
      },
      async available() {
        return true
      },
    },
    notifications: { show() {} },
  })

  const childId = await runtime.branch('parent-1', 2, 'do the thing')
  assert.equal(childId, 'child-1')

  const branchCall = calls.find((c) => c.command === 'session_branch')
  assert.ok(branchCall, 'session_branch was invoked')
  assert.deepEqual(branchCall!.args, {
    session_id: 'parent-1',
    at_round: 2,
    message: 'do the thing',
    auto_approve: true,
  })
  // The new child is selected, and busy is cleared after the run.
  assert.equal(useSessionsStore.getState().selectedId, 'child-1')
  assert.equal(useSessionsStore.getState().busy, false)
})

test('rewind omits the message field when none is given', async () => {
  useSessionsStore.getState().reset()
  const calls: Call[] = []
  const runtime = createSessionsRuntime({
    kernel: {
      async invoke<T = unknown>(_p: string, command: string, args?: unknown): Promise<T> {
        calls.push({ command, args })
        if (command === 'session_rewind') {
          return { id: 'rw', goal: 'g', rounds: [], outcome: 'complete' } as T
        }
        return null as T
      },
      async available() {
        return true
      },
    },
    notifications: { show() {} },
  })
  await runtime.rewind('s', 1)
  const call = calls.find((c) => c.command === 'session_rewind')
  assert.deepEqual(call!.args, { session_id: 's', at_round: 1, auto_approve: true })
})
