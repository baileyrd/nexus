// Regression tests for issue #76 — XSS in MarkdownDoc.tsx's hand-rolled
// markdown renderer.
//
// Before the fix:
//   - `escapeHtml` only escaped `&`, `<`, `>` — `"` and `'` passed through.
//   - The `[text](url)` regex interpolated the captured URL directly into
//     `href="..."` with no scheme validation.
//
// Two distinct payloads worked:
//   1. `[click](javascript:alert(1))` — `javascript:` href, executes on click.
//   2. `[click](" onmouseover=alert(1) x=")` — attribute injection, executes
//      on mouseover (no click required).
//
// `target="_blank" rel="noreferrer"` does not neutralize `javascript:` hrefs
// in modern browsers, so the renderer must reject the scheme up front.
//
// MarkdownDoc's output flows into `dangerouslySetInnerHTML`, and note content
// can come from AI responses, MCP tool outputs, and imported notes — all
// attacker-controllable surfaces. The attribute-injection assertions parse
// the HTML with happy-dom (already registered by the test setup) and inspect
// the resulting `<a>` element — the only thing that matters for XSS is
// whether the browser would parse a payload as a separate attribute, not
// whether the substring appears anywhere in the raw HTML.

import { strict as assert } from 'node:assert'
import { test } from 'node:test'

import {
  renderMarkdown,
  safeUrl,
} from '../src/plugins/core/editorArea/MarkdownDoc'

function parseLink(html: string): HTMLAnchorElement {
  const container = document.createElement('div')
  container.innerHTML = html
  const link = container.querySelector('a')
  assert.ok(link, `expected an <a> element in: ${html}`)
  return link as HTMLAnchorElement
}

test('javascript: URL in markdown link is neutralized', () => {
  const { html } = renderMarkdown('[click](javascript:alert(1))')
  assert.ok(
    !/href="javascript:/i.test(html),
    `javascript: scheme must not appear in href, got: ${html}`,
  )
  // Positive shape: the link is rendered, just with a safe href.
  assert.ok(/<a [^>]*>click<\/a>/.test(html), `link element must still render, got: ${html}`)
})

test('data: URL in markdown link is neutralized', () => {
  const { html } = renderMarkdown('[click](data:text/html,<script>alert(1)</script>)')
  assert.ok(
    !/href="data:/i.test(html),
    `data: scheme must not appear in href, got: ${html}`,
  )
})

test('vbscript: URL in markdown link is neutralized', () => {
  const { html } = renderMarkdown('[click](vbscript:msgbox)')
  assert.ok(
    !/href="vbscript:/i.test(html),
    `vbscript: scheme must not appear in href, got: ${html}`,
  )
})

test('attribute-injection payload cannot escape href', () => {
  // The `"` in the URL must not break out of the surrounding href="...".
  // After escapeHtml, `"` becomes `&quot;` so it's an entity inside the
  // attribute value rather than a quote character ending the attribute.
  // Parse with happy-dom and assert the link element has no extra
  // attributes — this is what actually matters for XSS, not whether the
  // substring appears in the raw HTML.
  const { html } = renderMarkdown('[click](" onmouseover=alert(1) x=")')
  const link = parseLink(html)
  assert.equal(
    link.getAttribute('onmouseover'),
    null,
    `onmouseover must not be a real attribute on the link`,
  )
  assert.equal(link.getAttribute('x'), null, `x must not be a real attribute on the link`)
  // The attribute injection text ends up as literal characters in the href.
  assert.ok(
    !link.getAttributeNames().some((n) => n.toLowerCase().startsWith('on')),
    `no on*-handler attributes allowed, got: ${link.getAttributeNames().join(',')}`,
  )
})

test('single-quote attribute-injection payload cannot escape href', () => {
  // Defense-in-depth — even though we use double-quoted attributes, escape
  // single quotes too so the renderer doesn't depend on a particular quote
  // style at the call site.
  const { html } = renderMarkdown("[click](' onclick=alert(1) x=')")
  const link = parseLink(html)
  assert.equal(
    link.getAttribute('onclick'),
    null,
    `onclick must not be a real attribute on the link`,
  )
  assert.ok(
    !link.getAttributeNames().some((n) => n.toLowerCase().startsWith('on')),
    `no on*-handler attributes allowed, got: ${link.getAttributeNames().join(',')}`,
  )
})

test('javascript: URL parsed as DOM attribute is rejected', () => {
  // Belt-and-suspenders: parse the rendered HTML and check the actual
  // href value — protects against future regressions where the regex
  // assertion above might match something stripped by the parser.
  const { html } = renderMarkdown('[click](javascript:alert(1))')
  const link = parseLink(html)
  const href = link.getAttribute('href') ?? ''
  assert.ok(
    !href.toLowerCase().startsWith('javascript:'),
    `href must not start with javascript:, got: ${href}`,
  )
})

test('https URL is preserved verbatim', () => {
  const { html } = renderMarkdown('[click](https://example.com/path?q=1)')
  assert.ok(
    /href="https:\/\/example\.com\/path\?q=1"/.test(html),
    `https URL must round-trip, got: ${html}`,
  )
})

test('http URL is preserved verbatim', () => {
  const { html } = renderMarkdown('[click](http://example.com)')
  assert.ok(
    /href="http:\/\/example\.com"/.test(html),
    `http URL must round-trip, got: ${html}`,
  )
})

test('mailto URL is preserved', () => {
  const { html } = renderMarkdown('[mail](mailto:foo@bar.com)')
  assert.ok(
    /href="mailto:foo@bar\.com"/.test(html),
    `mailto must round-trip, got: ${html}`,
  )
})

test('fragment-only URL is preserved', () => {
  const { html } = renderMarkdown('[top](#section)')
  assert.ok(/href="#section"/.test(html), `fragment URL must round-trip, got: ${html}`)
})

test('root-relative URL is preserved', () => {
  const { html } = renderMarkdown('[home](/notes/index.md)')
  assert.ok(
    /href="\/notes\/index\.md"/.test(html),
    `root-relative URL must round-trip, got: ${html}`,
  )
})

test('explicit relative URL is preserved', () => {
  const { html } = renderMarkdown('[sibling](./other.md)')
  assert.ok(
    /href="\.\/other\.md"/.test(html),
    `./relative URL must round-trip, got: ${html}`,
  )
})

test('scheme-less relative URL is preserved', () => {
  const { html } = renderMarkdown('[note](other.md)')
  assert.ok(/href="other\.md"/.test(html), `bare relative URL must round-trip, got: ${html}`)
})

test('safeUrl unit cases', () => {
  // Allowed schemes
  assert.equal(safeUrl('https://example.com'), 'https://example.com')
  assert.equal(safeUrl('http://example.com'), 'http://example.com')
  assert.equal(safeUrl('mailto:foo@bar.com'), 'mailto:foo@bar.com')
  assert.equal(safeUrl('HTTPS://EXAMPLE.COM'), 'HTTPS://EXAMPLE.COM') // case-insensitive scheme check

  // Allowed relative shapes
  assert.equal(safeUrl('#anchor'), '#anchor')
  assert.equal(safeUrl('/abs/path'), '/abs/path')
  assert.equal(safeUrl('./rel'), './rel')
  assert.equal(safeUrl('../up'), '../up')
  assert.equal(safeUrl('plain.md'), 'plain.md')

  // Rejected schemes collapse to '#'
  assert.equal(safeUrl('javascript:alert(1)'), '#')
  assert.equal(safeUrl('JavaScript:alert(1)'), '#') // case-insensitive
  assert.equal(safeUrl('data:text/html,<script>'), '#')
  assert.equal(safeUrl('vbscript:msgbox'), '#')
  assert.equal(safeUrl('file:///etc/passwd'), '#')

  // Whitespace tolerance
  assert.equal(safeUrl('  javascript:alert(1)  '), '#')
  assert.equal(safeUrl('  https://example.com  '), 'https://example.com')
})
