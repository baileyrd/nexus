// shell/src/plugins/nexus/ai/citationTransform.test.ts
//
// BL-038 — covers the [N] → superscript citation transform exposed by
// ChatView. We test the pure `splitTextOnCitations` helper directly
// since node --test runs without a DOM (no DOMParser); the DOM-walking
// `renderMarkdownWithCitations` wrapper trivially composes this helper
// + DOMParser in the browser.
//
// Run:
//   node --import tsx --test \
//     shell/src/plugins/nexus/ai/citationTransform.test.ts

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { splitTextOnCitations } from './citationTransform.ts'

test('splitTextOnCitations: passes text through when no markers', () => {
  const out = splitTextOnCitations('plain answer text', new Set([1, 2]))
  assert.deepEqual(out, ['plain answer text'])
})

test('splitTextOnCitations: substitutes [1] and [2] in order', () => {
  const out = splitTextOnCitations('A says [1] and B says [2] today', new Set([1, 2]))
  assert.deepEqual(out, [
    'A says ',
    { cite: 1 },
    ' and B says ',
    { cite: 2 },
    ' today',
  ])
})

test('splitTextOnCitations: leaves out-of-range markers as plain text', () => {
  // [9] isn't in the valid set, so it stays in the surrounding string.
  const out = splitTextOnCitations('see [1] but not [9]', new Set([1]))
  // The leading "see " and "[9]" both get folded into surrounding text
  // segments; we only split on valid markers.
  assert.deepEqual(out, ['see ', { cite: 1 }, ' but not [9]'])
})

test('splitTextOnCitations: preserves leading and trailing text', () => {
  const out = splitTextOnCitations('[1] at start, end too [2]', new Set([1, 2]))
  assert.deepEqual(out, [{ cite: 1 }, ' at start, end too ', { cite: 2 }])
})

test('splitTextOnCitations: handles back-to-back markers without dropping spans', () => {
  const out = splitTextOnCitations('many sources [1][2][3] cited', new Set([1, 2, 3]))
  assert.deepEqual(out, [
    'many sources ',
    { cite: 1 },
    { cite: 2 },
    { cite: 3 },
    ' cited',
  ])
})

test('splitTextOnCitations: falls back to passthrough when valid set is empty', () => {
  const out = splitTextOnCitations('claims [1] and [2]', new Set())
  assert.deepEqual(out, ['claims [1] and [2]'])
})
