// C68 (#421) — "Copy as rich text": write a sanitized HTML blob plus a
// plain-text fallback to the clipboard, so pastes into apps that
// understand text/html (Gmail, Google Docs, Slack, Word) arrive
// formatted instead of as raw markdown.
//
// Mirrors the multi-MIME `ClipboardItem` write + plain-text-fallback
// pattern already used for table data in
// `../bases/clipboard.ts::writeClipboardPayload` — generalized here to
// arbitrary HTML rather than the bases-specific `ClipPayload` shape.

/** Derive a plain-text fallback from rendered HTML via a detached
 *  element's `textContent`, so external apps that only understand
 *  `text/plain` still see readable (if unformatted) text. */
export function htmlToPlainText(html: string): string {
  const el = document.createElement('div')
  el.innerHTML = html
  return el.textContent ?? ''
}

/** Write `html` to the clipboard as `text/html` + a derived
 *  `text/plain` fallback. Falls back to a plain-text-only write when
 *  the browser doesn't support multi-MIME `ClipboardItem` writes (or
 *  the write rejects — e.g. an insecure context or missing
 *  permission), and throws only when neither path is available. */
export async function copyRichTextToClipboard(html: string): Promise<void> {
  if (typeof navigator === 'undefined' || !navigator.clipboard) {
    throw new Error('Clipboard API unavailable')
  }
  const text = htmlToPlainText(html)
  const ClipboardItemCtor = (globalThis as unknown as { ClipboardItem?: typeof ClipboardItem })
    .ClipboardItem
  const writeFn = (navigator.clipboard as Clipboard & {
    write?: (items: ClipboardItems) => Promise<void>
  }).write
  if (ClipboardItemCtor && typeof writeFn === 'function') {
    try {
      const item = new ClipboardItemCtor({
        'text/html': new Blob([html], { type: 'text/html' }),
        'text/plain': new Blob([text], { type: 'text/plain' }),
      })
      await writeFn.call(navigator.clipboard, [item])
      return
    } catch {
      // Fall through to the plain-text-only write.
    }
  }
  await navigator.clipboard.writeText(text)
}
