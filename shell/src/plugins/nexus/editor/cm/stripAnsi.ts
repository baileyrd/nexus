// BL-142 Phase 2b.1 — pure ANSI escape stripper for REPL output
// rendering. Mirrors the Rust `nexus_terminal::strip_ansi` impl
// closely enough that round-tripping output through the bus and
// then rendering it in the `<ReplOutput />` widget matches what
// the user would see in a regular terminal.
//
// The Phase 2b.2 widget calls this on each chunk before appending
// to the output buffer. Heavy lift (FTS / serialization) stays on
// the Rust side; this is purely the display-time strip.
//
// ESC is constructed via `String.fromCharCode(0x1b)` rather than a
// literal byte so the source stays printable for code-review tools.

const ESC = String.fromCharCode(0x1b)

/**
 * Strip ANSI escape sequences from `s`. Handles the three forms
 * the REPL kernels (python3, node, bash) actually emit:
 *
 * - **CSI** (`ESC [ … final-byte`): cursor moves, color codes
 *   (`ESC [ 31 m`), screen clears, etc. Final byte is in `0x40..=0x7e`.
 * - **OSC** (`ESC ] … BEL` or `ESC ] … ESC \`): window title sets,
 *   hyperlinks. Terminated by `BEL` (0x07) or `ESC \` (ST).
 * - **Char-set selects** (`ESC ( <char>`, `ESC ) <char>`, `ESC * <char>`,
 *   `ESC + <char>`): designator + final byte. Drop all three bytes.
 * - **Save/restore + ID escapes** (`ESC 7`, `ESC 8`, `ESC =`, `ESC >`,
 *   `ESC c`): two bytes total, drop both.
 *
 * Anything we don't recognize is passed through verbatim — the
 * cost of mangling unknown text is higher than leaving a literal
 * escape character in the output.
 */
export function stripAnsi(s: string): string {
  let out = ''
  let i = 0
  while (i < s.length) {
    const ch = s[i]
    if (ch !== ESC) {
      out += ch
      i++
      continue
    }
    // ESC seen — try to consume an escape sequence.
    if (i + 1 >= s.length) {
      // Trailing lone ESC — drop it.
      break
    }
    const next = s[i + 1]
    if (next === '[') {
      // CSI: ESC [ <params> <final byte 0x40-0x7e>
      let j = i + 2
      while (j < s.length) {
        const c = s.charCodeAt(j)
        if (c >= 0x40 && c <= 0x7e) {
          j++
          break
        }
        j++
      }
      i = j
      continue
    }
    if (next === ']') {
      // OSC: ESC ] <text> BEL  OR  ESC ] <text> ESC \
      let j = i + 2
      while (j < s.length) {
        if (s.charCodeAt(j) === 0x07) {
          j++
          break
        }
        if (s[j] === ESC && j + 1 < s.length && s[j + 1] === '\\') {
          j += 2
          break
        }
        j++
      }
      i = j
      continue
    }
    if ('()*+'.includes(next)) {
      // Char-set select: ESC + designator + final byte. Drop all
      // three bytes even when the final byte is missing (EOF mid-
      // sequence — partial-write state).
      i = Math.min(i + 3, s.length)
      continue
    }
    if ('78=>c'.includes(next)) {
      // Two-byte escape (save/restore cursor, app-mode, full reset).
      i += 2
      continue
    }
    // Unknown ESC <next> sequence: pass through verbatim.
    out += ch
    i++
  }
  return out
}
