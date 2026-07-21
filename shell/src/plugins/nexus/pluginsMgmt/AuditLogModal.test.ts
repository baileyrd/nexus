// C84 (#437) — unit tests for the pure formatting/filtering helpers
// behind the per-plugin audit-log overlay.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  filterAuditEntries,
  formatDetail,
  isDenialEvent,
} from './AuditLogModal.tsx'

test('formatDetail pretty-prints valid JSON', () => {
  assert.equal(formatDetail('{"capability":"net.http"}'), '{"capability":"net.http"}')
})

test('formatDetail falls back to the raw string on parse failure', () => {
  assert.equal(formatDetail('not json'), 'not json')
})

test('isDenialEvent matches any event type containing "denied"', () => {
  assert.equal(isDenialEvent('capability_denied'), true)
  assert.equal(isDenialEvent('path_traversal_denied'), true)
  assert.equal(isDenialEvent('capability_granted'), false)
  assert.equal(isDenialEvent('plugin_loaded'), false)
})

test('filterAuditEntries returns everything for an empty/blank query', () => {
  const entries = [
    { event_type: 'capability_denied', detail_json: '{"cap":"net.http"}' },
    { event_type: 'plugin_loaded', detail_json: '{}' },
  ]
  assert.deepEqual(filterAuditEntries(entries, ''), entries)
  assert.deepEqual(filterAuditEntries(entries, '   '), entries)
})

test('filterAuditEntries matches against event_type case-insensitively', () => {
  const entries = [
    { event_type: 'capability_denied', detail_json: '{}' },
    { event_type: 'plugin_loaded', detail_json: '{}' },
  ]
  const result = filterAuditEntries(entries, 'DENIED')
  assert.equal(result.length, 1)
  assert.equal(result[0]?.event_type, 'capability_denied')
})

test('filterAuditEntries matches against detail_json contents', () => {
  const entries = [
    { event_type: 'capability_denied', detail_json: '{"capability":"net.http"}' },
    { event_type: 'capability_denied', detail_json: '{"capability":"fs.write"}' },
  ]
  const result = filterAuditEntries(entries, 'fs.write')
  assert.equal(result.length, 1)
  assert.equal(result[0]?.detail_json, '{"capability":"fs.write"}')
})

test('filterAuditEntries returns an empty array when nothing matches', () => {
  const entries = [{ event_type: 'plugin_loaded', detail_json: '{}' }]
  assert.deepEqual(filterAuditEntries(entries, 'nonexistent'), [])
})
