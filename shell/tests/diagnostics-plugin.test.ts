// BL-141 follow-up — pure-helper tests for the diagnostics panel.
//
// The store + view pure functions (`bucketCounts`, `totalBuckets`,
// `composeHeader`, `buildFileGroups`) round-trip the LSP diagnostic
// shape that drives the panel. The subscriber + activity-bar wiring
// is covered by the shared plugin lifecycle tests; this file pins the
// projection shape so the panel doesn't silently drop or mis-render
// items.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import type { LspDiagnostic } from '../src/plugins/nexus/editor/cm/lspIpc'
import {
  bucketCounts,
  composeHeader,
  totalBuckets,
  type SeverityBuckets,
} from '../src/plugins/nexus/diagnostics/diagnosticsStore'
import { buildFileGroups } from '../src/plugins/nexus/diagnostics/DiagnosticsPanelView'

const FORGE = '/srv/forge'

function diag(line: number, severity?: number, message = 'm'): LspDiagnostic {
  return {
    range: {
      start: { line, character: 0 },
      end: { line, character: 3 },
    },
    severity: severity as LspDiagnostic['severity'],
    message,
  }
}

// ── bucketCounts ─────────────────────────────────────────────────────────────

test('bucketCounts groups by severity tag', () => {
  const out = bucketCounts([
    diag(0, 1),
    diag(1, 1),
    diag(2, 2),
    diag(3, 3),
    diag(4, 4),
  ])
  assert.deepEqual(out, { error: 2, warn: 1, info: 1, hint: 1 } as SeverityBuckets)
})

test('bucketCounts treats missing severity as error', () => {
  const out = bucketCounts([diag(0, undefined), diag(1, undefined)])
  assert.equal(out.error, 2)
  assert.equal(out.warn, 0)
})

test('bucketCounts on empty array returns zero buckets', () => {
  assert.deepEqual(bucketCounts([]), { error: 0, warn: 0, info: 0, hint: 0 })
})

// ── totalBuckets ─────────────────────────────────────────────────────────────

test('totalBuckets sums across every URI', () => {
  const map = new Map<string, LspDiagnostic[]>([
    ['file://x', [diag(0, 1), diag(1, 2)]],
    ['file://y', [diag(0, 2), diag(1, 3)]],
  ])
  assert.deepEqual(totalBuckets(map), {
    error: 1,
    warn: 2,
    info: 1,
    hint: 0,
  })
})

// ── composeHeader ────────────────────────────────────────────────────────────

test('composeHeader formats common pluralisation', () => {
  assert.equal(
    composeHeader({ error: 1, warn: 2, info: 0, hint: 0 }),
    '1 error · 2 warnings',
  )
})

test('composeHeader omits zero-count buckets', () => {
  assert.equal(
    composeHeader({ error: 0, warn: 0, info: 4, hint: 0 }),
    '4 info',
  )
})

test('composeHeader on all-zero returns empty (panel renders "no issues")', () => {
  assert.equal(
    composeHeader({ error: 0, warn: 0, info: 0, hint: 0 }),
    '',
  )
})

// ── buildFileGroups ──────────────────────────────────────────────────────────

test('buildFileGroups drops out-of-forge URIs and sorts files by relpath', () => {
  const map = new Map<string, LspDiagnostic[]>([
    ['file:///srv/forge/z.md', [diag(0, 1)]],
    ['file:///srv/forge/a.md', [diag(0, 2)]],
    ['file:///elsewhere/out.md', [diag(0, 1)]],
  ])
  const groups = buildFileGroups(map, FORGE)
  assert.equal(groups.length, 2)
  assert.deepEqual(groups.map((g) => g.relpath), ['a.md', 'z.md'])
})

test('buildFileGroups sorts diagnostics within a file by (line, character)', () => {
  const map = new Map<string, LspDiagnostic[]>([
    ['file:///srv/forge/x.md', [diag(10, 1), diag(2, 2), diag(5, 1)]],
  ])
  const groups = buildFileGroups(map, FORGE)
  assert.deepEqual(
    groups[0].diagnostics.map((d) => d.range.start.line),
    [2, 5, 10],
  )
})

test('buildFileGroups returns empty when no forge root', () => {
  const map = new Map<string, LspDiagnostic[]>([
    ['file:///srv/forge/x.md', [diag(0, 1)]],
  ])
  assert.deepEqual(buildFileGroups(map, null), [])
})

test('buildFileGroups drops files with zero diagnostics (caller already deletes on publish, but defensive)', () => {
  const map = new Map<string, LspDiagnostic[]>([
    ['file:///srv/forge/a.md', []],
    ['file:///srv/forge/b.md', [diag(0, 1)]],
  ])
  const groups = buildFileGroups(map, FORGE)
  assert.equal(groups.length, 1)
  assert.equal(groups[0].relpath, 'b.md')
})

test('buildFileGroups annotates each group with bucketCounts', () => {
  const map = new Map<string, LspDiagnostic[]>([
    ['file:///srv/forge/x.md', [diag(0, 1), diag(1, 1), diag(2, 2)]],
  ])
  const groups = buildFileGroups(map, FORGE)
  assert.deepEqual(groups[0].buckets, {
    error: 2,
    warn: 1,
    info: 0,
    hint: 0,
  })
})
