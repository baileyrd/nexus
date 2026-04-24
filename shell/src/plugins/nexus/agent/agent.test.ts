// shell/src/plugins/nexus/agent/agent.test.ts
//
// WI-07 Slice E unit tests for the agent plugin. Covers:
//
//   - Pure decoders   (decodePlan / decodeObservation / decodeHistoryList)
//   - Topic router    (handleAgentTopic against the four kernel events)
//   - Step machine    (planThenAwaitApproval → approve × N → finishStepRun,
//                      with skip / stop variants and response capture)
//   - History flow    (handleLoadHistory + handleDeleteHistory confirm paths)
//
// Run from the shell/ package with:
//   node --import tsx --test \
//     shell/src/plugins/nexus/agent/agent.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  AGENT_PLUGIN_ID,
  createAgentRuntime,
  decodeHistoryList,
  decodePlan,
  decodeObservation,
  type AgentRuntimeDeps,
} from './index.ts'
import { useAgentStore } from './agentStore.ts'

// ── Test helpers ──────────────────────────────────────────────────────────

interface InvokeCall {
  pluginId: string
  command: string
  args: unknown
}

interface NotificationCall {
  type?: string
  message: string
}

interface StubKernel {
  deps: AgentRuntimeDeps
  invokeCalls: InvokeCall[]
  notifications: NotificationCall[]
  /** Push a response (or thrown error) for the next invoke matching `command`. */
  queue(command: string, value: unknown | Error): void
  /** Inject a confirm() answer for the next prompt. Defaults to true. */
  setConfirm(answer: boolean): void
  /** Manually fire the topic handler (when `kernel.on` was called). */
  fireTopic(topic: string, payload: unknown): void
}

function makeKernel(): StubKernel {
  const invokeCalls: InvokeCall[] = []
  const notifications: NotificationCall[] = []
  const queues: Record<string, Array<unknown | Error>> = {}
  let confirmAnswer = true
  let topicHandler: ((topic: string, payload: unknown) => void) | null = null

  const deps: AgentRuntimeDeps = {
    kernel: {
      invoke: async <T = unknown>(
        pluginId: string,
        command: string,
        args?: unknown,
      ): Promise<T> => {
        invokeCalls.push({ pluginId, command, args })
        const queue = queues[command]
        if (queue && queue.length > 0) {
          const next = queue.shift()
          if (next instanceof Error) throw next
          return next as T
        }
        return null as T
      },
      on: async <T = unknown>(
        _topicPrefix: string,
        handler: (topic: string, payload: T) => void,
      ): Promise<() => void> => {
        topicHandler = handler as (t: string, p: unknown) => void
        return () => {
          topicHandler = null
        }
      },
      available: async () => true,
    },
    input: {
      confirm: async () => confirmAnswer,
    },
    notifications: {
      show: (n) => {
        notifications.push({ type: n.type, message: n.message })
      },
    },
  }

  return {
    deps,
    invokeCalls,
    notifications,
    queue(command, value) {
      ;(queues[command] ??= []).push(value)
    },
    setConfirm(answer) {
      confirmAnswer = answer
    },
    fireTopic(topic, payload) {
      if (!topicHandler) throw new Error('No topic handler registered yet — call subscribeAgentTopics first.')
      topicHandler(topic, payload)
    },
  }
}

function resetStore(): void {
  useAgentStore.getState().reset()
}

// ─────────────────────────────────────────────────────────────────────────
// Decoders
// ─────────────────────────────────────────────────────────────────────────

test('decodePlan: happy path produces a Plan with all fields', () => {
  const plan = decodePlan({
    id: 'p-1',
    goal: 'do a thing',
    steps: [
      {
        id: 's-1',
        description: 'first',
        tool_call: {
          target_plugin_id: 'com.nexus.storage',
          command_id: 'write',
          args: { path: 'foo.md', content: 'hi' },
        },
      },
      { id: 's-2', description: 'informational', tool_call: null },
    ],
  })
  assert.ok(plan)
  assert.equal(plan?.id, 'p-1')
  assert.equal(plan?.goal, 'do a thing')
  assert.equal(plan?.steps.length, 2)
  assert.equal(plan?.steps[0].id, 's-1')
  assert.equal(plan?.steps[0].tool_call?.target_plugin_id, 'com.nexus.storage')
  assert.deepEqual(plan?.steps[0].tool_call?.args, { path: 'foo.md', content: 'hi' })
  assert.equal(plan?.steps[1].tool_call, null)
})

test('decodePlan: missing id rejects the whole plan', () => {
  assert.equal(decodePlan({ goal: 'g', steps: [] }), null)
})

test('decodePlan: non-array steps rejects the plan', () => {
  assert.equal(decodePlan({ id: 'p', goal: 'g', steps: 'nope' }), null)
})

test('decodePlan: malformed step entries are dropped, valid ones survive', () => {
  const plan = decodePlan({
    id: 'p',
    goal: 'g',
    steps: [
      { id: 'good', description: 'ok', tool_call: null },
      { id: 'no-desc' }, // missing description
      'not-an-object',
      null,
      { description: 'no-id' }, // missing id
      {
        id: 'partial-tool',
        description: 'tool with bad shape',
        tool_call: { target_plugin_id: 'x' }, // missing command_id → drops tool_call only
      },
    ],
  })
  assert.ok(plan)
  assert.equal(plan?.steps.length, 2)
  assert.equal(plan?.steps[0].id, 'good')
  assert.equal(plan?.steps[1].id, 'partial-tool')
  assert.equal(plan?.steps[1].tool_call, null, 'malformed tool_call collapses to null, step survives')
})

test('decodeObservation: happy path with steps + success', () => {
  const obs = decodeObservation({
    plan_id: 'p-1',
    success: true,
    steps: [
      { step_id: 's-1', status: 'ok', response: { ok: true } },
      { step_id: 's-2', status: 'failed', response: 'boom' },
    ],
  })
  assert.ok(obs)
  assert.equal(obs?.plan_id, 'p-1')
  assert.equal(obs?.success, true)
  assert.equal(obs?.steps.length, 2)
  assert.equal(obs?.steps[0].status, 'ok')
  assert.deepEqual(obs?.steps[0].response, { ok: true })
})

test('decodeObservation: malformed step entries are filtered out', () => {
  const obs = decodeObservation({
    plan_id: 'p',
    steps: [
      { step_id: 'a', status: 'ok' },
      { status: 'ok' }, // no step_id → dropped
      null,
    ],
  })
  assert.equal(obs?.steps.length, 1)
  assert.equal(obs?.steps[0].step_id, 'a')
})

test('decodeObservation: unknown status falls back to failed', () => {
  const obs = decodeObservation({
    plan_id: 'p',
    steps: [{ step_id: 'a', status: 'mystery' }],
  })
  assert.equal(obs?.steps[0].status, 'failed')
})

test('decodeHistoryList: sorts newest-first by created_at', () => {
  const rows = decodeHistoryList([
    { plan_id: 'old', created_at: '2024-01-01T00:00:00Z', goal: 'old', success: true, steps: 1, bytes: 10 },
    { plan_id: 'new', created_at: '2024-06-01T00:00:00Z', goal: 'new', success: true, steps: 1, bytes: 10 },
    { plan_id: 'mid', created_at: '2024-03-01T00:00:00Z', goal: 'mid', success: true, steps: 1, bytes: 10 },
  ])
  assert.deepEqual(rows.map((r) => r.plan_id), ['new', 'mid', 'old'])
})

test('decodeHistoryList: rows missing plan_id are dropped; falls back to plan_id sort when timestamps absent', () => {
  const rows = decodeHistoryList([
    { plan_id: 'aaa', goal: 'a' },
    { goal: 'no-id' },
    { plan_id: 'ccc', goal: 'c' },
    { plan_id: 'bbb', goal: 'b' },
  ])
  assert.equal(rows.length, 3, 'no-id row dropped')
  assert.deepEqual(rows.map((r) => r.plan_id), ['ccc', 'bbb', 'aaa'])
})

test('decodeHistoryList: non-array input yields empty list', () => {
  assert.deepEqual(decodeHistoryList('nope'), [])
  assert.deepEqual(decodeHistoryList(null), [])
})

// ─────────────────────────────────────────────────────────────────────────
// Topic router (handleAgentTopic)
// ─────────────────────────────────────────────────────────────────────────

test('handleAgentTopic: step_start flips the matching step to running', async () => {
  resetStore()
  const k = makeKernel()
  const rt = createAgentRuntime(k.deps)
  await rt.subscribeAgentTopics()

  // Pre-populate a plan so stepRuntime entries exist.
  useAgentStore.getState().setPlan({
    id: 'p',
    goal: 'g',
    steps: [
      { id: 's-1', description: 'one', tool_call: null },
      { id: 's-2', description: 'two', tool_call: null },
    ],
  })

  k.fireTopic('com.nexus.agent.step_start', { plan_id: 'p', step_id: 's-1' })
  assert.equal(useAgentStore.getState().stepRuntime['s-1'].status, 'running')
  // s-2 untouched.
  assert.equal(useAgentStore.getState().stepRuntime['s-2'].status, 'queued')
})

test('handleAgentTopic: step_done routes ok / failed / skipped to setStepStatus', async () => {
  resetStore()
  const k = makeKernel()
  const rt = createAgentRuntime(k.deps)
  await rt.subscribeAgentTopics()
  useAgentStore.getState().setPlan({
    id: 'p',
    goal: 'g',
    steps: [
      { id: 'ok', description: '', tool_call: null },
      { id: 'fail', description: '', tool_call: null },
      { id: 'skip', description: '', tool_call: null },
      { id: 'unknown', description: '', tool_call: null },
    ],
  })

  k.fireTopic('com.nexus.agent.step_done', { step_id: 'ok', status: 'ok' })
  k.fireTopic('com.nexus.agent.step_done', { step_id: 'fail', status: 'failed', error: 'boom' })
  k.fireTopic('com.nexus.agent.step_done', { step_id: 'skip', status: 'skipped' })
  // Unknown / missing status → falls into the failed branch.
  k.fireTopic('com.nexus.agent.step_done', { step_id: 'unknown' })

  const rt2 = useAgentStore.getState().stepRuntime
  assert.equal(rt2['ok'].status, 'ok')
  assert.equal(rt2['fail'].status, 'failed')
  assert.equal(rt2['fail'].error, 'boom')
  assert.equal(rt2['skip'].status, 'skipped')
  assert.equal(rt2['unknown'].status, 'failed')
})

test('handleAgentTopic: run_start and run_done are no-ops on store state', async () => {
  resetStore()
  const k = makeKernel()
  const rt = createAgentRuntime(k.deps)
  await rt.subscribeAgentTopics()
  const before = useAgentStore.getState()

  k.fireTopic('com.nexus.agent.run_start', { plan_id: 'p', steps: 3 })
  k.fireTopic('com.nexus.agent.run_done', { plan_id: 'p', success: true })

  const after = useAgentStore.getState()
  assert.equal(after.phase, before.phase, 'run_start/run_done must not flip phase')
  assert.equal(after.plan, before.plan)
})

test('handleAgentTopic: non-object payload is silently ignored', async () => {
  resetStore()
  const k = makeKernel()
  const rt = createAgentRuntime(k.deps)
  await rt.subscribeAgentTopics()
  useAgentStore.getState().setPlan({
    id: 'p',
    goal: 'g',
    steps: [{ id: 's-1', description: '', tool_call: null }],
  })

  k.fireTopic('com.nexus.agent.step_start', null)
  k.fireTopic('com.nexus.agent.step_start', 'string-payload')
  // s-1 still queued — none of the bad payloads should mutate.
  assert.equal(useAgentStore.getState().stepRuntime['s-1'].status, 'queued')
})

// ─────────────────────────────────────────────────────────────────────────
// Step-by-step state machine
// ─────────────────────────────────────────────────────────────────────────

const SAMPLE_PLAN = {
  id: 'plan-1',
  goal: 'two-step plan',
  steps: [
    {
      id: 'step-a',
      description: 'first',
      tool_call: { target_plugin_id: 'com.nexus.storage', command_id: 'read', args: {} },
    },
    {
      id: 'step-b',
      description: 'second',
      tool_call: null,
    },
  ],
}

test('step machine: plan → approve × 2 → done with Observation built locally', async () => {
  resetStore()
  const k = makeKernel()
  const rt = createAgentRuntime(k.deps)

  useAgentStore.setState({ goal: 'do work' })
  k.queue('plan', SAMPLE_PLAN)
  k.queue('execute_step', { step_id: 'step-a', status: 'ok', response: { rows: 7 } })
  k.queue('execute_step', { step_id: 'step-b', status: 'ok', response: 'second-done' })

  await rt.planThenAwaitApproval('do work')
  assert.equal(useAgentStore.getState().phase, 'awaiting')
  assert.equal(useAgentStore.getState().pendingApprovalIndex, 0)
  assert.equal(useAgentStore.getState().plan?.steps.length, 2)

  await rt.handleApproveStep()
  // After step-a approved + ok, pending advances to step-b.
  assert.equal(useAgentStore.getState().pendingApprovalIndex, 1)
  assert.equal(useAgentStore.getState().stepRuntime['step-a'].status, 'ok')
  assert.deepEqual(useAgentStore.getState().stepRuntime['step-a'].response, { rows: 7 })

  await rt.handleApproveStep()
  // Done: observation built, pendingApprovalIndex cleared.
  const s = useAgentStore.getState()
  assert.equal(s.phase, 'done')
  assert.equal(s.pendingApprovalIndex, null)
  assert.ok(s.observation)
  assert.equal(s.observation?.success, true)
  assert.equal(s.observation?.steps.length, 2)
  assert.equal(s.observation?.steps[0].status, 'ok')
  assert.deepEqual(s.observation?.steps[0].response, { rows: 7 })
  assert.equal(s.observation?.steps[1].response, 'second-done')

  // Verify IPC arg shapes.
  const planCall = k.invokeCalls.find((c) => c.command === 'plan')
  assert.deepEqual(planCall?.args, { goal: 'do work' }, 'no archetype set → omits the key')
  const execs = k.invokeCalls.filter((c) => c.command === 'execute_step')
  assert.equal(execs.length, 2)
  assert.equal((execs[0].args as { index: number }).index, 0)
  assert.equal((execs[1].args as { index: number }).index, 1)
})

test('step machine: skip mid-flow advances index without invoking execute_step', async () => {
  resetStore()
  const k = makeKernel()
  const rt = createAgentRuntime(k.deps)

  k.queue('plan', SAMPLE_PLAN)
  k.queue('execute_step', { step_id: 'step-b', status: 'ok' })

  await rt.planThenAwaitApproval('go')
  rt.handleSkipStep() // skip step-a
  assert.equal(useAgentStore.getState().stepRuntime['step-a'].status, 'skipped')
  assert.equal(useAgentStore.getState().pendingApprovalIndex, 1)

  await rt.handleApproveStep() // approve step-b
  const s = useAgentStore.getState()
  assert.equal(s.phase, 'done')
  assert.equal(s.observation?.steps[0].status, 'skipped')
  assert.equal(s.observation?.steps[1].status, 'ok')
  // Mixed result → success=false.
  assert.equal(s.observation?.success, false)

  // execute_step only fired once, for step-b.
  const execs = k.invokeCalls.filter((c) => c.command === 'execute_step')
  assert.equal(execs.length, 1)
  assert.equal((execs[0].args as { index: number }).index, 1)
})

test('step machine: stop after one approval marks remaining as skipped', async () => {
  resetStore()
  const k = makeKernel()
  const rt = createAgentRuntime(k.deps)

  k.queue('plan', SAMPLE_PLAN)
  k.queue('execute_step', { step_id: 'step-a', status: 'ok' })

  await rt.planThenAwaitApproval('go')
  await rt.handleApproveStep() // step-a → ok, pending advances to step-b
  rt.handleStopRun()

  const s = useAgentStore.getState()
  assert.equal(s.phase, 'done')
  assert.equal(s.observation?.steps[0].status, 'ok', 'step-a survives the stop')
  assert.equal(s.observation?.steps[1].status, 'skipped', 'queued step-b marked skipped')
  assert.equal(s.observation?.success, false)
  assert.equal(s.pendingApprovalIndex, null)
})

test('step machine: approve sees a failed status → handleStopRun fires automatically', async () => {
  resetStore()
  const k = makeKernel()
  const rt = createAgentRuntime(k.deps)

  k.queue('plan', SAMPLE_PLAN)
  k.queue('execute_step', { step_id: 'step-a', status: 'failed' })

  await rt.planThenAwaitApproval('go')
  await rt.handleApproveStep()

  const s = useAgentStore.getState()
  assert.equal(s.phase, 'done')
  assert.equal(s.stepRuntime['step-a'].status, 'failed')
  assert.equal(s.stepRuntime['step-b'].status, 'skipped', 'failure aborts the rest')
})

test('step machine: archetype is forwarded into the plan IPC args', async () => {
  resetStore()
  const k = makeKernel()
  const rt = createAgentRuntime(k.deps)

  useAgentStore.getState().setArchetype('coder')
  k.queue('plan', SAMPLE_PLAN)

  await rt.planThenAwaitApproval('build it')
  const planCall = k.invokeCalls.find((c) => c.command === 'plan')
  assert.deepEqual(planCall?.args, { goal: 'build it', archetype: 'coder' })
})

test('step machine: empty plan finishes immediately with success=true', async () => {
  resetStore()
  const k = makeKernel()
  const rt = createAgentRuntime(k.deps)

  k.queue('plan', { id: 'p-empty', goal: 'g', steps: [] })
  await rt.planThenAwaitApproval('g')

  const s = useAgentStore.getState()
  assert.equal(s.phase, 'done')
  assert.equal(s.observation?.steps.length, 0)
  // every() over [] → true.
  assert.equal(s.observation?.success, true)
})

// ─────────────────────────────────────────────────────────────────────────
// History flow
// ─────────────────────────────────────────────────────────────────────────

test('history: handleLoadHistory populates plan + observation + goal', async () => {
  resetStore()
  const k = makeKernel()
  const rt = createAgentRuntime(k.deps)

  k.queue('history_get', {
    plan: SAMPLE_PLAN,
    observation: {
      plan_id: 'plan-1',
      success: true,
      steps: [
        { step_id: 'step-a', status: 'ok', response: { ok: true } },
        { step_id: 'step-b', status: 'ok', response: null },
      ],
    },
    goal: 'restored goal',
  })

  rt.handleLoadHistory('plan-1')
  // handleLoadHistory dispatches a microtask via void loadPlanIntoState — wait for it.
  await new Promise((r) => setTimeout(r, 0))

  const s = useAgentStore.getState()
  assert.equal(s.plan?.id, 'plan-1')
  assert.equal(s.goal, 'restored goal')
  assert.equal(s.observation?.steps.length, 2)
  assert.deepEqual(s.observation?.steps[0].response, { ok: true })
  assert.equal(s.phase, 'done')
})

test('history: handleDeleteHistory cancelled at confirm prompt does not invoke', async () => {
  resetStore()
  const k = makeKernel()
  k.setConfirm(false)
  const rt = createAgentRuntime(k.deps)

  await rt.handleDeleteHistory('plan-x')
  assert.equal(k.invokeCalls.filter((c) => c.command === 'history_delete').length, 0)
})

test('history: handleDeleteHistory confirmed → invokes delete + clears active plan + refreshes list', async () => {
  resetStore()
  const k = makeKernel()
  k.setConfirm(true)
  const rt = createAgentRuntime(k.deps)

  // Pre-load the deleted plan as the active one to exercise the
  // "clear if active" branch.
  useAgentStore.getState().setPlan(SAMPLE_PLAN)
  k.queue('history_delete', { ok: true })
  k.queue('history_list', [])

  await rt.handleDeleteHistory('plan-1')

  const s = useAgentStore.getState()
  assert.equal(s.plan, null, 'active plan cleared because it matched the deleted id')
  assert.equal(s.phase, 'idle')
  // Verify both invokes fired in order.
  const seq = k.invokeCalls.map((c) => c.command)
  assert.ok(seq.includes('history_delete'))
  assert.ok(seq.indexOf('history_delete') < seq.indexOf('history_list'), 'delete precedes list refresh')
})

test('history: handleDeleteHistory failure surfaces a notification', async () => {
  resetStore()
  const k = makeKernel()
  k.setConfirm(true)
  const rt = createAgentRuntime(k.deps)
  k.queue('history_delete', new Error('disk full'))

  await rt.handleDeleteHistory('plan-x')

  assert.equal(k.notifications.length, 1)
  assert.equal(k.notifications[0].type, 'error')
  assert.ok(k.notifications[0].message.includes('disk full'))
})

// Sanity: AGENT_PLUGIN_ID export is the const the tests assume.
test('AGENT_PLUGIN_ID is the kernel agent plugin id', () => {
  assert.equal(AGENT_PLUGIN_ID, 'com.nexus.agent')
})
