/**
 * Pure session-forest helpers for `nexus.sessions` (RFC 0008, Phase 5.4).
 *
 * `session_list` returns a flat array of session summaries, each carrying an
 * optional `parent_id` / `branch_point` (set on forked sessions — resume /
 * branch / rewind). These helpers decode that array and assemble it into a
 * navigable forest. Kept free of React + kernel deps so `node:test` can drive
 * them directly; the view layer renders the already-shaped result.
 */

export interface SessionNode {
  id: string
  goal: string
  outcome: string
  startedAt: string
  endedAt: string
  /** Parent session this node forked from, or `null` for a root. */
  parentId: string | null
  /** Parent round this node forked at, or `null` for a root. */
  branchPoint: number | null
}

export interface ForestNode extends SessionNode {
  children: ForestNode[]
  /** Distance from a root (0 for roots) — drives the render indent. */
  depth: number
}

/** Decode `session_list` rows, preserving the RFC 0008 tree linkage. */
export function decodeSessionNodes(raw: unknown): SessionNode[] {
  if (!Array.isArray(raw)) return []
  const out: SessionNode[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const r = item as Record<string, unknown>
    const id = typeof r.id === 'string' ? r.id : null
    if (!id) continue
    out.push({
      id,
      goal: typeof r.goal === 'string' ? r.goal : '',
      outcome: typeof r.outcome === 'string' ? r.outcome : 'unknown',
      startedAt: typeof r.started_at === 'string' ? r.started_at : '',
      endedAt: typeof r.ended_at === 'string' ? r.ended_at : '',
      parentId: typeof r.parent_id === 'string' ? r.parent_id : null,
      branchPoint: typeof r.branch_point === 'number' ? r.branch_point : null,
    })
  }
  return out
}

/**
 * Assemble flat nodes into a forest. A node whose `parentId` resolves to a
 * present node becomes that node's child; everything else (roots, plus orphans
 * whose parent was deleted) is a top-level root. Roots are ordered
 * newest-first by `startedAt`; children oldest-first so a branch reads
 * top-to-bottom. A cycle — which the immutable-fork model can't produce, but a
 * hand-edited file could — is broken by leaving the offending node as a root.
 */
export function buildForest(nodes: SessionNode[]): ForestNode[] {
  const byId = new Map<string, ForestNode>()
  for (const n of nodes) {
    byId.set(n.id, { ...n, children: [], depth: 0 })
  }
  const roots: ForestNode[] = []
  for (const node of byId.values()) {
    const parent = node.parentId ? byId.get(node.parentId) : undefined
    if (parent && !createsCycle(byId, node, parent)) {
      parent.children.push(node)
    } else {
      roots.push(node)
    }
  }
  const byStartDesc = (a: ForestNode, b: ForestNode) => b.startedAt.localeCompare(a.startedAt)
  const byStartAsc = (a: ForestNode, b: ForestNode) => a.startedAt.localeCompare(b.startedAt)
  roots.sort(byStartDesc)
  const assign = (node: ForestNode, depth: number): void => {
    node.depth = depth
    node.children.sort(byStartAsc)
    for (const c of node.children) assign(c, depth + 1)
  }
  for (const r of roots) assign(r, 0)
  return roots
}

/** Walk `parent`'s ancestry; if we reach `node`, linking them would cycle. */
function createsCycle(
  byId: Map<string, ForestNode>,
  node: ForestNode,
  parent: ForestNode,
): boolean {
  let cur: ForestNode | undefined = parent
  let hops = 0
  while (cur && hops < 4096) {
    if (cur.id === node.id) return true
    cur = cur.parentId ? byId.get(cur.parentId) : undefined
    hops += 1
  }
  return false
}

/** Pre-order flatten for rendering the forest as one indented list. */
export function flattenForest(forest: ForestNode[]): ForestNode[] {
  const out: ForestNode[] = []
  const visit = (node: ForestNode): void => {
    out.push(node)
    for (const c of node.children) visit(c)
  }
  for (const r of forest) visit(r)
  return out
}
