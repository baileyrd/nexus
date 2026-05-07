/**
 * BL-084: parse Git's conflict-marker syntax into structured hunks the
 * shell can render and resolve.
 *
 * Git emits conflict markers like:
 *
 *   <<<<<<< HEAD
 *   our version of the line
 *   |||||||
 *   common ancestor (only present with merge.conflictStyle = diff3)
 *   =======
 *   their version of the line
 *   >>>>>>> branch-name
 *
 * The parser is line-oriented (newline boundaries are preserved verbatim,
 * including the absence of a trailing newline) and tolerates:
 *
 *   - Mixed `\r\n` / `\n` line endings (each marker line is detected on
 *     its own; we don't try to normalise the whole file).
 *   - Files without a trailing newline.
 *   - Markers nested inside fenced code blocks the user might have
 *     written manually; we treat any line that *literally* starts with
 *     7+ markers as a structural marker, matching how git resolves
 *     them at write-back time.
 *   - The optional `|||||||` ancestor block (diff3 style) — captured
 *     and exposed as `base` so a future renderer can show three-way
 *     context, but resolution still operates on `ours` / `theirs`.
 */

export interface ConflictHunk {
  /** Inclusive start byte of the `<<<<<<<` line. */
  start: number
  /** Exclusive end byte (one past the trailing newline of the
   * `>>>>>>>` line, or end-of-string if the file has no final
   * newline). */
  end: number
  /** Text after the `<<<<<<<` marker on the same line, e.g. `HEAD`. */
  oursLabel: string
  /** Text after the `>>>>>>>` marker on the same line, e.g. a branch. */
  theirsLabel: string
  /** Body of the "ours" side, with original line endings preserved. */
  ours: string
  /** Body of the "theirs" side. */
  theirs: string
  /** Body of the diff3 ancestor block, when present. */
  base: string | null
}

export interface ParsedConflict {
  /** All conflict hunks discovered in the file, in document order. */
  hunks: ConflictHunk[]
}

/**
 * Find every conflict block in `content`. Returns an empty `hunks`
 * array for a clean file. Malformed markers (missing `=======` or
 * `>>>>>>>` after a `<<<<<<<`) are skipped — we don't try to invent
 * a closing marker, which would risk silently corrupting the file
 * on resolve.
 */
export function parseConflicts(content: string): ParsedConflict {
  const hunks: ConflictHunk[] = []
  // Walk line-by-line via newline indices; this keeps original byte
  // offsets exact even when line endings vary mid-file.
  let cursor = 0
  while (cursor < content.length) {
    const nl = content.indexOf('\n', cursor)
    const lineEnd = nl === -1 ? content.length : nl
    const lineStart = cursor
    const line = stripCR(content.slice(lineStart, lineEnd))

    if (isStartMarker(line)) {
      const hunk = scanHunk(content, lineStart, lineEnd)
      if (hunk) {
        hunks.push(hunk)
        cursor = hunk.end
        continue
      }
    }

    cursor = nl === -1 ? content.length : nl + 1
  }
  return { hunks }
}

/**
 * Replace the conflict block at `hunk.start..hunk.end` in `content`
 * with the resolved text. The replacement preserves the trailing
 * newline that originally followed the `>>>>>>>` line — if the block
 * was the file's last line and had no trailing newline, the
 * replacement is also tail-trimmed so the file shape is preserved.
 *
 * `replacement` should be the raw text the user wants to substitute
 * (typically `hunk.ours` or `hunk.theirs`); if it does not end with
 * a newline and the original block did, one is appended.
 */
export function applyResolution(
  content: string,
  hunk: ConflictHunk,
  replacement: string,
): string {
  const blockHadTrailingNewline =
    hunk.end > 0 && content.charAt(hunk.end - 1) === '\n'
  const repHasTrailingNewline =
    replacement.length > 0 && replacement.charAt(replacement.length - 1) === '\n'
  let normalised = replacement
  if (blockHadTrailingNewline && !repHasTrailingNewline) {
    normalised = `${replacement}\n`
  }
  if (!blockHadTrailingNewline && repHasTrailingNewline) {
    normalised = replacement.slice(0, replacement.length - 1)
  }
  return content.slice(0, hunk.start) + normalised + content.slice(hunk.end)
}

/**
 * Resolve every conflict in `content` using the same `side` for all of
 * them. Convenience for the "Accept all ours" / "Accept all theirs"
 * buttons; processes hunks in reverse so earlier offsets stay valid
 * after later replacements.
 */
export function applyAll(content: string, parsed: ParsedConflict, side: 'ours' | 'theirs'): string {
  let out = content
  for (let i = parsed.hunks.length - 1; i >= 0; i--) {
    const h = parsed.hunks[i]
    out = applyResolution(out, h, side === 'ours' ? h.ours : h.theirs)
  }
  return out
}

// ── Internals ────────────────────────────────────────────────────────────────

function stripCR(s: string): string {
  return s.endsWith('\r') ? s.slice(0, s.length - 1) : s
}

function isStartMarker(line: string): boolean {
  return line.startsWith('<<<<<<<') && (line.length === 7 || line.charAt(7) === ' ')
}
function isMidMarker(line: string): boolean {
  return line === '=======' || line.startsWith('======= ')
}
function isEndMarker(line: string): boolean {
  return line.startsWith('>>>>>>>') && (line.length === 7 || line.charAt(7) === ' ')
}
function isBaseMarker(line: string): boolean {
  // diff3 style — `|||||||` followed by an optional revision id.
  return line.startsWith('|||||||') && (line.length === 7 || line.charAt(7) === ' ')
}

function scanHunk(
  content: string,
  startLineStart: number,
  startLineEnd: number,
): ConflictHunk | null {
  const startLine = stripCR(content.slice(startLineStart, startLineEnd))
  const oursLabel = startLine.slice(7).trimStart()

  // Cursor walks line-by-line from the line *after* the start marker.
  let cursor = startLineEnd === content.length ? content.length : startLineEnd + 1
  const oursStart = cursor

  let oursEndExclusive = -1
  let baseStart = -1
  let baseEndExclusive = -1
  let theirsStart = -1
  let theirsEndExclusive = -1
  let endLineStart = -1
  let endLineEnd = -1
  let theirsLabel = ''

  // Phase: 0 = collecting ours, 1 = collecting base (diff3), 2 = collecting theirs.
  let phase: 0 | 1 | 2 = 0

  while (cursor < content.length) {
    const nl = content.indexOf('\n', cursor)
    const lineEnd = nl === -1 ? content.length : nl
    const lineStart = cursor
    const line = stripCR(content.slice(lineStart, lineEnd))

    if (isBaseMarker(line) && phase === 0) {
      oursEndExclusive = lineStart
      baseStart = nl === -1 ? content.length : nl + 1
      phase = 1
    } else if (isMidMarker(line)) {
      if (phase === 0) {
        oursEndExclusive = lineStart
      } else if (phase === 1) {
        baseEndExclusive = lineStart
      }
      theirsStart = nl === -1 ? content.length : nl + 1
      phase = 2
    } else if (isEndMarker(line)) {
      if (phase !== 2) {
        // Marker ordering is wrong — bail out without recording a
        // hunk so a malformed block can't quietly resolve to an
        // empty replacement.
        return null
      }
      theirsEndExclusive = lineStart
      theirsLabel = line.slice(7).trimStart()
      endLineStart = lineStart
      endLineEnd = lineEnd
      break
    } else if (isStartMarker(line)) {
      // A second `<<<<<<<` before a closing marker means the opening
      // block was malformed — bail rather than nest.
      return null
    }

    cursor = nl === -1 ? content.length : nl + 1
  }

  if (theirsEndExclusive < 0 || endLineStart < 0) return null

  // Hunk's exclusive end is one past the trailing newline of the
  // `>>>>>>>` line, falling back to end-of-content for files with no
  // final newline.
  const hunkEnd = endLineEnd === content.length ? content.length : endLineEnd + 1

  return {
    start: startLineStart,
    end: hunkEnd,
    oursLabel,
    theirsLabel,
    ours: content.slice(oursStart, oursEndExclusive),
    theirs: content.slice(theirsStart, theirsEndExclusive),
    base:
      baseStart >= 0 && baseEndExclusive >= 0
        ? content.slice(baseStart, baseEndExclusive)
        : null,
  }
}
