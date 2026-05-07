// BL-053 Phase 2 regression tests:
//   - `isCodepath` decides which inline `<code>` tokens get the
//     ember `codepath` class.
//   - `extractFrontmatter` peels a leading YAML frontmatter block off
//     the source and returns the parsed map plus the body. Misshapen
//     frontmatter must not crash.

import { strict as assert } from 'node:assert'
import { test } from 'node:test'

import {
  extractFrontmatter,
  isCodepath,
  normaliseCalloutType,
  parseCalloutHeader,
  renderMarkdown,
} from '../src/plugins/core/editorArea/MarkdownDoc'

test('isCodepath: matches file-path / glob shapes', () => {
  assert.equal(isCodepath('crates/nexus-storage/src/find_replace.rs'), true)
  assert.equal(isCodepath('docs/PRDs/*.md'), true)
  assert.equal(isCodepath('./shell/src/main.tsx'), true)
  assert.equal(isCodepath('a/b'), true)
})

test('isCodepath: rejects prose code', () => {
  assert.equal(isCodepath('useState'), false) // no slash
  assert.equal(isCodepath('n + 1'), false)    // spaces + non-allowed chars
  assert.equal(isCodepath('foo()'), false)    // parens
  assert.equal(isCodepath('foo bar/baz'), false) // space disqualifies
  assert.equal(isCodepath(''), false)         // empty
})

test('renderMarkdown: tags codepath inline code with the .codepath class', () => {
  const { html } = renderMarkdown('See `crates/nexus-storage/src/lib.rs` for context.')
  assert.match(html, /<code class="codepath">crates\/nexus-storage\/src\/lib\.rs<\/code>/)
})

test('renderMarkdown: leaves prose inline code without the .codepath class', () => {
  const { html } = renderMarkdown('Call `useState` from React.')
  assert.match(html, /<code>useState<\/code>/)
  assert.doesNotMatch(html, /class="codepath"/)
})

test('extractFrontmatter: parses simple key/value + inline list + block list', () => {
  const src = [
    '---',
    'title: Demo Note',
    'category: documentation',
    'tags: [alpha, beta]',
    'updated: 2026-04-17',
    'authors:',
    '  - Alice',
    '  - "Bob the Builder"',
    '---',
    '',
    'Body text.',
  ].join('\n')
  const { frontmatter, body } = extractFrontmatter(src)
  assert.equal(frontmatter['title'], 'Demo Note')
  assert.equal(frontmatter['category'], 'documentation')
  assert.deepEqual(frontmatter['tags'], ['alpha', 'beta'])
  assert.equal(frontmatter['updated'], '2026-04-17')
  assert.deepEqual(frontmatter['authors'], ['Alice', 'Bob the Builder'])
  assert.equal(body.trim(), 'Body text.')
})

test('extractFrontmatter: returns empty map when no opening fence', () => {
  const src = '# Just a heading\n\nNo frontmatter.'
  const { frontmatter, body } = extractFrontmatter(src)
  assert.deepEqual(frontmatter, {})
  assert.equal(body, src)
})

test('extractFrontmatter: returns empty map when closing fence is missing', () => {
  const src = '---\ntitle: Unclosed\n\nbody'
  const { frontmatter, body } = extractFrontmatter(src)
  assert.deepEqual(frontmatter, {})
  assert.equal(body.startsWith('---'), true) // unchanged source
})

test('extractFrontmatter: handles CRLF line endings', () => {
  const src = '---\r\ntitle: Win\r\n---\r\nbody\r\n'
  const { frontmatter, body } = extractFrontmatter(src)
  assert.equal(frontmatter['title'], 'Win')
  assert.equal(body, 'body\n')
})

test('extractFrontmatter: ignores comments and blank lines inside the block', () => {
  const src = [
    '---',
    '# this is a comment',
    '',
    'title: Has Comment',
    '---',
    'body',
  ].join('\n')
  const { frontmatter } = extractFrontmatter(src)
  assert.equal(frontmatter['title'], 'Has Comment')
})

test('extractFrontmatter: malformed lines are dropped, parsing continues', () => {
  const src = [
    '---',
    'title: Survivor',
    'this line is not yaml shaped',
    'updated: 2026-04-17',
    '---',
    'body',
  ].join('\n')
  const { frontmatter } = extractFrontmatter(src)
  assert.equal(frontmatter['title'], 'Survivor')
  assert.equal(frontmatter['updated'], '2026-04-17')
})

// ─── BL-053 Phase 3: callouts ───────────────────────────────────────────

test('parseCalloutHeader: pulls type and title off the marker line', () => {
  assert.deepEqual(parseCalloutHeader('[!info] Heads up'), { type: 'info', title: 'Heads up' })
  assert.deepEqual(parseCalloutHeader('[!warning]'), { type: 'warning', title: '' })
  assert.equal(parseCalloutHeader('not a callout'), null)
  assert.equal(parseCalloutHeader('[!info'), null) // unclosed bracket
})

test('normaliseCalloutType: collapses aliases and falls back to note', () => {
  assert.equal(normaliseCalloutType('Warning'), 'warning')
  assert.equal(normaliseCalloutType('warn'), 'warning')
  assert.equal(normaliseCalloutType('risk'), 'danger')
  assert.equal(normaliseCalloutType('UPDATE'), 'update')
  assert.equal(normaliseCalloutType('mystery'), 'note')
})

test('renderMarkdown: turns `> [!info] Title` blockquote into a callout div', () => {
  const src = [
    '> [!info] Update cadence',
    '> Body line one.',
    '> Body line two.',
  ].join('\n')
  const { html } = renderMarkdown(src)
  assert.match(html, /<div class="nx-callout nx-callout--info">/)
  assert.match(html, /<span class="nx-callout-dot"/)
  assert.match(html, /Update cadence<\/span>/)
  assert.match(html, /Body line one\. Body line two\./)
})

test('renderMarkdown: omits header title when none provided, falls back to type label', () => {
  const src = '> [!warning]\n> Be careful.'
  const { html } = renderMarkdown(src)
  assert.match(html, /<div class="nx-callout nx-callout--warning">/)
  assert.match(html, />Warning<\/span>/) // default title
})

test('renderMarkdown: blockquote without callout marker stays a <blockquote>', () => {
  const { html } = renderMarkdown('> just a quote\n> spanning two lines')
  assert.match(html, /<blockquote>just a quote spanning two lines<\/blockquote>/)
  assert.doesNotMatch(html, /nx-callout/)
})

test('renderMarkdown: ember `update` callout maps to the update class', () => {
  const { html } = renderMarkdown('> [!update] Build status\n> Green across the board.')
  assert.match(html, /<div class="nx-callout nx-callout--update">/)
})

test('renderMarkdown: callout body separates paragraphs on a blank `>` line', () => {
  // Within a blockquote the "blank" continuation line is `>` alone,
  // which the strip pass turns into an empty body line — that's the
  // paragraph break our renderer honours.
  const src = [
    '> [!note] Two paragraphs',
    '> First para.',
    '>',
    '> Second para.',
  ].join('\n')
  const { html } = renderMarkdown(src)
  // Two <p> children inside the body
  const matches = html.match(/<p>/g) ?? []
  assert.equal(matches.length, 2)
})
