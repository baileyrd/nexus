import type { OutlineHeading } from './outlineStore'

// ATX heading with optional closing # run: `## Title ##`
// Captures: #-run (level), body (before optional trailing # run).
const HEADING_RE = /^(#{1,6})\s+(.+?)\s*#*\s*$/
// Fenced code fence: ``` or ~~~, with optional info string.
const FENCE_RE = /^\s{0,3}(`{3,}|~{3,})/

/**
 * Collapse non-alphanumeric runs to `-`, trim leading/trailing
 * dashes, lowercase. Empty input yields the empty string; callers
 * append an in-doc index so ids stay unique regardless.
 */
function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
}

/**
 * Parse ATX headings (`#`, `##`, …) out of a markdown document.
 * Lines inside fenced code blocks are ignored so `# not a heading`
 * in a code sample doesn't pollute the outline. Setext underline
 * headings (`===` / `---`) are not supported — ATX only.
 */
export function parseHeadings(markdown: string): OutlineHeading[] {
  if (!markdown) return []
  const lines = markdown.split(/\r?\n/)
  const out: OutlineHeading[] = []

  let inFence = false
  let fenceMarker: string | null = null
  let index = 0

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const fenceMatch = line.match(FENCE_RE)
    if (fenceMatch) {
      const marker = fenceMatch[1][0] // '`' or '~'
      if (!inFence) {
        inFence = true
        fenceMarker = marker
      } else if (fenceMarker && line.trimStart().startsWith(fenceMarker)) {
        // Closing fence must match opener's char (``` only closes ```).
        inFence = false
        fenceMarker = null
      }
      continue
    }
    if (inFence) continue

    const m = line.match(HEADING_RE)
    if (!m) continue
    const level = m[1].length
    const text = m[2].trim()
    if (!text) continue
    const slug = slugify(text)
    out.push({
      id: `${level}-${slug}-${index}`,
      text,
      level,
      line: i + 1,
    })
    index++
  }

  return out
}
