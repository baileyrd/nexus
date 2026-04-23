// shell/src/plugins/nexus/skills/skillsStore.test.ts
//
// WI-08 unit tests for the skills store render flow. We exercise the
// new public actions (toggleRenderForm / setParamValue / renderSkill /
// clearRenderResult / reset) with a mocked kernel that records every
// invoke + drives the response.
//
// Run from the shell/ package with:
//   node --import tsx --test \
//     shell/src/plugins/nexus/skills/skillsStore.test.ts
//
// Same node:test pattern as terminal/savedCommandsStore.test.ts —
// `@ts-expect-error` keeps tsc quiet without depending on
// `@types/node` (not in the shell tsconfig lib set).

// @ts-expect-error tsc lib doesn't include node builtins
import { test } from 'node:test'
// @ts-expect-error tsc lib doesn't include node builtins
import assert from 'node:assert/strict'
import {
  useSkillsStore,
  type SkillEntry,
  type SkillParameter,
  type SkillsKernelAPI,
} from './skillsStore.ts'

interface InvokeCall {
  pluginId: string
  command: string
  args: unknown
}

function makeKernel(): {
  api: SkillsKernelAPI
  calls: InvokeCall[]
  responses: Record<string, unknown[]>
} {
  const calls: InvokeCall[] = []
  const responses: Record<string, unknown[]> = {}
  const api: SkillsKernelAPI = {
    invoke: async <T = unknown>(
      pluginId: string,
      command: string,
      args?: unknown,
    ): Promise<T> => {
      calls.push({ pluginId, command, args })
      const queue = responses[command]
      if (queue && queue.length > 0) {
        return queue.shift() as T
      }
      return null as T
    },
  }
  return { api, calls, responses }
}

function param(
  name: string,
  type: string,
  extra: Partial<SkillParameter> = {},
): SkillParameter {
  return {
    name,
    type,
    description: '',
    values: [],
    items: null,
    default: undefined,
    ...extra,
  }
}

function skill(id: string, parameters: SkillParameter[] = []): SkillEntry {
  return {
    id,
    name: id,
    description: '',
    version: '',
    author: '',
    tags: [],
    applicableContexts: [],
    triggers: [],
    parameters,
    body: '',
  }
}

function reset(): void {
  useSkillsStore.getState().reset()
}

// ── toggleRenderForm ─────────────────────────────────────────────────────────

test('toggleRenderForm: opens form and seeds drafts from parameter defaults', () => {
  reset()
  const s = useSkillsStore.getState()
  s.setSkills([
    skill('p1', [
      param('tone', 'string', { default: 'friendly' }),
      param('length', 'number', { default: 200 }),
      param('opt', 'string'), // no default — stays absent
    ]),
  ])

  s.toggleRenderForm('p1')

  const after = useSkillsStore.getState()
  assert.equal(after.renderingId, 'p1')
  assert.deepEqual(after.paramDrafts['p1'], { tone: 'friendly', length: 200 })
})

test('toggleRenderForm: same id closes the form, drafts retained', () => {
  reset()
  const s = useSkillsStore.getState()
  s.setSkills([skill('p1', [param('tone', 'string', { default: 'x' })])])
  s.toggleRenderForm('p1')
  s.toggleRenderForm('p1')

  const after = useSkillsStore.getState()
  assert.equal(after.renderingId, null)
  // Drafts persist across close so re-opening doesn't clobber edits.
  assert.deepEqual(after.paramDrafts['p1'], { tone: 'x' })
})

test('toggleRenderForm: switching to a new skill does not re-seed an existing draft', () => {
  reset()
  const s = useSkillsStore.getState()
  s.setSkills([
    skill('p1', [param('tone', 'string', { default: 'a' })]),
    skill('p2', [param('mode', 'string', { default: 'b' })]),
  ])
  s.toggleRenderForm('p1')
  s.setParamValue('p1', 'tone', 'edited')
  s.toggleRenderForm('p2')
  s.toggleRenderForm('p1')

  assert.deepEqual(useSkillsStore.getState().paramDrafts['p1'], { tone: 'edited' })
})

// ── setParamValue ────────────────────────────────────────────────────────────

test('setParamValue: writes a single field without disturbing siblings', () => {
  reset()
  const s = useSkillsStore.getState()
  s.setSkills([skill('p1', [param('a', 'string'), param('b', 'string')])])
  s.toggleRenderForm('p1')
  s.setParamValue('p1', 'a', 'one')
  s.setParamValue('p1', 'b', 'two')
  s.setParamValue('p1', 'a', 'one-edited')

  assert.deepEqual(useSkillsStore.getState().paramDrafts['p1'], {
    a: 'one-edited',
    b: 'two',
  })
})

test('setParamValue: works for skills the form has not been opened on', () => {
  reset()
  useSkillsStore.getState().setParamValue('p1', 'x', 42)
  assert.deepEqual(useSkillsStore.getState().paramDrafts['p1'], { x: 42 })
})

// ── renderSkill ──────────────────────────────────────────────────────────────

test('renderSkill: posts render IPC with current draft, stashes result', async () => {
  reset()
  const s = useSkillsStore.getState()
  s.setSkills([skill('p1', [param('tone', 'string', { default: 'friendly' })])])
  s.toggleRenderForm('p1')
  s.setParamValue('p1', 'tone', 'formal')

  const { api, calls, responses } = makeKernel()
  responses['render'] = [{ id: 'p1', name: 'P1', body: 'rendered formal body' }]

  await useSkillsStore.getState().renderSkill(api, 'p1')

  const after = useSkillsStore.getState()
  assert.equal(after.renderResults['p1'].body, 'rendered formal body')
  assert.equal(after.renderResults['p1'].name, 'P1')
  assert.equal(after.renderErrors['p1'], undefined)
  assert.equal(after.rendering, null)

  assert.equal(calls.length, 1)
  assert.equal(calls[0].pluginId, 'com.nexus.skills')
  assert.equal(calls[0].command, 'render')
  assert.deepEqual(calls[0].args, { id: 'p1', values: { tone: 'formal' } })
})

test('renderSkill: missing draft → sends empty values, kernel applies defaults', async () => {
  reset()
  useSkillsStore.getState().setSkills([skill('p1')])
  const { api, calls, responses } = makeKernel()
  responses['render'] = [{ id: 'p1', name: 'P1', body: 'default body' }]

  await useSkillsStore.getState().renderSkill(api, 'p1')

  assert.deepEqual(calls[0].args, { id: 'p1', values: {} })
  assert.equal(useSkillsStore.getState().renderResults['p1'].body, 'default body')
})

test('renderSkill: kernel error stashed on renderErrors, no result, rendering cleared', async () => {
  reset()
  useSkillsStore.getState().setSkills([skill('p1')])
  const api: SkillsKernelAPI = {
    invoke: async () => {
      throw new Error('ExecutionFailed: render: bad param')
    },
  }
  await useSkillsStore.getState().renderSkill(api, 'p1')

  const after = useSkillsStore.getState()
  assert.match(after.renderErrors['p1'] ?? '', /bad param/)
  assert.equal(after.renderResults['p1'], undefined)
  assert.equal(after.rendering, null)
})

test('renderSkill: success after a prior error clears the stale error', async () => {
  reset()
  useSkillsStore.getState().setSkills([skill('p1')])

  const failing: SkillsKernelAPI = {
    invoke: async () => {
      throw new Error('boom')
    },
  }
  await useSkillsStore.getState().renderSkill(failing, 'p1')
  assert.match(useSkillsStore.getState().renderErrors['p1'] ?? '', /boom/)

  const { api, responses } = makeKernel()
  responses['render'] = [{ id: 'p1', name: 'P1', body: 'ok' }]
  await useSkillsStore.getState().renderSkill(api, 'p1')

  const after = useSkillsStore.getState()
  assert.equal(after.renderResults['p1'].body, 'ok')
  assert.equal(after.renderErrors['p1'], undefined)
})

test('renderSkill: defensive decode tolerates a missing/garbage body field', async () => {
  reset()
  useSkillsStore.getState().setSkills([skill('p1')])
  const { api, responses } = makeKernel()
  responses['render'] = [{ id: 'p1', name: 'P1' }] // no body
  await useSkillsStore.getState().renderSkill(api, 'p1')
  assert.equal(useSkillsStore.getState().renderResults['p1'].body, '')
})

// ── clearRenderResult ────────────────────────────────────────────────────────

test('clearRenderResult: drops result + error for the given skill only', async () => {
  reset()
  useSkillsStore.getState().setSkills([skill('p1'), skill('p2')])
  const { api, responses } = makeKernel()
  responses['render'] = [
    { id: 'p1', name: 'P1', body: 'one' },
    { id: 'p2', name: 'P2', body: 'two' },
  ]
  await useSkillsStore.getState().renderSkill(api, 'p1')
  await useSkillsStore.getState().renderSkill(api, 'p2')

  useSkillsStore.getState().clearRenderResult('p1')

  const after = useSkillsStore.getState()
  assert.equal(after.renderResults['p1'], undefined)
  assert.equal(after.renderResults['p2'].body, 'two')
})

// ── reset ────────────────────────────────────────────────────────────────────

test('reset: clears all render state (workspace:closed contract)', async () => {
  reset()
  useSkillsStore.getState().setSkills([skill('p1', [param('a', 'string', { default: 'x' })])])
  useSkillsStore.getState().toggleRenderForm('p1')
  const { api, responses } = makeKernel()
  responses['render'] = [{ id: 'p1', name: 'P1', body: 'r' }]
  await useSkillsStore.getState().renderSkill(api, 'p1')

  useSkillsStore.getState().reset()

  const after = useSkillsStore.getState()
  assert.equal(after.skills.length, 0)
  assert.equal(after.renderingId, null)
  assert.deepEqual(after.paramDrafts, {})
  assert.deepEqual(after.renderResults, {})
  assert.deepEqual(after.renderErrors, {})
  assert.equal(after.rendering, null)
})
