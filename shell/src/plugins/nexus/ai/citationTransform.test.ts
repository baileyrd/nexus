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
import {
  splitTextOnCitations,
  substituteCitationsInHtml,
} from './citationTransform.ts'

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

// FU-3 regression: substitution must not touch `[N]` literals that
// the markdown renderer placed inside `<pre>`, `<code>`, or the
// fenced-code placeholder.
test('substituteCitationsInHtml: leaves [N] inside <pre><code> verbatim', () => {
  const html =
    '<p>see [1]</p><pre><code>const arr = [1] // first\nconst x = [2]</code></pre><p>and [2]</p>'
  const out = substituteCitationsInHtml(html, new Set([1, 2]))
  assert.match(out, /<p>see <sup class="nexus-citation" data-cite="1"[^>]*>\[1\]<\/sup><\/p>/)
  // Code block contents preserved verbatim.
  assert.ok(out.includes('<pre><code>const arr = [1] // first\nconst x = [2]</code></pre>'))
  assert.match(out, /<p>and <sup class="nexus-citation" data-cite="2"[^>]*>\[2\]<\/sup><\/p>/)
})

test('substituteCitationsInHtml: leaves [N] inside inline <code> verbatim', () => {
  const html = '<p>cite <code>arr[1]</code> then [1] applies</p>'
  const out = substituteCitationsInHtml(html, new Set([1]))
  assert.ok(out.includes('<code>arr[1]</code>'))
  assert.match(out, /then <sup class="nexus-citation" data-cite="1"[^>]*>\[1\]<\/sup> applies/)
})

test('substituteCitationsInHtml: leaves [N] inside fenced-code placeholder verbatim', () => {
  const html =
    '<p>opens [1]</p><div class="nexus-fenced-pending" data-nexus-fenced-lang="mermaid" data-nexus-fenced-source="encoded"></div><p>closes [2]</p>'
  const out = substituteCitationsInHtml(html, new Set([1, 2]))
  assert.ok(out.includes('<div class="nexus-fenced-pending" data-nexus-fenced-lang="mermaid" data-nexus-fenced-source="encoded"></div>'))
  assert.match(out, /opens <sup class="nexus-citation" data-cite="1"[^>]*>\[1\]<\/sup>/)
  assert.match(out, /closes <sup class="nexus-citation" data-cite="2"[^>]*>\[2\]<\/sup>/)
})

test('substituteCitationsInHtml: empty valid set is a no-op', () => {
  const html = '<p>see [1] and [2]</p>'
  const out = substituteCitationsInHtml(html, new Set())
  assert.equal(out, html)
})

test('substituteCitationsInHtml: out-of-range marker stays plain text', () => {
  const html = '<p>see [1] but not [9]</p>'
  const out = substituteCitationsInHtml(html, new Set([1]))
  assert.match(out, /<sup class="nexus-citation" data-cite="1"[^>]*>\[1\]<\/sup>/)
  assert.ok(out.includes('not [9]'))
})
