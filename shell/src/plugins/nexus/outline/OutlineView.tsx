import { useEffect, useRef } from 'react'
import { useOutlineStore, type OutlineHeading } from './outlineStore'
import { eventBus } from '../../../host/EventBus'

const EVENT_SCROLL_TO = 'editor:scrollToHeading'

interface RowProps {
  heading: OutlineHeading
  active: boolean
  rowRef: ((el: HTMLDivElement | null) => void) | null
}

function Row({ heading, active, rowRef }: RowProps) {
  return (
    <div
      ref={rowRef}
      onClick={() =>
        eventBus.emit(EVENT_SCROLL_TO, {
          headingId: heading.id,
          line: heading.line,
          index: heading.index,
        })
      }
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        padding: '4px 10px',
        paddingLeft: 10 + heading.level * 12,
        cursor: 'pointer',
        fontSize: 12,
        color: active ? 'var(--accent)' : 'var(--fg)',
        background: active ? 'var(--bg-hover)' : 'transparent',
        borderLeft: `2px solid ${active ? 'var(--accent)' : 'transparent'}`,
        lineHeight: 1.35,
        whiteSpace: 'nowrap',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
      }}
      title={heading.text}
      onMouseEnter={(e) => {
        if (!active) e.currentTarget.style.background = 'var(--bg-hover)'
      }}
      onMouseLeave={(e) => {
        if (!active) e.currentTarget.style.background = 'transparent'
      }}
    >
      <span
        style={{
          fontSize: 10,
          color: 'var(--fg-dim)',
          fontFamily: 'var(--font-mono, monospace)',
          flex: '0 0 auto',
        }}
      >
        H{heading.level}
      </span>
      <span
        style={{
          flex: '1 1 auto',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          fontWeight: active ? 500 : 400,
        }}
      >
        {heading.text}
      </span>
    </div>
  )
}

export function OutlineView() {
  const headings = useOutlineStore((s) => s.headings)
  const activeIndex = useOutlineStore((s) => s.activeIndex)
  const activeRowRef = useRef<HTMLDivElement | null>(null)

  // Keep the active row visible as the editor scrolls. `nearest` avoids
  // jumping when the row is already on screen, which would otherwise
  // wrestle scroll input from a user manually scrubbing the outline.
  useEffect(() => {
    if (activeRowRef.current) {
      activeRowRef.current.scrollIntoView({ block: 'nearest' })
    }
  }, [activeIndex])

  if (headings.length === 0) {
    return (
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          height: '100%',
          padding: 16,
          color: 'var(--fg-dim)',
          fontSize: 12,
        }}
      >
        No headings
      </div>
    )
  }

  return (
    <div style={{ padding: '4px 0' }}>
      {headings.map((h, i) => {
        const active = i === activeIndex
        return (
          <Row
            key={h.id}
            heading={h}
            active={active}
            rowRef={active ? (el) => { activeRowRef.current = el } : null}
          />
        )
      })}
    </div>
  )
}
