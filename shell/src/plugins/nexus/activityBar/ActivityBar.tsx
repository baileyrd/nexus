import { useState } from 'react'
import { useActivityBarStore, type ActivityBarItem } from './activityBarStore'
import { Icon } from '../../../icons'

interface ActivityBarProps {
  onItemClick: (item: ActivityBarItem) => void
}

export function ActivityBar({ onItemClick }: ActivityBarProps) {
  const items = useActivityBarStore((s) => s.items)
  const activeViewId = useActivityBarStore((s) => s.activeViewId)

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'stretch',
        width: '100%',
        height: '100%',
      }}
    >
      {items.map((item) => (
        <ActivityBarButton
          key={item.id}
          item={item}
          active={item.viewId === activeViewId}
          onClick={() => onItemClick(item)}
        />
      ))}
    </div>
  )
}

function ActivityBarButton({
  item,
  active,
  onClick,
}: {
  item: ActivityBarItem
  active: boolean
  onClick: () => void
}) {
  const [hover, setHover] = useState(false)
  const showAccent = active
  const showHover = hover && !active
  return (
    <button
      type="button"
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      aria-label={item.title}
      title={item.title}
      style={{
        position: 'relative',
        height: 44,
        background: showHover ? 'var(--bg-hover)' : 'transparent',
        border: 'none',
        color: active ? 'var(--fg)' : 'var(--fg-muted)',
        cursor: 'pointer',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 0,
        font: 'inherit',
        fontSize: 18,
        transition: 'background 0.08s, color 0.08s',
      }}
    >
      {showAccent && (
        <span
          aria-hidden
          style={{
            position: 'absolute',
            left: 0,
            top: 0,
            bottom: 0,
            width: 2,
            background: 'var(--accent)',
          }}
        />
      )}
      {item.iconName ? (
        <Icon name={item.iconName} size={18} />
      ) : item.iconPath ? (
        <svg
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.75"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden
        >
          <path d={item.iconPath} />
        </svg>
      ) : (
        <span style={{ lineHeight: 1 }}>{item.icon}</span>
      )}
    </button>
  )
}
