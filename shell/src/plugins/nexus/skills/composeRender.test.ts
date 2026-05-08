// shell/src/plugins/nexus/skills/composeRender.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { splitMergedBody, fragmentTint } from './composeRender.ts'

const FRAGMENTS = [
  { id: 'a', name: 'Skill A' },
  { id: 'b', name: 'Skill B' },
  { id: 'c', name: 'Skill C' },
]

function buildMerged(parts: Array<{ name: string; id: string; body: string }>): string {
  return parts
    .map((p) => `## Skill: ${p.name} [${p.id}]\n${p.body}`)
    .join('\n\n')
}

test('splitMergedBody: empty input returns one unattributed span', () => {
  const spans = splitMergedBody('', FRAGMENTS)
  assert.equal(spans.length, 1)
  assert.equal(spans[0].fragmentId, null)
  assert.equal(spans[0].text, '')
})

test('splitMergedBody: empty fragments list returns one unattributed span', () => {
  const spans = splitMergedBody('hello', [])
  assert.equal(spans.length, 1)
  assert.equal(spans[0].fragmentId, null)
  assert.equal(spans[0].text, 'hello')
})

test('splitMergedBody: single fragment yields heading + body span', () => {
  const merged = buildMerged([{ name: 'Skill A', id: 'a', body: 'BODY_A' }])
  const spans = splitMergedBody(merged, [FRAGMENTS[0]])
  assert.equal(spans.length, 2)
  assert.equal(spans[0].fragmentId, 'a')
  assert.equal(spans[0].isHeading, true)
  assert.ok(spans[0].text.startsWith('## Skill: Skill A [a]'))
  assert.equal(spans[1].fragmentId, 'a')
  assert.equal(spans[1].isHeading, false)
  assert.ok(spans[1].text.includes('BODY_A'))
})

test('splitMergedBody: three fragments yield 6 spans (heading+body each)', () => {
  const merged = buildMerged([
    { name: 'Skill A', id: 'a', body: 'aaa' },
    { name: 'Skill B', id: 'b', body: 'bbb' },
    { name: 'Skill C', id: 'c', body: 'ccc' },
  ])
  const spans = splitMergedBody(merged, FRAGMENTS)
  assert.equal(spans.length, 6)
  // Spans alternate heading / body for each fragment.
  assert.equal(spans[0].fragmentId, 'a')
  assert.equal(spans[0].isHeading, true)
  assert.equal(spans[1].fragmentId, 'a')
  assert.equal(spans[1].isHeading, false)
  assert.equal(spans[2].fragmentId, 'b')
  assert.equal(spans[2].isHeading, true)
  assert.equal(spans[3].fragmentId, 'b')
  assert.equal(spans[3].isHeading, false)
  assert.equal(spans[4].fragmentId, 'c')
  assert.equal(spans[4].isHeading, true)
  assert.equal(spans[5].fragmentId, 'c')
  assert.equal(spans[5].isHeading, false)
})

test('splitMergedBody: round-trip preserves the original text', () => {
  const merged = buildMerged([
    { name: 'Skill A', id: 'a', body: 'aaa\nline2' },
    { name: 'Skill B', id: 'b', body: 'bbb' },
  ])
  const spans = splitMergedBody(merged, FRAGMENTS.slice(0, 2))
  const reconstructed = spans.map((s) => s.text).join('')
  assert.equal(reconstructed, merged)
})

test('splitMergedBody: heading-only fragment emits heading span without body', () => {
  // Mirrors the kernel's behaviour for heading-only ancestors.
  const merged = `## Skill: Skill A [a]\n\n## Skill: Skill B [b]\nbbb`
  const spans = splitMergedBody(merged, FRAGMENTS.slice(0, 2))
  // Spans for `a`: heading line. Then spans for `b`: heading line + body.
  // The blank-line separator between fragments lands in `a`'s body slot.
  const aSpans = spans.filter((s) => s.fragmentId === 'a')
  const bSpans = spans.filter((s) => s.fragmentId === 'b')
  assert.ok(aSpans.length >= 1)
  assert.equal(aSpans[0].isHeading, true)
  assert.ok(bSpans.length >= 1)
  assert.equal(bSpans[0].isHeading, true)
  assert.ok(bSpans.some((s) => !s.isHeading && s.text.includes('bbb')))
})

test('splitMergedBody: unrecognised body falls back to single unattributed span', () => {
  const spans = splitMergedBody('plain text with no headings', FRAGMENTS)
  assert.equal(spans.length, 1)
  assert.equal(spans[0].fragmentId, null)
})

test('splitMergedBody: skips fragments missing from the body', () => {
  // Defensive: if the kernel writer ever drops a fragment, the
  // remaining headings still get spans.
  const merged = buildMerged([
    { name: 'Skill A', id: 'a', body: 'aaa' },
    { name: 'Skill C', id: 'c', body: 'ccc' },
  ])
  const spans = splitMergedBody(merged, FRAGMENTS)
  const ids = new Set(spans.map((s) => s.fragmentId))
  assert.ok(ids.has('a'))
  assert.ok(ids.has('c'))
  assert.ok(!ids.has('b'))
})

test('fragmentTint: deterministic per index, cycles every 8 steps', () => {
  const t0 = fragmentTint(0)
  const t8 = fragmentTint(8)
  assert.equal(t0.border, t8.border)
  assert.equal(t0.background, t8.background)
  // Adjacent indices differ.
  assert.notEqual(fragmentTint(0).border, fragmentTint(1).border)
})
