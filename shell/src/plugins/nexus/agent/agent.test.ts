// shell/src/plugins/nexus/agent/agent.test.ts
//
// Tests for the session-driven agent runtime (ADR 0024 + 0025
// Phase 2). Exercises every flow that crosses the kernel boundary:
//   - goal validation
//   - session_run lifecycle
//   - round_proposed → pending state
//   - round_decide payload shapes (approve_all / partial / abort)
//   - session_list / session_get / session_delete

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { createAgentRuntime } from './agentRuntime.ts'
import {
  decodeProposedRound,
  decodeSessionList,
  decodeTranscript,
  useAgentSessionStore,
} from './sessionStore.ts'

interface KernelCall {
  pluginId: string
  commandId: string
  args: unknown
  timeoutMs?: number
}

interface StubKernel {
  invoke<T = unknown>(
    pluginId: string,
    commandId: string,
    args?: unknown,
    timeoutMs?: number,
  ): Promise<T>
  on<T = unknown>(
    topicPrefix: string,
    handler: (topic: string, payload: T) => void,
  ): Promise<() => void>
  available(): Promise<boolean>
  /** Test handles. */
  calls: KernelCall[]
  responses: Map<string, unknown[]>
  topicSubscribers: Array<(topic: string, payload: unknown) => void>
}

function buildKernel(): StubKernel {
  const calls: KernelCall[] = []
  const responses = new Map<string, unknown[]>()
  const subs: Array<(topic: string, payload: unknown) => void> = []
  const queueKey = (cmd: string) => `com.nexus.agent::${cmd}`
  return {
    calls,
    responses,
    topicSubscribers: subs,
    async invoke(pluginId, commandId, args, timeoutMs) {
      calls.push({ pluginId, commandId, args, timeoutMs })
      const queue = responses.get(queueKey(commandId)) ?? []
      const next = queue.shift()
      if (next === undefined) {
        // Default success-noop response so tests that don't pre-load
        // can still drive the IPC without throwing.
        return null as never
      }
      if (next instanceof Error) throw next
      return next as never
    },
    async on(_topicPrefix, handler) {
      subs.push(handler as (topic: string, payload: unknown) => void)
      return () => {
        const idx = subs.indexOf(handler as (t: string, p: unknown) => void)
        if (idx >= 0) subs.splice(idx, 1)
      }
    },
    async available() {
      return true
    },
  }
}

function buildNotifier() {
  const shown: Array<{ message: string; type?: string }> = []
  return {
    shown,
    show(n: { message: string; type?: 'info' | 'warning' | 'error' | 'success' }) {
      shown.push(n)
    },
  }
}

function reset(): void {
  useAgentSessionStore.getState().reset()
}

// ── Decoder coverage ───────────────────────────────────────────────────

test('decodeProposedRound: accepts a well-formed payload', () => {
  const payload = {
    session_id: 'sess-1',
    round: 1,
    text: 'reading notes',
    tool_calls: [
      {
        id: 'toolu-abc',
        name: 'read_file',
        tool_call: {
          target_plugin_id: 'com.nexus.storage',
          command_id: 'read_file',
          args: { path: 'a.md' },
        },
      },
    ],
  }
  const decoded = decodeProposedRound('sess-1', payload)
  assert.ok(decoded, 'decoder must accept the round')
  assert.equal(decoded?.round, 1)
  assert.equal(decoded?.toolCalls.length, 1)
  assert.equal(decoded?.toolCalls[0].name, 'read_file')
  assert.equal(decoded?.approvals['toolu-abc'], true, 'tool calls default to approved')
})

test('decodeProposedRound: rejects payloads missing round', () => {
  assert.equal(decodeProposedRound('s', { session_id: 's' }), null)
})

test('decodeTranscript: decodes a round-trip transcript', () => {
  const transcript = decodeTranscript({
    id: 'sess-2',
    goal: 'do thing',
    archetype: null,
    started_at: '2026-05-05T00:00:00Z',
    ended_at: '2026-05-05T00:01:00Z',
    rounds: [
      {
        round: 1,
        text: 'doing it',
        tool_calls: [
          { id: 't1', name: 'read_file', approved: true, response: { ok: true }, error: '' },
        ],
      },
    ],
    outcome: 'complete',
  })
  assert.ok(transcript)
  assert.equal(transcript?.rounds.length, 1)
  assert.equal(transcript?.outcome, 'complete')
})

test('decodeSessionList: drops malformed rows', () => {
  const list = decodeSessionList([
    { id: 'a', goal: 'one', started_at: 't1', ended_at: 't2', outcome: 'complete' },
    { id: '', goal: 'no id' },
    'not an object',
    { id: 'b', goal: 'two', started_at: 't3', ended_at: 't4', outcome: 'aborted' },
  ])
  assert.equal(list.length, 2)
  assert.equal(list[0].id, 'a')
  assert.equal(list[1].outcome, 'aborted')
})

// ── Composer / startSession ────────────────────────────────────────────

test('startSession: empty goal does not call session_run', async () => {
  reset()
  const kernel = buildKernel()
  const runtime = createAgentRuntime({ kernel, notifications: buildNotifier() })
  await runtime.startSession()
  assert.equal(kernel.calls.length, 0)
  assert.equal(useAgentSessionStore.getState().phase, 'idle')
})

test('startSession: posts session_run with auto_approve=false and archetype when set', async () => {
  reset()
  const kernel = buildKernel()
  // Pre-load the session_run reply so applyFinalTranscript flips
  // the phase to 'completed'.
  const finalTranscript = {
    id: 'sess-99',
    goal: 'summarise',
    archetype: 'writer',
    started_at: 't',
    ended_at: 't',
    rounds: [],
    outcome: 'complete',
  }
  kernel.responses.set('com.nexus.agent::session_run', [finalTranscript])
  // session_list called by the post-run refresh.
  kernel.responses.set('com.nexus.agent::session_list', [[]])

  const runtime = createAgentRuntime({ kernel, notifications: buildNotifier() })
  useAgentSessionStore.getState().setGoal('summarise notes')
  useAgentSessionStore.getState().setArchetype('writer')

  await runtime.startSession()

  const ipc = kernel.calls.find((c) => c.commandId === 'session_run')
  assert.ok(ipc, 'session_run must be invoked')
  const args = ipc?.args as Record<string, unknown>
  assert.equal(args.goal, 'summarise notes')
  assert.equal(args.auto_approve, false, 'agent runs with interactive approval')
  assert.equal(args.archetype, 'writer')
  assert.equal(useAgentSessionStore.getState().phase, 'completed')
  assert.equal(useAgentSessionStore.getState().currentSessionId, 'sess-99')
})

test('startSession: surfaces transport errors as liveError', async () => {
  reset()
  const kernel = buildKernel()
  kernel.responses.set('com.nexus.agent::session_run', [new Error('boom')])
  kernel.responses.set('com.nexus.agent::session_list', [[]])
  const runtime = createAgentRuntime({ kernel, notifications: buildNotifier() })
  useAgentSessionStore.getState().setGoal('do thing')
  await runtime.startSession()
  assert.match(useAgentSessionStore.getState().liveError ?? '', /boom/)
  assert.equal(useAgentSessionStore.getState().phase, 'errored')
})

// ── Topic subscription ────────────────────────────────────────────────

test('subscribeTopics: round_proposed populates pendingRound', async () => {
  reset()
  const kernel = buildKernel()
  const runtime = createAgentRuntime({ kernel, notifications: buildNotifier() })
  await runtime.subscribeTopics()
  // Simulate the agent core plugin emitting a round.
  kernel.topicSubscribers[0]('com.nexus.agent.round_proposed', {
    session_id: 'sess-1',
    round: 1,
    text: 'fetch a file',
    tool_calls: [
      {
        id: 'toolu-1',
        name: 'read_file',
        tool_call: { target_plugin_id: 'com.nexus.storage', command_id: 'read_file', args: {} },
      },
    ],
  })
  const s = useAgentSessionStore.getState()
  assert.equal(s.currentSessionId, 'sess-1')
  assert.equal(s.phase, 'awaiting_approval')
  assert.equal(s.pendingRound?.toolCalls.length, 1)
})

// ── round_decide payload shapes ───────────────────────────────────────

test('submitDecision approve_all sends kind=approve_all without entries', async () => {
  reset()
  const kernel = buildKernel()
  const runtime = createAgentRuntime({ kernel, notifications: buildNotifier() })
  await runtime.subscribeTopics()
  kernel.topicSubscribers[0]('com.nexus.agent.round_proposed', {
    session_id: 'sess-1',
    round: 1,
    text: '',
    tool_calls: [
      {
        id: 't1',
        name: 'read_file',
        tool_call: { target_plugin_id: 'com.nexus.storage', command_id: 'read_file', args: {} },
      },
    ],
  })
  await runtime.submitDecision('approve_all')
  const ipc = kernel.calls.find((c) => c.commandId === 'round_decide')
  assert.ok(ipc)
  const args = ipc?.args as Record<string, unknown>
  assert.equal(args.kind, 'approve_all')
  assert.equal(args.session_id, 'sess-1')
  assert.equal('entries' in args, false, 'approve_all carries no entries')
})

test('submitDecision partial sends per-tool entries with denial reasons', async () => {
  reset()
  const kernel = buildKernel()
  const runtime = createAgentRuntime({ kernel, notifications: buildNotifier() })
  await runtime.subscribeTopics()
  kernel.topicSubscribers[0]('com.nexus.agent.round_proposed', {
    session_id: 'sess-2',
    round: 1,
    text: '',
    tool_calls: [
      { id: 'a', name: 'read_file', tool_call: { target_plugin_id: 's', command_id: 'r', args: {} } },
      { id: 'b', name: 'write_file', tool_call: { target_plugin_id: 's', command_id: 'w', args: {} } },
    ],
  })
  // Deny tool b.
  useAgentSessionStore.getState().toggleApproval('b', false)
  await runtime.submitDecision('partial')
  const ipc = kernel.calls.find((c) => c.commandId === 'round_decide')
  const args = ipc?.args as Record<string, unknown>
  assert.equal(args.kind, 'partial')
  const entries = args.entries as Array<{ tool_use_id: string; approve: boolean; reason?: string }>
  assert.equal(entries.length, 2)
  const a = entries.find((e) => e.tool_use_id === 'a')
  const b = entries.find((e) => e.tool_use_id === 'b')
  assert.equal(a?.approve, true)
  assert.equal(b?.approve, false)
  assert.match(b?.reason ?? '', /denied/)
})

test('submitDecision abort sends kind=abort with reason', async () => {
  reset()
  const kernel = buildKernel()
  const runtime = createAgentRuntime({ kernel, notifications: buildNotifier() })
  await runtime.subscribeTopics()
  kernel.topicSubscribers[0]('com.nexus.agent.round_proposed', {
    session_id: 'sess-3',
    round: 1,
    text: '',
    tool_calls: [
      { id: 'x', name: 'read_file', tool_call: { target_plugin_id: 's', command_id: 'r', args: {} } },
    ],
  })
  await runtime.submitDecision('abort', 'changed my mind')
  const ipc = kernel.calls.find((c) => c.commandId === 'round_decide')
  const args = ipc?.args as Record<string, unknown>
  assert.equal(args.kind, 'abort')
  assert.equal(args.reason, 'changed my mind')
})

// ── Sessions sidebar ──────────────────────────────────────────────────

test('refreshSessions: populates sessions from session_list reply', async () => {
  reset()
  const kernel = buildKernel()
  kernel.responses.set('com.nexus.agent::session_list', [
    [{ id: 'a', goal: 'do', started_at: 't', ended_at: 't', outcome: 'complete' }],
  ])
  const runtime = createAgentRuntime({ kernel, notifications: buildNotifier() })
  await runtime.refreshSessions()
  const list = useAgentSessionStore.getState().sessions
  assert.equal(list.length, 1)
  assert.equal(list[0].id, 'a')
})

test('selectSession: pulls session_get and stores transcript', async () => {
  reset()
  const kernel = buildKernel()
  kernel.responses.set('com.nexus.agent::session_get', [
    {
      id: 'a',
      goal: 'do',
      archetype: null,
      started_at: 't',
      ended_at: 't',
      rounds: [{ round: 1, text: 'one', tool_calls: [] }],
      outcome: 'complete',
    },
  ])
  const runtime = createAgentRuntime({ kernel, notifications: buildNotifier() })
  await runtime.selectSession('a')
  const t = useAgentSessionStore.getState().selectedTranscript
  assert.equal(t?.id, 'a')
  assert.equal(t?.rounds[0].text, 'one')
})

test('deleteSession: posts session_delete and clears selection if matching', async () => {
  reset()
  const kernel = buildKernel()
  kernel.responses.set('com.nexus.agent::session_delete', [{ deleted: true, id: 'a' }])
  kernel.responses.set('com.nexus.agent::session_list', [[]])
  useAgentSessionStore.getState().setSelectedSession('a', null, null)
  const runtime = createAgentRuntime({ kernel, notifications: buildNotifier() })
  await runtime.deleteSession('a')
  const ipc = kernel.calls.find((c) => c.commandId === 'session_delete')
  assert.ok(ipc)
  assert.equal(useAgentSessionStore.getState().selectedSessionId, null)
})
