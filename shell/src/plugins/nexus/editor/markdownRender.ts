// Shared markdown → sanitized HTML helper. Extracted out of
// EditorView so other plugins (nexus.ai chat) can reuse the exact
// same pipeline + CSS (.nexus-markdown-body).
//
// marked.parse returns string when `async: false`. Sanitize before
// handing HTML to React's dangerouslySetInnerHTML — user notes
// aren't hostile, but DOMPurify is cheap insurance, and AI output
// is even less trustworthy.
//
// BL-053 Phases 2–4: the renderer additionally handles
//   - Phase 2a path-style inline `<code>` (ember tint via class)
//   - Phase 2b `[[wikilinks]]` (custom inline tokenizer)
//   - Phase 2c YAML frontmatter (stripped + emitted as a metadata
//     pill bar below H1)
//   - Phase 3   `> [!type] Title\n> body` Obsidian-style callouts
//     (blockquote renderer override)
//   - Phase 4a  status pills in table cells (`<td>Complete</td>`
//     → `<span class="nx-status-pill --ok">Complete</span>`)
// File-tree dots (Phase 4b in the PRD §1 row D) are out of scope
// here because they live in the file-tree plugin and need
// frontmatter for every visible node — tracked as a follow-up.

import { marked, type Tokens } from 'marked'
import DOMPurify from 'dompurify'
import { fencedCodeRegistry } from './cm/fencedCodeRegistry'

const FENCED_PENDING_CLASS = 'nexus-fenced-pending'
const FENCED_PENDING_ATTR = 'data-nexus-fenced'

/** Inline `<code>` whose text looks like a path or version gets
 * `nx-codepath` so the CSS can tint it ember. Heuristic: text
 * matches `^[\w./*-]+$` AND contains a `/` OR a `.` between
 * digit/letters (version literals like `0.4.0`). Matches the
 * mockup's "01-17", "crates/**" examples. */
const PATH_CODE_RE = /^[A-Za-z0-9_./*-]+$/

/** Status keywords recognised in table cells + standalone inline
 * code. The token left of the colon is the user-visible label;
 * the right side maps to the `nx-status-pill__chip--{kind}` CSS
 * tone bucket. New labels stay text-only (no pill) so unknown
 * status strings degrade safely. */
const STATUS_KIND: Record<string, 'ok' | 'warn' | 'risk' | 'info' | 'muted'> = {
  Complete: 'ok',
  Substantial: 'ok',
  Partial: 'warn',
  Scaffolded: 'info',
  'Not started': 'muted',
  Deferred: 'muted',
}

let rendererPatched = false

function ensureFencedRendererPatch(): void {
  if (rendererPatched) return
  rendererPatched = true
  marked.use({
    extensions: [wikilinkExtension(), calloutExtension()],
    renderer: {
      code(this: { parser: { parse: unknown } }, token: Tokens.Code): string | false {
        const lang = (token.lang ?? '').trim().split(/\s+/)[0] ?? ''
        if (!lang || !fencedCodeRegistry.has(lang)) return false
        const encoded = encodeFencedSource(token.text)
        return (
          `<div class="${FENCED_PENDING_CLASS}" ` +
          `${FENCED_PENDING_ATTR}-lang="${escapeAttr(lang)}" ` +
          `${FENCED_PENDING_ATTR}-source="${encoded}"></div>`
        )
      },
      codespan(token: Tokens.Codespan): string {
        // BL-053 Phase 2a — tint path-shaped inline code via a
        // class hook. The CSS lives in `markdown.css`. Status
        // keywords inside a `<code>` (e.g. ``Complete`` outside a
        // table) become pills in their own right.
        const text = token.text
        const status = STATUS_KIND[text]
        if (status) {
          return `<span class="nx-status-pill nx-status-pill__chip--${status}">${escapeHtml(text)}</span>`
        }
        if (PATH_CODE_RE.test(text) && (text.includes('/') || /\d.*\d/.test(text))) {
          return `<code class="nx-codepath">${escapeHtml(text)}</code>`
        }
        return `<code>${escapeHtml(text)}</code>`
      },
      tablecell(
        this: { parser: { parseInline: (tokens: Tokens.Generic[]) => string } },
        token: Tokens.TableCell,
      ): string {
        // BL-053 Phase 4a — status pills in table cells. Cell
        // bodies that are exactly a known status label render as
        // a pill; everything else falls through to the default
        // text rendering with inline markdown re-parsed.
        const tag = token.header ? 'th' : 'td'
        const align = token.align ? ` style="text-align: ${token.align}"` : ''
        const plain = this.parser.parseInline((token.tokens ?? []) as Tokens.Generic[]).trim()
        const stripped = stripTags(plain).trim()
        const status = STATUS_KIND[stripped]
        if (status) {
          return `<${tag}${align}><span class="nx-status-pill nx-status-pill__chip--${status}">${escapeHtml(stripped)}</span></${tag}>`
        }
        return `<${tag}${align}>${plain}</${tag}>`
      },
    },
  })
}

function stripTags(html: string): string {
  return html.replace(/<[^>]*>/g, '')
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

// ── BL-053 Phase 2b — wikilink extension ─────────────────────────

interface WikilinkToken {
  type: 'wikilink'
  raw: string
  target: string
  display: string
  fragment: string | null
}

function wikilinkExtension() {
  return {
    name: 'wikilink',
    level: 'inline' as const,
    start(src: string): number | undefined {
      const idx = src.indexOf('[[')
      return idx >= 0 ? idx : undefined
    },
    tokenizer(src: string): WikilinkToken | undefined {
      const m = /^\[\[([^\]\n|#]+)(#[^\]\n|]+)?(\|[^\]\n]+)?\]\]/.exec(src)
      if (!m) return undefined
      const target = m[1]!.trim()
      const fragment = m[2] ? m[2]!.slice(1) : null
      const display = m[3] ? m[3]!.slice(1).trim() : target
      return {
        type: 'wikilink',
        raw: m[0],
        target,
        display,
        fragment,
      }
    },
    renderer(token: WikilinkToken): string {
      const href = encodeURI(token.target)
      const label = escapeHtml(token.display)
      const frag = token.fragment ? `#${escapeHtml(token.fragment)}` : ''
      return `<a class="nx-wikilink" href="${href}${frag}" data-wikilink="${escapeAttr(token.target)}">${label}</a>`
    },
  }
}

// ── BL-053 Phase 3 — Obsidian-style callouts ─────────────────────

interface CalloutToken {
  type: 'callout'
  raw: string
  kind: string
  title: string
  body: string
}

const CALLOUT_KINDS = new Set([
  'info',
  'note',
  'tip',
  'warn',
  'warning',
  'risk',
  'danger',
  'error',
  'success',
  'ok',
  'update',
  'todo',
])

function calloutExtension() {
  return {
    name: 'callout',
    level: 'block' as const,
    start(src: string): number | undefined {
      const idx = src.indexOf('> [!')
      return idx >= 0 ? idx : undefined
    },
    tokenizer(src: string): CalloutToken | undefined {
      // Match `> [!kind] Title\n> body lines` blockquote-style.
      const m = /^> \[!([A-Za-z]+)\]\s*([^\n]*)\n((?:> .*\n?)*)/.exec(src)
      if (!m) return undefined
      const kind = m[1]!.toLowerCase()
      if (!CALLOUT_KINDS.has(kind)) return undefined
      const title = m[2]!.trim()
      const body = (m[3] ?? '')
        .split('\n')
        .map((line) => line.replace(/^> ?/, ''))
        .join('\n')
        .trim()
      return {
        type: 'callout',
        raw: m[0],
        kind,
        title,
        body,
      }
    },
    renderer(token: CalloutToken): string {
      const cls = `nx-callout nx-callout--${token.kind}`
      const titleHtml = token.title ? escapeHtml(token.title) : labelFor(token.kind)
      const bodyHtml = token.body
        ? (marked.parse(token.body, { async: false }) as string)
        : ''
      return (
        `<div class="${cls}">` +
        `<div class="nx-callout__head">` +
        `<span class="nx-callout__dot" aria-hidden="true"></span>` +
        `<span class="nx-callout__title">${titleHtml}</span>` +
        `</div>` +
        `<div class="nx-callout__body">${bodyHtml}</div>` +
        `</div>`
      )
    },
  }
}

function labelFor(kind: string): string {
  // Default human label per kind. Title-case the input.
  return kind.charAt(0).toUpperCase() + kind.slice(1)
}

// ── BL-053 Phase 2c — frontmatter metadata bar ───────────────────

/** Strip + parse a leading YAML frontmatter block. Returns
 *  `{ raw, body, fields }`; `raw` is the original frontmatter
 *  (preserved for tooling that wants it), `body` is the
 *  frontmatter-stripped markdown ready for marked, and `fields`
 *  is a flat key → string map for the metadata bar. Supports
 *  simple `key: value` lines and `- item` list values; nested
 *  structures fall back to a raw-string representation so the bar
 *  has *something* to render. */
export function parseFrontmatter(source: string): {
  raw: string
  body: string
  fields: Record<string, string>
  hasFrontmatter: boolean
} {
  const fences = ['---\n', '---\r\n']
  let opener: string | null = null
  for (const f of fences) {
    if (source.startsWith(f)) {
      opener = f
      break
    }
  }
  if (!opener) {
    return { raw: '', body: source, fields: {}, hasFrontmatter: false }
  }
  const after = source.slice(opener.length)
  const closeMarkerIdx = findFrontmatterClose(after)
  if (closeMarkerIdx < 0) {
    return { raw: '', body: source, fields: {}, hasFrontmatter: false }
  }
  const yaml = after.slice(0, closeMarkerIdx)
  // Skip past the closing `---` plus its newline.
  const afterClose = after.slice(closeMarkerIdx).replace(/^---\r?\n?/, '')
  const fields = parseSimpleYaml(yaml)
  return {
    raw: yaml,
    body: afterClose,
    fields,
    hasFrontmatter: true,
  }
}

function findFrontmatterClose(text: string): number {
  // Look for a line that is exactly `---` (with optional CR).
  let pos = 0
  while (pos < text.length) {
    const nl = text.indexOf('\n', pos)
    const line = nl < 0 ? text.slice(pos) : text.slice(pos, nl)
    const trimmed = line.replace(/\r$/, '')
    if (trimmed === '---') return pos
    if (nl < 0) break
    pos = nl + 1
  }
  return -1
}

/** Tiny YAML subset parser — handles flat `key: value` lines and
 *  one-level lists (`- item`). Nested maps fall back to a raw
 *  string. Sufficient for the metadata bar's needs; richer
 *  structures route through the kernel's `read_frontmatter` IPC
 *  when a caller needs them. */
function parseSimpleYaml(yaml: string): Record<string, string> {
  const out: Record<string, string> = {}
  let lastKey: string | null = null
  let listAccum: string[] = []
  const flush = () => {
    if (lastKey && listAccum.length > 0) {
      out[lastKey] = listAccum.join(', ')
    }
    lastKey = null
    listAccum = []
  }
  for (const rawLine of yaml.split('\n')) {
    const line = rawLine.replace(/\r$/, '')
    if (!line.trim() || line.trim().startsWith('#')) continue
    const listMatch = /^\s+-\s*(.*)$/.exec(line)
    if (listMatch && lastKey) {
      listAccum.push(listMatch[1]!.trim())
      continue
    }
    const kvMatch = /^([A-Za-z0-9_-]+)\s*:\s*(.*)$/.exec(line)
    if (!kvMatch) continue
    flush()
    const [, key, valueRaw] = kvMatch
    const value = (valueRaw ?? '').trim()
    if (value === '') {
      // Start of a list value — accumulate via `lastKey`.
      lastKey = key!
      continue
    }
    out[key!] = stripQuotes(value)
  }
  flush()
  return out
}

function stripQuotes(s: string): string {
  if ((s.startsWith('"') && s.endsWith('"')) || (s.startsWith("'") && s.endsWith("'"))) {
    return s.slice(1, -1)
  }
  return s
}

/** Render a metadata bar HTML block from parsed frontmatter
 *  fields. Returns an empty string when no fields are present.
 *  The bar surfaces a `forge` chip + the `updated` date + every
 *  remaining scalar field; the design mockup shows the bar
 *  directly under the H1. */
export function renderFrontmatterBar(
  fields: Record<string, string>,
  forgeName: string | null,
): string {
  const chips: string[] = []
  if (forgeName) {
    chips.push(
      `<span class="nx-frontmatter__chip">forge · <strong>${escapeHtml(forgeName)}</strong></span>`,
    )
  }
  for (const [key, value] of Object.entries(fields)) {
    if (!value || value.length === 0) continue
    // Status renders as a pill (Phase 4a flavour), not a flat chip.
    if (key === 'status') {
      const kind = STATUS_KIND[value]
      if (kind) {
        chips.push(
          `<span class="nx-status-pill nx-status-pill__chip--${kind}">${escapeHtml(value)}</span>`,
        )
        continue
      }
    }
    chips.push(
      `<span class="nx-frontmatter__chip">${escapeHtml(key)} · ${escapeHtml(value)}</span>`,
    )
  }
  if (chips.length === 0) return ''
  return `<div class="nx-frontmatter">${chips.join('')}</div>`
}

function escapeAttr(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

function encodeFencedSource(source: string): string {
  return btoa(unescape(encodeURIComponent(source)))
}

function decodeFencedSource(encoded: string): string {
  return decodeURIComponent(escape(atob(encoded)))
}

export interface RenderMarkdownOptions {
  /** Forge name surfaced inside the frontmatter metadata bar
   *  (Phase 2c). Omitted when callers don't have one in scope. */
  forgeName?: string | null
}

export function renderMarkdown(
  content: string,
  options: RenderMarkdownOptions = {},
): string {
  ensureFencedRendererPatch()
  const { body, fields, hasFrontmatter } = parseFrontmatter(content)
  const parsedBody = marked.parse(body, { async: false }) as string
  const barHtml = hasFrontmatter
    ? renderFrontmatterBar(fields, options.forgeName ?? null)
    : ''
  // Splice the bar in *after* the first H1 so it sits directly
  // under the title — matches the mockup's `FORGE · NEXUS_WORK ·
  // UPDATED …` band. Falls back to "before everything" when
  // there's no H1 (which is the rare case).
  const withBar = barHtml ? spliceBarAfterFirstH1(parsedBody, barHtml) : parsedBody
  return DOMPurify.sanitize(withBar, {
    ADD_ATTR: [
      `${FENCED_PENDING_ATTR}-lang`,
      `${FENCED_PENDING_ATTR}-source`,
      'data-wikilink',
    ],
  })
}

/** Insert the metadata bar HTML directly after the document's
 *  first `<h1>`. When the document has no `<h1>`, prepend the bar
 *  to keep the chip row visible. */
function spliceBarAfterFirstH1(html: string, bar: string): string {
  const match = /<h1[^>]*>[\s\S]*?<\/h1>/.exec(html)
  if (!match) return `${bar}${html}`
  const end = match.index + match[0].length
  return `${html.slice(0, end)}${bar}${html.slice(end)}`
}

/**
 * Walks a sanitized markdown DOM tree and replaces every
 * `.nexus-fenced-pending` placeholder with the rendered HTMLElement
 * produced by `fencedCodeRegistry`. Synchronous renders swap in
 * immediately; async renders show a `<pre><code>` placeholder until
 * the underlying promise resolves, at which point the placeholder is
 * replaced — only if the placeholder is still attached to the DOM.
 */
export function hydrateFencedCode(root: HTMLElement | null): void {
  if (!root) return
  const placeholders = root.querySelectorAll<HTMLElement>(`.${FENCED_PENDING_CLASS}`)
  for (const node of Array.from(placeholders)) {
    const lang = node.getAttribute(`${FENCED_PENDING_ATTR}-lang`) ?? ''
    const encoded = node.getAttribute(`${FENCED_PENDING_ATTR}-source`) ?? ''
    if (!lang || !fencedCodeRegistry.has(lang)) {
      replaceWithRawSource(node, encoded)
      continue
    }
    let source: string
    try {
      source = decodeFencedSource(encoded)
    } catch {
      replaceWithRawSource(node, encoded)
      continue
    }
    const sync = fencedCodeRegistry.renderCached(lang, source)
    if (sync) {
      node.replaceWith(wrap(sync, lang))
      continue
    }
    swapPlaceholderWithRawSource(node, source, lang)
    const pending = fencedCodeRegistry.awaitPending(lang, source)
    if (!pending) continue
    void pending.then((result) => {
      if (!node.isConnected) return
      if (result instanceof Error) {
        node.replaceWith(buildErrorBox(lang, result))
        return
      }
      node.replaceWith(wrap(result, lang))
    })
  }
}

function wrap(rendered: HTMLElement, language: string): HTMLElement {
  const wrap = document.createElement('div')
  wrap.className = 'nexus-fenced-rendered'
  wrap.dataset.language = language
  wrap.appendChild(rendered)
  return wrap
}

function swapPlaceholderWithRawSource(
  node: HTMLElement,
  source: string,
  language: string,
): void {
  node.textContent = ''
  node.dataset.language = language
  const pre = document.createElement('pre')
  const code = document.createElement('code')
  code.textContent = source
  pre.appendChild(code)
  node.appendChild(pre)
}

function replaceWithRawSource(node: HTMLElement, encoded: string): void {
  let source = ''
  try {
    source = decodeFencedSource(encoded)
  } catch {
    /* fall through to empty */
  }
  const pre = document.createElement('pre')
  const code = document.createElement('code')
  code.textContent = source
  pre.appendChild(code)
  node.replaceWith(pre)
}

function buildErrorBox(language: string, err: Error): HTMLElement {
  const box = document.createElement('div')
  box.className = 'nexus-fenced-error'
  const tag = document.createElement('span')
  tag.className = 'nexus-fenced-error-lang'
  tag.textContent = language
  const msg = document.createElement('span')
  msg.className = 'nexus-fenced-error-msg'
  msg.textContent = err.message || 'render failed'
  box.append(tag, msg)
  return box
}
