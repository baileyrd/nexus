// shell/src/plugins/nexus/status/StatusPill.test.ts
//
// BL-053 Phase 4 — pure helpers covered by unit tests; the React
// rendering is integration-tested as part of `MarkdownDoc` /
// `FilesTree`.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { isKnownStatus, statusAccentVar } from './StatusPill.tsx'

test('isKnownStatus picks the four canonical values', () => {
  for (const k of ['info', 'warn', 'risk', 'ok']) {
    assert.equal(isKnownStatus(k), true, `'${k}' is canonical`)
  }
  assert.equal(isKnownStatus('foo'), false)
  assert.equal(isKnownStatus(''), false)
  assert.equal(isKnownStatus(null), false)
  assert.equal(isKnownStatus(undefined), false)
})

test('statusAccentVar maps each known status to the matching callout token', () => {
  // The mapping must mirror `.nx-callout--<type>` in shell.css so a
  // pill rendered next to a callout of the same type shows the
  // identical accent.
  assert.equal(statusAccentVar('info'), '--cool')
  assert.equal(statusAccentVar('warn'), '--warn')
  assert.equal(statusAccentVar('risk'), '--risk')
  assert.equal(statusAccentVar('ok'), '--ok')
})

test('statusAccentVar falls back to neutral for unknown / nullable values', () => {
  assert.equal(statusAccentVar('foo'), '--text-faint')
  assert.equal(statusAccentVar(null), '--text-faint')
  assert.equal(statusAccentVar(undefined), '--text-faint')
})
