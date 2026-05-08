/**
 * AIG-02 — minimal line-level diff for the agent approval card.
 *
 * We only need a coarse "what's about to change" signal so the user
 * can spot a destructive overwrite before approving. A full
 * Myers-quality diff would be overkill (and would pull in a
 * dependency); a longest-common-subsequence walk on whole lines is
 * accurate enough at the scale of forge markdown files.
 *
 * Paired remove/add blocks additionally carry word-level segments so
 * a one-word edit doesn't read as a wholesale line replacement.
 */

export type DiffLineKind = 'context' | 'add' | 'remove'

export type WordSegmentKind = 'common' | 'add' | 'remove'

export interface WordSegment {
  kind: WordSegmentKind
  text: string
}

export interface DiffLine {
  kind: DiffLineKind
  text: string
  /** Word-level segments. Present only on `add` / `remove` lines
   *  that belong to a paired replace block (one removed line lined
   *  up with one added line). Lines outside a pairing — or the
   *  unmatched tail when remove/add counts differ — have no
   *  segments and render as a flat coloured row. */
  segments?: WordSegment[]
}

/** Cap diffs at this many lines so a wholesale-rewrite paste doesn't
 *  flood the approval card. The card surfaces a "+N more changes"
 *  hint when truncation kicks in. */
export const DIFF_MAX_LINES = 200

interface DiffResult {
  lines: DiffLine[]
  /** True when [`DIFF_MAX_LINES`] forced a truncation. */
  truncated: boolean
  /** True when both inputs are byte-identical. The card hides the
   *  diff and shows a "no changes" hint in this case. */
  unchanged: boolean
}

function splitLines(s: string): string[] {
  if (s.length === 0) return []
  // Preserve trailing newlines as their own empty line so a missing
  // trailing newline shows up in the diff.
  return s.split('\n')
}

/**
 * LCS-based diff. Walks the table backwards to emit the merged
 * sequence; ties prefer adds-before-removes so consecutive replaces
 * render as a remove block followed by the add block (easier to
 * read than interleaved).
 */
export function diffLines(before: string, after: string): DiffResult {
  if (before === after) {
    return { lines: [], truncated: false, unchanged: true }
  }
  const a = splitLines(before)
  const b = splitLines(after)
  const m = a.length
  const n = b.length

  // LCS DP — row-major Int32 array kept small enough that even a
  // 1000-line file stays under 4 MB.
  const dp = new Int32Array((m + 1) * (n + 1))
  const w = n + 1
  for (let i = m - 1; i >= 0; i--) {
    for (let j = n - 1; j >= 0; j--) {
      if (a[i] === b[j]) {
        dp[i * w + j] = dp[(i + 1) * w + (j + 1)] + 1
      } else {
        const down = dp[(i + 1) * w + j]
        const right = dp[i * w + (j + 1)]
        dp[i * w + j] = down >= right ? down : right
      }
    }
  }

  const out: DiffLine[] = []
  let i = 0
  let j = 0
  let truncated = false
  while (i < m && j < n) {
    if (out.length >= DIFF_MAX_LINES) {
      truncated = true
      break
    }
    if (a[i] === b[j]) {
      out.push({ kind: 'context', text: a[i] })
      i++
      j++
    } else if (dp[(i + 1) * w + j] >= dp[i * w + (j + 1)]) {
      out.push({ kind: 'remove', text: a[i] })
      i++
    } else {
      out.push({ kind: 'add', text: b[j] })
      j++
    }
  }
  while (i < m && out.length < DIFF_MAX_LINES) {
    out.push({ kind: 'remove', text: a[i++] })
  }
  while (j < n && out.length < DIFF_MAX_LINES) {
    out.push({ kind: 'add', text: b[j++] })
  }
  if (i < m || j < n) truncated = true

  enrichWordDiff(out)

  return { lines: out, truncated, unchanged: false }
}

const TOKEN_RE = /\w+|\s+|[^\w\s]/g

/**
 * Split a line into tokens for word-level LCS. Matches one of:
 * a run of word chars, a run of whitespace, or a single
 * non-word/non-whitespace char (so punctuation diffs cleanly
 * without dragging neighbouring words along).
 */
export function tokenize(line: string): string[] {
  return line.match(TOKEN_RE) ?? []
}

/**
 * LCS-based word diff. Returns segments tagged `common` / `add` /
 * `remove` ready to render inline. Adjacent same-kind segments are
 * coalesced so the renderer doesn't emit a wrapper per token.
 */
export function diffWords(before: string, after: string): {
  before: WordSegment[]
  after: WordSegment[]
} {
  const a = tokenize(before)
  const b = tokenize(after)
  const m = a.length
  const n = b.length

  const dp = new Int32Array((m + 1) * (n + 1))
  const w = n + 1
  for (let i = m - 1; i >= 0; i--) {
    for (let j = n - 1; j >= 0; j--) {
      if (a[i] === b[j]) {
        dp[i * w + j] = dp[(i + 1) * w + (j + 1)] + 1
      } else {
        const down = dp[(i + 1) * w + j]
        const right = dp[i * w + (j + 1)]
        dp[i * w + j] = down >= right ? down : right
      }
    }
  }

  const beforeSegs: WordSegment[] = []
  const afterSegs: WordSegment[] = []
  let i = 0
  let j = 0
  while (i < m && j < n) {
    if (a[i] === b[j]) {
      pushSegment(beforeSegs, 'common', a[i])
      pushSegment(afterSegs, 'common', b[j])
      i++
      j++
    } else if (dp[(i + 1) * w + j] >= dp[i * w + (j + 1)]) {
      pushSegment(beforeSegs, 'remove', a[i])
      i++
    } else {
      pushSegment(afterSegs, 'add', b[j])
      j++
    }
  }
  while (i < m) {
    pushSegment(beforeSegs, 'remove', a[i++])
  }
  while (j < n) {
    pushSegment(afterSegs, 'add', b[j++])
  }
  return { before: beforeSegs, after: afterSegs }
}

function pushSegment(segs: WordSegment[], kind: WordSegmentKind, text: string): void {
  const tail = segs[segs.length - 1]
  if (tail && tail.kind === kind) {
    tail.text += text
  } else {
    segs.push({ kind, text })
  }
}

/**
 * Walks a line-level diff in place, attaching word-level segments
 * to paired remove/add blocks. A "pair" is a contiguous run of
 * removes immediately followed by a contiguous run of adds (the
 * order LCS emits when ties prefer remove-before-add); we line up
 * lines 1:1 up to `min(removeCount, addCount)` and leave any tail
 * untouched (so a 3-line remove + 1-line add highlights one pair
 * and renders the other two removes as flat).
 *
 * Skips pairs where word-level differences would explode the
 * segment count (e.g. completely different lines): if more than
 * 80% of tokens differ we bail and let the line render flat,
 * since the highlighting would be visual noise rather than signal.
 */
export function enrichWordDiff(lines: DiffLine[]): void {
  let i = 0
  while (i < lines.length) {
    if (lines[i].kind !== 'remove') {
      i++
      continue
    }
    let removeEnd = i
    while (removeEnd < lines.length && lines[removeEnd].kind === 'remove') {
      removeEnd++
    }
    let addEnd = removeEnd
    while (addEnd < lines.length && lines[addEnd].kind === 'add') {
      addEnd++
    }
    const pairCount = Math.min(removeEnd - i, addEnd - removeEnd)
    for (let k = 0; k < pairCount; k++) {
      const removeLine = lines[i + k]
      const addLine = lines[removeEnd + k]
      const { before, after } = diffWords(removeLine.text, addLine.text)
      if (segmentsAreInformative(before, after)) {
        removeLine.segments = before
        addLine.segments = after
      }
    }
    i = addEnd
  }
}

function segmentsAreInformative(before: WordSegment[], after: WordSegment[]): boolean {
  const beforeChars = before.reduce((n, s) => n + s.text.length, 0)
  const afterChars = after.reduce((n, s) => n + s.text.length, 0)
  const total = beforeChars + afterChars
  if (total === 0) return false
  const commonChars =
    before.filter((s) => s.kind === 'common').reduce((n, s) => n + s.text.length, 0) +
    after.filter((s) => s.kind === 'common').reduce((n, s) => n + s.text.length, 0)
  // Need at least 20% of the rendered text to be shared for the
  // highlight to read as "edit" rather than "wholesale rewrite".
  return commonChars * 5 >= total
}

/** Pull `path` and `contents` out of a `write_file` tool-call args
 *  blob. Returns `null` whenever the shape doesn't match — the card
 *  falls back to the raw JSON view in that case. */
export function extractWriteFileArgs(
  args: unknown,
): { path: string; contents: string } | null {
  if (!args || typeof args !== 'object') return null
  const a = args as Record<string, unknown>
  const path = typeof a.path === 'string' ? a.path : null
  const contents = typeof a.contents === 'string' ? a.contents : null
  if (!path || contents === null) return null
  return { path, contents }
}
