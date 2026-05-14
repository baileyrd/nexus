// BL-053 Phases 2/3/4 regression tests for the live-preview
// `renderMarkdown` pipeline in `nexus/editor`.
//
// The legacy `core/editorArea/MarkdownDoc` path also implements
// Phase 2/3 but uses different class names + lives behind a
// detached slot; its own tests at `tests/markdown-doc-bl053.test.ts`
// cover that path. This file pins the BL-053 behaviour for the
// path that's actually mounted in the running shell.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  parseFrontmatter,
  renderFrontmatterBar,
  renderMarkdown,
} from './markdownRender.ts'

// ── Phase 2a — path-shaped inline code ────────────────────────────

test('inline `code` looking like a path gets the nx-codepath class', () => {
  const html = renderMarkdown('See `crates/nexus-storage/src/lib.rs` for context.')
  assert.match(html, /<code class="nx-codepath">crates\/nexus-storage\/src\/lib\.rs<\/code>/)
})

test('inline `code` that is a version tag also gets nx-codepath', () => {
  const html = renderMarkdown('Tagged as `0.4.0` after merge.')
  assert.match(html, /<code class="nx-codepath">0\.4\.0<\/code>/)
})

test('inline `code` for prose identifiers stays neutral', () => {
  const html = renderMarkdown('Call `useState` from React.')
  assert.match(html, /<code>useState<\/code>/)
  assert.doesNotMatch(html, /class="nx-codepath"/)
})

// ── Phase 2b — wikilinks ──────────────────────────────────────────

test('[[wikilink]] renders as an nx-wikilink anchor with the target as href', () => {
  const html = renderMarkdown('See [[BACKLOG.md]] for the full list.')
  assert.match(html, /<a class="nx-wikilink" href="BACKLOG\.md" data-wikilink="BACKLOG\.md">BACKLOG\.md<\/a>/)
})

test('[[target|display]] forms preserve the pipe alias', () => {
  const html = renderMarkdown('Cf. [[Status-Doc|the status doc]] for context.')
  assert.match(html, /<a class="nx-wikilink"/)
  assert.match(html, /data-wikilink="Status-Doc"/)
  assert.match(html, />the status doc<\/a>/)
})

test('[[target#heading]] forms preserve the fragment', () => {
  const html = renderMarkdown('See [[NOTE#section-a]].')
  assert.match(html, /href="NOTE#section-a"/)
})

// ── Phase 2c — frontmatter parsing + metadata bar ────────────────

test('parseFrontmatter pulls out a leading YAML block', () => {
  const src = '---\ntitle: Hello\nupdated: 2026-04-17\n---\n\nbody text\n'
  const parsed = parseFrontmatter(src)
  assert.equal(parsed.hasFrontmatter, true)
  assert.equal(parsed.fields.title, 'Hello')
  assert.equal(parsed.fields.updated, '2026-04-17')
  assert.equal(parsed.body.trim(), 'body text')
})

test('parseFrontmatter handles CRLF + list values', () => {
  const src = '---\r\ntitle: Win\r\ntags:\r\n  - alpha\r\n  - beta\r\n---\r\nbody\r\n'
  const parsed = parseFrontmatter(src)
  assert.equal(parsed.fields.title, 'Win')
  assert.equal(parsed.fields.tags, 'alpha, beta')
})

test('parseFrontmatter returns hasFrontmatter=false for files without one', () => {
  const src = '# Just a heading\n\nNo frontmatter here.'
  const parsed = parseFrontmatter(src)
  assert.equal(parsed.hasFrontmatter, false)
  assert.equal(parsed.body, src)
  assert.deepEqual(parsed.fields, {})
})

test('parseFrontmatter is robust to unclosed frontmatter blocks', () => {
  const src = '---\ntitle: Unclosed\n\nbody'
  const parsed = parseFrontmatter(src)
  assert.equal(parsed.hasFrontmatter, false)
})

test('renderFrontmatterBar emits chips + forge name', () => {
  const html = renderFrontmatterBar(
    { updated: '2026-04-17', category: 'docs' },
    'nexus_work',
  )
  assert.match(html, /<div class="nx-frontmatter">/)
  assert.match(html, /forge · <strong>nexus_work<\/strong>/)
  assert.match(html, /updated · 2026-04-17/)
  assert.match(html, /category · docs/)
})

test('renderFrontmatterBar returns empty string for empty input', () => {
  assert.equal(renderFrontmatterBar({}, null), '')
})

test('renderMarkdown splices the metadata bar directly after the H1', () => {
  const src = '---\ncategory: docs\n---\n# Heading\n\nBody.'
  const html = renderMarkdown(src, { forgeName: 'nexus_work' })
  // The bar HTML should appear AFTER `</h1>` and BEFORE the body
  // paragraph. Two cheap substring checks confirm the ordering.
  const h1End = html.indexOf('</h1>')
  const barAt = html.indexOf('nx-frontmatter')
  const bodyAt = html.indexOf('<p>Body.</p>')
  assert.ok(h1End > 0)
  assert.ok(barAt > h1End)
  assert.ok(bodyAt > barAt)
})

test('renderMarkdown promotes status: frontmatter to a status pill', () => {
  const src = '---\nstatus: Complete\n---\n# Heading'
  const html = renderMarkdown(src, { forgeName: null })
  assert.match(html, /class="nx-status-pill nx-status-pill__chip--ok">Complete<\/span>/)
})

// ── Phase 3 — Obsidian callouts ──────────────────────────────────

test('renderMarkdown emits a callout for `> [!info] Title\\n> body`', () => {
  const src = [
    '> [!info] Update cadence',
    '> First body line.',
    '> Second body line.',
  ].join('\n')
  const html = renderMarkdown(src)
  assert.match(html, /<div class="nx-callout nx-callout--info">/)
  assert.match(html, /class="nx-callout__dot"/)
  assert.match(html, /Update cadence<\/span>/)
  assert.match(html, /First body line\.[\s\S]*Second body line\./)
})

test('renderMarkdown falls back to a default label for headerless callouts', () => {
  const html = renderMarkdown('> [!warn]\n> Heads up.')
  assert.match(html, /nx-callout--warn/)
  assert.match(html, />Warn<\/span>/)
})

test('renderMarkdown leaves regular blockquotes as <blockquote>', () => {
  const html = renderMarkdown('> just a quote\n> spanning two lines')
  assert.match(html, /<blockquote>/)
  assert.doesNotMatch(html, /nx-callout/)
})

test('renderMarkdown rejects unknown callout kinds and keeps them as blockquotes', () => {
  const html = renderMarkdown('> [!mystery] Should not render as callout\n> body')
  assert.doesNotMatch(html, /nx-callout/)
  assert.match(html, /<blockquote>/)
})

// ── Phase 4a — status pills in tables + inline code ──────────────

test('renderMarkdown turns a `Complete` table cell into an ok pill', () => {
  const src = '| Spec | Status |\n|---|---|\n| Foo | Complete |\n'
  const html = renderMarkdown(src)
  assert.match(html, /<td><span class="nx-status-pill nx-status-pill__chip--ok">Complete<\/span><\/td>/)
})

test('renderMarkdown turns a `Partial` table cell into a warn pill', () => {
  const src = '| Spec | Status |\n|---|---|\n| Bar | Partial |\n'
  const html = renderMarkdown(src)
  assert.match(html, /nx-status-pill__chip--warn">Partial<\/span>/)
})

test('renderMarkdown leaves non-status table cells unchanged', () => {
  const src = '| Spec | Notes |\n|---|---|\n| Bar | needs design |\n'
  const html = renderMarkdown(src)
  assert.match(html, /<td>needs design<\/td>/)
  assert.doesNotMatch(html, /nx-status-pill/)
})

test('inline `Complete` codespan renders as a pill', () => {
  const html = renderMarkdown('Status `Complete` as of today.')
  assert.match(html, /nx-status-pill__chip--ok">Complete<\/span>/)
})

test('inline `Not started` codespan renders as a muted pill', () => {
  const html = renderMarkdown('Status `Not started`.')
  assert.match(html, /nx-status-pill__chip--muted">Not started<\/span>/)
})
