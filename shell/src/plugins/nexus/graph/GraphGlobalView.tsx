import { useEffect, useMemo, useRef, useState } from 'react'
import { useGlobalGraphStore, type GlobalGraphSettings } from './graphGlobalStore'
import {
  ALPHA_DECAY,
  ALPHA_MIN,
  ALPHA_REHEAT,
  makeNodes,
  step,
  type SimEdge,
  type SimNode,
} from './forceLayout'
import { GraphGlobalGearDrawer } from './GraphGlobalGearDrawer'
import { Icon } from '../../../icons'
import { eventBus } from '../../../host/EventBus'

const EVENT_FILE_OPEN = 'files:open'

const NODE_RADIUS = 4
const HOVER_RADIUS = 6
const MIN_ZOOM = 0.1
const MAX_ZOOM = 8

interface ViewTransform {
  x: number
  y: number
  k: number
}

const IDENTITY: ViewTransform = { x: 0, y: 0, k: 1 }

function basename(p: string): string {
  const i = p.lastIndexOf('/')
  return i === -1 ? p : p.slice(i + 1)
}

function folderOf(p: string): string {
  const i = p.lastIndexOf('/')
  return i === -1 ? '' : p.slice(0, i)
}

// Stable hash → HSL hue. Same folder → same colour across the run.
function folderColour(folder: string): string {
  let h = 0
  for (let i = 0; i < folder.length; i++) {
    h = (h * 31 + folder.charCodeAt(i)) >>> 0
  }
  const hue = h % 360
  return `hsl(${hue}, 55%, 60%)`
}

function applyFilters(
  nodes: { path: string; isPhantom: boolean }[],
  edges: { source: string; target: string; isResolved: boolean }[],
  s: GlobalGraphSettings,
): {
  nodes: { path: string; isPhantom: boolean }[]
  edges: { source: string; target: string; isResolved: boolean }[]
} {
  const filter = s.pathFilter.trim().toLowerCase()
  const matchesFilter = (p: string) =>
    filter === '' || p.toLowerCase().includes(filter)

  let kept = nodes.filter((n) => {
    if (!s.includeUnresolved && n.isPhantom) return false
    if (!matchesFilter(n.path)) return false
    return true
  })
  const allowed = new Set(kept.map((n) => n.path))
  const keptEdges = edges.filter(
    (e) => allowed.has(e.source) && allowed.has(e.target),
  )

  if (!s.includeOrphans) {
    const linked = new Set<string>()
    for (const e of keptEdges) {
      linked.add(e.source)
      linked.add(e.target)
    }
    kept = kept.filter((n) => linked.has(n.path))
  }

  return { nodes: kept, edges: keptEdges }
}

export function GraphGlobalView() {
  const rawNodes = useGlobalGraphStore((s) => s.nodes)
  const rawEdges = useGlobalGraphStore((s) => s.edges)
  const loading = useGlobalGraphStore((s) => s.loading)
  const error = useGlobalGraphStore((s) => s.error)
  const settings = useGlobalGraphStore((s) => s.settings)

  const containerRef = useRef<HTMLDivElement | null>(null)
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const [size, setSize] = useState<{ w: number; h: number }>({ w: 600, h: 400 })

  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const rect = el.getBoundingClientRect()
    if (rect.width > 0 && rect.height > 0) {
      setSize({ w: rect.width, h: rect.height })
    }
    const ro = new ResizeObserver((entries) => {
      for (const e of entries) {
        const { width, height } = e.contentRect
        if (width > 0 && height > 0) setSize({ w: width, h: height })
      }
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const filtered = useMemo(
    () => applyFilters(rawNodes, rawEdges, settings),
    [rawNodes, rawEdges, settings],
  )

  // Sim nodes are mutable across frames — keep them in a ref, not state. Re-seed
  // only when the node id-set changes, so a settings tweak (forces, filters)
  // doesn't yank existing positions.
  const simNodesRef = useRef<SimNode[]>([])
  const simEdgesRef = useRef<SimEdge[]>([])
  const lastIdSetRef = useRef<string>('')
  // Alpha cools the simulation toward rest. Re-heated whenever the
  // input changes (new nodes, edge churn, force-knob tweak, drag end).
  const alphaRef = useRef<number>(ALPHA_REHEAT)

  const idSetKey = useMemo(
    () => filtered.nodes.map((n) => n.path).sort().join('|'),
    [filtered.nodes],
  )

  useEffect(() => {
    if (lastIdSetRef.current === idSetKey) {
      simEdgesRef.current = filtered.edges.map((e) => ({
        source: e.source,
        target: e.target,
      }))
      // Edge churn alone — give the sim a small kick rather than a
      // full re-heat so existing positions don't fly apart.
      alphaRef.current = Math.max(alphaRef.current, 0.3)
      return
    }
    const previous = new Map(simNodesRef.current.map((n) => [n.id, n]))
    simNodesRef.current = filtered.nodes.map((n, i) => {
      const prev = previous.get(n.path)
      if (prev) return prev
      const seeded = makeNodes([n.path], size.w, size.h)[0]!
      // Spread fresh nodes around the perimeter to avoid stacking.
      const angle = (2 * Math.PI * i) / Math.max(1, filtered.nodes.length)
      const r = Math.min(size.w, size.h) * 0.35
      seeded.x = size.w / 2 + r * Math.cos(angle)
      seeded.y = size.h / 2 + r * Math.sin(angle)
      return seeded
    })
    simEdgesRef.current = filtered.edges.map((e) => ({
      source: e.source,
      target: e.target,
    }))
    lastIdSetRef.current = idSetKey
    alphaRef.current = ALPHA_REHEAT
  }, [idSetKey, filtered.edges, filtered.nodes, size.w, size.h])

  // Force-knob changes (link distance/strength, repulsion, gravity)
  // need a re-heat too — the equilibrium position shifts under new
  // params and the sim is otherwise too cool to find it.
  useEffect(() => {
    alphaRef.current = ALPHA_REHEAT
  }, [
    settings.linkDistance,
    settings.linkStrength,
    settings.repulsion,
    settings.centerGravity,
  ])

  const transformRef = useRef<ViewTransform>({ ...IDENTITY })
  const [, forceRedraw] = useState(0)
  const dragRef = useRef<
    | { kind: 'pan'; lastX: number; lastY: number }
    | { kind: 'node'; node: SimNode }
    | null
  >(null)
  const [hoveredId, setHoveredId] = useState<string | null>(null)
  const [gearOpen, setGearOpen] = useState(false)

  const phantomLookup = useMemo(() => {
    const m = new Map<string, boolean>()
    for (const n of filtered.nodes) m.set(n.path, n.isPhantom)
    return m
  }, [filtered.nodes])

  const neighbourLookup = useMemo(() => {
    const m = new Map<string, Set<string>>()
    for (const e of filtered.edges) {
      if (!m.has(e.source)) m.set(e.source, new Set())
      if (!m.has(e.target)) m.set(e.target, new Set())
      m.get(e.source)!.add(e.target)
      m.get(e.target)!.add(e.source)
    }
    return m
  }, [filtered.edges])

  // Animation loop. Keeps ticking while not frozen and there are nodes.
  useEffect(() => {
    let raf = 0
    let cancelled = false
    const loop = () => {
      if (cancelled) return
      const sim = simNodesRef.current
      // Skip the physics step once alpha has cooled below threshold —
      // velocities damp out and positions stop drifting. The redraw
      // path still runs so hover/selection updates render.
      const alpha = alphaRef.current
      const shouldStep =
        sim.length > 0 && !settings.freeze && alpha > ALPHA_MIN
      if (shouldStep) {
        step(
          sim,
          simEdgesRef.current,
          {
            linkDistance: settings.linkDistance,
            linkStrength: settings.linkStrength,
            repulsion: settings.repulsion,
            centerGravity: settings.centerGravity,
            width: size.w,
            height: size.h,
          },
          alpha,
        )
        // Cool toward 0. Same shape as d3-force: alpha += (target -
        // alpha) * decay. Snap to exactly 0 once below the floor so
        // the next tick short-circuits cleanly.
        const next = alpha + (0 - alpha) * ALPHA_DECAY
        alphaRef.current = next < ALPHA_MIN ? 0 : next
      }
      draw()
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => {
      cancelled = true
      cancelAnimationFrame(raf)
    }
    // Intentionally only depends on values that change the loop body; sim
    // mutation lives in refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    settings.freeze,
    settings.linkDistance,
    settings.linkStrength,
    settings.repulsion,
    settings.centerGravity,
    size.w,
    size.h,
    settings.showLabels,
    settings.colourByFolder,
    hoveredId,
  ])

  function draw() {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const dpr = window.devicePixelRatio || 1
    const targetW = Math.floor(size.w * dpr)
    const targetH = Math.floor(size.h * dpr)
    if (canvas.width !== targetW || canvas.height !== targetH) {
      canvas.width = targetW
      canvas.height = targetH
    }

    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, size.w, size.h)

    const t = transformRef.current
    ctx.save()
    ctx.translate(t.x, t.y)
    ctx.scale(t.k, t.k)

    const sim = simNodesRef.current
    const positions = new Map<string, SimNode>()
    for (const n of sim) positions.set(n.id, n)

    const hoverNeighbours = hoveredId
      ? neighbourLookup.get(hoveredId) ?? new Set()
      : null

    // Edges
    ctx.lineWidth = 1 / t.k
    for (const e of filtered.edges) {
      const a = positions.get(e.source)
      const b = positions.get(e.target)
      if (!a || !b) continue
      const dim = !e.isResolved
      const isHover =
        hoveredId !== null &&
        (e.source === hoveredId || e.target === hoveredId)
      ctx.strokeStyle = isHover
        ? 'rgba(120,170,255,0.85)'
        : dim
        ? 'rgba(110,110,110,0.25)'
        : 'rgba(150,150,150,0.5)'
      ctx.beginPath()
      ctx.moveTo(a.x, a.y)
      ctx.lineTo(b.x, b.y)
      ctx.stroke()
    }

    // Nodes
    for (const node of sim) {
      const phantom = phantomLookup.get(node.id) === true
      const isHover = node.id === hoveredId
      const isNeighbour = hoverNeighbours?.has(node.id) === true
      const r = isHover ? HOVER_RADIUS : NODE_RADIUS
      let fill: string
      if (phantom) {
        fill = 'rgba(140,140,140,0.55)'
      } else if (settings.colourByFolder) {
        fill = folderColour(folderOf(node.id))
      } else {
        fill = 'rgba(200,200,210,0.95)'
      }
      if (isHover) fill = '#7aa7ff'
      else if (isNeighbour) fill = '#a4c4ff'
      ctx.fillStyle = fill
      ctx.beginPath()
      ctx.arc(node.x, node.y, r, 0, 2 * Math.PI)
      ctx.fill()
    }

    // Labels — only zoomed-in enough or on hover, otherwise the canvas turns
    // into a mat of text.
    if (settings.showLabels) {
      ctx.fillStyle = 'rgba(220,220,220,0.85)'
      ctx.font = `${11 / t.k}px ui-sans-serif, system-ui`
      ctx.textAlign = 'center'
      ctx.textBaseline = 'top'
      for (const node of sim) {
        const isHover = node.id === hoveredId
        if (t.k < 0.6 && !isHover) continue
        ctx.fillText(basename(node.id), node.x, node.y + NODE_RADIUS + 2 / t.k)
      }
    }

    ctx.restore()
  }

  function eventToWorld(ev: { clientX: number; clientY: number }) {
    const canvas = canvasRef.current
    if (!canvas) return { x: 0, y: 0 }
    const rect = canvas.getBoundingClientRect()
    const sx = ev.clientX - rect.left
    const sy = ev.clientY - rect.top
    const t = transformRef.current
    return { x: (sx - t.x) / t.k, y: (sy - t.y) / t.k }
  }

  function nodeAt(x: number, y: number): SimNode | null {
    const sim = simNodesRef.current
    const t = transformRef.current
    const r = (HOVER_RADIUS + 2) / t.k
    let best: SimNode | null = null
    let bestD2 = r * r
    for (const node of sim) {
      const dx = node.x - x
      const dy = node.y - y
      const d2 = dx * dx + dy * dy
      if (d2 <= bestD2) {
        best = node
        bestD2 = d2
      }
    }
    return best
  }

  function onPointerDown(ev: React.PointerEvent) {
    const w = eventToWorld(ev)
    const hit = nodeAt(w.x, w.y)
    if (hit) {
      hit.fx = hit.x
      hit.fy = hit.y
      dragRef.current = { kind: 'node', node: hit }
      // Wake the sim so neighbours respond as the node is dragged.
      alphaRef.current = Math.max(alphaRef.current, 0.3)
    } else {
      dragRef.current = { kind: 'pan', lastX: ev.clientX, lastY: ev.clientY }
    }
    ;(ev.target as Element).setPointerCapture(ev.pointerId)
  }

  function onPointerMove(ev: React.PointerEvent) {
    const drag = dragRef.current
    const w = eventToWorld(ev)

    if (drag?.kind === 'pan') {
      const dx = ev.clientX - drag.lastX
      const dy = ev.clientY - drag.lastY
      transformRef.current.x += dx
      transformRef.current.y += dy
      drag.lastX = ev.clientX
      drag.lastY = ev.clientY
      forceRedraw((n) => n + 1)
    } else if (drag?.kind === 'node') {
      drag.node.fx = w.x
      drag.node.fy = w.y
      // Keep the sim warm while the user is actively dragging.
      alphaRef.current = Math.max(alphaRef.current, 0.3)
    } else {
      const hit = nodeAt(w.x, w.y)
      const id = hit?.id ?? null
      if (id !== hoveredId) setHoveredId(id)
    }
  }

  function onPointerUp(ev: React.PointerEvent) {
    const drag = dragRef.current
    if (drag?.kind === 'node') {
      // Click-without-drag heuristic: if the node didn't move, treat as click.
      const node = drag.node
      const movedFar =
        Math.abs((node.fx ?? node.x) - node.x) > 2 ||
        Math.abs((node.fy ?? node.y) - node.y) > 2
      node.fx = null
      node.fy = null
      if (!movedFar) {
        const phantom = phantomLookup.get(node.id) === true
        if (!phantom) {
          eventBus.emit(EVENT_FILE_OPEN, {
            relpath: node.id,
            name: basename(node.id),
          })
        }
      }
    }
    dragRef.current = null
    ;(ev.target as Element).releasePointerCapture?.(ev.pointerId)
  }

  function onWheel(ev: React.WheelEvent) {
    ev.preventDefault()
    const t = transformRef.current
    const factor = ev.deltaY < 0 ? 1.12 : 1 / 1.12
    const newK = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, t.k * factor))
    const canvas = canvasRef.current
    if (!canvas) return
    const rect = canvas.getBoundingClientRect()
    const sx = ev.clientX - rect.left
    const sy = ev.clientY - rect.top
    // Keep cursor-anchored point fixed.
    transformRef.current = {
      k: newK,
      x: sx - ((sx - t.x) * newK) / t.k,
      y: sy - ((sy - t.y) * newK) / t.k,
    }
    forceRedraw((n) => n + 1)
  }

  function resetZoom() {
    transformRef.current = { ...IDENTITY }
    forceRedraw((n) => n + 1)
  }

  // Empty / loading / error states.
  let overlay: React.ReactNode = null
  if (loading && rawNodes.length === 0) {
    overlay = <Centered color="var(--text-muted)">Loading graph…</Centered>
  } else if (error) {
    overlay = <Centered color="var(--risk)">{error}</Centered>
  } else if (rawNodes.length === 0) {
    overlay = <Centered color="var(--text-faint)">No notes in this forge.</Centered>
  } else if (filtered.nodes.length === 0) {
    overlay = <Centered color="var(--text-faint)">No nodes match filter.</Centered>
  }

  return (
    <div
      ref={containerRef}
      style={{ position: 'relative', width: '100%', height: '100%' }}
    >
      <canvas
        ref={canvasRef}
        style={{
          display: 'block',
          width: size.w,
          height: size.h,
          background: 'var(--background-primary)',
          cursor: dragRef.current?.kind === 'pan' ? 'grabbing' : 'default',
        }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onWheel={onWheel}
      />

      {/* Top-right floating overlay */}
      <div
        style={{
          position: 'absolute',
          top: 8,
          right: 8,
          display: 'flex',
          gap: 6,
        }}
      >
        <OverlayButton
          title="Reset zoom"
          onClick={resetZoom}
          aria-label="Reset zoom"
        >
          <Icon name="crosshair" size={14} />
        </OverlayButton>
        <OverlayButton
          title="Graph settings"
          onClick={() => setGearOpen((v) => !v)}
          aria-label="Toggle graph settings"
        >
          <Icon name="settings" size={14} />
        </OverlayButton>
      </div>

      {/* Bottom-right zoom badge */}
      <div
        style={{
          position: 'absolute',
          bottom: 8,
          right: 10,
          padding: '2px 6px',
          background: 'var(--background-secondary)',
          color: 'var(--text-muted)',
          fontFamily: 'var(--font-interface)',
          fontSize: 11,
          border: '1px solid var(--background-modifier-border)',
          borderRadius: 4,
          pointerEvents: 'none',
        }}
      >
        {(transformRef.current.k * 100).toFixed(0)}%
      </div>

      <GraphGlobalGearDrawer open={gearOpen} onClose={() => setGearOpen(false)} />

      {overlay}
    </div>
  )
}

function OverlayButton({
  children,
  title,
  onClick,
  ...rest
}: {
  children: React.ReactNode
  title: string
  onClick: () => void
  'aria-label'?: string
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      style={{
        background: 'var(--background-secondary)',
        color: 'var(--text-normal)',
        border: '1px solid var(--background-modifier-border)',
        borderRadius: 4,
        padding: '4px 6px',
        cursor: 'pointer',
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
      }}
      {...rest}
    >
      {children}
    </button>
  )
}

function Centered({
  children,
  color,
}: {
  children: React.ReactNode
  color: string
}) {
  return (
    <div
      style={{
        position: 'absolute',
        inset: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        color,
        fontFamily: 'var(--font-interface)',
        fontSize: 12,
        pointerEvents: 'none',
      }}
    >
      {children}
    </div>
  )
}
