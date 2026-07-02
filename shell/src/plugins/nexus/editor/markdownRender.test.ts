// BL-053 Phases 2/3/4 regression tests for the live-preview
// `renderMarkdown` pipeline in `nexus/editor` — the path that's
// actually mounted in the running shell. (The legacy
// `core/editorArea/MarkdownDoc` renderer this pipeline replaced is
// gone, along with its tests.)
//
// Also pins the issue #76 XSS regression: `renderMarkdown` feeds its
// output to `dangerouslySetInnerHTML`, and note content can come from
// AI responses, MCP tool output, and imported notes — all
// attacker-controllable — so dangerous link schemes and attribute
// injection must be neutralized by the DOMPurify pass.

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

// ── issue #76 — XSS in markdown links ─────────────────────────────
//
// The renderer must neutralize dangerous href schemes and reject
// attribute-injection payloads. We parse the sanitized HTML with
// happy-dom (registered by the test setup) and inspect the resulting
// <a> element — what matters for XSS is whether the browser would
// parse the payload as a real attribute / executable href, not
// whether a substring survives in the raw string.

function parseLink(html: string): HTMLAnchorElement | null {
  const container = document.createElement('div')
  container.innerHTML = html
  return container.querySelector('a')
}

test('javascript: link scheme is stripped from the href', () => {
  const html = renderMarkdown('[click](javascript:alert(1))')
  assert.doesNotMatch(html, /href="javascript:/i)
  const link = parseLink(html)
  assert.ok(link, `link element must still render, got: ${html}`)
  const href = link.getAttribute('href') ?? ''
  assert.ok(
    !href.toLowerCase().startsWith('javascript:'),
    `href must not start with javascript:, got: ${href}`,
  )
})

test('data: link scheme is stripped from the href', () => {
  const html = renderMarkdown('[click](data:text/html,<script>alert(1)</script>)')
  assert.doesNotMatch(html, /href="data:/i)
})

test('vbscript: link scheme is stripped from the href', () => {
  const html = renderMarkdown('[click](vbscript:msgbox)')
  assert.doesNotMatch(html, /href="vbscript:/i)
})

test('attribute-injection payload cannot add an event handler', () => {
  // This payload isn't even a valid markdown link — it survives as
  // inert text content — but assert on the parsed DOM so the test
  // stays meaningful if marked's link grammar ever loosens: no
  // element may carry an on*-handler attribute.
  const html = renderMarkdown('[click](" onmouseover=alert(1) x=")')
  const container = document.createElement('div')
  container.innerHTML = html
  const offenders = Array.from(container.querySelectorAll('*')).filter((el) =>
    el.getAttributeNames().some((n) => n.toLowerCase().startsWith('on')),
  )
  assert.equal(
    offenders.length,
    0,
    `no on*-handler attributes allowed, got: ${offenders
      .map((el) => el.getAttributeNames().join(','))
      .join(' | ')}`,
  )
})

test('safe http(s)/mailto schemes round-trip unharmed', () => {
  assert.match(
    renderMarkdown('[click](https://example.com/path?q=1)'),
    /href="https:\/\/example\.com\/path\?q=1"/,
  )
  assert.match(
    renderMarkdown('[mail](mailto:foo@bar.com)'),
    /href="mailto:foo@bar\.com"/,
  )
})

// ── C1 (#354) — `![[…]]` embeds + image passthrough ──────────────

test('![[image]] embeds render as an img with data-forge-src, no src', () => {
  const html = renderMarkdown('Before ![[assets/pic.png]] after.')
  assert.match(html, /<img[^>]*class="nx-forge-image"[^>]*>/)
  assert.match(html, /data-forge-src="assets\/pic\.png"/)
  assert.doesNotMatch(html, /<img[^>]*\ssrc=/)
})

test('![[image|alias]] carries the alias as alt text', () => {
  const html = renderMarkdown('![[pic.png|My diagram]]')
  assert.match(html, /alt="My diagram"/)
})

test('![[note]] embeds of non-images degrade to a wikilink chip', () => {
  const html = renderMarkdown('See ![[Some Note]] for details.')
  assert.match(html, /<span class="nx-embed">/)
  assert.match(html, /data-wikilink="Some Note"/)
  assert.doesNotMatch(html, /<img/)
})

test('standard ![](relative) images survive sanitization with their src', () => {
  const html = renderMarkdown('![shot](attachments/shot.png)')
  assert.match(html, /<img[^>]*src="attachments\/shot\.png"[^>]*>/)
})
