// Pure parser + serializer for the BL-049 block-link syntax
// `[[<file>#^<block-id>]]`. Lives outside the CM extensions tree
// so the right-click "Copy block link" command (future), the
// link-suggester (future), and the decoration walker can all
// share one definition.
//
// Syntax (per ADR 0017 + BL-049 entry):
//
//   [[Notes/A.md#^d8e9f0a1-...-...]]
//   [[A.md#^d8e9f0a1-...-...|short label]]      // pipe alias
//
// `<block-id>` is a v4 UUID — the value `format_stable_id_marker`
// emits as `<!-- ^<uuid> -->` on disk. Anything that fails the
// UUID shape is rejected at parse time so a bare `[[A.md#header]]`
// (heading anchor, not block) doesn't masquerade as a block link.

const BLOCK_LINK_RE = /\[\[([^\]|]+?)#\^([0-9a-fA-F-]{36})(?:\|([^\]]+))?\]\]/g

const UUID_RE =
  /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/

/** A single parsed `[[<file>#^<id>]]` occurrence. */
export interface ParsedBlockLink {
  /** Source-text offset of the opening `[`. Test callers pass
   *  `offset` to `parseBlockLinks` to translate per-line scans
   *  back to doc coordinates. */
  from: number
  /** Source-text offset just past the closing `]]`. */
  to: number
  /** Forge-relative file path (the part before `#^`). */
  filePath: string
  /** Stable block-id UUID (the part after `#^`, lowercase-normalised). */
  blockId: string
  /** Optional display label (the part after `|`, when present). */
  label: string | null
}

/** Scan `text` for `[[<file>#^<uuid>]]` block links. Pure — no
 *  side effects, no module-level regex state leakage. */
export function parseBlockLinks(text: string, offset = 0): ParsedBlockLink[] {
  const links: ParsedBlockLink[] = []
  // Local regex so two concurrent scans don't share `lastIndex`.
  const re = new RegExp(BLOCK_LINK_RE.source, 'g')
  let match: RegExpExecArray | null
  while ((match = re.exec(text)) !== null) {
    const filePath = match[1].trim()
    const blockId = match[2].toLowerCase()
    if (!filePath) continue
    if (!UUID_RE.test(blockId)) continue
    const label = match[3]?.trim() || null
    links.push({
      from: offset + match.index,
      to: offset + match.index + match[0].length,
      filePath,
      blockId,
      label,
    })
  }
  return links
}

/** Inverse of `parseBlockLinks`. Used by future right-click "Copy
 *  block link" and link-suggester flows; exported so the markdown
 *  serializer can also reach the canonical form when the editor's
 *  block-tree learns to round-trip block links. */
export function serializeBlockLink(
  filePath: string,
  blockId: string,
  label?: string,
): string {
  if (!filePath) throw new Error('blockLinks.serialize: file path required')
  if (!UUID_RE.test(blockId)) {
    throw new Error(`blockLinks.serialize: invalid block id '${blockId}'`)
  }
  const tail = label ? `|${label}` : ''
  return `[[${filePath}#^${blockId.toLowerCase()}${tail}]]`
}

/** Convenience: returns the first block link starting at or
 *  spanning `pos` in `text`, or `null` if none. Used by the
 *  click-handler to map a mouse position to a navigation target. */
export function blockLinkAt(text: string, pos: number): ParsedBlockLink | null {
  for (const link of parseBlockLinks(text)) {
    if (pos >= link.from && pos <= link.to) return link
  }
  return null
}
