// BL-058 unit tests for the TS port of `urls.rs` (URL detection + the
// stream-aware `urlExtractor`). Re-exported via `tests/terminal-urls.test.ts`
// so the top-level glob picks them up.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { detectUrls, resolveUrl } from './urls.ts'
import { createUrlExtractor } from './urlExtractor.ts'

// ── detectUrls ───────────────────────────────────────────────────────────────

test('detectUrls: empty string returns no matches', () => {
  assert.deepEqual(detectUrls('just some words, no urls here'), [])
})

test('detectUrls: a single https URL is detected and resolved unchanged', () => {
  const hits = detectUrls('see https://example.com for details')
  assert.equal(hits.length, 1)
  assert.equal(hits[0].kind, 'HttpHttps')
  assert.equal(hits[0].raw, 'https://example.com')
  assert.equal(hits[0].resolved, 'https://example.com')
})

test('detectUrls: trailing punctuation is stripped from the match', () => {
  const hits = detectUrls('go to https://example.com.')
  assert.equal(hits.length, 1)
  assert.equal(hits[0].raw, 'https://example.com')

  const wrapped = detectUrls('see (https://example.com), etc')
  assert.equal(wrapped.length, 1)
  assert.equal(wrapped[0].raw, 'https://example.com')
})

test('detectUrls: multiple URLs on one line are returned in document order', () => {
  const hits = detectUrls('first https://a.example then https://b.example end')
  assert.equal(hits.length, 2)
  assert.equal(hits[0].raw, 'https://a.example')
  assert.equal(hits[1].raw, 'https://b.example')
})

test('detectUrls: bare localhost:PORT is detected and resolved to loopback', () => {
  const hits = detectUrls('Server listening at localhost:3000')
  assert.equal(hits.length, 1)
  assert.equal(hits[0].kind, 'Localhost')
  assert.equal(hits[0].raw, 'localhost:3000')
  assert.equal(hits[0].resolved, 'http://127.0.0.1:3000')
})

test('detectUrls: 127.0.0.1:PORT without scheme gets http:// prefix', () => {
  const hits = detectUrls('bound to 127.0.0.1:8080/health')
  assert.equal(hits.length, 1)
  assert.equal(hits[0].kind, 'Localhost')
  assert.equal(hits[0].resolved, 'http://127.0.0.1:8080/health')
})

test('detectUrls: https://localhost:3000 surfaces once as HttpHttps, not also Localhost', () => {
  const hits = detectUrls('Open https://localhost:3000/dashboard')
  assert.equal(hits.length, 1)
  assert.equal(hits[0].kind, 'HttpHttps')
})

test('detectUrls: file:// URLs are detected', () => {
  const hits = detectUrls('error at file:///tmp/error.log:42')
  assert.equal(hits.length, 1)
  assert.equal(hits[0].kind, 'File')
  assert.equal(hits[0].raw, 'file:///tmp/error.log:42')
})

test('detectUrls: bare localhost without port is NOT matched (too noisy)', () => {
  assert.deepEqual(detectUrls('connecting to localhost soon'), [])
})

test('detectUrls: localhost embedded in a larger word does not match', () => {
  // Word boundary in front of the regex prevents `nolocalhost:1234`
  // from matching.
  assert.deepEqual(detectUrls('mynolocalhost:1234 is bogus'), [])
})

// ── resolveUrl ──────────────────────────────────────────────────────────────

test('resolveUrl: well-formed schemes pass through verbatim', () => {
  assert.equal(resolveUrl('https://example.com'), 'https://example.com')
  assert.equal(resolveUrl('http://example.com'), 'http://example.com')
  assert.equal(resolveUrl('file:///tmp'), 'file:///tmp')
})

test('resolveUrl: localhost / 127.0.0.1 get http:// synthesised', () => {
  assert.equal(resolveUrl('localhost:3000'), 'http://127.0.0.1:3000')
  assert.equal(resolveUrl('127.0.0.1:8080'), 'http://127.0.0.1:8080')
})

// ── createUrlExtractor ──────────────────────────────────────────────────────

function bytes(text: string): Uint8Array {
  return new TextEncoder().encode(text)
}

test('extractor: emits URLs once each containing line completes', () => {
  const seen: string[] = []
  const ex = createUrlExtractor((m) => seen.push(m.raw))
  ex.push(bytes('Server up at https://example.com\n'))
  assert.deepEqual(seen, ['https://example.com'])
})

test('extractor: holds partial lines until a newline arrives', () => {
  const seen: string[] = []
  const ex = createUrlExtractor((m) => seen.push(m.raw))
  ex.push(bytes('Server up at https://exa'))
  assert.deepEqual(seen, [], 'no newline → no emit yet')
  ex.push(bytes('mple.com\n'))
  assert.deepEqual(seen, ['https://example.com'])
})

test('extractor: flush() drains a buffered line without a trailing newline', () => {
  const seen: string[] = []
  const ex = createUrlExtractor((m) => seen.push(m.raw))
  ex.push(bytes('check https://example.com'))
  ex.flush()
  assert.deepEqual(seen, ['https://example.com'])
})

test('extractor: reset() drops the buffered partial line', () => {
  const seen: string[] = []
  const ex = createUrlExtractor((m) => seen.push(m.raw))
  ex.push(bytes('partial https://drop.example'))
  ex.reset()
  ex.push(bytes('next line\n'))
  assert.deepEqual(seen, [])
})

test('extractor: CRLF line endings are stripped from the extracted line', () => {
  const seen: string[] = []
  const ex = createUrlExtractor((m) => seen.push(m.raw))
  ex.push(bytes('Server at https://example.com\r\n'))
  // The trailing \r is stripped before detection, so the URL doesn't
  // pick up a stray CR character.
  assert.deepEqual(seen, ['https://example.com'])
})

test('extractor: split UTF-8 multi-byte sequence at the chunk boundary survives', () => {
  // "café https://example.com\n" — the é is two bytes (0xC3 0xA9).
  // Split exactly between them and confirm the URL still surfaces.
  const full = bytes('café https://example.com\n')
  const splitAt = full.indexOf(0xa9) // second byte of é
  const a = full.slice(0, splitAt)
  const b = full.slice(splitAt)
  const seen: string[] = []
  const ex = createUrlExtractor((m) => seen.push(m.raw))
  ex.push(a)
  ex.push(b)
  assert.deepEqual(seen, ['https://example.com'])
})
