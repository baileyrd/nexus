// Pure-logic tests for the BL-049 block-link parser / serializer.
// Re-exported via `shell/tests/block-links.test.ts` so the default
// `pnpm test` glob picks them up.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  blockLinkAt,
  parseBlockLinks,
  serializeBlockLink,
} from './blockLinks.ts'

const A_UUID = 'd8e9f0a1-2b3c-4d5e-9f01-abcdef012345'
const B_UUID = '11111111-2222-3333-4444-555555555555'

test('parser returns no links for plain markdown', () => {
  const links = parseBlockLinks('# Heading\n\nsome [[wikilink]] text\n')
  assert.equal(links.length, 0)
})

test('parser recognises bare block link', () => {
  const text = `before [[Notes/A.md#^${A_UUID}]] after`
  const links = parseBlockLinks(text)
  assert.equal(links.length, 1)
  const l = links[0]
  assert.equal(l.filePath, 'Notes/A.md')
  assert.equal(l.blockId, A_UUID)
  assert.equal(l.label, null)
  assert.equal(text.slice(l.from, l.to), `[[Notes/A.md#^${A_UUID}]]`)
})

test('parser recognises pipe-aliased block link', () => {
  const text = `[[A.md#^${A_UUID}|see this]]`
  const links = parseBlockLinks(text)
  assert.equal(links.length, 1)
  assert.equal(links[0].label, 'see this')
})

test('parser rejects heading-anchor wikilinks (no UUID after #)', () => {
  const links = parseBlockLinks('[[A.md#some-heading]]')
  assert.equal(links.length, 0)
})

test('parser rejects malformed UUIDs even with the leading `#^`', () => {
  // Wrong length / non-hex.
  assert.equal(parseBlockLinks('[[A.md#^not-a-uuid]]').length, 0)
  assert.equal(parseBlockLinks('[[A.md#^d8e9f0a1]]').length, 0)
})

test('parser normalises UUID casing to lowercase', () => {
  const upper = A_UUID.toUpperCase()
  const links = parseBlockLinks(`[[A.md#^${upper}]]`)
  assert.equal(links.length, 1)
  assert.equal(links[0].blockId, A_UUID)
})

test('parser handles multiple links on the same line and reports correct ranges', () => {
  const text = `intro [[A.md#^${A_UUID}]] middle [[B.md#^${B_UUID}|alt]] end`
  const links = parseBlockLinks(text)
  assert.equal(links.length, 2)
  assert.ok(links[0].to <= links[1].from)
  assert.equal(text.slice(links[0].from, links[0].to), `[[A.md#^${A_UUID}]]`)
  assert.equal(links[1].label, 'alt')
})

test('parser is reentrant — concurrent scans share no state', () => {
  const text = `[[A.md#^${A_UUID}]]`
  // First scan.
  assert.equal(parseBlockLinks(text).length, 1)
  // Second concurrent scan also returns 1 — module-level regex
  // would have left lastIndex non-zero and missed it.
  assert.equal(parseBlockLinks(text).length, 1)
})

test('parser respects offset for line-relative scans', () => {
  const links = parseBlockLinks(`[[A.md#^${A_UUID}]]`, 100)
  assert.equal(links[0].from, 100)
})

// ── blockLinkAt ─────────────────────────────────────────────────────────────

test('blockLinkAt returns the link covering a position; null elsewhere', () => {
  const text = `prefix [[A.md#^${A_UUID}]] tail`
  const linkRange = parseBlockLinks(text)[0]
  assert.ok(blockLinkAt(text, linkRange.from + 3))
  assert.equal(blockLinkAt(text, 0), null)
  assert.equal(blockLinkAt(text, text.length - 1), null)
})

// ── serializeBlockLink ──────────────────────────────────────────────────────

test('serializer round-trips through the parser', () => {
  const out = serializeBlockLink('Notes/A.md', A_UUID)
  const links = parseBlockLinks(out)
  assert.equal(links.length, 1)
  assert.equal(links[0].filePath, 'Notes/A.md')
  assert.equal(links[0].blockId, A_UUID)
  assert.equal(links[0].label, null)
})

test('serializer round-trips with a pipe label', () => {
  const out = serializeBlockLink('A.md', A_UUID, 'see this')
  assert.equal(out, `[[A.md#^${A_UUID}|see this]]`)
  const links = parseBlockLinks(out)
  assert.equal(links[0].label, 'see this')
})

test('serializer rejects invalid UUIDs and empty paths', () => {
  assert.throws(() => serializeBlockLink('A.md', 'not-a-uuid'))
  assert.throws(() => serializeBlockLink('', A_UUID))
})
