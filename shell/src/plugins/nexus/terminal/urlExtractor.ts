import { detectUrls, type UrlMatch } from './urls'

/**
 * BL-058: stream-aware URL extractor.
 *
 * Wraps a UTF-8 `TextDecoder` plus a small line buffer. Bytes flow in
 * from the terminal output stream as `Uint8Array` chunks (PTY output);
 * the extractor concatenates decoded text, splits on `\n`, and emits
 * URL matches once each line is complete. Lines that don't contain a
 * full `\n` yet stay buffered until more bytes arrive — printing
 * "https://example.com" via two separate write calls still surfaces
 * exactly one match.
 *
 * ANSI / VT escape sequences (cursor positioning, colour codes, etc.)
 * are left untouched: the regex set used by `detectUrls` rejects the
 * `\x1b[` byte through its character-class terminators, so escape
 * sequences that happen to land inside a line don't pollute the match
 * — they just get filtered out by the regex's stop set.
 */
export interface UrlExtractor {
  /**
   * Feed a chunk of raw PTY bytes. Newly-detected URLs are passed to
   * the consumer one match at a time, in line-then-position order.
   */
  push(bytes: Uint8Array): void
  /**
   * Flush any buffered text without waiting for a trailing newline.
   * Useful at session end / on a `reset()` so a final URL on a line
   * with no `\n` still surfaces.
   */
  flush(): void
  /** Drop the buffered partial line. Called on session change. */
  reset(): void
}

/**
 * Build an extractor that calls `onUrl` for every detected URL.
 * The decoder is `fatal: false` so invalid UTF-8 is replaced with `�`
 * rather than throwing — partial multi-byte sequences at chunk
 * boundaries are handled by the decoder's `stream: true` mode.
 */
export function createUrlExtractor(onUrl: (m: UrlMatch) => void): UrlExtractor {
  const decoder = new TextDecoder('utf-8', { fatal: false })
  let buffer = ''

  const drainCompleteLines = () => {
    while (true) {
      const nl = buffer.indexOf('\n')
      if (nl < 0) return
      const line = stripCR(buffer.slice(0, nl))
      buffer = buffer.slice(nl + 1)
      emitFor(line)
    }
  }

  const emitFor = (line: string) => {
    if (line.length === 0) return
    for (const m of detectUrls(line)) onUrl(m)
  }

  return {
    push(bytes) {
      buffer += decoder.decode(bytes, { stream: true })
      drainCompleteLines()
    },
    flush() {
      // Force any trailing partial multi-byte sequence out of the
      // decoder, then drain whatever's left in the line buffer.
      buffer += decoder.decode()
      drainCompleteLines()
      if (buffer.length > 0) {
        emitFor(stripCR(buffer))
        buffer = ''
      }
    },
    reset() {
      buffer = ''
      // A new decoder discards any partial multi-byte sequence — the
      // session changed, the bytes that follow won't continue from
      // there.
      decoder.decode(new Uint8Array(), { stream: false })
    },
  }
}

function stripCR(s: string): string {
  return s.endsWith('\r') ? s.slice(0, s.length - 1) : s
}
