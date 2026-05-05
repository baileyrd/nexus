// shell/src/plugins/nexus/recall/insertFormat.test.ts
//
// BL-044 — pure formatter coverage. Pins the markdown shape so a
// future tweak to the quote layout shows up as a snapshot diff.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { formatRecallLink, formatRecallSnippet } from './insertFormat.ts'

test('formats a single-line chunk as a quote block with [[basename]] attribution', () => {
  const out = formatRecallSnippet({
    file_path: 'Inbox.md',
    chunk_text: 'A short captured thought.',
    score: 0.9,
  })
  assert.equal(out, '> A short captured thought.\n>\n> — [[Inbox]]\n')
})

test('preserves internal newlines by prefixing every line with "> "', () => {
  const out = formatRecallSnippet({
    file_path: 'notes/Captures/2026-04-29.md',
    chunk_text: 'first line\nsecond line\n  third with leading spaces',
    score: 0.8,
  })
  assert.equal(
    out,
    '> first line\n> second line\n>   third with leading spaces\n>\n> — [[2026-04-29]]\n',
  )
})

test('trims surrounding whitespace before quoting', () => {
  const out = formatRecallSnippet({
    file_path: 'Inbox.md',
    chunk_text: '   padded\n   ',
    score: 0.5,
  })
  assert.equal(out, '> padded\n>\n> — [[Inbox]]\n')
})

test('handles empty chunk_text without producing a malformed block', () => {
  const out = formatRecallSnippet({
    file_path: 'Inbox.md',
    chunk_text: '',
    score: 0.0,
  })
  assert.equal(out, '>\n>\n> — [[Inbox]]\n')
})

test('strips .md suffix only (not .markdown / no suffix)', () => {
  assert.equal(
    formatRecallSnippet({ file_path: 'a/b/Foo.md', chunk_text: 'x', score: 0 }),
    '> x\n>\n> — [[Foo]]\n',
  )
  assert.equal(
    formatRecallSnippet({ file_path: 'a/b/Foo.markdown', chunk_text: 'x', score: 0 }),
    '> x\n>\n> — [[Foo.markdown]]\n',
  )
  assert.equal(
    formatRecallSnippet({ file_path: 'NoSuffix', chunk_text: 'x', score: 0 }),
    '> x\n>\n> — [[NoSuffix]]\n',
  )
})

// ── AIG-06 — formatRecallLink ───────────────────────────────────────

test('formatRecallLink returns a bare wikilink to the basename', () => {
  assert.equal(
    formatRecallLink({ file_path: 'a/b/Foo.md', chunk_text: 'ignored', score: 0 }),
    '[[Foo]]',
  )
  assert.equal(
    formatRecallLink({ file_path: 'NoSuffix', chunk_text: '', score: 0 }),
    '[[NoSuffix]]',
  )
})

test('formatRecallLink ignores chunk_text — quote body is dropped', () => {
  const out = formatRecallLink({
    file_path: 'Inbox.md',
    chunk_text: 'A long\nmulti-line\ncapture',
    score: 0.5,
  })
  assert.equal(out, '[[Inbox]]')
})
