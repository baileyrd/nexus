import { useEffect, useMemo, useRef, useState } from 'react'
import { useGraphStore } from './graphStore'
import { radialLayout, type LaidOutNode } from './layout'
import { eventBus } from '../../../host/EventBus'

const EVENT_FILE_OPEN = 'files:open'

/** Fallback viewport size used before the first ResizeObserver tick lands. */
const DEFAULT_WIDTH = 240
const DEFAULT_HEIGHT = 300

/** Label truncation width in characters — SVG text has no reliable
 *  CSS text-overflow, so we clip in JS. 14 chars ≈ fits "shell-kernel". */
const LABEL_MAX_CHARS = 14

function truncate(label: string): string {
  if (label.length <= LABEL_MAX_CHARS) return label
  return label.slice(0, LABEL_MAX_CHARS - 1) + '…'
}

/**
 * Right-panel inspector: draws a radial 1-hop graph around the
 * currently-open file. Click a neighbour to open it.
 *
 * Layout is recomputed on every render and on every ResizeObserver
 * tick — the layout fn is pure and cheap, no memoisation required
 * beyond `useMemo` for the neighbours array reference stability.
 */
export function GraphView() {
  const currentRelpath = useGraphStore((s) => s.currentRelpath)
  const currentName = useGraphStore((s) => s.currentName)
  const neighbours = useGraphStore((s) => s.neighbours)
  const loading = useGraphStore((s) => s.loading)
  const error = useGraphStore((s) => s.error)

  // Container measurement. We render with a sensible default on the
  // first paint so the SVG isn't blank for a frame while the
  // ResizeObserver catches up, then swap in the real dims once we
  // have them.
  const containerRef = useRef<HTMLDivElement | null>(null)
  const [size, setSize] = useState<{ w: number; h: number }>({
    w: DEFAULT_WIDTH,
    h: DEFAULT_HEIGHT,
  })

  useEffect(() => {
    const el = containerRef.current
    if (!el) return

    // Seed from the synchronous bounding rect so the first render
    // after mount uses the real panel width rather than the default
    // constants. ResizeObserver only fires on subsequent changes.
    const rect = el.getBoundingClientRect()
    if (rect.width > 0 && rect.height > 0) {
      setSize({ w: rect.width, h: rect.height })
    }

    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect
        if (width > 0 && height > 0) {
          setSize({ w: width, h: height })
        }
      }
    })
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  // Empty states render a caption instead of the SVG.
  if (!currentRelpath) {
    return (
      <Container>
        <StateMessage color="var(--text-faint)">
          Open a file to see its neighbourhood.
        </StateMessage>
      </Container>
    )
  }
  if (error) {
    return (
      <Container>
        <StateMessage color="var(--risk)">{error}</StateMessage>
      </Container>
    )
  }
  if (loading) {
    return (
      <Container>
        <StateMessage color="var(--text-muted)">Loading…</StateMessage>
      </Container>
    )
  }

  return (
    <div
      ref={containerRef}
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        width: '100%',
        position: 'relative',
      }}
    >
      <GraphSvg
        currentRelpath={currentRelpath}
        currentName={currentName ?? currentRelpath}
        neighbours={neighbours}
        width={size.w}
        height={neighbours.length === 0 ? Math.max(0, size.h - 28) : size.h}
      />
      {neighbours.length === 0 && (
        <div
          style={{
            textAlign: 'center',
            padding: '4px 12px 10px',
            color: 'var(--text-faint)',
            fontFamily: 'var(--font-interface)',
            fontSize: 12,
          }}
        >
          No linked files.
        </div>
      )}
    </div>
  )
}

/**
 * Wrapper for the non-graph states — same flex scaffolding as the
 * SVG path so the tab height behaves consistently.
 */
function Container({ children }: { children: React.ReactNode }) {
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        width: '100%',
      }}
    >
      {children}
    </div>
  )
}

function StateMessage({
  children,
  color,
}: {
  children: React.ReactNode
  color: string
}) {
  return (
    <div
      style={{
        padding: '12px 14px',
        color,
        fontFamily: 'var(--font-interface)',
        fontSize: 12,
      }}
    >
      {children}
    </div>
  )
}

interface GraphSvgProps {
  currentRelpath: string
  currentName: string
  neighbours: ReturnType<typeof useGraphStore.getState>['neighbours']
  width: number
  height: number
}

/**
 * The SVG itself — edges first (drawn under the circles), then the
 * circles, then the labels. Neighbour circles emit `files:open` on
 * click and grow/switch stroke on hover.
 */
function GraphSvg({
  currentRelpath,
  currentName,
  neighbours,
  width,
  height,
}: GraphSvgProps) {
  const layout = useMemo(
    () =>
      radialLayout(
        { relpath: currentRelpath, name: currentName },
        neighbours,
        width,
        height,
      ),
    [currentRelpath, currentName, neighbours, width, height],
  )

  const [hoveredRelpath, setHoveredRelpath] = useState<string | null>(null)

  return (
    <svg
      width={layout.width}
      height={layout.height}
      viewBox={`0 0 ${layout.width} ${layout.height}`}
      style={{ display: 'block', flex: 1, width: '100%' }}
    >
      {/* edges under nodes */}
      {layout.edges.map((edge) => {
        const from = layout.nodes.find((n) => n.relpath === edge.fromRelpath)
        const to = layout.nodes.find((n) => n.relpath === edge.toRelpath)
        if (!from || !to) return null
        return (
          <line
            key={`${edge.fromRelpath}->${edge.toRelpath}`}
            x1={from.x}
            y1={from.y}
            x2={to.x}
            y2={to.y}
            stroke="var(--background-modifier-border)"
            strokeWidth={1}
          />
        )
      })}

      {/* nodes + labels */}
      {layout.nodes.map((node) => (
        <GraphNode
          key={node.relpath}
          node={node}
          hovered={hoveredRelpath === node.relpath}
          onHoverIn={() =>
            node.direction !== 'centre' && setHoveredRelpath(node.relpath)
          }
          onHoverOut={() =>
            node.direction !== 'centre' && setHoveredRelpath(null)
          }
          onPick={() => {
            if (node.direction === 'centre') return
            eventBus.emit(EVENT_FILE_OPEN, {
              relpath: node.relpath,
              name: node.name,
            })
          }}
        />
      ))}
    </svg>
  )
}

interface GraphNodeProps {
  node: LaidOutNode
  hovered: boolean
  onHoverIn: () => void
  onHoverOut: () => void
  onPick: () => void
}

function GraphNode({
  node,
  hovered,
  onHoverIn,
  onHoverOut,
  onPick,
}: GraphNodeProps) {
  const isCentre = node.direction === 'centre'
  const baseRadius = isCentre ? 8 : 6
  const radius = isCentre ? baseRadius : hovered ? baseRadius + 1 : baseRadius

  const fill = isCentre ? 'var(--interactive-accent)' : 'var(--background-secondary)'
  const stroke = isCentre
    ? 'none'
    : hovered
      ? 'var(--interactive-accent)'
      : 'var(--background-modifier-border)'
  const strokeWidth = isCentre ? 0 : 1.25

  // Label sits below the circle. Gap scales with radius so hover
  // doesn't make the label leap.
  const labelY = node.y + baseRadius + 12

  const labelFill = isCentre ? 'var(--text-normal)' : 'var(--text-muted)'
  const labelWeight = isCentre ? 600 : 400

  return (
    <g
      onClick={onPick}
      onMouseEnter={onHoverIn}
      onMouseLeave={onHoverOut}
      style={{ cursor: isCentre ? 'default' : 'pointer' }}
    >
      <circle
        cx={node.x}
        cy={node.y}
        r={radius}
        fill={fill}
        stroke={stroke}
        strokeWidth={strokeWidth}
      />
      <text
        x={node.x}
        y={labelY}
        textAnchor="middle"
        fill={labelFill}
        fontSize={11}
        fontFamily="var(--font-interface)"
        fontWeight={labelWeight}
        style={{ userSelect: 'none' }}
      >
        {truncate(node.name)}
      </text>
    </g>
  )
}
