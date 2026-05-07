/**
 * BL-058: URL detection for the terminal output stream.
 *
 * Direct port of `crates/nexus-terminal/src/urls.rs` so the Rust core
 * and the shell agree byte-for-byte on what counts as a URL. Three
 * regex families:
 *
 *   - `http(s)://` absolute URLs.
 *   - `file://` URLs (editor jump-to-line style references).
 *   - bare `localhost:PORT…` / `127.0.0.1:PORT…` (rewritten with a
 *     synthesised `http://` prefix so the OS opener accepts them).
 *
 * Terminating ASCII punctuation (`.,;:!?)]}>`) is stripped from the
 * tail of every match — sentence-end punctuation that almost always
 * isn't part of the URL.
 */

export type UrlKind = 'HttpHttps' | 'File' | 'Localhost'

export interface UrlMatch {
  /** UTF-16 code unit offset where the URL starts inside the scanned text. */
  start: number
  /** Code unit offset one past the end. */
  end: number
  /** URL as it appears in the source text (no normalisation). */
  raw: string
  /** URL ready to pass to a browser / OS opener. */
  resolved: string
  /** Which family matched. */
  kind: UrlKind
}

const TRAILING_PUNCT = '.,;:!?)]}>'
const URL_TERMINATORS = '[^\\s()\\[\\]<>"\']+'

const HTTP_RE = new RegExp(`https?://${URL_TERMINATORS}`, 'g')
const FILE_RE = new RegExp(`file://${URL_TERMINATORS}`, 'g')
const LOCALHOST_RE = new RegExp(
  `\\b(?:localhost|127\\.0\\.0\\.1):\\d+${URL_TERMINATORS.replace('+', '*')}`,
  'g',
)

/**
 * Rewrite a raw URL into the form a browser / `open` command accepts.
 * `localhost:PORT…` → `http://127.0.0.1:PORT…`; `127.0.0.1:PORT…`
 * without scheme → `http://…`. Everything else is returned unchanged.
 */
export function resolveUrl(raw: string): string {
  if (raw.startsWith('http://') || raw.startsWith('https://') || raw.startsWith('file://')) {
    return raw
  }
  if (raw.startsWith('localhost:')) {
    return `http://127.0.0.1:${raw.slice('localhost:'.length)}`
  }
  if (raw.startsWith('127.0.0.1:')) {
    return `http://${raw}`
  }
  return raw
}

/**
 * Scan `text` for URLs and return every match, sorted by start offset.
 * Overlapping matches are resolved by preferring the more specific
 * scheme — a `localhost:PORT` match that overlaps an `https://localhost`
 * is dropped so the same span doesn't surface twice.
 */
export function detectUrls(text: string): UrlMatch[] {
  const hits: UrlMatch[] = []
  pushAllMatches(hits, text, HTTP_RE, 'HttpHttps')
  pushAllMatches(hits, text, FILE_RE, 'File')

  for (const m of allMatches(text, LOCALHOST_RE)) {
    const overlaps = hits.some((h) => rangesOverlap(h.start, h.end, m.start, m.end))
    if (!overlaps) hits.push(buildMatch(text, m.start, m.end, 'Localhost'))
  }
  hits.sort((a, b) => a.start - b.start)
  return hits
}

function pushAllMatches(out: UrlMatch[], text: string, re: RegExp, kind: UrlKind): void {
  for (const m of allMatches(text, re)) {
    out.push(buildMatch(text, m.start, m.end, kind))
  }
}

function* allMatches(text: string, re: RegExp): Iterable<{ start: number; end: number }> {
  // Reset lastIndex so callers can reuse the module-level singletons.
  re.lastIndex = 0
  let m: RegExpExecArray | null = re.exec(text)
  while (m !== null) {
    const start = m.index
    const end = start + m[0].length
    yield { start, end }
    if (re.lastIndex === start) {
      // Zero-width match guard — advance manually to avoid an infinite loop.
      re.lastIndex = end + 1
    }
    m = re.exec(text)
  }
}

function rangesOverlap(aStart: number, aEnd: number, bStart: number, bEnd: number): boolean {
  return aStart < bEnd && bStart < aEnd
}

function buildMatch(text: string, start: number, end: number, kind: UrlKind): UrlMatch {
  let trimmedEnd = end
  while (trimmedEnd > start) {
    const ch = text.charAt(trimmedEnd - 1)
    if (TRAILING_PUNCT.includes(ch)) trimmedEnd -= 1
    else break
  }
  const raw = text.slice(start, trimmedEnd)
  return { start, end: trimmedEnd, raw, resolved: resolveUrl(raw), kind }
}
