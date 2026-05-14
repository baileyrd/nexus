import type { OutlineHeading } from './outlineStore'
import type { BlockTree } from '../editor/types.ts'

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
 * Count whitespace-separated tokens in `text` that look like words
 * (at least one alphanumeric character). Punctuation-only tokens
 * are skipped so a section that's mostly em-dashes doesn't inflate
 * the count.
 */
export function countWordsIn(text: string): number {
  if (!text) return 0
  let n = 0
  for (const tok of text.split(/\s+/)) {
    if (tok.length === 0) continue
    if (/[\p{L}\p{N}]/u.test(tok)) n++
  }
  return n
}

/**
 * Parse ATX headings (`#`, `##`, …) out of a markdown document.
 * Lines inside fenced code blocks are ignored so `# not a heading`
 * in a code sample doesn't pollute the outline. Setext underline
 * headings (`===` / `---`) are not supported — ATX only.
 *
 * BL-053 mockup row N — also computes a `wordCount` per heading
 * for the faint badge in the outline view. Counts every non-empty
 * non-heading line between this heading and the next; lines inside
 * fenced code blocks contribute their words too (the badge is an
 * approximate progress indicator, not a publication-grade metric).
 */
export function parseHeadings(markdown: string): OutlineHeading[] {
  if (!markdown) return []
  const lines = markdown.split(/\r?\n/)
  interface HeadingDraft {
    level: number
    text: string
    sourceIdx: number
  }
  const drafts: HeadingDraft[] = []

  let inFence = false
  let fenceMarker: string | null = null

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const fenceMatch = line.match(FENCE_RE)
    if (fenceMatch) {
      const marker = fenceMatch[1][0]
      if (!inFence) {
        inFence = true
        fenceMarker = marker
      } else if (fenceMarker && line.trimStart().startsWith(fenceMarker)) {
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
    drafts.push({ level, text, sourceIdx: i })
  }

  const out: OutlineHeading[] = []
  for (let h = 0; h < drafts.length; h++) {
    const draft = drafts[h]
    const startLine = draft.sourceIdx + 1
    const endLine =
      h + 1 < drafts.length ? drafts[h + 1].sourceIdx : lines.length
    let words = 0
    for (let i = startLine; i < endLine; i++) {
      words += countWordsIn(lines[i] ?? '')
    }
    const slug = slugify(draft.text)
    out.push({
      id: `${draft.level}-${slug}-${h}`,
      text: draft.text,
      level: draft.level,
      line: draft.sourceIdx + 1,
      index: h,
      wordCount: words,
    })
  }

  return out
}

/**
 * Walk a `BlockTree` in root-block order, collecting heading blocks as
 * {@link OutlineHeading}s. Used by the Phase 7 editor-event subscription
 * so outline data derives from the kernel's canonical block tree rather
 * than a parallel parse of `tab.content`.
 *
 * The Rust wire shape for headings is
 * `{ kind: "heading", level: N }` — see `BlockType::Heading` in
 * `crates/nexus-editor/src/block.rs`.
 *
 * `line` is populated from `lineHints[i]` when provided (the caller can
 * pair a `getMarkdown` result with `parseHeadings` to recover source
 * line numbers for source-mode scroll-to). When no hint is given we
 * fall back to `0`, which is a no-op for CM scroll — preview-mode
 * scrolling is index-based and works unchanged.
 */
export function treeToHeadings(
  tree: BlockTree | null | undefined,
  lineHints?: number[],
): OutlineHeading[] {
  if (!tree || !tree.root_blocks || !tree.blocks) return []
  interface HeadingPos {
    level: number
    text: string
    rootIdx: number
  }
  const positions: HeadingPos[] = []
  for (let r = 0; r < tree.root_blocks.length; r++) {
    const id = tree.root_blocks[r]
    const block = tree.blocks[id]
    if (!block || block.is_deleted) continue
    const ty = block.ty as { kind?: unknown; level?: unknown } | undefined
    if (!ty || ty.kind !== 'heading') continue
    const rawLevel = ty.level
    const level =
      typeof rawLevel === 'number' && rawLevel >= 1 && rawLevel <= 6
        ? Math.floor(rawLevel)
        : 1
    const text = (block.content ?? '').trim()
    if (!text) continue
    positions.push({ level, text, rootIdx: r })
  }

  const out: OutlineHeading[] = []
  for (let h = 0; h < positions.length; h++) {
    const pos = positions[h]
    const endRootIdx =
      h + 1 < positions.length ? positions[h + 1].rootIdx : tree.root_blocks.length
    let words = 0
    for (let r = pos.rootIdx + 1; r < endRootIdx; r++) {
      const sibling = tree.blocks[tree.root_blocks[r]]
      if (!sibling || sibling.is_deleted) continue
      words += countWordsIn(sibling.content ?? '')
    }
    const slug = slugify(pos.text)
    const line = lineHints && h < lineHints.length ? lineHints[h] : 0
    out.push({
      id: `${pos.level}-${slug}-${h}`,
      text: pos.text,
      level: pos.level,
      line,
      index: h,
      wordCount: words,
    })
  }
  return out
}
