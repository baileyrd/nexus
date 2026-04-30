// BL-048 — canvas-side drop handling for block-reference drags
// originating from the editor's block handle. Pure helpers so
// the integration test (and the future outline-pane drop site)
// can drive the same logic without standing up a real
// `DragEvent` plus `DataTransfer`.
//
// The contract is symmetric with
// `shell/src/plugins/nexus/editor/blockRefDrag.ts`:
//
//   * source emits `BLOCK_REF_MIME` with a JSON payload;
//   * canvas reads it, builds a `text` node carrying the
//     BL-049 link form, and dispatches `node_add` through the
//     existing patch queue.

import {
  BLOCK_REF_MIME,
  blockRefToLink,
  parseBlockRef,
  type BlockRefPayload,
} from '../editor/blockRefDrag'
import type { CanvasNode } from './kernelClient'
import { newNodeId } from './canvasStore'
import { DEFAULT_TEXT_NODE_SIZE } from './renderer'

/** Inspect a `dragover` `DragEvent` for the typed MIME without
 *  pulling the data — used in the `dragover` handler so the
 *  drop cursor flips to "copy allowed" only over genuine
 *  block-ref drags. */
export function hasBlockRefPayload(event: DragEvent): boolean {
  const types = event.dataTransfer?.types
  if (!types) return false
  // happy-dom returns `string[]`; native is a `DOMStringList`.
  // Both expose `includes` / `contains`-style membership; coerce
  // to array for portability.
  const arr: string[] = Array.from(types as unknown as Iterable<string>)
  return arr.includes(BLOCK_REF_MIME)
}

/** Read + parse the MIME payload off a `drop` `DragEvent`.
 *  Returns `null` for unrelated drops (file drag from OS, image
 *  paste, etc.) so the handler can short-circuit cleanly. */
export function readBlockRefPayload(event: DragEvent): BlockRefPayload | null {
  if (!event.dataTransfer) return null
  const raw = event.dataTransfer.getData(BLOCK_REF_MIME)
  return parseBlockRef(raw)
}

/** Build the `CanvasNode` to append on drop. World-space origin
 *  is the drop point centred on the default text-node size so
 *  the user sees the new card under their cursor. The text body
 *  is the BL-049 link form so a downstream click in the canvas
 *  text node activates the navigator. */
export function buildBlockRefDropNode(
  payload: BlockRefPayload,
  world: { x: number; y: number },
): CanvasNode {
  const { width, height } = DEFAULT_TEXT_NODE_SIZE
  return {
    id: newNodeId(),
    type: 'text',
    x: Math.round(world.x - width / 2),
    y: Math.round(world.y - height / 2),
    width,
    height,
    text: blockRefToLink(payload),
    label: payload.label ?? undefined,
  }
}
