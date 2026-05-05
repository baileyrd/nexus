// shell/tests/capability-info.test.ts
//
// WI-18 — Risk-bucketing helper tests.
//
// Pure-function module; no React, no DOM. We only assert the
// classification contract (what bucket each variant lands in, what
// the manifest parser returns for funny inputs) — visual chip
// rendering is the verification step in the work item.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  CAPABILITY_INFO,
  ALL_CAPABILITIES,
  bucketByRisk,
  highestRisk,
  hasHighRisk,
  parseManifestCapabilities,
} from '../src/plugins/nexus/pluginsMgmt/capabilityInfo'

test('CAPABILITY_INFO covers every Capability variant', () => {
  // Sanity: the exhaustiveness check in the source already enforces
  // this at typecheck-time, but a runtime assertion catches the
  // case where the file ships with a stub like `{} as any`.
  const expected = [
    'FsRead', 'FsWrite', 'FsReadExternal', 'FsWriteExternal',
    'NetHttp', 'NetHttpLocalhost', 'ProcessSpawn',
    'KvRead', 'KvWrite', 'IpcCall',
    'DbQuery', 'DbWrite',
    'EventsPublish', 'UiNotify',
    // ADR 0022 (per-handler ai.* + tools-policy enforcement)
    'AiChat', 'AiIndex', 'AiSessionRead', 'AiSessionWrite',
    'AiConfigWrite', 'AiActivityWrite', 'AiToolsWrite', 'AiToolsMcp',
  ]
  for (const variant of expected) {
    assert.ok(
      Object.prototype.hasOwnProperty.call(CAPABILITY_INFO, variant),
      `CAPABILITY_INFO is missing variant: ${variant}`,
    )
    const meta = CAPABILITY_INFO[variant as keyof typeof CAPABILITY_INFO]
    assert.match(meta.risk, /^(low|medium|high)$/)
    assert.ok(meta.description.length > 0, `${variant} has empty description`)
  }
  // ALL_CAPABILITIES echoes the keys; should match length.
  assert.equal(ALL_CAPABILITIES.length, expected.length)
})

test('bucketByRisk groups capabilities into low / medium / high', () => {
  const buckets = bucketByRisk([
    'UiNotify', 'EventsPublish', 'IpcCall',
    'KvRead', 'KvWrite', 'DbQuery', 'DbWrite', 'FsRead', 'NetHttpLocalhost',
    'FsWrite', 'FsReadExternal', 'FsWriteExternal', 'NetHttp', 'ProcessSpawn',
  ])

  assert.deepEqual(
    new Set(buckets.low),
    new Set(['UiNotify', 'EventsPublish', 'IpcCall']),
    'low bucket mismatch',
  )
  assert.deepEqual(
    new Set(buckets.medium),
    new Set(['KvRead', 'KvWrite', 'DbQuery', 'DbWrite', 'FsRead', 'NetHttpLocalhost']),
    'medium bucket mismatch',
  )
  assert.deepEqual(
    new Set(buckets.high),
    new Set(['FsWrite', 'FsReadExternal', 'FsWriteExternal', 'NetHttp', 'ProcessSpawn']),
    'high bucket mismatch',
  )

  // Total preserved (no dropped variants).
  const total = buckets.low.length + buckets.medium.length + buckets.high.length
  assert.equal(total, 14)
})

test('bucketByRisk silently drops unknown variants', () => {
  // `as Capability` is the cast the runtime would see if a future
  // shell shipped against an older generated enum.
  const buckets = bucketByRisk([
    'UiNotify',
    'NotARealCapability' as unknown as never,
    'FsWrite',
  ] as never)
  assert.deepEqual(buckets.low, ['UiNotify'])
  assert.deepEqual(buckets.medium, [])
  assert.deepEqual(buckets.high, ['FsWrite'])
})

test('highestRisk returns the worst bucket present (or null when empty)', () => {
  assert.equal(highestRisk([]), null)
  assert.equal(highestRisk(['UiNotify']), 'low')
  assert.equal(highestRisk(['UiNotify', 'KvRead']), 'medium')
  assert.equal(highestRisk(['UiNotify', 'KvRead', 'ProcessSpawn']), 'high')
  // High wins even when listed first or last.
  assert.equal(highestRisk(['NetHttp', 'UiNotify']), 'high')
})

test('hasHighRisk is the boolean predicate over highestRisk', () => {
  assert.equal(hasHighRisk([]), false)
  assert.equal(hasHighRisk(['UiNotify']), false)
  assert.equal(hasHighRisk(['DbWrite']), false)
  assert.equal(hasHighRisk(['ProcessSpawn']), true)
  assert.equal(hasHighRisk(['UiNotify', 'NetHttp']), true)
})

test('parseManifestCapabilities distinguishes missing / empty / declared', () => {
  // Missing: undefined and null both → null ("(unknown)").
  assert.equal(parseManifestCapabilities(undefined), null)
  assert.equal(parseManifestCapabilities(null), null)

  // Non-array inputs are treated as missing — better than throwing.
  assert.equal(parseManifestCapabilities('FsRead'), null)
  assert.equal(parseManifestCapabilities(42), null)
  assert.equal(parseManifestCapabilities({ FsRead: true }), null)

  // Declared empty: ("(none)") preserved as a distinct state.
  assert.deepEqual(parseManifestCapabilities([]), [])

  // Declared list: known variants kept, unknowns and non-strings dropped.
  assert.deepEqual(
    parseManifestCapabilities(['FsRead', 'NetHttp', 'NotReal', 7, null, 'UiNotify']),
    ['FsRead', 'NetHttp', 'UiNotify'],
  )
})
