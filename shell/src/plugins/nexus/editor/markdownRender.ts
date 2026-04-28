// Shared markdown → sanitized HTML helper. Extracted out of
// EditorView so other plugins (nexus.ai chat) can reuse the exact
// same pipeline + CSS (.nexus-markdown-body).
//
// marked.parse returns string when `async: false`. Sanitize before
// handing HTML to React's dangerouslySetInnerHTML — user notes
// aren't hostile, but DOMPurify is cheap insurance, and AI output
// is even less trustworthy.

import { marked, type Tokens } from 'marked'
import DOMPurify from 'dompurify'
import { fencedCodeRegistry } from './cm/fencedCodeRegistry'

const FENCED_PENDING_CLASS = 'nexus-fenced-pending'
const FENCED_PENDING_ATTR = 'data-nexus-fenced'

let rendererPatched = false

function ensureFencedRendererPatch(): void {
  if (rendererPatched) return
  rendererPatched = true
  marked.use({
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
    },
  })
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

export function renderMarkdown(content: string): string {
  ensureFencedRendererPatch()
  const raw = marked.parse(content, { async: false }) as string
  return DOMPurify.sanitize(raw, {
    ADD_ATTR: [`${FENCED_PENDING_ATTR}-lang`, `${FENCED_PENDING_ATTR}-source`],
  })
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
