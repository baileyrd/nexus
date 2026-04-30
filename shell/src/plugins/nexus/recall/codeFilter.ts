// BL-046 phase 2 — recall-side "from project" filter.
//
// The capture half (phase 1, `memory/codeCapture.ts`) emits a
// `#code/<language>` recall tag plus a fenced code block on every
// IDE-driven capture. This module surfaces that on the recall
// overlay as a filter chip — toggle it on, and only matches whose
// `chunk_text` contains the tag (or the `File:` / ```` fence
// header pair) survive.
//
// Pure helpers — the React overlay imports them, the store holds
// the toggle state, and the unit tests pin every detection branch
// without standing up any UI.

import type { RecallMatch } from './recallStore'

/** Match either the explicit `#code/<lang>` recall tag (the
 *  canonical form emitted by BL-046 phase 1) or the older / hand-
 *  written `File: <path>\n…\n``` ` opening-fence pair so we don't
 *  miss captures the user authored before phase 1 shipped. The
 *  `m` flag lets `^#code/` anchor at line starts inside a chunk
 *  that includes preceding context. */
const CODE_TAG_RE = /(^|\n)#code\//
const FILE_HEADER_RE = /(^|\n)File:\s+\S+/
const FENCE_OPEN_RE = /(^|\n)```[a-zA-Z][\w+-]*/

/** Heuristic: is this match a code capture? Returns true on any
 *  of three signals (any one is sufficient):
 *
 *   * `#code/<language>` tag on its own line;
 *   * `File: <path>` header line;
 *   * a fence opener with a language info-string (` ```rust` /
 *     ` ```typescript`, …).
 *
 *  Each signal is a positive on its own — a chunk_text containing
 *  any one of them came from the BL-046 capture path or a hand-
 *  written equivalent the user thinks of as a "code capture".
 *  The fence-opener heuristic alone could over-match a regular
 *  markdown note; the recall side queries the *capture inbox*, so
 *  the false-positive blast radius is bounded. */
export function isCodeCaptureMatch(match: RecallMatch): boolean {
  const text = match.chunk_text ?? ''
  if (!text) return false
  if (CODE_TAG_RE.test(text)) return true
  if (FILE_HEADER_RE.test(text)) return true
  if (FENCE_OPEN_RE.test(text)) return true
  return false
}

/** Apply the code-only filter to a result list. When `codeOnly`
 *  is `false` the input is returned untouched (filter chip off);
 *  when `true` the result keeps only matches `isCodeCaptureMatch`
 *  reports as positive. Stable order — input order preserved. */
export function applyCodeFilter(
  matches: RecallMatch[],
  codeOnly: boolean,
): RecallMatch[] {
  if (!codeOnly) return matches
  return matches.filter(isCodeCaptureMatch)
}

/** Extract a list of language tags present in a match's chunk
 *  text — drives the BL-046 phase-3 per-language chip row. */
export function extractCodeLanguages(match: RecallMatch): string[] {
  const text = match.chunk_text ?? ''
  if (!text) return []
  const out = new Set<string>()
  const tagRe = /(?:^|\n)#code\/([a-zA-Z][\w+-]*)/g
  let m: RegExpExecArray | null
  while ((m = tagRe.exec(text)) !== null) out.add(m[1].toLowerCase())
  const fenceRe = /(?:^|\n)```([a-zA-Z][\w+-]*)/g
  while ((m = fenceRe.exec(text)) !== null) out.add(m[1].toLowerCase())
  return Array.from(out)
}

/** Roll up the languages present across a result set, sorted
 *  alphabetically and deduplicated. Used by the chip row to know
 *  which language pills to render for the *currently visible*
 *  results — pills only appear for languages a user can actually
 *  filter to. */
export function availableLanguages(matches: RecallMatch[]): string[] {
  const out = new Set<string>()
  for (const m of matches) {
    for (const lang of extractCodeLanguages(m)) out.add(lang)
  }
  return Array.from(out).sort()
}

/** OR-semantics language filter: when `languages` is non-empty,
 *  keep only matches whose `extractCodeLanguages` intersects the
 *  selected set. An empty selection is the unfiltered passthrough
 *  (consistent with the chip-row idiom — clicking every chip off
 *  is equivalent to no filter). Stable order — input order is
 *  preserved. */
export function applyLanguageFilter(
  matches: RecallMatch[],
  languages: readonly string[],
): RecallMatch[] {
  if (languages.length === 0) return matches
  const wanted = new Set(languages.map((l) => l.toLowerCase()))
  return matches.filter((m) => {
    const langs = extractCodeLanguages(m)
    for (const l of langs) {
      if (wanted.has(l)) return true
    }
    return false
  })
}
