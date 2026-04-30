// BL-048 — cross-plugin "drag a block reference" contract.
//
// The editor's block-handle (six-dot grip) becomes a native HTML5
// drag source carrying a typed payload; the canvas plugin's drop
// surface reads that payload and creates an embedding node. The
// MIME type + payload shape live here so neither side has to
// reach into the other's internals — a future plugin (e.g. an
// outline pane that accepts block drops) can hook in by importing
// `BLOCK_REF_MIME` + `parseBlockRef`.
//
// Soft-blocked dependency: a BL-049-stable block id (`stable_id`
// per ADR 0017) on the source side. Without it the payload still
// carries the *current* block id, but a downstream edit shifts
// upstream blocks and the embed orphans. Editors should stamp the
// block via `com.nexus.editor::stamp_block` before / during drag.

/** MIME type the dataTransfer carries. The `x-nexus-` prefix
 *  matches the comments-bridge convention from BL-050 — both
 *  surfaces speak the same family of typed payloads. */
export const BLOCK_REF_MIME = 'application/x-nexus-block-ref'

/** Payload format the editor encodes into `dataTransfer.setData`
 *  on `dragstart` and the canvas plugin decodes on `drop`. */
export interface BlockRefPayload {
  /** Forge-relative path of the source file. */
  relpath: string
  /** Block id — preferably the stamped `stable_id` (UUID per ADR
   *  0017). The shape matches the BL-049 link parser
   *  (`[[<file>#^<uuid>]]`) so the canvas drop handler can
   *  produce a text node carrying that exact link form. */
  blockId: string
  /** Optional display label — usually the block's first line of
   *  content, truncated. Canvas falls back to the block id when
   *  absent. */
  label?: string | null
}

const UUID_RE =
  /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/

/** Encode `payload` as a JSON string for `dataTransfer.setData`.
 *  Validates the payload shape so a malformed source-side caller
 *  trips the assertion at the source rather than producing a
 *  dangling drop on the canvas side. */
export function serializeBlockRef(payload: BlockRefPayload): string {
  if (!payload.relpath) {
    throw new Error('blockRefDrag.serialize: relpath required')
  }
  if (!UUID_RE.test(payload.blockId)) {
    throw new Error(
      `blockRefDrag.serialize: blockId must be a UUID, got '${payload.blockId}'`,
    )
  }
  const label =
    typeof payload.label === 'string' && payload.label.trim()
      ? payload.label.trim()
      : null
  return JSON.stringify({
    relpath: payload.relpath,
    blockId: payload.blockId.toLowerCase(),
    label,
  })
}

/** Decode a `dataTransfer.getData(BLOCK_REF_MIME)` value back to
 *  a structured payload. Returns `null` for any malformed input
 *  — empty string, non-JSON, missing keys, bad UUID — so drop
 *  handlers can guard with a single null check. */
export function parseBlockRef(raw: string | null | undefined): BlockRefPayload | null {
  if (!raw) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return null
  }
  if (!parsed || typeof parsed !== 'object') return null
  const obj = parsed as Record<string, unknown>
  const relpath = typeof obj.relpath === 'string' ? obj.relpath : null
  const blockId = typeof obj.blockId === 'string' ? obj.blockId : null
  if (!relpath || !blockId) return null
  if (!UUID_RE.test(blockId)) return null
  const label = typeof obj.label === 'string' && obj.label.length > 0 ? obj.label : null
  return { relpath, blockId: blockId.toLowerCase(), label }
}

/** Build the inline `[[<file>#^<uuid>|<label>]]` link form a
 *  drop handler can store on a canvas text node. Mirrors
 *  `serializeBlockLink` in [`blockLinks.ts`](./blockLinks.ts) so
 *  drops produce the same syntax the editor's own block-link
 *  navigator (BL-049) recognises. */
export function blockRefToLink(payload: BlockRefPayload): string {
  const tail = payload.label ? `|${payload.label}` : ''
  return `[[${payload.relpath}#^${payload.blockId}${tail}]]`
}
