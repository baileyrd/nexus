// Phase-5a DOM overlay on top of the 2D canvas. The overlay's root
// mirrors the camera transform so child divs positioned in world
// coordinates line up with the 2D-canvas render underneath.
//
// Pointer events on the overlay root are disabled so the underlying
// canvas still receives pan/drag/marquee gestures. Individual overlay
// children may opt back in via `pointer-events: auto` once future
// phases need scrollbars, links, or input focus — but Phase 5a keeps
// everything passive so text nodes can't eat clicks.

import { forwardRef, useMemo } from 'react'
import type { CanvasNode } from './kernelClient'
import { renderMarkdown } from '../editor/markdownRender'

interface Props {
  /** Nodes to overlay. Callers filter to the types the overlay handles
   *  so the 2D renderer can keep drawing the rest. */
  nodes: readonly CanvasNode[]
}

/**
 * The forwarded ref points at the **transformed layer** (not the
 * outer clipping wrapper), which is what the RAF tick updates with
 * the camera transform each frame.
 */
export const CanvasOverlay = forwardRef<HTMLDivElement, Props>(function CanvasOverlay(
  { nodes },
  layerRef,
) {
  return (
    <div
      style={{
        position: 'absolute',
        inset: 0,
        overflow: 'hidden',
        pointerEvents: 'none',
      }}
    >
      <div
        ref={layerRef}
        style={{
          position: 'absolute',
          top: 0,
          left: 0,
          transformOrigin: '0 0',
          // transform is written imperatively on every RAF tick; see
          // CanvasView's render loop.
        }}
      >
        {nodes.map((n) => {
          if (n.type === 'text') return <TextNodeOverlay key={n.id} node={n} />
          return null
        })}
      </div>
    </div>
  )
})

function TextNodeOverlay({ node }: { node: CanvasNode }) {
  // Parse once per text change — resizing and moving don't re-parse,
  // only re-position.
  const html = useMemo(() => renderMarkdown(node.text ?? ''), [node.text])
  return (
    <div
      className="nexus-markdown-body"
      style={{
        position: 'absolute',
        left: node.x,
        top: node.y,
        width: node.width,
        height: node.height,
        padding: 10,
        boxSizing: 'border-box',
        overflow: 'hidden',
        color: 'var(--fg, #e5e7eb)',
        fontFamily: 'var(--font-family, system-ui, sans-serif)',
        fontSize: 13,
        lineHeight: 1.4,
      }}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  )
}
