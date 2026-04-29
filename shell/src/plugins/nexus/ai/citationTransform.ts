// shell/src/plugins/nexus/ai/citationTransform.ts
//
// BL-038 — pure helpers for the [N] → superscript citation transform
// used by the chat view. Split out of ChatView.tsx so node:test can
// exercise the splitter without pulling in React + chat.css.

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

/**
 * Replace `[N]` markers in the rendered HTML with clickable superscript
 * chips, leaving `<code>` and `<pre>` regions untouched. The shell
 * binds a delegated click handler to `.nexus-citation` to dispatch
 * `onCitationClick`.
 *
 * `validIndices` is the set of citation indices the kernel attached to
 * this turn. `[N]` markers whose N isn't in the set are left as plain
 * text.
 *
 * Falls back to the unmodified rendered markdown when `DOMParser`
 * isn't available (node:test); production always has it via the
 * browser.
 */
export function renderMarkdownWithCitations(source: string, validIndices: Set<number>): string {
  const baseHtml = renderMarkdown(source)
  if (typeof DOMParser === 'undefined') return baseHtml
  const parser = new DOMParser()
  const doc = parser.parseFromString(`<div>${baseHtml}</div>`, 'text/html')
  const root = doc.body.firstElementChild as HTMLElement | null
  if (!root) return baseHtml
  walkAndReplaceCitations(root, validIndices, doc)
  return root.innerHTML
}

function walkAndReplaceCitations(
  node: Element,
  valid: Set<number>,
  doc: Document,
): void {
  const skip = new Set(['CODE', 'PRE'])
  const children = Array.from(node.childNodes)
  for (const child of children) {
    if (child.nodeType === 3 /* TEXT */) {
      const text = child.nodeValue ?? ''
      if (!text.includes('[')) continue
      const replacement = buildCitationFragment(text, valid, doc)
      if (replacement) node.replaceChild(replacement, child)
    } else if (child.nodeType === 1 /* ELEMENT */) {
      const el = child as Element
      if (skip.has(el.tagName)) continue
      walkAndReplaceCitations(el, valid, doc)
    }
  }
}

function buildCitationFragment(
  text: string,
  valid: Set<number>,
  doc: Document,
): DocumentFragment | null {
  const segments = splitTextOnCitations(text, valid)
  if (segments.length === 1 && typeof segments[0] === 'string') return null
  const frag = doc.createDocumentFragment()
  for (const seg of segments) {
    if (typeof seg === 'string') {
      frag.appendChild(doc.createTextNode(seg))
    } else {
      const sup = doc.createElement('sup')
      sup.className = 'nexus-citation'
      sup.dataset.cite = String(seg.cite)
      sup.setAttribute('role', 'button')
      sup.setAttribute('tabindex', '0')
      sup.textContent = `[${seg.cite}]`
      frag.appendChild(sup)
    }
  }
  return frag
}
