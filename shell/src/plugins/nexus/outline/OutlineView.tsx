import { useOutlineStore, type OutlineHeading } from './outlineStore'
import { eventBus } from '../../../host/EventBus'

const EVENT_SCROLL_TO = 'editor:scrollToHeading'

interface RowProps {
  heading: OutlineHeading
}

function Row({ heading }: RowProps) {
  return (
    <div
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
        color: 'var(--fg)',
        lineHeight: 1.35,
        whiteSpace: 'nowrap',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
      }}
      title={heading.text}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = 'var(--bg-hover)'
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = 'transparent'
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
        }}
      >
        {heading.text}
      </span>
    </div>
  )
}

export function OutlineView() {
  const headings = useOutlineStore((s) => s.headings)

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
      {headings.map((h) => (
        <Row key={h.id} heading={h} />
      ))}
    </div>
  )
}
