// BL-142 Phase 2b.1 — pure scanners for REPL-flagged code fences.
//
// All functions in this module operate on plain document text +
// return plain data. No CM6 state, no React, no kernel IPC. The
// Phase 2b.2 CM6 extensions (run gutter, Shift-Enter keymap, output
// widget) take these results and turn them into UI affordances.
//
// The fence grammar mirrors `parse_code_fence_info` in
// `crates/nexus-editor/src/markdown/parse.rs` so a markdown file
// authored in one mode round-trips identically to the other.

/** First token = language (or empty); `repl` token anywhere after
 *  flips the flag. Mirrors the Rust impl 1:1. */
export interface FenceInfo {
  language: string
  repl: boolean
}

export function parseFenceInfo(info: string): FenceInfo {
  const tokens = info.split(/\s+/).filter((t) => t.length > 0)
  let language = ''
  let repl = false
  if (tokens.length > 0) {
    if (tokens[0] === 'repl') {
      repl = true
    } else {
      language = tokens[0]
    }
  }
  for (let i = 1; i < tokens.length; i++) {
    if (tokens[i] === 'repl') repl = true
  }
  return { language, repl }
}

/** One REPL-flagged code block discovered in a document.
 *
 * All line numbers are 1-based to match CM6's `Doc.lineAt(...).number`.
 * `bodyStart` / `bodyEnd` are inclusive line numbers — the code that
 * the Run action sends to `repl_eval`. The opening fence line is
 * `openLine` (carries the Run gutter marker); the closing fence
 * line is `closeLine`.
 */
export interface ReplFenceBlock {
  /** Line of the opening ```… fence. */
  openLine: number
  /** Line of the closing ``` fence, or the last line of doc if the
   *  fence was never closed (defensive — partial-write state). */
  closeLine: number
  /** First line of the code body (always `openLine + 1`). */
  bodyStart: number
  /** Last line of the code body (always `closeLine - 1`). */
  bodyEnd: number
  /** Language tag from the fence info string (may be empty). */
  language: string
}

const FENCE_RE = /^```(.*)$/

/**
 * Scan `docText` and return every REPL-flagged code block in
 * document order. Unbalanced opens (no closing fence before EOF)
 * are reported with `closeLine` clamped to the last doc line so
 * the gutter still finds them — typing a partial fence shouldn't
 * make the gutter disappear.
 *
 * Non-REPL fences are skipped silently. Indented fences (markdown
 * convention requires column 0) are NOT recognized — the regex
 * anchors at line start.
 */
export function findReplBlocks(docText: string): ReplFenceBlock[] {
  const lines = docText.split('\n')
  const out: ReplFenceBlock[] = []
  let i = 0
  while (i < lines.length) {
    const m = FENCE_RE.exec(lines[i])
    if (!m) {
      i++
      continue
    }
    const info = parseFenceInfo(m[1])
    if (!info.repl) {
      // Still need to skip past the closing fence so we don't pick
      // up a `repl` fence nested inside a non-REPL fence body.
      const openLine = i + 1 // 1-based
      let j = i + 1
      while (j < lines.length && !/^```/.test(lines[j])) j++
      // `j` is either the closing fence line index or `lines.length`.
      i = j + 1
      void openLine
      continue
    }
    const openLine = i + 1 // 1-based
    let j = i + 1
    while (j < lines.length && !/^```/.test(lines[j])) j++
    const closeLineIdx = j < lines.length ? j : lines.length - 1
    out.push({
      openLine,
      closeLine: closeLineIdx + 1,
      bodyStart: openLine + 1,
      bodyEnd: closeLineIdx, // == (closeLineIdx + 1) - 1
      language: info.language,
    })
    i = j + 1
  }
  return out
}

/**
 * Find the REPL block containing `line` (1-based). Returns `null`
 * when the line is outside every REPL block. A line that lands on
 * the opening or closing fence itself counts as "inside" — the
 * Shift-Enter binding fires the block when the cursor is on either
 * the fence or the body.
 */
export function findReplBlockAtLine(
  blocks: ReplFenceBlock[],
  line: number,
): ReplFenceBlock | null {
  for (const b of blocks) {
    if (line >= b.openLine && line <= b.closeLine) return b
  }
  return null
}

/**
 * Extract the body code of `block` from `docText`. Body lines are
 * joined with `\n`; a trailing newline is appended so the REPL
 * kernel sees the code as a complete logical line (Python's `-i`
 * mode buffers until newline). Empty body returns the empty string.
 */
export function extractBlockCode(docText: string, block: ReplFenceBlock): string {
  if (block.bodyStart > block.bodyEnd) return ''
  const lines = docText.split('\n')
  const body = lines.slice(block.bodyStart - 1, block.bodyEnd)
  return body.length > 0 ? `${body.join('\n')}\n` : ''
}
