import type { EdgeDirection, GraphNeighbour } from './graphStore'

/**
 * A node's final position in the laid-out graph. `direction` is the
 * edge direction for neighbour nodes, or the sentinel `'centre'` for
 * the currently-open file (which has no edge *to itself*).
 */
export interface LaidOutNode {
  relpath: string
  name: string
  direction: EdgeDirection | 'centre'
  x: number
  y: number
}

/**
 * A single edge. Straight line from the centre to the neighbour for
 * this layout — we never draw neighbour-to-neighbour edges.
 */
export interface LaidOutEdge {
  fromRelpath: string
  toRelpath: string
  direction: EdgeDirection
}

export interface Layout {
  nodes: LaidOutNode[]
  edges: LaidOutEdge[]
  width: number
  height: number
}

/**
 * Pure radial layout for a 1-hop graph.
 *
 * - Centre node at `(width/2, height/2)`.
 * - Neighbours on a single ring at radius `min(width, height) * 0.35`.
 *   The 0.35 factor leaves room for the label text that extends below
 *   each circle; a tighter ring (e.g. 0.45) pushes neighbour labels
 *   against the panel edge at narrow panel widths.
 * - First neighbour at 12 o'clock (`-π/2`), the rest evenly spaced
 *   clockwise so the layout is deterministic given a stable
 *   neighbour order.
 *
 * When `neighbours` is empty we still emit the centre node so the
 * view can render the "alone" state as a graph-of-one rather than a
 * blank SVG.
 */
export function radialLayout(
  current: { relpath: string; name: string },
  neighbours: GraphNeighbour[],
  viewportWidth: number,
  viewportHeight: number,
): Layout {
  const width = Math.max(0, viewportWidth)
  const height = Math.max(0, viewportHeight)
  const cx = width / 2
  const cy = height / 2
  const radius = Math.min(width, height) * 0.35

  const centre: LaidOutNode = {
    relpath: current.relpath,
    name: current.name,
    direction: 'centre',
    x: cx,
    y: cy,
  }

  if (neighbours.length === 0) {
    return { nodes: [centre], edges: [], width, height }
  }

  const nodes: LaidOutNode[] = [centre]
  const edges: LaidOutEdge[] = []

  for (let i = 0; i < neighbours.length; i++) {
    const n = neighbours[i]!
    const angle = (2 * Math.PI * i) / neighbours.length - Math.PI / 2
    nodes.push({
      relpath: n.relpath,
      name: n.name,
      direction: n.direction,
      x: cx + radius * Math.cos(angle),
      y: cy + radius * Math.sin(angle),
    })
    edges.push({
      fromRelpath: current.relpath,
      toRelpath: n.relpath,
      direction: n.direction,
    })
  }

  return { nodes, edges, width, height }
}
