// BL-084 unit tests for the conflict-marker parser. Re-exported via
// `tests/conflict-parser.test.ts` so the top-level glob picks them up.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  applyAll,
  applyResolution,
  parseConflicts,
  type ConflictHunk,
} from './conflictParser.ts'

const SIMPLE = [
  'context above\n',
  '<<<<<<< HEAD\n',
  'our line\n',
  '=======\n',
  'their line\n',
  '>>>>>>> feature\n',
  'context below\n',
].join('')

test('parses a single conflict block with HEAD / branch labels', () => {
  const { hunks } = parseConflicts(SIMPLE)
  assert.equal(hunks.length, 1)
  const h = hunks[0]
  assert.equal(h.oursLabel, 'HEAD')
  assert.equal(h.theirsLabel, 'feature')
  assert.equal(h.ours, 'our line\n')
  assert.equal(h.theirs, 'their line\n')
  assert.equal(h.base, null)
  // The hunk byte range should land on the markers, leaving the
  // surrounding context untouched on either side.
  assert.equal(SIMPLE.slice(h.start, h.end).startsWith('<<<<<<<'), true)
  assert.equal(SIMPLE.slice(h.start, h.end).endsWith('feature\n'), true)
})

test('captures the diff3 ancestor block when present', () => {
  const doc = [
    '<<<<<<< ours\n',
    'A\n',
    '|||||||  base\n',
    'X\n',
    '=======\n',
    'B\n',
    '>>>>>>> theirs\n',
  ].join('')
  const { hunks } = parseConflicts(doc)
  assert.equal(hunks.length, 1)
  assert.equal(hunks[0].ours, 'A\n')
  assert.equal(hunks[0].base, 'X\n')
  assert.equal(hunks[0].theirs, 'B\n')
})

test('finds multiple hunks in document order', () => {
  const doc = [
    '<<<<<<< HEAD\n',
    'a-ours\n',
    '=======\n',
    'a-theirs\n',
    '>>>>>>> b\n',
    'middle\n',
    '<<<<<<< HEAD\n',
    'c-ours\n',
    '=======\n',
    'c-theirs\n',
    '>>>>>>> d\n',
  ].join('')
  const { hunks } = parseConflicts(doc)
  assert.equal(hunks.length, 2)
  assert.equal(hunks[0].ours, 'a-ours\n')
  assert.equal(hunks[1].ours, 'c-ours\n')
  // Hunks shouldn't overlap.
  assert.ok(hunks[0].end <= hunks[1].start)
})

test('handles CRLF line endings without losing offsets', () => {
  const doc = [
    'ctx\r\n',
    '<<<<<<< HEAD\r\n',
    'O\r\n',
    '=======\r\n',
    'T\r\n',
    '>>>>>>> branch\r\n',
  ].join('')
  const { hunks } = parseConflicts(doc)
  assert.equal(hunks.length, 1)
  // Body sides preserve their original line endings.
  assert.equal(hunks[0].ours, 'O\r\n')
  assert.equal(hunks[0].theirs, 'T\r\n')
  // Range covers the marker triple verbatim.
  assert.equal(doc.slice(hunks[0].start, hunks[0].end).startsWith('<<<<<<<'), true)
})

test('handles a final-line conflict with no trailing newline', () => {
  const doc = '<<<<<<< HEAD\nO\n=======\nT\n>>>>>>> branch'
  const { hunks } = parseConflicts(doc)
  assert.equal(hunks.length, 1)
  assert.equal(hunks[0].end, doc.length, 'hunk end clamps to EOF')
})

test('skips a malformed block (start with no closing marker) without crashing', () => {
  const doc = '<<<<<<< HEAD\nO\n=======\nT\n(no closing marker here)\n'
  const { hunks } = parseConflicts(doc)
  assert.equal(hunks.length, 0)
})

test('nested-start: bails the outer block and recovers on the inner one', () => {
  // A second `<<<<<<<` before a closing marker is malformed for the
  // outer block (it'd be ambiguous which side `inner` belongs to).
  // The parser drops the outer hunk entirely and re-scans starting at
  // the next line — which finds the inner block as a clean conflict.
  // The user-visible safety property: `applyResolution` still only
  // touches the inner block's byte range, so the outer literal
  // `<<<<<<<` line stays in the file and the user can see something
  // is off.
  const doc = [
    '<<<<<<< HEAD\n',
    'outer-ours\n',
    '<<<<<<< HEAD\n',
    'inner\n',
    '=======\n',
    'theirs\n',
    '>>>>>>> branch\n',
  ].join('')
  const { hunks } = parseConflicts(doc)
  assert.equal(hunks.length, 1)
  assert.equal(hunks[0].ours, 'inner\n')
  // The outer `<<<<<<<` line is *not* part of the parsed hunk's range.
  assert.equal(doc.slice(hunks[0].start, hunks[0].end).startsWith('<<<<<<< HEAD'), true)
  assert.ok(hunks[0].start > 0, 'hunk starts past the outer marker line')
})

// ── applyResolution / applyAll ─────────────────────────────────────────────

test('applyResolution swaps the conflict block for the chosen side', () => {
  const { hunks } = parseConflicts(SIMPLE)
  const out = applyResolution(SIMPLE, hunks[0], hunks[0].ours)
  assert.equal(out, 'context above\nour line\ncontext below\n')
})

test('applyResolution preserves trailing-newline shape from the original block', () => {
  // Block ends WITH a newline → resolution gets one if missing.
  const { hunks } = parseConflicts(SIMPLE)
  const noNl = 'kept without newline'
  const out = applyResolution(SIMPLE, hunks[0], noNl)
  assert.equal(out.includes('kept without newline\n'), true)

  // Block ends WITHOUT a newline (final-line conflict) → resolution
  // is tail-trimmed if it has one.
  const tail = '<<<<<<< HEAD\nO\n=======\nT\n>>>>>>> branch'
  const tailParsed = parseConflicts(tail)
  const tailOut = applyResolution(tail, tailParsed.hunks[0], 'pick\n')
  assert.equal(tailOut, 'pick')
})

test('applyAll resolves every block to the same side', () => {
  const doc = [
    '<<<<<<< HEAD\n',
    'one-ours\n',
    '=======\n',
    'one-theirs\n',
    '>>>>>>> b\n',
    '---\n',
    '<<<<<<< HEAD\n',
    'two-ours\n',
    '=======\n',
    'two-theirs\n',
    '>>>>>>> b\n',
  ].join('')
  const parsed = parseConflicts(doc)
  const out = applyAll(doc, parsed, 'theirs')
  assert.equal(out, 'one-theirs\n---\ntwo-theirs\n')
})

test('parseConflicts: clean file produces empty hunks', () => {
  assert.deepEqual(parseConflicts('no markers here\nat all\n').hunks, [] as ConflictHunk[])
})
