// shell/src/plugins/nexus/agent/aig02.test.ts
//
// AIG-02 — coverage for the new risk classifier, diff helper, and
// the policy-driven auto-decide path through the agent runtime.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { createAgentRuntime } from './agentRuntime.ts'
import {
  classifyToolCall,
  isRoundEntirelySafe,
  riskLabel,
} from './riskClassifier.ts'
import {
  diffLines,
  extractWriteFileArgs,
  DIFF_MAX_LINES,
} from './diffPreview.ts'
import { useAgentSessionStore } from './sessionStore.ts'

// ── riskClassifier ─────────────────────────────────────────────────────

test('classifyToolCall: storage reads are safe, writes are write', () => {
  assert.equal(classifyToolCall('com.nexus.storage', 'read_file'), 'safe')
  assert.equal(classifyToolCall('com.nexus.storage', 'search'), 'safe')
  assert.equal(classifyToolCall('com.nexus.storage', 'backlinks'), 'safe')
  assert.equal(classifyToolCall('com.nexus.storage', 'write_file'), 'write')
  assert.equal(classifyToolCall('com.nexus.storage', 'delete_file'), 'write')
})

test('classifyToolCall: git push/pull/fetch are network', () => {
  assert.equal(classifyToolCall('com.nexus.git', 'log'), 'safe')
  assert.equal(classifyToolCall('com.nexus.git', 'push'), 'network')
  assert.equal(classifyToolCall('com.nexus.git', 'pull'), 'network')
  assert.equal(classifyToolCall('com.nexus.git', 'commit'), 'write')
})

test('classifyToolCall: terminal/process always exec', () => {
  assert.equal(classifyToolCall('com.nexus.terminal', 'spawn'), 'exec')
  assert.equal(classifyToolCall('com.nexus.terminal', 'anything'), 'exec')
  assert.equal(classifyToolCall('com.nexus.processes', 'kill'), 'exec')
})

test('classifyToolCall: unknown plugins fall through to write', () => {
  assert.equal(classifyToolCall('com.example.thirdparty', 'do_thing'), 'write')
})

test('isRoundEntirelySafe: empty round is safe; mixed is not', () => {
  assert.equal(isRoundEntirelySafe([]), true)
  assert.equal(
    isRoundEntirelySafe([
      { target_plugin_id: 'com.nexus.storage', command_id: 'read_file' },
      { target_plugin_id: 'com.nexus.git', command_id: 'log' },
    ]),
    true,
  )
  assert.equal(
    isRoundEntirelySafe([
      { target_plugin_id: 'com.nexus.storage', command_id: 'read_file' },
      { target_plugin_id: 'com.nexus.storage', command_id: 'write_file' },
    ]),
    false,
  )
})

test('riskLabel: returns short string per level', () => {
  assert.equal(riskLabel('safe'), 'read')
  assert.equal(riskLabel('write'), 'write')
  assert.equal(riskLabel('exec'), 'exec')
  assert.equal(riskLabel('network'), 'network')
})

// ── diffPreview ────────────────────────────────────────────────────────

test('diffLines: identical inputs report unchanged', () => {
  const r = diffLines('a\nb\nc', 'a\nb\nc')
  assert.equal(r.unchanged, true)
  assert.equal(r.lines.length, 0)
  assert.equal(r.truncated, false)
})

test('diffLines: simple replace renders remove + add', () => {
  const r = diffLines('a\nb\nc', 'a\nB\nc')
  assert.equal(r.unchanged, false)
  const kinds = r.lines.map((l) => l.kind)
  assert.ok(kinds.includes('remove'))
  assert.ok(kinds.includes('add'))
  // Context lines preserved either side.
  assert.ok(kinds.includes('context'))
})

test('diffLines: truncates at DIFF_MAX_LINES', () => {
  const before = ''
  const after = Array.from({ length: DIFF_MAX_LINES + 100 }, (_, i) => `line ${i}`).join('\n')
  const r = diffLines(before, after)
  assert.equal(r.truncated, true)
  assert.ok(r.lines.length <= DIFF_MAX_LINES)
})

test('extractWriteFileArgs: pulls path + contents, rejects malformed', () => {
  assert.deepEqual(extractWriteFileArgs({ path: 'a.md', contents: 'x' }), {
    path: 'a.md',
    contents: 'x',
  })
  assert.equal(extractWriteFileArgs(null), null)
  assert.equal(extractWriteFileArgs({ path: 'a.md' }), null)
  assert.equal(extractWriteFileArgs({ contents: 'x' }), null)
  assert.equal(extractWriteFileArgs({ path: 1, contents: 'x' }), null)
})

// ── auto-decide via agentRuntime ───────────────────────────────────────

interface KernelCall {
  pluginId: string
  commandId: string
  args: unknown
}
function buildKernel() {
  const calls: KernelCall[] = []
  const subs: Array<(topic: string, payload: unknown) => void> = []
  return {
    calls,
    subs,
    kernel: {
      async invoke(pluginId: string, commandId: string, args?: unknown) {
        calls.push({ pluginId, commandId, args })
        return null as never
      },
      async on(_p: string, h: (t: string, p: unknown) => void) {
        subs.push(h)
        return () => {
          const i = subs.indexOf(h)
          if (i >= 0) subs.splice(i, 1)
        }
      },
      async available() {
        return true
      },
    },
    notifications: { show() {} },
  }
}

function freshStore() {
  useAgentSessionStore.getState().reset()
}

test('ask_on_risky: read-only round auto-submits approve_all', async () => {
  freshStore()
  useAgentSessionStore.getState().setStepPolicy('ask_on_risky')
  const k = buildKernel()
  const runtime = createAgentRuntime({ kernel: k.kernel, notifications: k.notifications })
  await runtime.subscribeTopics()

  k.subs[0]('com.nexus.agent.round_proposed', {
    session_id: 'sess-r',
    round: 1,
    text: '',
    tool_calls: [
      {
        id: 't1',
        name: 'read_file',
        tool_call: {
          target_plugin_id: 'com.nexus.storage',
          command_id: 'read_file',
          args: { path: 'a.md' },
        },
      },
    ],
  })
  // Yield so the queued submitDecision microtask runs.
  await Promise.resolve()
  await Promise.resolve()

  const decide = k.calls.find((c) => c.commandId === 'round_decide')
  assert.ok(decide, 'auto-approve should dispatch round_decide')
  assert.equal((decide!.args as Record<string, unknown>).kind, 'approve_all')
  assert.equal(useAgentSessionStore.getState().pendingRound, null)
})

test('ask_on_risky: round with a write surfaces the approval card', async () => {
  freshStore()
  useAgentSessionStore.getState().setStepPolicy('ask_on_risky')
  const k = buildKernel()
  const runtime = createAgentRuntime({ kernel: k.kernel, notifications: k.notifications })
  await runtime.subscribeTopics()

  k.subs[0]('com.nexus.agent.round_proposed', {
    session_id: 'sess-w',
    round: 1,
    text: '',
    tool_calls: [
      {
        id: 't1',
        name: 'write_file',
        tool_call: {
          target_plugin_id: 'com.nexus.storage',
          command_id: 'write_file',
          args: { path: 'a.md', contents: 'x' },
        },
      },
    ],
  })
  await Promise.resolve()

  assert.equal(
    k.calls.find((c) => c.commandId === 'round_decide'),
    undefined,
    'no auto-approval when a tool writes',
  )
  assert.equal(useAgentSessionStore.getState().pendingRound?.toolCalls.length, 1)
  assert.equal(useAgentSessionStore.getState().phase, 'awaiting_approval')
})

test('auto_approve: even a write round auto-submits approve_all', async () => {
  freshStore()
  useAgentSessionStore.getState().setStepPolicy('auto_approve')
  const k = buildKernel()
  const runtime = createAgentRuntime({ kernel: k.kernel, notifications: k.notifications })
  await runtime.subscribeTopics()

  k.subs[0]('com.nexus.agent.round_proposed', {
    session_id: 'sess-auto',
    round: 1,
    text: '',
    tool_calls: [
      {
        id: 't1',
        name: 'write_file',
        tool_call: {
          target_plugin_id: 'com.nexus.storage',
          command_id: 'write_file',
          args: { path: 'a.md', contents: 'x' },
        },
      },
    ],
  })
  await Promise.resolve()
  await Promise.resolve()

  const decide = k.calls.find((c) => c.commandId === 'round_decide')
  assert.ok(decide)
  assert.equal((decide!.args as Record<string, unknown>).kind, 'approve_all')
})

test('always_ask: read-only round still surfaces the card', async () => {
  freshStore()
  useAgentSessionStore.getState().setStepPolicy('always_ask')
  const k = buildKernel()
  const runtime = createAgentRuntime({ kernel: k.kernel, notifications: k.notifications })
  await runtime.subscribeTopics()

  k.subs[0]('com.nexus.agent.round_proposed', {
    session_id: 'sess-ask',
    round: 1,
    text: '',
    tool_calls: [
      {
        id: 't1',
        name: 'read_file',
        tool_call: {
          target_plugin_id: 'com.nexus.storage',
          command_id: 'read_file',
          args: {},
        },
      },
    ],
  })
  await Promise.resolve()

  assert.equal(
    k.calls.find((c) => c.commandId === 'round_decide'),
    undefined,
    'always_ask never auto-approves',
  )
  assert.equal(useAgentSessionStore.getState().pendingRound?.toolCalls.length, 1)
})

// ── readFile passthrough ───────────────────────────────────────────────

test('runtime.readFile: returns string from storage::read_file', async () => {
  freshStore()
  const k = buildKernel()
  // Override invoke to return a fixed contents value.
  k.kernel.invoke = async (pluginId: string, commandId: string, args?: unknown) => {
    k.calls.push({ pluginId, commandId, args })
    if (pluginId === 'com.nexus.storage' && commandId === 'read_file') {
      return 'hello world' as never
    }
    return null as never
  }
  const runtime = createAgentRuntime({ kernel: k.kernel, notifications: k.notifications })
  const result = await runtime.readFile('a.md')
  assert.equal(result, 'hello world')
  const call = k.calls.find((c) => c.commandId === 'read_file')
  assert.ok(call)
  assert.deepEqual(call!.args, { path: 'a.md' })
})

test('runtime.readFile: returns null on transport error', async () => {
  freshStore()
  const k = buildKernel()
  k.kernel.invoke = async () => {
    throw new Error('boom')
  }
  const runtime = createAgentRuntime({ kernel: k.kernel, notifications: k.notifications })
  const result = await runtime.readFile('missing.md')
  assert.equal(result, null)
})
