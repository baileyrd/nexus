import { useEffect, useMemo, useRef, useState } from 'react'
import { useOutlineStore, type OutlineHeading } from './outlineStore'
import { eventBus } from '../../../host/EventBus'
import { Icon, type IconName } from '../../../icons'

const EVENT_SCROLL_TO = 'editor:scrollToHeading'
const EVENT_REQUEST_REFRESH = 'nexus.outline:requestRefresh'

/** One row in the flat "visible" list we build for rendering.
 *  Carries the depth in the section hierarchy (0-based, smallest
 *  level in the doc = 0) and whether it has children to toggle. */
interface VisibleRow {
  heading: OutlineHeading
  depth: number
  hasChildren: boolean
  flatIndex: number
  collapsed: boolean
}

/** Walk the flat heading list and compute the visible rows under the
 *  current collapse state + filter. Also returns, for each rendered
 *  row, the set of ancestor depths whose branch still has a sibling
 *  below — used to decide which connector lines to draw on that row. */
function buildVisibleRows(
  headings: OutlineHeading[],
  collapsed: Set<string>,
  filter: string,
): VisibleRow[] {
  if (headings.length === 0) return []
  const minLevel = Math.min(...headings.map((h) => h.level))
  const out: VisibleRow[] = []

  // Pre-compute, for each heading, whether anything follows it at a
  // deeper level before a peer / shallower heading shows up. We use
  // "any deeper descendant" rather than "direct +1 child" so an h1
  // followed by an h3 (skipped h2) still gets a chevron.
  const hasDescendants = new Array<boolean>(headings.length).fill(false)
  for (let i = 0; i < headings.length - 1; i++) {
    if (headings[i + 1].level > headings[i].level) hasDescendants[i] = true
  }

  // Filter: keep any heading whose text matches, plus all its
  // ancestors for context. We compute the match set in a first pass,
  // then expand it with ancestors.
  const trimmed = filter.trim().toLowerCase()
  let visibleIdx: Set<number> | null = null
  if (trimmed) {
    visibleIdx = new Set()
    const stack: number[] = [] // indices on the ancestor path
    for (let i = 0; i < headings.length; i++) {
      const h = headings[i]
      while (stack.length > 0 && headings[stack[stack.length - 1]].level >= h.level) {
        stack.pop()
      }
      if (h.text.toLowerCase().includes(trimmed)) {
        visibleIdx.add(i)
        for (const a of stack) visibleIdx.add(a)
      }
      stack.push(i)
    }
  }

  // Track whether we're inside a collapsed subtree by holding the
  // level of the collapsed ancestor; anything at a higher level is
  // skipped until we leave the subtree.
  let suppressUntilLevel: number | null = null

  for (let i = 0; i < headings.length; i++) {
    const h = headings[i]
    if (suppressUntilLevel !== null) {
      if (h.level > suppressUntilLevel) continue
      suppressUntilLevel = null
    }
    if (visibleIdx && !visibleIdx.has(i)) continue
    const isCollapsed = collapsed.has(h.id)
    out.push({
      heading: h,
      depth: h.level - minLevel,
      hasChildren: hasDescendants[i],
      flatIndex: i,
      collapsed: isCollapsed,
    })
    if (isCollapsed && hasDescendants[i]) {
      suppressUntilLevel = h.level
    }
  }
  return out
}

const INDENT_PX = 18

export function OutlineView() {
  const headings = useOutlineStore((s) => s.headings)
  const activeIndex = useOutlineStore((s) => s.activeIndex)
  const filter = useOutlineStore((s) => s.filter)
  const collapsed = useOutlineStore((s) => s.collapsed)
  const autoScroll = useOutlineStore((s) => s.autoScroll)
  const setFilter = useOutlineStore((s) => s.setFilter)
  const toggleCollapsed = useOutlineStore((s) => s.toggleCollapsed)
  const collapseAll = useOutlineStore((s) => s.collapseAll)
  const expandAll = useOutlineStore((s) => s.expandAll)
  const setAutoScroll = useOutlineStore((s) => s.setAutoScroll)
  const anyCollapsed = collapsed.size > 0
  const searchRef = useRef<HTMLInputElement>(null)
  const activeRowRef = useRef<HTMLDivElement | null>(null)

  const rows = useMemo(
    () => buildVisibleRows(headings, collapsed, filter),
    [headings, collapsed, filter],
  )

  useEffect(() => {
    if (!autoScroll) return
    if (activeRowRef.current) {
      activeRowRef.current.scrollIntoView({ block: 'nearest' })
    }
  }, [activeIndex, autoScroll])

  // Ask the plugin to rebuild headings whenever this view mounts — the
  // view may have been hidden (right-dock tab switch) during the last
  // editor-store transition, in which case its last recompute predates
  // the current active file.
  useEffect(() => {
    eventBus.emit(EVENT_REQUEST_REFRESH, null)
  }, [])

  const focusSearch = () => {
    searchRef.current?.focus()
    searchRef.current?.select()
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', overflow: 'hidden' }}>
      <Toolbar
        autoScroll={autoScroll}
        anyCollapsed={anyCollapsed}
        onFocusSearch={focusSearch}
        onToggleAutoScroll={() => setAutoScroll(!autoScroll)}
        onCollapseOrExpandAll={anyCollapsed ? expandAll : collapseAll}
      />

      <div
        style={{
          flexShrink: 0,
          padding: '4px 8px 6px',
          borderBottom: '1px solid var(--divider-color)',
        }}
      >
        <SearchInput inputRef={searchRef} value={filter} onChange={setFilter} />
      </div>

      <div style={{ flex: 1, overflow: 'auto', padding: '4px 0' }}>
        {headings.length === 0 ? (
          <div
            style={{
              padding: 16,
              textAlign: 'center',
              color: 'var(--text-faint)',
              fontSize: 12,
            }}
          >
            No headings found.
          </div>
        ) : rows.length === 0 ? (
          <div
            style={{
              padding: 16,
              textAlign: 'center',
              color: 'var(--text-faint)',
              fontSize: 12,
            }}
          >
            No headings match “{filter}”
          </div>
        ) : (
          rows.map((r) => {
            const active = r.flatIndex === activeIndex
            return (
              <Row
                key={r.heading.id}
                row={r}
                active={active}
                rowRef={active ? (el) => { activeRowRef.current = el } : null}
                onToggle={() => toggleCollapsed(r.heading.id)}
                onClick={() =>
                  eventBus.emit(EVENT_SCROLL_TO, {
                    headingId: r.heading.id,
                    line: r.heading.line,
                    index: r.heading.index,
                  })
                }
              />
            )
          })
        )}
      </div>
    </div>
  )
}

function Toolbar({
  autoScroll,
  anyCollapsed,
  onFocusSearch,
  onToggleAutoScroll,
  onCollapseOrExpandAll,
}: {
  autoScroll: boolean
  anyCollapsed: boolean
  onFocusSearch: () => void
  onToggleAutoScroll: () => void
  onCollapseOrExpandAll: () => void
}) {
  return (
    <div
      style={{
        flexShrink: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 4,
        padding: '6px 8px',
        borderBottom: '1px solid var(--divider-color)',
      }}
    >
      <ToolbarButton label="Focus search" icon="search" onClick={onFocusSearch} />
      <ToolbarButton
        label={autoScroll ? 'Auto-scroll: on' : 'Auto-scroll to current section'}
        icon="crosshair"
        active={autoScroll}
        onClick={onToggleAutoScroll}
      />
      <ToolbarButton
        label={anyCollapsed ? 'Expand all' : 'Collapse all'}
        icon="collapseAll"
        onClick={onCollapseOrExpandAll}
      />
    </div>
  )
}

function ToolbarButton({
  label,
  icon,
  active,
  onClick,
}: {
  label: string
  icon: IconName
  active?: boolean
  onClick: () => void
}) {
  const [hover, setHover] = useState(false)
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={active}
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        width: 26,
        height: 24,
        padding: 0,
        border: 0,
        background: active ? 'var(--background-primary)' : hover ? 'var(--background-modifier-hover)' : 'transparent',
        color: active || hover ? 'var(--text-normal)' : 'var(--text-muted)',
        cursor: 'pointer',
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        borderRadius: 'var(--radius-s)',
        transition: 'background 0.08s, color 0.08s',
      }}
    >
      <Icon name={icon} size={14} />
    </button>
  )
}

function SearchInput({
  value,
  onChange,
  inputRef,
}: {
  value: string
  onChange: (v: string) => void
  inputRef: React.RefObject<HTMLInputElement>
}) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 6,
        background: 'var(--background-primary)',
        border: '1px solid var(--divider-color)',
        borderRadius: 'var(--radius-s)',
        padding: '4px 10px',
      }}
    >
      <span style={{ color: 'var(--text-faint)', display: 'inline-flex', flexShrink: 0 }} aria-hidden>
        <Icon name="search" size={12} />
      </span>
      <input
        ref={inputRef}
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder="Search..."
        aria-label="Filter headings"
        style={{
          flex: 1,
          background: 'transparent',
          border: 0,
          outline: 'none',
          color: 'var(--text-normal)',
          fontSize: 'var(--ui-size, 12px)',
          fontFamily: 'var(--font-interface)',
          padding: 0,
          lineHeight: '20px',
        }}
      />
    </div>
  )
}

function Row({
  row,
  active,
  rowRef,
  onToggle,
  onClick,
}: {
  row: VisibleRow
  active: boolean
  rowRef: ((el: HTMLDivElement | null) => void) | null
  onToggle: () => void
  onClick: () => void
}) {
  const { heading, depth, hasChildren, collapsed } = row

  return (
    <div
      ref={rowRef}
      onClick={onClick}
      title={heading.text}
      style={{
        position: 'relative',
        display: 'flex',
        alignItems: 'center',
        gap: 4,
        padding: `4px 10px 4px ${10 + depth * INDENT_PX}px`,
        cursor: 'pointer',
        fontSize: 12,
        color: active ? 'var(--text-normal)' : 'var(--text-muted)',
        background: active ? 'var(--background-modifier-hover)' : 'transparent',
        lineHeight: 1.4,
        fontWeight: active ? 500 : 400,
      }}
      onMouseEnter={(e) => {
        if (!active) e.currentTarget.style.background = 'var(--background-modifier-hover)'
      }}
      onMouseLeave={(e) => {
        if (!active) e.currentTarget.style.background = 'transparent'
      }}
    >
      {/* Vertical connector lines at each ancestor depth. */}
      {Array.from({ length: depth }).map((_, i) => (
        <span
          key={i}
          aria-hidden
          style={{
            position: 'absolute',
            left: 10 + i * INDENT_PX + 4,
            top: 0,
            bottom: 0,
            width: 1,
            background: 'var(--divider-color)',
          }}
        />
      ))}

      <span
        aria-hidden
        onClick={(e) => {
          e.stopPropagation()
          if (hasChildren) onToggle()
        }}
        style={{
          width: 14,
          height: 14,
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          color: 'var(--text-faint)',
          cursor: hasChildren ? 'pointer' : 'default',
          flexShrink: 0,
        }}
      >
        {hasChildren ? (
          <Icon
            name="chev"
            size={10}
            style={{
              transform: collapsed ? undefined : 'rotate(90deg)',
              transition: 'transform 0.08s',
            }}
          />
        ) : null}
      </span>

      {/* BL-053 mockup row N — `01` / `02` ember-numbered prefix on
          top-level (depth=0) headings. Sub-headings drop the prefix
          to keep the visual band anchored at section starts. The
          number is the 1-based count among headings at depth 0; we
          recompute it during render rather than threading it through
          OutlineHeading because the depth filter can hide rows. */}
      {depth === 0 ? <span className="nx-outline__prefix">{formatPrefix(row.flatIndex)}</span> : null}

      <span
        style={{
          flex: '1 1 auto',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
        }}
      >
        {heading.text}
      </span>

      {/* BL-053 mockup row N tail — faint word-count badge. Renders
          only when the parser populated `wordCount` (always today;
          guarded for forward-compat with future parsers that may
          omit it). Hidden for sections that wrap to zero words. */}
      {typeof heading.wordCount === 'number' && heading.wordCount > 0 ? (
        <span className="nx-outline__count">{compactCount(heading.wordCount)}</span>
      ) : null}
    </div>
  )
}

/** Zero-padded prefix used for the leading "01 / 02" stripe on
 *  top-level outline rows. Matches the mockup's visual rhythm. */
function formatPrefix(index: number): string {
  // `index` here is the 0-based position in the flat heading list.
  // Convert to 1-based for display and zero-pad to two digits.
  const n = index + 1
  return n < 100 ? n.toString().padStart(2, '0') : n.toString()
}

/** Compact word-count: 950 → "950", 1240 → "1.2k", 12_400 → "12k".
 *  Stays under 4 chars so the badge doesn't push the heading text. */
function compactCount(n: number): string {
  if (n < 1000) return n.toString()
  const k = n / 1000
  if (k < 10) return `${k.toFixed(1)}k`
  return `${Math.round(k)}k`
}
