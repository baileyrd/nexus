// Pure-logic tests for the BL-046 phase-2 recall filter.
// Re-exported via `shell/tests/code-filter.test.ts`.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  applyCodeFilter,
  extractCodeLanguages,
  isCodeCaptureMatch,
} from './codeFilter.ts'
import type { RecallMatch } from './recallStore.ts'
import { useRecallStore } from './recallStore.ts'

function match(text: string, file = 'Inbox.md'): RecallMatch {
  return { file_path: file, chunk_text: text, score: 0.9 }
}

// ── isCodeCaptureMatch ─────────────────────────────────────────────────────

test('isCodeCaptureMatch: positive on the canonical #code/<lang> tag', () => {
  assert.equal(
    isCodeCaptureMatch(match('… arbitrary prefix\n#code/rust\n')),
    true,
  )
})

test('isCodeCaptureMatch: positive on a leading File: header', () => {
  assert.equal(
    isCodeCaptureMatch(match('## Captured at 2026-04-30T...\n\nFile: src/main.rs\n')),
    true,
  )
})

test('isCodeCaptureMatch: positive on a fenced opener with language tag', () => {
  assert.equal(
    isCodeCaptureMatch(match('arbitrary text\n```typescript\nlet x = 1;\n```\n')),
    true,
  )
})

test('isCodeCaptureMatch: negative on a plain capture (no fence, no #code, no File:)', () => {
  assert.equal(isCodeCaptureMatch(match('Quick thought about lunch.')), false)
  assert.equal(isCodeCaptureMatch(match('')), false)
})

test('isCodeCaptureMatch: positive on a #code tag at the very start of the chunk', () => {
  // The leading-newline anchor in the regex must also accept the
  // string-start case — otherwise a chunk that begins with the
  // tag (no preceding header) would miss.
  assert.equal(isCodeCaptureMatch(match('#code/python\nprint(1)')), true)
})

test('isCodeCaptureMatch: negative on a fenced block with no language tag', () => {
  // A bare ``` opener doesn't match the language-tagged regex —
  // we only want to match real code-capture fences, not arbitrary
  // user-typed fences.
  assert.equal(isCodeCaptureMatch(match('thought\n```\nplain\n```')), false)
})

// ── applyCodeFilter ────────────────────────────────────────────────────────

test('applyCodeFilter: passthrough when codeOnly is false', () => {
  const list = [match('thought'), match('#code/rust\nfn x() {}')]
  assert.equal(applyCodeFilter(list, false), list)
})

test('applyCodeFilter: keeps code captures, drops the rest, preserves order', () => {
  const a = match('thought one')
  const b = match('#code/rust\nfn x() {}', 'a.md')
  const c = match('thought two')
  const d = match('File: src/main.ts\n```typescript\nconst x = 1;\n```', 'b.md')
  const out = applyCodeFilter([a, b, c, d], true)
  assert.deepEqual(out, [b, d])
})

test('applyCodeFilter: empty input → empty output', () => {
  assert.deepEqual(applyCodeFilter([], true), [])
})

// ── extractCodeLanguages ───────────────────────────────────────────────────

test('extractCodeLanguages: deduplicates tags + fences across the chunk', () => {
  const out = extractCodeLanguages(
    match(
      '#code/rust\n```rust\nfn x() {}\n```\nlater #code/Rust again',
    ),
  )
  // Lowercased + deduped (case-insensitive set).
  assert.deepEqual(out.sort(), ['rust'])
})

test('extractCodeLanguages: multi-language captures expose every tag', () => {
  // Tags on their own lines (the canonical form emitted by
  // BL-046 phase 1) plus a fenced opener — all three should be
  // collected.
  const out = extractCodeLanguages(
    match('preamble\n#code/rust\nbody\n#code/typescript\n```python\np=1\n```'),
  )
  assert.deepEqual(out.sort(), ['python', 'rust', 'typescript'])
})

test('extractCodeLanguages: returns empty array for plain text', () => {
  assert.deepEqual(extractCodeLanguages(match('plain note')), [])
})

// ── store: setCodeOnly ─────────────────────────────────────────────────────

test('store: setCodeOnly toggles the flag and reclamps selectedIndex', () => {
  // Reset store to a known shape.
  useRecallStore.setState({
    visible: true,
    query: '',
    results: [
      match('thought one'),
      match('#code/rust\nfn x() {}'),
      match('thought two'),
      match('File: a.ts\n```typescript\nconst x = 1\n```'),
    ],
    selectedIndex: 3,
    status: 'idle',
    error: null,
    currentRequestId: null,
    codeOnly: false,
  })
  useRecallStore.getState().setCodeOnly(true)
  // Two code matches survive; selectedIndex = 3 must clamp to 1.
  assert.equal(useRecallStore.getState().codeOnly, true)
  assert.equal(useRecallStore.getState().selectedIndex, 1)

  useRecallStore.getState().setCodeOnly(false)
  // Toggling off doesn't widen selection beyond the current index.
  assert.equal(useRecallStore.getState().codeOnly, false)
  assert.equal(useRecallStore.getState().selectedIndex, 1)
})

test('store: open() resets codeOnly to false', () => {
  useRecallStore.setState({ codeOnly: true })
  useRecallStore.getState().open()
  assert.equal(useRecallStore.getState().codeOnly, false)
})
