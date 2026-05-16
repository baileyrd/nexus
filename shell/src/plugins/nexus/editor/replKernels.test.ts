// BL-142 Phase 2a — unit tests for the replKernels config helpers.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  parseReplKernelsConfig,
  resolveKernelForLang,
  splitKernelCommand,
} from './replKernels.ts'

// ── parseReplKernelsConfig ───────────────────────────────────────────────────

test('parseReplKernelsConfig accepts a normal map', () => {
  const out = parseReplKernelsConfig('{"python":"python3 -i","node":"node --interactive"}')
  assert.deepEqual(out, {
    python: 'python3 -i',
    node: 'node --interactive',
  })
})

test('parseReplKernelsConfig returns empty object for malformed JSON', () => {
  assert.deepEqual(parseReplKernelsConfig('not json'), {})
  assert.deepEqual(parseReplKernelsConfig(''), {})
})

test('parseReplKernelsConfig returns empty object for a JSON array', () => {
  assert.deepEqual(parseReplKernelsConfig('["python3 -i"]'), {})
})

test('parseReplKernelsConfig drops non-string values silently', () => {
  const out = parseReplKernelsConfig('{"python":"python3 -i","broken":42,"alsobroken":null}')
  assert.deepEqual(out, { python: 'python3 -i' })
})

test('parseReplKernelsConfig drops whitespace-only commands', () => {
  const out = parseReplKernelsConfig('{"python":"python3 -i","empty":"   "}')
  assert.deepEqual(out, { python: 'python3 -i' })
})

// ── splitKernelCommand ───────────────────────────────────────────────────────

test('splitKernelCommand splits on whitespace', () => {
  assert.deepEqual(splitKernelCommand('python3 -i'), {
    program: 'python3',
    args: ['-i'],
  })
})

test('splitKernelCommand collapses runs of whitespace', () => {
  assert.deepEqual(splitKernelCommand('python3   -i   -q'), {
    program: 'python3',
    args: ['-i', '-q'],
  })
})

test('splitKernelCommand preserves double-quoted args as single tokens', () => {
  assert.deepEqual(splitKernelCommand('python3 -c "print(2 + 2)"'), {
    program: 'python3',
    args: ['-c', 'print(2 + 2)'],
  })
})

test('splitKernelCommand handles tabs as separators', () => {
  assert.deepEqual(splitKernelCommand('python3\t-i'), {
    program: 'python3',
    args: ['-i'],
  })
})

test('splitKernelCommand returns null for empty / whitespace-only input', () => {
  assert.equal(splitKernelCommand(''), null)
  assert.equal(splitKernelCommand('   '), null)
  assert.equal(splitKernelCommand('\t\n'), null)
})

test('splitKernelCommand handles bare program with no args', () => {
  assert.deepEqual(splitKernelCommand('python3'), {
    program: 'python3',
    args: [],
  })
})

// ── resolveKernelForLang ─────────────────────────────────────────────────────

test('resolveKernelForLang returns the parsed command for a configured lang', () => {
  const got = resolveKernelForLang(
    '{"python":"python3 -i"}',
    'python',
  )
  assert.deepEqual(got, { program: 'python3', args: ['-i'] })
})

test('resolveKernelForLang returns null for an unconfigured lang', () => {
  assert.equal(
    resolveKernelForLang('{"python":"python3 -i"}', 'ruby'),
    null,
  )
})

test('resolveKernelForLang returns null when the configured command is empty', () => {
  // Should not happen via parseReplKernelsConfig (which drops empty
  // values), but the layered guard means a manually-constructed map
  // doesn't crash either.
  assert.equal(
    resolveKernelForLang('{"python":""}', 'python'),
    null,
  )
})

test('resolveKernelForLang gracefully handles malformed config JSON', () => {
  assert.equal(resolveKernelForLang('not json', 'python'), null)
})
