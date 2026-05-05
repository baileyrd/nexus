/**
 * AIG-02 — minimal line-level diff for the agent approval card.
 *
 * We only need a coarse "what's about to change" signal so the user
 * can spot a destructive overwrite before approving. A full
 * Myers-quality diff would be overkill (and would pull in a
 * dependency); a longest-common-subsequence walk on whole lines is
 * accurate enough at the scale of forge markdown files.
 */

export type DiffLineKind = 'context' | 'add' | 'remove'

export interface DiffLine {
  kind: DiffLineKind
  text: string
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

  return { lines: out, truncated, unchanged: false }
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
