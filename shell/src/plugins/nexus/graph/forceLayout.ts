// Hand-rolled force-directed layout. We have no d3-force in `shell/package.json`
// and `pnpm add` is sandbox-blocked, so this module rolls a small Verlet-style
// simulator. The model is the standard three-force composition:
//   • repulsion  — pairwise inverse-square push between every node
//   • spring     — Hooke pull along each edge toward `linkDistance`
//   • gravity    — soft pull toward viewport centre
// O(n^2) per tick is fine through a few thousand nodes; we cap effective node
// pairs by sampling for very large graphs to keep frame time bounded.

export interface SimNode {
  id: string
  x: number
  y: number
  vx: number
  vy: number
  fx: number | null
  fy: number | null
}

export interface SimEdge {
  source: string
  target: string
}

export interface ForceParams {
  linkDistance: number
  linkStrength: number
  repulsion: number
  centerGravity: number
  width: number
  height: number
}

const DAMPING_FACTOR = 0.82
const MAX_VELOCITY = 40
// Above this many nodes we stride pairwise iteration to keep tick budget sane.
const PAIRWISE_FULL_LIMIT = 600

// Alpha cooling — same shape d3-force uses. `alpha` scales every applied
// force; on each tick the caller relaxes it toward 0 (`alpha += (0 -
// alpha) * ALPHA_DECAY`) and stops driving the loop once it crosses
// `ALPHA_MIN`. This is what makes the simulation actually settle —
// without it, residual jitter from the inverse-square repulsion plus
// the random kick on coincident nodes keeps injecting energy forever.
export const ALPHA_DECAY = 0.0228
export const ALPHA_MIN = 0.001
export const ALPHA_REHEAT = 1

export function makeNodes(ids: string[], width: number, height: number): SimNode[] {
  const out: SimNode[] = []
  const cx = width / 2
  const cy = height / 2
  const radius = Math.min(width, height) * 0.35
  for (let i = 0; i < ids.length; i++) {
    const id = ids[i]!
    const angle = (2 * Math.PI * i) / Math.max(1, ids.length)
    out.push({
      id,
      x: cx + radius * Math.cos(angle),
      y: cy + radius * Math.sin(angle),
      vx: 0,
      vy: 0,
      fx: null,
      fy: null,
    })
  }
  return out
}

export function step(
  nodes: SimNode[],
  edges: SimEdge[],
  params: ForceParams,
  alpha: number = 1,
): void {
  const cx = params.width / 2
  const cy = params.height / 2
  const n = nodes.length
  if (n === 0) return

  const idx = new Map<string, number>()
  for (let i = 0; i < n; i++) idx.set(nodes[i]!.id, i)

  // Pairwise repulsion. For very large graphs we stride to keep this O(n)-ish.
  const stride = n > PAIRWISE_FULL_LIMIT ? Math.ceil(n / PAIRWISE_FULL_LIMIT) : 1
  const repulsionConst = params.repulsion * alpha
  for (let i = 0; i < n; i++) {
    const a = nodes[i]!
    for (let j = i + 1; j < n; j += stride) {
      const b = nodes[j]!
      let dx = a.x - b.x
      let dy = a.y - b.y
      let d2 = dx * dx + dy * dy
      if (d2 < 0.01) {
        // Jitter coincident nodes so the inverse-square doesn't blow up.
        dx = Math.random() - 0.5
        dy = Math.random() - 0.5
        d2 = dx * dx + dy * dy + 0.01
      }
      const force = repulsionConst / d2
      const inv = 1 / Math.sqrt(d2)
      const fx = dx * inv * force
      const fy = dy * inv * force
      a.vx += fx
      a.vy += fy
      b.vx -= fx
      b.vy -= fy
    }
  }

  // Spring forces along edges.
  const k = params.linkStrength * alpha
  const rest = params.linkDistance
  for (const e of edges) {
    const i = idx.get(e.source)
    const j = idx.get(e.target)
    if (i === undefined || j === undefined) continue
    const a = nodes[i]!
    const b = nodes[j]!
    const dx = b.x - a.x
    const dy = b.y - a.y
    const dist = Math.sqrt(dx * dx + dy * dy) || 0.01
    const delta = (dist - rest) * k
    const ux = dx / dist
    const uy = dy / dist
    a.vx += ux * delta
    a.vy += uy * delta
    b.vx -= ux * delta
    b.vy -= uy * delta
  }

  // Gravity toward centre + integrate.
  const g = params.centerGravity * alpha
  for (const node of nodes) {
    if (node.fx !== null && node.fy !== null) {
      node.x = node.fx
      node.y = node.fy
      node.vx = 0
      node.vy = 0
      continue
    }
    node.vx += (cx - node.x) * g
    node.vy += (cy - node.y) * g
    node.vx *= DAMPING_FACTOR
    node.vy *= DAMPING_FACTOR
    if (node.vx > MAX_VELOCITY) node.vx = MAX_VELOCITY
    else if (node.vx < -MAX_VELOCITY) node.vx = -MAX_VELOCITY
    if (node.vy > MAX_VELOCITY) node.vy = MAX_VELOCITY
    else if (node.vy < -MAX_VELOCITY) node.vy = -MAX_VELOCITY
    node.x += node.vx
    node.y += node.vy
  }
}
