// Phase-6 auto-layout: a Fruchterman–Reingold-ish force-directed
// pass that nudges non-group nodes into a clean-looking arrangement
// while keeping the canvas's overall centre of mass in place.
//
// Design choices:
// - Pure synchronous function — callers map the returned positions
//   into `node_move` patch ops + inverses so undo works. No zustand
//   or IPC leaks into this module.
// - Only repositions nodes the user is likely to want auto-laid-out:
//   non-group, non-empty. Groups stay put because they're containers
//   the user arranged on purpose; a group moving under its children
//   would be disorienting.
// - Node sizes are baked into the repulsion distance so two large
//   cards don't overlap once the layout settles.
// - Fixed iteration budget (no real-time animation loop). The typical
//   30-node board settles visibly in ~200 iterations; 1000 nodes
//   would be noticeably slow but that's way beyond a reasonable
//   canvas.

import type { CanvasDoc } from './kernelClient'

/** Fixed iteration budget for the auto-layout force pass. */
const AUTO_LAYOUT_ITERATIONS = 250

/** Result of a layout pass — only the nodes that actually moved. */
export interface LayoutMove {
  id: string
  x: number
  y: number
}

export interface AutoLayoutOptions {
  /** Rest length for edges. Also used to compute the area constant
   *  that balances repulsion vs attraction. Default 220 world units,
   *  roughly two card-widths. */
  edgeLength?: number
  /** Iteration count. Fruchterman–Reingold converges fast enough
   *  that 250 is plenty for a few hundred nodes. */
  iterations?: number
  /** Minimum squared distance between two nodes before repulsion
   *  saturates. Prevents a divide-by-zero and stops two overlapping
   *  nodes from flying across the screen on the first tick. */
  minDistSq?: number
}

/**
 * Run a force-directed layout pass over every non-group node in `doc`.
 * Returns the set of moves (only nodes whose final positions differ
 * from their starting positions by more than a sub-pixel threshold).
 *
 * The caller is responsible for translating `LayoutMove[]` into
 * kernel patch ops + inverses. This function is deterministic given
 * the same input (seeded by starting positions, not a PRNG).
 */
export function autoLayout(
  doc: CanvasDoc,
  options: AutoLayoutOptions = {},
): LayoutMove[] {
  const iterations = options.iterations ?? AUTO_LAYOUT_ITERATIONS
  const edgeLength = options.edgeLength ?? 220
  const minDistSq = options.minDistSq ?? 0.5

  const movable = doc.nodes.filter((n) => n.type !== 'group')
  if (movable.length <= 1) return []

  // Starting positions = node centres. Using centres simplifies
  // repulsion math; we translate back to top-left at the end.
  interface Body {
    id: string
    x: number
    y: number
    radius: number
    startX: number
    startY: number
    vx: number
    vy: number
  }
  const bodies: Body[] = movable.map((n) => {
    const cx = n.x + n.width / 2
    const cy = n.y + n.height / 2
    // Half-diagonal of the card gives a rough "how close is too close"
    // radius. Padding added so two nodes don't sit flush.
    const radius = Math.sqrt(n.width * n.width + n.height * n.height) / 2 + 20
    return { id: n.id, x: cx, y: cy, radius, startX: cx, startY: cy, vx: 0, vy: 0 }
  })

  const byId = new Map(bodies.map((b) => [b.id, b]))
  // Only edges between movable nodes apply attraction — otherwise
  // a lone edge between a group and a regular node would tug the
  // regular node into the group forever.
  const springs: Array<{ a: Body; b: Body }> = []
  for (const e of doc.edges) {
    const a = byId.get(e.fromNode)
    const b = byId.get(e.toNode)
    if (a && b && a !== b) springs.push({ a, b })
  }

  // Fruchterman–Reingold "optimal distance" — controls the global
  // scale of the layout. A simple `area / N` picks a scale that
  // grows with node count so dense graphs don't implode.
  const area = edgeLength * edgeLength * bodies.length
  const k = Math.sqrt(area / bodies.length)
  // Initial "temperature" — max per-iteration displacement.
  let temp = edgeLength * 0.8

  for (let iter = 0; iter < iterations; iter++) {
    // Repulsion: every pair of nodes pushes apart.
    for (let i = 0; i < bodies.length; i++) bodies[i].vx = bodies[i].vy = 0
    for (let i = 0; i < bodies.length; i++) {
      const a = bodies[i]
      for (let j = i + 1; j < bodies.length; j++) {
        const b = bodies[j]
        const dx = a.x - b.x
        const dy = a.y - b.y
        let distSq = dx * dx + dy * dy
        if (distSq < minDistSq) distSq = minDistSq
        const dist = Math.sqrt(distSq)
        // Standard FR repulsion (k^2 / d) plus a soft-core boost
        // when two rects overlap — otherwise cards that start on
        // top of each other barely separate.
        const overlap = a.radius + b.radius - dist
        const baseForce = (k * k) / dist
        const bump = overlap > 0 ? overlap * 2 : 0
        const force = baseForce + bump
        const fx = (dx / dist) * force
        const fy = (dy / dist) * force
        a.vx += fx
        a.vy += fy
        b.vx -= fx
        b.vy -= fy
      }
    }

    // Attraction: each edge pulls its endpoints together.
    for (const s of springs) {
      const dx = s.a.x - s.b.x
      const dy = s.a.y - s.b.y
      const dist = Math.sqrt(dx * dx + dy * dy) || 0.01
      // FR attraction: d^2 / k
      const force = (dist * dist) / k
      const fx = (dx / dist) * force
      const fy = (dy / dist) * force
      s.a.vx -= fx
      s.a.vy -= fy
      s.b.vx += fx
      s.b.vy += fy
    }

    // Integrate — step each body by min(|v|, temp) along v/|v|.
    for (const body of bodies) {
      const speed = Math.sqrt(body.vx * body.vx + body.vy * body.vy)
      if (speed < 1e-6) continue
      const step = Math.min(speed, temp)
      body.x += (body.vx / speed) * step
      body.y += (body.vy / speed) * step
    }
    // Cool the system — linear decay lands us at ~2% of start
    // temperature on the last iteration, small enough to look
    // "done" when the user sees the snap.
    temp = Math.max(edgeLength * 0.02, temp * 0.985)
  }

  // Re-centre so the final centroid matches the starting centroid —
  // keeps the user's camera reasonably aligned after layout.
  let startCX = 0
  let startCY = 0
  let endCX = 0
  let endCY = 0
  for (const b of bodies) {
    startCX += b.startX
    startCY += b.startY
    endCX += b.x
    endCY += b.y
  }
  startCX /= bodies.length
  startCY /= bodies.length
  endCX /= bodies.length
  endCY /= bodies.length
  const dx = startCX - endCX
  const dy = startCY - endCY

  const moves: LayoutMove[] = []
  // `bodies[i]` was built from `movable[i]` in order, so the two
  // arrays stay aligned by index — no id lookup needed.
  for (let i = 0; i < bodies.length; i++) {
    const b = bodies[i]
    const node = movable[i]
    const newX = b.x + dx - node.width / 2
    const newY = b.y + dy - node.height / 2
    // Sub-pixel movements aren't worth a patch op.
    if (Math.abs(newX - node.x) < 0.5 && Math.abs(newY - node.y) < 0.5) continue
    moves.push({ id: b.id, x: newX, y: newY })
  }
  return moves
}
