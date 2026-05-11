// CM doc offset → kernel (block_id, block-byte-offset) translation.
//
// CodeMirror addresses positions by JS UTF-16 character offsets across
// the whole document. The kernel addresses positions by UTF-8 byte
// offsets *within a specific block's `content`* — which has the source
// prefix (`# `, `> `, list markers, indent) stripped. The transaction
// bridge needs to translate between the two whenever it builds an
// `insert_text` or `delete_text` op, or the kernel either rejects the
// op (pos > content.len()) or — worse — silently inserts at the wrong
// byte and corrupts the block.
//
// Strategy:
//   1. Walk the Lezer markdown tree's top-level blocks. The Nth Lezer
//      block matches `snapshot.tree.root_blocks[N]`.
//   2. Compute that block's "content area" — the source span whose text
//      equals the kernel block's `content` verbatim. For `Paragraph`
//      that's the whole node span; for `ATXHeading*` it's after the
//      `#`s and following whitespace.
//   3. Verify the source slice over that area === `block.content`. If
//      not equal, the block has inline formatting (`*emph*`, soft
//      breaks, link syntax, …) that the parser strips into annotations.
//      Byte arithmetic over the source would land in the wrong place,
//      so we bail and let the bridge use its coarser fallback.
//   4. The block byte offset for a CM offset `o` is the UTF-8 byte
//      length of the source slice from `contentStart` to `o`.
//
// Scope: top-level Paragraph and ATXHeading only. Lists, blockquotes,
// fenced code, tables fall back to the coarse path. The mapping is
// keyed off `snapshot` — callers pass the snapshot that matches the
// `EditorState` whose offsets they're translating.

import { syntaxTree } from '@codemirror/language'
import type { EditorState } from '@codemirror/state'

import type { BlockId, EditorSnapshot } from '../types.ts'

/** A resolved kernel-side position. `bytePos` is the UTF-8 byte offset
 *  within the block's `content` string (what the kernel sees on the wire);
 *  `charPos` is the JS UTF-16 char offset (what the bridge uses to
 *  advance its local mirror). */
export interface BlockPos {
  blockId: BlockId
  bytePos: number
  charPos: number
}

/** A resolved kernel-side range, both ends inside the *same* block. */
export interface BlockRange {
  blockId: BlockId
  byteFrom: number
  byteTo: number
  charFrom: number
  charTo: number
}

// Minimal shape of the @lezer/common node we touch. Matches the runtime
// surface; re-declared locally so we don't take a direct dependency on
// the transitive package.
interface SyntaxNode {
  name: string
  from: number
  to: number
  firstChild: SyntaxNode | null
  nextSibling: SyntaxNode | null
}

const TEXT_ENCODER = new TextEncoder()

function utf8Bytes(s: string): number {
  return TEXT_ENCODER.encode(s).length
}

interface ContentBounds {
  contentStart: number
  contentEnd: number
}

/**
 * Compute the source range whose text is expected to equal the kernel
 * block's `content`. Returns `null` for block types we don't translate.
 */
function contentBounds(state: EditorState, node: SyntaxNode): ContentBounds | null {
  const name = node.name
  if (name === 'Paragraph') {
    return { contentStart: node.from, contentEnd: node.to }
  }
  if (/^ATXHeading[1-6]$/.test(name)) {
    const headerMark = node.firstChild
    if (!headerMark || headerMark.name !== 'HeaderMark') return null
    let contentStart = headerMark.to
    while (contentStart < node.to) {
      const ch = state.doc.sliceString(contentStart, contentStart + 1)
      if (ch !== ' ' && ch !== '\t') break
      contentStart++
    }
    return { contentStart, contentEnd: node.to }
  }
  return null
}

interface TopLevelMatch {
  node: SyntaxNode
  /** 0-based index among the document's top-level children — used to
   *  pair the Lezer block with `snapshot.tree.root_blocks[index]`. */
  index: number
}

/**
 * Find the top-level Lezer block containing `cmOffset`. Uses inclusive
 * bounds on both ends so a cursor at the trailing edge of a block (the
 * common "end of paragraph" case) resolves to that block.
 */
function findTopLevelBlock(state: EditorState, cmOffset: number): TopLevelMatch | null {
  const top = syntaxTree(state).topNode as unknown as SyntaxNode
  let child = top.firstChild
  let idx = 0
  let lastMatch: TopLevelMatch | null = null
  while (child) {
    if (cmOffset >= child.from && cmOffset <= child.to) {
      // Keep walking — a later sibling with `from === cmOffset` (the
      // boundary case where the cursor is at the start of the next
      // block) should win over an earlier one whose `to === cmOffset`.
      // The kernel's block boundary is exclusive on the right; CM's is
      // inclusive on the left.
      lastMatch = { node: child, index: idx }
      if (cmOffset < child.to) return lastMatch
    } else if (cmOffset < child.from) {
      break
    }
    child = child.nextSibling
    idx++
  }
  return lastMatch
}

/**
 * Translate a CM doc offset into a kernel-side `(blockId, bytePos)`.
 *
 * Returns `null` when:
 *   - The offset falls outside any top-level Paragraph/ATXHeading block
 *     (blank line between blocks, list item, fenced code, table row).
 *   - The offset is inside the block source but not in the "content
 *     area" — e.g. on the `# ` prefix of a heading.
 *   - The block has inline formatting whose stripped form makes byte
 *     arithmetic over the source unsafe.
 *   - The Lezer block index doesn't have a matching `root_blocks` entry
 *     (snapshot is out of sync with CM — rare, possible during a
 *     reconcile race).
 *
 * `state` should be the `EditorState` whose offsets the caller is
 * translating — typically `update.startState` from the bridge, so the
 * snapshot's block tree matches the doc the offsets were taken in.
 */
export function resolveBlockPos(
  state: EditorState,
  snapshot: EditorSnapshot,
  cmOffset: number,
): BlockPos | null {
  const found = findTopLevelBlock(state, cmOffset)
  if (!found) return null
  const { node, index } = found
  const roots = snapshot.tree.root_blocks
  if (index >= roots.length) return null
  const blockId = roots[index]!
  const block = snapshot.tree.blocks[blockId]
  if (!block) return null

  const bounds = contentBounds(state, node)
  if (!bounds) return null
  if (cmOffset < bounds.contentStart || cmOffset > bounds.contentEnd) return null

  const source = state.doc.sliceString(bounds.contentStart, bounds.contentEnd)
  if (source !== block.content) return null

  const before = state.doc.sliceString(bounds.contentStart, cmOffset)
  return { blockId, bytePos: utf8Bytes(before), charPos: before.length }
}

/**
 * Translate a CM doc range `[from, to]` into a kernel-side range. Both
 * ends must resolve to the *same* block — cross-block ranges return
 * `null` so the caller falls back to a coarser path (multi-block
 * deletes can't be expressed as a single `DeleteText`).
 */
export function resolveBlockRange(
  state: EditorState,
  snapshot: EditorSnapshot,
  cmFrom: number,
  cmTo: number,
): BlockRange | null {
  const start = resolveBlockPos(state, snapshot, cmFrom)
  if (!start) return null
  const end = resolveBlockPos(state, snapshot, cmTo)
  if (!end) return null
  if (start.blockId !== end.blockId) return null
  return {
    blockId: start.blockId,
    byteFrom: start.bytePos,
    byteTo: end.bytePos,
    charFrom: start.charPos,
    charTo: end.charPos,
  }
}
