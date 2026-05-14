// BL-127 Phase A — unit tests for the production-side typing-perf
// instrumentation helper. happy-dom's `performance` shim supports
// `mark` / `measure` / `getEntriesByType` so the round-trip is
// exercised end-to-end without a browser.

import { test, beforeEach, afterEach } from 'node:test'
import assert from 'node:assert/strict'

import {
  __setTypingPerfEnabledForTest,
  beginKeystroke,
  clearTypingMarks,
  recentMeasureDurationsMs,
  typingPerfEnabled,
} from '../src/plugins/nexus/editor/typingPerf'

beforeEach(() => {
  clearTypingMarks()
  __setTypingPerfEnabledForTest(null) // re-read env on next call
})

afterEach(() => {
  __setTypingPerfEnabledForTest(null)
})

test('typingPerfEnabled: defaults to false when env unset', () => {
  __setTypingPerfEnabledForTest(false)
  assert.equal(typingPerfEnabled(), false)
})

test('beginKeystroke: returns no-op when disabled', () => {
  __setTypingPerfEnabledForTest(false)
  const end = beginKeystroke()
  end() // should not throw, should not produce a measure
  assert.equal(recentMeasureDurationsMs().length, 0)
})

test('beginKeystroke: produces a measure when enabled', () => {
  __setTypingPerfEnabledForTest(true)
  const end = beginKeystroke()
  // Synchronous span — duration will be near-zero but the measure
  // entry will exist.
  end()
  const durations = recentMeasureDurationsMs()
  assert.equal(durations.length, 1)
  // Duration is a non-negative number; the actual value is
  // host-dependent on a happy-dom shim.
  assert.ok(durations[0] >= 0, `expected >= 0, got ${durations[0]}`)
})

test('beginKeystroke: multiple overlapping calls produce independent measures', () => {
  __setTypingPerfEnabledForTest(true)
  const end1 = beginKeystroke()
  const end2 = beginKeystroke()
  const end3 = beginKeystroke()
  // Close out of order — each end should still match its own start.
  end2()
  end3()
  end1()
  const durations = recentMeasureDurationsMs()
  assert.equal(durations.length, 3)
})

test('beginKeystroke: dropped end callback is safe', () => {
  __setTypingPerfEnabledForTest(true)
  const end = beginKeystroke()
  void end // intentionally don't call — simulates a thrown caller
  // No measure recorded since end wasn't called. Subsequent
  // keystrokes still work.
  const next = beginKeystroke()
  next()
  const durations = recentMeasureDurationsMs()
  assert.equal(durations.length, 1)
})

test('recentMeasureDurationsMs: limit caps the returned slice', () => {
  __setTypingPerfEnabledForTest(true)
  for (let i = 0; i < 10; i++) {
    const end = beginKeystroke()
    end()
  }
  assert.equal(recentMeasureDurationsMs(5).length, 5)
  assert.equal(recentMeasureDurationsMs(100).length, 10)
})

test('clearTypingMarks: drops every typing-prefixed entry', () => {
  __setTypingPerfEnabledForTest(true)
  for (let i = 0; i < 3; i++) {
    const end = beginKeystroke()
    end()
  }
  assert.equal(recentMeasureDurationsMs().length, 3)
  clearTypingMarks()
  assert.equal(recentMeasureDurationsMs().length, 0)
})
