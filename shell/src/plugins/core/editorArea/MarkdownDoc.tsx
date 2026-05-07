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

/** Parsed YAML frontmatter — only the small subset BL-053 Phase 2 cares
 *  about (title / tags / updated / category). Unknown keys are still
 *  parsed and round-tripped so future surfaces (Phase 4 status pills,
 *  Bases queries) can read them. */
export interface Frontmatter {
  [key: string]: string | string[]
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

  const { frontmatter, body } = useMemo(() => extractFrontmatter(source), [source])
  const { html, headings } = useMemo(() => renderMarkdown(body), [body])
  const fmTitle = stringValue(frontmatter['title'])

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

  const displayTitle = fmTitle ?? title

  return (
    <div className="doc" ref={rootRef}>
      {displayTitle && <div className="title">{displayTitle}</div>}
      <FrontmatterBar frontmatter={frontmatter} />
      <div dangerouslySetInnerHTML={{ __html: html }} />
    </div>
  )
}

/** Renders a `.metaline` row below the H1 with the BL-053-spec'd
 *  fields (`category`, `tags`, `updated`). Returns null when none of
 *  those keys are populated so plain documents stay uncluttered. */
function FrontmatterBar({ frontmatter }: { frontmatter: Frontmatter }) {
  const category = stringValue(frontmatter['category'])
  const tags = listValue(frontmatter['tags'])
  const updated = stringValue(frontmatter['updated'])
  if (!category && tags.length === 0 && !updated) return null
  return (
    <div className="metaline">
      {category && <span className="chip">{category}</span>}
      {tags.map((t) => <span key={t} className="chip">{t}</span>)}
      {updated && <span>Updated {updated}</span>}
    </div>
  )
}

function stringValue(v: string | string[] | undefined): string | undefined {
  if (typeof v === 'string') return v.length === 0 ? undefined : v
  return undefined
}

function listValue(v: string | string[] | undefined): string[] {
  if (Array.isArray(v)) return v
  if (typeof v === 'string' && v.length > 0) return [v]
  return []
}

// ─── Frontmatter parser ─────────────────────────────────────────────────

/** Pulls a leading YAML frontmatter block (`---` … `---`) off the
 *  source and returns the parsed map plus the remaining body. The
 *  parser handles the shapes Phase 2 actually emits — `key: value`,
 *  `key: [a, b]`, and a multi-line list block. Anything more exotic
 *  (nested objects, anchors, multi-line strings) is dropped silently
 *  so a malformed frontmatter never crashes the editor. */
export function extractFrontmatter(src: string): { frontmatter: Frontmatter; body: string } {
  const lf = src.replace(/\r\n/g, '\n')
  const open = lf.match(/^---\s*\n/)
  if (!open) return { frontmatter: {}, body: lf }
  const after = lf.slice(open[0].length)
  const close = after.search(/(^|\n)---\s*(\n|$)/)
  if (close < 0) return { frontmatter: {}, body: lf }
  const yaml = after.slice(0, close).replace(/\n$/, '')
  const rest = after.slice(close).replace(/^\n?---\s*(\n|$)/, '')
  return { frontmatter: parseYamlFrontmatter(yaml), body: rest }
}

function parseYamlFrontmatter(yaml: string): Frontmatter {
  const out: Frontmatter = {}
  const lines = yaml.split('\n')
  let i = 0
  while (i < lines.length) {
    const line = lines[i]
    if (line.trim() === '' || line.trim().startsWith('#')) { i++; continue }
    const m = line.match(/^([A-Za-z_][\w-]*)\s*:\s*(.*)$/)
    if (!m) { i++; continue }
    const key = m[1]
    const rawValue = m[2]
    if (rawValue === '') {
      // Multi-line list block: scan following indented `- item` lines.
      const items: string[] = []
      i++
      while (i < lines.length && /^\s+-\s+/.test(lines[i])) {
        items.push(unquote(lines[i].replace(/^\s+-\s+/, '').trim()))
        i++
      }
      if (items.length > 0) out[key] = items
      continue
    }
    if (rawValue.startsWith('[') && rawValue.endsWith(']')) {
      out[key] = rawValue.slice(1, -1).split(',').map(s => unquote(s.trim())).filter(Boolean)
    } else {
      out[key] = unquote(rawValue.trim())
    }
    i++
  }
  return out
}

function unquote(s: string): string {
  if ((s.startsWith('"') && s.endsWith('"')) || (s.startsWith("'") && s.endsWith("'"))) {
    return s.slice(1, -1)
  }
  return s
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

    // Blockquote block (also covers BL-053 Phase 3 Obsidian-style
    // callouts: `> [!type] title` on the first line lifts the block
    // out of <blockquote> and into <div class="nx-callout …">.)
    if (/^>\s?/.test(line)) {
      const buf: string[] = []
      while (i < lines.length && /^>\s?/.test(lines[i])) {
        buf.push(lines[i].replace(/^>\s?/, ''))
        i++
      }
      const callout = parseCalloutHeader(buf[0] ?? '')
      if (callout) {
        const bodyLines = buf.slice(1)
        out.push(renderCallout(callout.type, callout.title, bodyLines))
      } else {
        out.push(`<blockquote>${inline(buf.join(' '))}</blockquote>`)
      }
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
  // `code` — BL-053 Phase 2: tag path-style tokens (slash + restricted
  // alphabet) so the doc theme can tint them ember without disturbing
  // prose code like `useState` or `n + 1`.
  s = s.replace(/`([^`]+)`/g, (_m, t) =>
    isCodepath(t) ? `<code class="codepath">${t}</code>` : `<code>${t}</code>`,
  )
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

/** True when the inline-code text reads like a file path / glob.
 *  Heuristic from BL-053 §3 Phase 2: the text contains a `/` and is
 *  built from `\w` / `.` / `*` / `-` only. Tokens like
 *  `crates/nexus-storage/src/find_replace.rs` and `docs/PRDs/*.md`
 *  match; `useState` and `n + 1` do not. The regex applies to the
 *  HTML-escaped text, so `&` / `<` / `>` / quotes already moot the
 *  match (they are not in the allowed set). */
export function isCodepath(text: string): boolean {
  return text.includes('/') && /^[\w./*-]+$/.test(text)
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

// ─── Callouts (BL-053 Phase 3) ──────────────────────────────────────────

/** The canonical callout types. Aliases (`warn` → `warning`, `risk` →
 *  `danger`) collapse onto these in [`normaliseCalloutType`] so the
 *  CSS only has to style seven kinds. `update` is the mockup-specific
 *  ember-dotted variant — not in Obsidian's vocabulary but documented
 *  in the BL-053 plan §3.3. */
export type CalloutType = 'note' | 'tip' | 'info' | 'warning' | 'danger' | 'quote' | 'update'

/** Pulls `[!type] optional title` off the leading line of a
 *  blockquote. Returns null when the line doesn't match the pattern,
 *  in which case the caller falls back to a plain `<blockquote>`. */
export function parseCalloutHeader(firstLine: string): { type: CalloutType; title: string } | null {
  const m = firstLine.match(/^\[!([A-Za-z]+)\]\s*(.*)$/)
  if (!m) return null
  return {
    type: normaliseCalloutType(m[1]),
    title: m[2].trim(),
  }
}

/** Map the raw type token (case-insensitive, alias-tolerant) to the
 *  canonical [`CalloutType`]. Unknown tokens collapse to `note` so the
 *  block still renders rather than vanishing. Public so unit tests can
 *  pin the alias matrix. */
export function normaliseCalloutType(raw: string): CalloutType {
  switch (raw.toLowerCase()) {
    case 'note':    return 'note'
    case 'tip':     return 'tip'
    case 'info':    return 'info'
    case 'warning':
    case 'warn':    return 'warning'
    case 'danger':
    case 'risk':    return 'danger'
    case 'quote':   return 'quote'
    case 'update':  return 'update'
    default:        return 'note'
  }
}

function renderCallout(type: CalloutType, title: string, bodyLines: string[]): string {
  // Body: paragraphs separated by a blank line; blank-only lines split
  // paragraphs. Inline transforms (wikilink, code, bold, …) run on
  // each paragraph just like top-level text would.
  const paragraphs: string[] = []
  let current: string[] = []
  for (const line of bodyLines) {
    if (line.trim() === '') {
      if (current.length > 0) {
        paragraphs.push(current.join(' '))
        current = []
      }
    } else {
      current.push(line)
    }
  }
  if (current.length > 0) paragraphs.push(current.join(' '))

  const headerTitle = title || defaultCalloutTitle(type)
  const body = paragraphs.length === 0
    ? ''
    : paragraphs.map((p) => `<p>${inline(p)}</p>`).join('')
  return (
    `<div class="nx-callout nx-callout--${type}">`
    + `<div class="nx-callout-header">`
    + `<span class="nx-callout-dot" aria-hidden="true"></span>`
    + `<span class="nx-callout-title">${inline(headerTitle)}</span>`
    + `</div>`
    + (body ? `<div class="nx-callout-body">${body}</div>` : '')
    + `</div>`
  )
}

function defaultCalloutTitle(type: CalloutType): string {
  switch (type) {
    case 'note':    return 'Note'
    case 'tip':     return 'Tip'
    case 'info':    return 'Info'
    case 'warning': return 'Warning'
    case 'danger':  return 'Danger'
    case 'quote':   return 'Quote'
    case 'update':  return 'Update'
  }
}
