// BL-054 Phase 2 — architecture.md parser unit tests.

import { strict as assert } from 'node:assert'
import { test } from 'node:test'

import {
  parseArchitecture,
  parseTaskLine,
} from '../src/plugins/nexus/osArchitecture/architectureParser'
import {
  detectDrift,
  taskKey,
} from '../src/plugins/nexus/osArchitecture/driftDetect'

test('parseArchitecture: returns empty domains for an empty source', () => {
  const arch = parseArchitecture('')
  assert.equal(arch.domains.length, 0)
  assert.equal(arch.preamble, '')
})

test('parseArchitecture: ignores anything inside a fenced code block', () => {
  // The seeded architecture.md placeholder has the four-attribute
  // example inside a fenced block — those tasks must NOT be parsed.
  const src = [
    '# Architecture',
    '',
    '## Knowledge',
    '',
    '```',
    '- ghost-task [skill | foundation | raw | local cron 0700]',
    '```',
    '',
    '- real-task [skill | foundation | raw | local cron 0700]',
  ].join('\n')
  const arch = parseArchitecture(src)
  assert.equal(arch.domains.length, 1)
  assert.equal(arch.domains[0].tasks.length, 1)
  assert.equal(arch.domains[0].tasks[0].id, 'real-task')
})

test('parseArchitecture: H2 starts a domain, list items become tasks', () => {
  const src = [
    '# Architecture',
    '',
    'Some preamble.',
    '',
    '## Knowledge',
    '',
    '- daily-trend-scan [skill | foundation | raw | local cron 0700]',
    '- deep-research    [skill | capability | raw | none]',
    '',
    '## Inbox',
    '',
    '- inbox-triage [skill | foundation | wiki | local cron 0530]',
  ].join('\n')
  const arch = parseArchitecture(src)
  assert.equal(arch.domains.length, 2)
  assert.equal(arch.domains[0].name, 'Knowledge')
  assert.equal(arch.domains[0].tasks.length, 2)
  assert.equal(arch.domains[0].tasks[0].id, 'daily-trend-scan')
  assert.equal(arch.domains[0].tasks[0].class, 'foundation')
  assert.equal(arch.domains[0].tasks[0].automation.kind, 'cron')
  assert.equal(arch.domains[0].tasks[1].class, 'capability')
  assert.equal(arch.domains[0].tasks[1].automation.kind, 'none')
  assert.equal(arch.domains[1].name, 'Inbox')
  assert.equal(arch.domains[1].tasks[0].id, 'inbox-triage')
  assert.equal(arch.preamble.includes('Some preamble'), true)
})

test('parseTaskLine: ignores list items without a four-attribute tag', () => {
  assert.equal(parseTaskLine('- just a regular bullet'), null)
  assert.equal(parseTaskLine('- task [too | few]'), null)
  assert.equal(parseTaskLine('- task [a|b|c|d|e]')?.id, 'task') // five fields ok, slug still parsed
})

test('parseTaskLine: parses unknown values as `unknown` rather than failing', () => {
  const t = parseTaskLine('- mystery [???? | ???? | ???? | ????]')
  assert.ok(t)
  assert.equal(t.type, 'unknown')
  assert.equal(t.class, 'unknown')
  assert.equal(t.memoryDest, 'unknown')
  assert.equal(t.automation.kind, 'unknown')
})

test('parseTaskLine: pulls a description after the slug', () => {
  const t = parseTaskLine('- daily-trend-scan brief blurb [skill | foundation | raw | none]')
  assert.ok(t)
  assert.equal(t.id, 'daily-trend-scan')
  assert.equal(t.description, 'brief blurb')
})

test('detectDrift: flags missing skills + missing automations', () => {
  const arch = parseArchitecture([
    '## Knowledge',
    '- registered-skill [skill | foundation | raw | local cron 0700]',
    '- missing-skill   [skill | foundation | raw | local cron 0530]',
  ].join('\n'))
  const drift = detectDrift({
    architecture: arch,
    skillIds: new Set(['registered-skill']),
    workflowNames: new Set(['registered-skill']),
  })
  assert.equal(drift.byTask.has(taskKey('Knowledge', 'registered-skill')), false)
  const missing = drift.byTask.get(taskKey('Knowledge', 'missing-skill'))
  assert.ok(missing)
  // missing-skill should hit BOTH skill-missing and automation-missing
  // because no workflow is registered for it either.
  const kinds = new Set(missing.map((d) => d.kind))
  assert.equal(kinds.has('skillMissing'), true)
  assert.equal(kinds.has('automationMissing'), true)
})

test('detectDrift: surfaces undocumented skills', () => {
  const arch = parseArchitecture('## Knowledge\n- documented [skill | capability | raw | none]\n')
  const drift = detectDrift({
    architecture: arch,
    skillIds: new Set(['documented', 'orphan-one', 'orphan-two']),
    workflowNames: new Set(),
  })
  assert.equal(drift.unattached.length, 2)
  assert.equal(drift.unattached[0].id, 'orphan-one') // alphabetical
  assert.equal(drift.unattached[1].id, 'orphan-two')
  assert.equal(drift.unattached[0].kind, 'undocumentedSkill')
})

test('detectDrift: capability tasks do not require a matching workflow', () => {
  const arch = parseArchitecture('## Ops\n- one-shot [skill | capability | raw | none]\n')
  const drift = detectDrift({
    architecture: arch,
    skillIds: new Set(['one-shot']),
    workflowNames: new Set(),
  })
  // No drift — capability + automation=none means no workflow required.
  assert.equal(drift.byTask.size, 0)
})

test('detectDrift: manual tasks do not require a matching skill', () => {
  const arch = parseArchitecture('## Ops\n- check-server [manual | foundation | none | local cron 0900]\n')
  const drift = detectDrift({
    architecture: arch,
    skillIds: new Set(),
    workflowNames: new Set(['check-server']),
  })
  assert.equal(drift.byTask.size, 0)
})
