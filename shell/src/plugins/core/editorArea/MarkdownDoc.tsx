// Bespoke Forge-styled markdown renderer.
//
// Handles the document subset the Forge mockup actually uses: title (# or
// frontmatter-style), H2/H3, paragraphs, fenced code, inline code, blockquote,
// unordered/ordered lists, tables, horizontal rules, [[wikilinks]], bold/italic.
// It is deliberately small — once a real markdown stack (remark + rehype)
// is added, swap this file's implementation behind the same props.

import { useEffect, useMemo, useRef } from 'react'

export interface Heading {
  id: string
  level: 1 | 2 | 3
  text: string
}

interface Props {
  source: string
  title?: string
  /** Called each time the rendered headings change. */
  onHeadings?: (headings: Heading[]) => void
  /** Called when a heading scrolls into view. */
  onActiveHeading?: (id: string | null) => void
}

export function MarkdownDoc({ source, title, onHeadings, onActiveHeading }: Props) {
  const rootRef = useRef<HTMLDivElement>(null)

  const { html, headings } = useMemo(() => renderMarkdown(source), [source])

  useEffect(() => { onHeadings?.(headings) }, [headings, onHeadings])

  // Scroll-spy: mark the top-most heading that's visible.
  useEffect(() => {
    if (!onActiveHeading || !rootRef.current) return
    const surface = rootRef.current.closest('.surface') as HTMLElement | null
    if (!surface) return
    const hs = Array.from(rootRef.current.querySelectorAll<HTMLElement>('[data-heading-id]'))
    const observer = new IntersectionObserver(
      () => {
        // Pick the last heading whose top is above the surface's top edge.
        const surfaceTop = surface.getBoundingClientRect().top
        let active: string | null = null
        for (const h of hs) {
          if (h.getBoundingClientRect().top - surfaceTop < 60) active = h.dataset.headingId ?? null
          else break
        }
        onActiveHeading(active ?? hs[0]?.dataset.headingId ?? null)
      },
      { root: surface, threshold: [0, 1] }
    )
    hs.forEach(h => observer.observe(h))
    return () => observer.disconnect()
  }, [html, onActiveHeading])

  return (
    <div className="doc" ref={rootRef}>
      {title && <div className="title">{title}</div>}
      <div dangerouslySetInnerHTML={{ __html: html }} />
    </div>
  )
}

// ─── Renderer ────────────────────────────────────────────────────────────

interface Rendered { html: string; headings: Heading[] }

export function renderMarkdown(src: string): Rendered {
  const lines = src.replace(/\r\n/g, '\n').split('\n')
  const out: string[] = []
  const headings: Heading[] = []
  let i = 0

  const pushHeading = (level: 1 | 2 | 3, text: string) => {
    const id = slugify(text)
    headings.push({ id, level, text })
    const tag = level === 1 ? 'div class="title"' : `h${level}`
    const close = level === 1 ? 'div' : `h${level}`
    out.push(`<${tag} data-heading-id="${id}" id="${id}">${inline(text)}</${close}>`)
  }

  while (i < lines.length) {
    const line = lines[i]

    // Fenced code
    const fence = line.match(/^```(\w*)\s*$/)
    if (fence) {
      const lang = fence[1] ?? ''
      i++
      const buf: string[] = []
      while (i < lines.length && !/^```\s*$/.test(lines[i])) { buf.push(lines[i]); i++ }
      i++ // skip closing fence
      out.push(`<pre><code data-lang="${lang}">${escapeHtml(buf.join('\n'))}</code></pre>`)
      continue
    }

    // ATX headings
    const h = line.match(/^(#{1,3})\s+(.+?)\s*$/)
    if (h) { pushHeading(h[1].length as 1 | 2 | 3, h[2]); i++; continue }

    // Horizontal rule
    if (/^(\*\*\*|---|___)\s*$/.test(line)) { out.push('<hr/>'); i++; continue }

    // Blockquote block
    if (/^>\s?/.test(line)) {
      const buf: string[] = []
      while (i < lines.length && /^>\s?/.test(lines[i])) {
        buf.push(lines[i].replace(/^>\s?/, ''))
        i++
      }
      out.push(`<blockquote>${inline(buf.join(' '))}</blockquote>`)
      continue
    }

    // Table: heading row | --- separator | body rows
    if (/^\s*\|.+\|\s*$/.test(line) && i + 1 < lines.length && /^\s*\|?\s*:?-+:?/.test(lines[i + 1])) {
      const header = splitRow(line)
      i += 2 // skip separator
      const rows: string[][] = []
      while (i < lines.length && /^\s*\|.+\|\s*$/.test(lines[i])) {
        rows.push(splitRow(lines[i])); i++
      }
      out.push('<table><thead><tr>' +
        header.map(c => `<th>${inline(c)}</th>`).join('') +
        '</tr></thead><tbody>' +
        rows.map(r => '<tr>' + r.map(c => `<td>${inline(c)}</td>`).join('') + '</tr>').join('') +
        '</tbody></table>')
      continue
    }

    // Lists (unordered / ordered)
    const uli = line.match(/^(\s*)([-*+])\s+(.*)$/)
    const oli = line.match(/^(\s*)(\d+)\.\s+(.*)$/)
    if (uli || oli) {
      const ordered = !!oli
      const buf: string[] = []
      while (i < lines.length) {
        const m = lines[i].match(ordered ? /^(\s*)(\d+)\.\s+(.*)$/ : /^(\s*)([-*+])\s+(.*)$/)
        if (!m) break
        buf.push(`<li>${inline(m[3])}</li>`)
        i++
      }
      out.push(`<${ordered ? 'ol' : 'ul'}>${buf.join('')}</${ordered ? 'ol' : 'ul'}>`)
      continue
    }

    // Blank line
    if (line.trim() === '') { i++; continue }

    // Paragraph — accumulate until blank line
    const buf: string[] = [line]
    i++
    while (i < lines.length && lines[i].trim() !== '' && !/^(#{1,3}\s|>|\s*\||\s*[-*+]\s|\s*\d+\.\s|```)/.test(lines[i])) {
      buf.push(lines[i]); i++
    }
    out.push(`<p>${inline(buf.join(' '))}</p>`)
  }

  return { html: out.join('\n'), headings }
}

function inline(text: string): string {
  let s = escapeHtml(text)
  // [[wikilink]] → styled span
  s = s.replace(/\[\[([^\]]+)\]\]/g, (_m, t) => `<a class="wikilink" href="#">${t}</a>`)
  // `code`
  s = s.replace(/`([^`]+)`/g, (_m, t) => `<code>${t}</code>`)
  // **bold**
  s = s.replace(/\*\*([^*]+)\*\*/g, (_m, t) => `<strong>${t}</strong>`)
  // _italic_ / *italic*
  s = s.replace(/(^|\W)_([^_\n]+)_(?=\W|$)/g, (_m, p, t) => `${p}<em>${t}</em>`)
  s = s.replace(/(^|\W)\*([^*\n]+)\*(?=\W|$)/g, (_m, p, t) => `${p}<em>${t}</em>`)
  // [text](url)
  s = s.replace(/\[([^\]]+)\]\(([^)]+)\)/g,
    (_m, t, u) => `<a href="${safeUrl(u)}" target="_blank" rel="noreferrer">${t}</a>`)
  return s
}

function splitRow(line: string): string[] {
  return line.trim().replace(/^\||\|$/g, '').split('|').map(c => c.trim())
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

// URL-scheme allowlist for `[text](url)` link rendering. Reject any
// scheme that can execute script (`javascript:`, `data:`, `vbscript:`,
// `file:`, ...) by collapsing it to a harmless `#`. Allow:
//   - http: / https: / mailto: (the schemes a markdown document
//     legitimately uses)
//   - fragment-only (`#anchor`), root-relative (`/path`), and explicit
//     relative paths (`./` / `../`)
//   - scheme-less relative paths (no `:` before the first `/` or `?`)
//
// `target="_blank" rel="noreferrer"` does NOT neutralize `javascript:`
// hrefs in modern browsers, so attribute hygiene alone is insufficient.
// See issue #76.
export function safeUrl(raw: string): string {
  const url = raw.trim()
  if (
    url.startsWith('#') ||
    url.startsWith('/') ||
    url.startsWith('./') ||
    url.startsWith('../')
  ) {
    return url
  }
  const schemeMatch = url.match(/^([a-zA-Z][a-zA-Z0-9+.-]*):/)
  if (schemeMatch) {
    const scheme = schemeMatch[1].toLowerCase()
    if (scheme === 'http' || scheme === 'https' || scheme === 'mailto') {
      return url
    }
    return '#'
  }
  // No scheme prefix at all — treat as a scheme-less relative path.
  return url
}

function slugify(s: string): string {
  return 'h-' + s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 48)
}
