// shell/src/plugins/nexus/ai/citationTransform.ts
//
// BL-038 — pure helpers for the [N] → superscript citation transform
// used by the chat view. Split out of ChatView.tsx so node:test can
// exercise the splitter without pulling in React + chat.css.
//
// FU-3: substitution runs over already-rendered HTML, skipping
// `<pre>`, `<code>`, and the `nexus-fenced-pending` placeholder
// regions emitted by the fenced-code renderer patch. Operating on
// rendered HTML (rather than the raw markdown source) means
// `[1]` literals inside fenced code blocks survive verbatim.

import { renderMarkdown } from '../editor/markdownRender'

export type CitationSegment = string | { cite: number }

/**
 * Split a plain text string on `[N]` citation markers whose N is in
 * `valid`. Returns alternating string + `{cite: N}` segments.
 *
 * Markers whose N isn't in `valid` are emitted as plain text so the
 * user still sees what the model wrote. When no valid markers are
 * found the result is `[text]` (length 1) so callers can short-circuit
 * the transform.
 */
export function splitTextOnCitations(text: string, valid: Set<number>): CitationSegment[] {
  const re = /\[(\d+)\]/g
  const out: CitationSegment[] = []
  let m: RegExpExecArray | null
  let last = 0
  let touched = false
  while ((m = re.exec(text)) !== null) {
    const n = Number(m[1])
    if (!valid.has(n)) continue
    touched = true
    if (m.index > last) out.push(text.slice(last, m.index))
    out.push({ cite: n })
    last = m.index + m[0].length
  }
  if (!touched) return [text]
  if (last < text.length) out.push(text.slice(last))
  return out
}

// Regions of the rendered HTML that must be skipped during citation
// substitution. Order matters only for the alternation; each
// alternative is non-greedy. `marked` always emits well-formed
// `<pre>` and `<code>` blocks, and the fenced-code renderer emits a
// self-closing `nexus-fenced-pending` `<div>` placeholder whose
// source is base64-encoded (so any `[N]` in the source can't
// accidentally match here).
const SKIP_REGION_RE =
  /<pre\b[\s\S]*?<\/pre>|<code\b[\s\S]*?<\/code>|<div class="nexus-fenced-pending"[^>]*><\/div>/gi

const CITATION_RE = /\[(\d+)\]/g

function substituteOutsideSkip(html: string, valid: Set<number>): string {
  return html.replace(CITATION_RE, (full, raw: string) => {
    const n = Number(raw)
    if (!valid.has(n)) return full
    return (
      `<sup class="nexus-citation" data-cite="${n}" ` +
      `role="button" tabindex="0">[${n}]</sup>`
    )
  })
}

/**
 * Replace `[N]` markers in the rendered HTML with clickable
 * superscript chips, leaving `<code>`, `<pre>`, and
 * `nexus-fenced-pending` regions untouched. The shell binds a
 * delegated click handler to `.nexus-citation` to dispatch
 * `onCitationClick`.
 *
 * `validIndices` is the set of citation indices the kernel attached
 * to this turn. `[N]` markers whose N isn't in the set are left as
 * plain text.
 */
export function renderMarkdownWithCitations(source: string, validIndices: Set<number>): string {
  const baseHtml = renderMarkdown(source)
  if (validIndices.size === 0) return baseHtml
  return substituteCitationsInHtml(baseHtml, validIndices)
}

/**
 * Visible-for-testing variant: takes already-rendered HTML and
 * applies the citation substitution. Lets node:test exercise the
 * skip-region behaviour without instantiating a DOM.
 */
export function substituteCitationsInHtml(html: string, valid: Set<number>): string {
  if (valid.size === 0) return html
  let out = ''
  let cursor = 0
  SKIP_REGION_RE.lastIndex = 0
  let m: RegExpExecArray | null
  while ((m = SKIP_REGION_RE.exec(html)) !== null) {
    if (m.index > cursor) {
      out += substituteOutsideSkip(html.slice(cursor, m.index), valid)
    }
    out += m[0]
    cursor = m.index + m[0].length
  }
  if (cursor < html.length) {
    out += substituteOutsideSkip(html.slice(cursor), valid)
  }
  return out
}
