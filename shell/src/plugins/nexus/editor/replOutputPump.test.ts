// BL-142 Phase 2b.2 — unit tests for the pure factors used by the
// REPL output pump. The actual bus subscription side-effect is
// covered by an end-to-end test against a real kernel runtime; the
// helpers here (`decodeBytes`, `sessionIdFromTopic`) are pure and
// trivially testable.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { decodeBytes, sessionIdFromTopic } from './replOutputPump.ts'

test('decodeBytes converts a Vec<u8> wire form to UTF-8 text', () => {
  // "hello\n" = 68 65 6c 6c 6f 0a
  const bytes = [0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x0a]
  assert.equal(decodeBytes(bytes), 'hello\n')
})

test('decodeBytes handles empty input', () => {
  assert.equal(decodeBytes([]), '')
})

test('decodeBytes maps invalid UTF-8 to U+FFFD (lossy)', () => {
  // 0xff is never a valid start byte; non-fatal decoder yields
  // the replacement character.
  const decoded = decodeBytes([0xff, 0x65])
  assert.ok(decoded.includes('�'), `expected U+FFFD in: ${decoded}`)
})

test('decodeBytes preserves multi-byte UTF-8 codepoints', () => {
  // ✓ (U+2713) = 0xe2 0x9c 0x93
  assert.equal(decodeBytes([0xe2, 0x9c, 0x93]), '✓')
})

test('sessionIdFromTopic extracts the trailing id', () => {
  assert.equal(
    sessionIdFromTopic('com.nexus.terminal.output.session-abc-123'),
    'session-abc-123',
  )
})

test('sessionIdFromTopic returns null for unrelated topics', () => {
  assert.equal(sessionIdFromTopic('com.nexus.editor.changed.note.md'), null)
  assert.equal(sessionIdFromTopic(''), null)
})

test('sessionIdFromTopic returns the empty string for a bare prefix', () => {
  // Defensive — the server shouldn't emit this but the parser
  // shouldn't crash on it either.
  assert.equal(sessionIdFromTopic('com.nexus.terminal.output.'), '')
})
