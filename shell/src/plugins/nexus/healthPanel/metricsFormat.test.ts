// shell/src/plugins/nexus/healthPanel/metricsFormat.test.ts
//
// BL-093 follow-up — unit tests for the kernel-metrics formatter
// helpers. Pure functions: zero IPC, zero React, zero kernel mock —
// the goal is to nail the routing matrix that the live panel relies
// on.

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  buildCapabilityRows,
  buildEventBusRows,
  buildIpcRows,
  formatCount,
  formatDuration,
  type MetricsSnapshot,
} from './metricsFormat.ts'

function emptySnapshot(): MetricsSnapshot {
  return {
    ipc_calls_total: {},
    ipc_call_duration: {},
    event_bus_published_total: {},
    capability_checks_total: {},
    plugin_lifecycle_duration: {},
    event_bus_queue_depth: 0,
    metrics_dropped_total: 0,
  }
}

// ── formatDuration ──────────────────────────────────────────────────────────

test('formatDuration: zero / NaN / negative → em-dash', () => {
  assert.equal(formatDuration(0), '—')
  assert.equal(formatDuration(NaN), '—')
  assert.equal(formatDuration(-1), '—')
})

test('formatDuration: sub-microsecond renders as ns', () => {
  assert.equal(formatDuration(500), '500 ns')
  assert.equal(formatDuration(999), '999 ns')
})

test('formatDuration: microseconds, milliseconds, seconds with one decimal', () => {
  assert.equal(formatDuration(1_000), '1.0 µs')
  assert.equal(formatDuration(12_500), '12.5 µs')
  assert.equal(formatDuration(1_000_000), '1.0 ms')
  assert.equal(formatDuration(34_500_000), '34.5 ms')
  assert.equal(formatDuration(1_000_000_000), '1.00 s')
  assert.equal(formatDuration(2_500_000_000), '2.50 s')
})

// ── formatCount ─────────────────────────────────────────────────────────────

test('formatCount: groups thousands with commas', () => {
  assert.equal(formatCount(0), '0')
  assert.equal(formatCount(42), '42')
  assert.equal(formatCount(1_234), '1,234')
  assert.equal(formatCount(1_234_567), '1,234,567')
})

test('formatCount: NaN renders as "0" (defensive)', () => {
  assert.equal(formatCount(NaN), '0')
})

// ── buildIpcRows ────────────────────────────────────────────────────────────

test('buildIpcRows: joins counter rows with their duration histogram and sorts by total desc', () => {
  const snap = emptySnapshot()
  // Two commands, three statuses across them.
  snap.ipc_calls_total = {
    'com.nexus.storage::read_file::ok': 10,
    'com.nexus.storage::read_file::error': 2,
    'com.nexus.git::status::ok': 50,
  }
  snap.ipc_call_duration = {
    'com.nexus.storage::read_file': {
      count: 12,
      sum_ns: 120_000,
      mean_ns: 10_000,
      p50_ns: 8_000,
      p95_ns: 25_000,
      p99_ns: 40_000,
    },
    'com.nexus.git::status': {
      count: 50,
      sum_ns: 5_000_000,
      mean_ns: 100_000,
      p50_ns: 80_000,
      p95_ns: 200_000,
      p99_ns: 350_000,
    },
  }
  const rows = buildIpcRows(snap)
  assert.equal(rows.length, 2)
  // Sorted by total desc — git::status (50) first, then storage::read_file (12).
  assert.equal(rows[0]!.plugin, 'com.nexus.git')
  assert.equal(rows[0]!.command, 'status')
  assert.equal(rows[0]!.total, 50)
  assert.equal(rows[0]!.ok, 50)
  assert.equal(rows[0]!.errors, 0)
  assert.equal(rows[0]!.histogram?.p95_ns, 200_000)

  assert.equal(rows[1]!.plugin, 'com.nexus.storage')
  assert.equal(rows[1]!.command, 'read_file')
  assert.equal(rows[1]!.total, 12)
  assert.equal(rows[1]!.ok, 10)
  assert.equal(rows[1]!.errors, 2)
  assert.equal(rows[1]!.histogram?.p50_ns, 8_000)
})

test('buildIpcRows: a histogram with no matching counter still surfaces (with zero counts)', () => {
  const snap = emptySnapshot()
  snap.ipc_call_duration = {
    'com.nexus.ai::stream_chat': {
      count: 1,
      sum_ns: 1_000,
      mean_ns: 1_000,
      p50_ns: 1_000,
      p95_ns: 1_000,
      p99_ns: 1_000,
    },
  }
  const rows = buildIpcRows(snap)
  assert.equal(rows.length, 1)
  assert.equal(rows[0]!.plugin, 'com.nexus.ai')
  assert.equal(rows[0]!.command, 'stream_chat')
  assert.equal(rows[0]!.total, 0)
  assert.equal(rows[0]!.ok, 0)
  assert.equal(rows[0]!.errors, 0)
  assert.equal(rows[0]!.histogram?.p95_ns, 1_000)
})

test('buildIpcRows: malformed counter keys are skipped', () => {
  const snap = emptySnapshot()
  snap.ipc_calls_total = {
    'too::short': 5,
    onlyone: 5,
  }
  const rows = buildIpcRows(snap)
  assert.equal(rows.length, 0)
})

test('buildIpcRows: empty snapshot → empty array', () => {
  assert.deepEqual(buildIpcRows(emptySnapshot()), [])
})

test('buildIpcRows: non-ok statuses other than "error" still bucket as errors', () => {
  // The kernel emits five statuses: ok, capability_denied, not_found,
  // timeout, error. Only "ok" is non-error.
  const snap = emptySnapshot()
  snap.ipc_calls_total = {
    'plugin::cmd::ok': 1,
    'plugin::cmd::capability_denied': 2,
    'plugin::cmd::not_found': 3,
    'plugin::cmd::timeout': 4,
    'plugin::cmd::error': 5,
  }
  const rows = buildIpcRows(snap)
  assert.equal(rows.length, 1)
  assert.equal(rows[0]!.ok, 1)
  // Everything else lumps into errors.
  assert.equal(rows[0]!.errors, 2 + 3 + 4 + 5)
  assert.equal(rows[0]!.total, 15)
})

// ── buildEventBusRows ───────────────────────────────────────────────────────

test('buildEventBusRows: sorted by total desc, ties broken alphabetically', () => {
  const snap = emptySnapshot()
  snap.event_bus_published_total = {
    'com.nexus.ai': 10,
    'com.nexus.git': 30,
    'com.nexus.storage': 30,
  }
  const rows = buildEventBusRows(snap)
  assert.equal(rows.length, 3)
  // Two 30s — alphabetical break: git < storage.
  assert.equal(rows[0]!.plugin, 'com.nexus.git')
  assert.equal(rows[1]!.plugin, 'com.nexus.storage')
  assert.equal(rows[2]!.plugin, 'com.nexus.ai')
})

// ── buildCapabilityRows ────────────────────────────────────────────────────

test('buildCapabilityRows: granted vs denied bucketed; denied sorts first', () => {
  const snap = emptySnapshot()
  snap.capability_checks_total = {
    'pluginA::fs.read::granted': 100,
    'pluginA::fs.read::denied': 5,
    'pluginB::net.http::granted': 50,
    'pluginB::net.http::denied': 0,
  }
  const rows = buildCapabilityRows(snap)
  assert.equal(rows.length, 2)
  // pluginA has the denial, sorts first.
  assert.equal(rows[0]!.plugin, 'pluginA')
  assert.equal(rows[0]!.capability, 'fs.read')
  assert.equal(rows[0]!.granted, 100)
  assert.equal(rows[0]!.denied, 5)
  assert.equal(rows[1]!.plugin, 'pluginB')
  assert.equal(rows[1]!.capability, 'net.http')
  assert.equal(rows[1]!.denied, 0)
})

test('buildCapabilityRows: malformed keys are skipped', () => {
  const snap = emptySnapshot()
  snap.capability_checks_total = {
    'too::short': 5,
    'pluginA::cap::granted': 7,
  }
  const rows = buildCapabilityRows(snap)
  assert.equal(rows.length, 1)
  assert.equal(rows[0]!.plugin, 'pluginA')
})
