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

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  useSkillsStore,
  serializeDraft,
  validateDraft,
  type SkillDraft,
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
    created: '',
    tags: [],
    applicableContexts: [],
    triggers: [],
    parameters,
    dependsOn: [],
    body: '',
    relpath: `.forge/skills/${id}.skill.md`,
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

// ── BL-022 — editor coverage ───────────────────────────────────────────────

function draft(over: Partial<SkillDraft> = {}): SkillDraft {
  return {
    relpath: '.forge/skills/code-reviewer.skill.md',
    isNew: false,
    name: 'Code Reviewer',
    id: 'code-reviewer',
    description: 'Review code for clarity and safety.',
    version: '1.0.0',
    author: 'team',
    created: '2026-04-29',
    tags: ['review', 'safety'],
    applicableContexts: ['ai-chat'],
    triggers: ['review this'],
    dependsOn: ['concise'],
    body: '# Reviewer\n\nFocus on clarity.',
    ...over,
  }
}

test('BL-022 serializeDraft: round-trips frontmatter + body', () => {
  const text = serializeDraft(draft())
  // Frontmatter delimited by ---/---.
  assert.match(text, /^---\n/)
  assert.match(text, /\n---\n/)
  // Required fields all land verbatim, in the documented order.
  assert.match(text, /\nid: code-reviewer\n/)
  assert.match(text, /\nname: Code Reviewer\n/)
  assert.match(text, /\ntags: \[review, safety\]\n/)
  assert.match(text, /\napplicable_contexts: \[ai-chat\]\n/)
  assert.match(text, /\ndepends_on: \[concise\]\n/)
  // Body comes after the closing fence.
  const split = text.split('\n---\n')
  assert.equal(split.length, 2)
  assert.match(split[1], /# Reviewer/)
})

test('BL-022 serializeDraft: quotes scalars that look numeric or reserved', () => {
  const d = draft({
    name: 'true', // YAML-reserved bare word — must be quoted
    version: '2.0', // numeric-looking → quote so it stays a string
  })
  const text = serializeDraft(d)
  assert.match(text, /name: "true"/)
  assert.match(text, /version: "2\.0"/)
})

test('BL-022 validateDraft: required fields surface a clear message', () => {
  assert.equal(validateDraft(draft({ id: '' })), 'id is required')
  assert.equal(validateDraft(draft({ name: '' })), 'name is required')
  assert.match(
    validateDraft(draft({ id: 'BadCase' })) ?? '',
    /kebab-case/,
  )
  assert.equal(validateDraft(draft()), null)
})

test('BL-022 openEditor seeds the draft from the listing snapshot', () => {
  const s = useSkillsStore.getState()
  s.reset()
  s.setSkills([
    {
      id: 'p1',
      name: 'P1',
      description: 'd',
      version: '1',
      author: 'a',
      created: '2026-04-29',
      tags: ['t'],
      applicableContexts: ['ai-chat'],
      triggers: ['hi'],
      parameters: [],
      dependsOn: [],
      body: 'BODY',
      relpath: '.forge/skills/p1.skill.md',
    },
  ])
  s.openEditor('p1')
  const d = useSkillsStore.getState().draft
  assert.ok(d, 'draft populated')
  assert.equal(d.id, 'p1')
  assert.equal(d.body, 'BODY')
  assert.equal(d.relpath, '.forge/skills/p1.skill.md')
  assert.equal(d.isNew, false)
})

test('BL-022 saveDraft: write_file then reload, then editor closes', async () => {
  const s = useSkillsStore.getState()
  s.reset()
  s.openNewSkill()
  s.patchDraft({
    id: 'fresh',
    name: 'Fresh',
    description: 'd',
    version: '0.1.0',
    author: 'me',
    created: '2026-04-29',
    body: 'B',
  })
  const calls: Array<{ pluginId: string; commandId: string }> = []
  const api = {
    invoke: async <T = unknown>(pluginId: string, commandId: string) => {
      calls.push({ pluginId, commandId })
      return {} as T
    },
  }
  const ok = await useSkillsStore.getState().saveDraft(api)
  assert.equal(ok, true)
  assert.deepEqual(
    calls.map((c) => `${c.pluginId}::${c.commandId}`),
    ['com.nexus.storage::write_file', 'com.nexus.skills::reload'],
  )
  assert.equal(useSkillsStore.getState().draft, null)
  assert.equal(useSkillsStore.getState().editingId, null)
})

test('BL-022 saveDraft: validation failure sets saveError without IPC', async () => {
  const s = useSkillsStore.getState()
  s.reset()
  s.openNewSkill()
  // Don't patch — required fields are still empty.
  let invoked = 0
  const api = {
    invoke: async <T = unknown>() => {
      invoked += 1
      return {} as T
    },
  }
  const ok = await useSkillsStore.getState().saveDraft(api)
  assert.equal(ok, false)
  assert.equal(invoked, 0)
  assert.match(useSkillsStore.getState().saveError ?? '', /id is required/)
})

// ── AIG-01 — compose action ──────────────────────────────────────────────

test('composeSkill: caches successful compose and clears stale error', async () => {
  reset()
  const { api, calls, responses } = makeKernel()
  responses.compose = [
    {
      root_id: 'child',
      fragments: [
        { id: 'parent', name: 'Parent', body: 'P' },
        { id: 'child', name: 'Child', body: 'C' },
      ],
      merged_body: 'P\n\nC',
      conflicts: [],
    },
  ]
  // Pre-seed an old error to confirm it's cleared on success.
  useSkillsStore.setState({ composeErrors: { child: 'previous failure' } })

  await useSkillsStore.getState().composeSkill(api, 'child')

  const result = useSkillsStore.getState().composeResults['child']
  assert.ok(result, 'composeResults should be populated')
  assert.equal(result.rootId, 'child')
  assert.equal(result.fragments.length, 2)
  assert.equal(result.fragments[1].id, 'child') // root is last
  assert.equal(result.mergedBody, 'P\n\nC')
  assert.equal(useSkillsStore.getState().composeErrors['child'], undefined)
  assert.equal(useSkillsStore.getState().composing, null)
  assert.equal(calls.length, 1)
  assert.deepEqual(calls[0].args, { id: 'child' })
})

test('composeSkill: cycle error from kernel surfaces on composeErrors', async () => {
  reset()
  // Pre-seed a stale success to confirm it's purged on failure.
  useSkillsStore.setState({
    composeResults: {
      a: { rootId: 'a', fragments: [], mergedBody: 'old', conflicts: [] },
    },
  })
  const api: SkillsKernelAPI = {
    invoke: async () => {
      throw new Error('compose: cycle detected: a → b → a')
    },
  }

  await useSkillsStore.getState().composeSkill(api, 'a')

  assert.equal(useSkillsStore.getState().composeResults['a'], undefined)
  assert.match(useSkillsStore.getState().composeErrors['a'] ?? '', /cycle detected/)
  assert.equal(useSkillsStore.getState().composing, null)
})

test('composeSkill: malformed payload reported as decoder failure', async () => {
  reset()
  const { api, responses } = makeKernel()
  // Kernel returned something we can't recognise (e.g. bare string).
  responses.compose = ['oops']
  await useSkillsStore.getState().composeSkill(api, 'x')
  assert.match(
    useSkillsStore.getState().composeErrors['x'] ?? '',
    /unparseable|compose/i,
  )
})

test('composeSkill: parameter_clash conflict survives the round-trip', async () => {
  reset()
  const { api, responses } = makeKernel()
  responses.compose = [
    {
      root_id: 'r',
      fragments: [{ id: 'r', name: 'R', body: '' }],
      merged_body: '',
      conflicts: [
        { kind: 'parameter_clash', parameter: 'tone', skill_ids: ['a', 'b'] },
        // Unknown conflict shape silently dropped.
        { kind: 'unknown_kind', skill_ids: ['z'] },
      ],
    },
  ]
  await useSkillsStore.getState().composeSkill(api, 'r')
  const conflicts = useSkillsStore.getState().composeResults['r'].conflicts
  assert.equal(conflicts.length, 1)
  assert.equal(conflicts[0].kind, 'parameter_clash')
  if (conflicts[0].kind === 'parameter_clash') {
    assert.deepEqual(conflicts[0].skill_ids, ['a', 'b'])
  }
})

test('toggleComposePanel: opens, fetches once, closes without refetch', async () => {
  reset()
  const { api, calls, responses } = makeKernel()
  responses.compose = [
    {
      root_id: 'r',
      fragments: [{ id: 'r', name: 'R', body: '' }],
      merged_body: '',
      conflicts: [],
    },
  ]
  await useSkillsStore.getState().toggleComposePanel(api, 'r')
  assert.equal(useSkillsStore.getState().composeOpenId, 'r')
  assert.equal(calls.length, 1, 'first open should fetch')

  // Close and reopen — cache should short-circuit the IPC.
  await useSkillsStore.getState().toggleComposePanel(api, 'r')
  assert.equal(useSkillsStore.getState().composeOpenId, null)
  await useSkillsStore.getState().toggleComposePanel(api, 'r')
  assert.equal(useSkillsStore.getState().composeOpenId, 'r')
  assert.equal(calls.length, 1, 'cached result should short-circuit IPC')
})

test('clearCompose: drops both result and error for the id', () => {
  reset()
  useSkillsStore.setState({
    composeResults: {
      a: { rootId: 'a', fragments: [], mergedBody: '', conflicts: [] },
    },
    composeErrors: { a: 'boom' },
  })
  useSkillsStore.getState().clearCompose('a')
  assert.equal(useSkillsStore.getState().composeResults['a'], undefined)
  assert.equal(useSkillsStore.getState().composeErrors['a'], undefined)
})
