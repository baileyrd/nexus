// Pure-logic tests for the BL-049 phase-3 backlinks decoder +
// fragment pill. Re-exported via
// `shell/tests/backlinks-decode.test.ts`.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import { decode } from './index.ts'

const A_UUID = 'd8e9f0a1-2b3c-4d5e-9f01-abcdef012345'

test('decode: passes through fragment when present', () => {
  const out = decode(
    [
      {
        source_path: 'Notes/A.md',
        link_text: 'see this',
        link_type: 'wikilink',
        fragment: `^${A_UUID}`,
      },
    ],
    'Notes/B.md',
  )
  assert.equal(out.length, 1)
  assert.equal(out[0].fragment, `^${A_UUID}`)
})

test('decode: maps absent fragment to null (BL-043 backwards compat)', () => {
  const out = decode(
    [
      {
        source_path: 'Notes/A.md',
        link_text: 'plain link',
        link_type: 'wikilink',
      },
    ],
    'Notes/B.md',
  )
  assert.equal(out[0].fragment, null)
})

test('decode: drops self-references regardless of fragment presence', () => {
  const out = decode(
    [
      {
        source_path: 'Notes/A.md',
        link_text: 'self',
        link_type: 'wikilink',
        fragment: `^${A_UUID}`,
      },
      {
        source_path: 'Notes/B.md',
        link_text: 'other',
        link_type: 'wikilink',
        fragment: '^11111111-2222-3333-4444-555555555555',
      },
    ],
    'Notes/A.md',
  )
  assert.equal(out.length, 1)
  assert.equal(out[0].sourceRelpath, 'Notes/B.md')
})

test('decode: ignores non-string fragment values defensively', () => {
  const out = decode(
    [
      {
        source_path: 'Notes/A.md',
        link_text: 'oops',
        link_type: 'wikilink',
        fragment: 42,
      },
      {
        source_path: 'Notes/C.md',
        link_text: 'good',
        link_type: 'wikilink',
        fragment: '',
      },
    ],
    'Notes/B.md',
  )
  assert.equal(out[0].fragment, null)
  assert.equal(out[1].fragment, null)
})

test('decode: preserves heading-anchor fragments alongside block ones', () => {
  const out = decode(
    [
      {
        source_path: 'Notes/A.md',
        link_text: 'block',
        link_type: 'wikilink',
        fragment: `^${A_UUID}`,
      },
      {
        source_path: 'Notes/B.md',
        link_text: 'heading',
        link_type: 'wikilink',
        fragment: 'Section Title',
      },
    ],
    'Notes/X.md',
  )
  assert.equal(out[0].fragment, `^${A_UUID}`)
  assert.equal(out[1].fragment, 'Section Title')
})
