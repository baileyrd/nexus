import { createElement, useMemo } from 'react'
import { useSlotStore } from '../../../registry/SlotRegistry'
import { useRightPanelStore } from './rightPanelStore'
import { useLayoutStore } from '../../../stores/layoutStore'
import { useOutlineStore } from '../outline/outlineStore'
import { useBacklinksStore } from '../backlinks/backlinksStore'
import { Icon } from '../../../icons'

const OUTLINE_VIEW_ID = 'nexus.outline.view'
const BACKLINKS_VIEW_ID = 'nexus.backlinks.view'

function StarButton() {
  return (
    <button
      type="button"
      aria-label="Pin inspector"
      title="Pin inspector"
      onMouseEnter={(e) => {
        ;(e.currentTarget as HTMLButtonElement).style.background = 'var(--bg-hover)'
        ;(e.currentTarget as HTMLButtonElement).style.color = 'var(--fg)'
      }}
      onMouseLeave={(e) => {
        ;(e.currentTarget as HTMLButtonElement).style.background = 'transparent'
        ;(e.currentTarget as HTMLButtonElement).style.color = 'var(--fg-muted)'
      }}
      style={{
        width: 22,
        height: 22,
        padding: 0,
        border: 0,
        background: 'transparent',
        color: 'var(--fg-muted)',
        cursor: 'pointer',
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        borderRadius: 'var(--r)',
        flexShrink: 0,
      }}
    >
      <Icon name="star" size={14} />
    </button>
  )
}

function CloseButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      aria-label="Close inspector"
      title="Close inspector"
      onClick={onClick}
      onMouseEnter={(e) => {
        ;(e.currentTarget as HTMLButtonElement).style.background = 'var(--bg-hover)'
        ;(e.currentTarget as HTMLButtonElement).style.color = 'var(--fg)'
      }}
      onMouseLeave={(e) => {
        ;(e.currentTarget as HTMLButtonElement).style.background = 'transparent'
        ;(e.currentTarget as HTMLButtonElement).style.color = 'var(--fg-muted)'
      }}
      style={{
        width: 22,
        height: 22,
        padding: 0,
        border: 0,
        background: 'transparent',
        color: 'var(--fg-muted)',
        cursor: 'pointer',
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        borderRadius: 'var(--r)',
        flexShrink: 0,
      }}
    >
      <Icon name="x" size={14} />
    </button>
  )
}

/**
 * The view the rightPanel host registers into the `rightPanel` slot.
 * Renders an "INSPECTOR" header row, a tab row, and the active tab's body.
 *
 * Plugins contribute a tab by:
 *   1. `api.views.register(viewId, { slot: 'rightPanelContent', component, priority })`
 *   2. `api.events.emit('rightPanel:registerTab', { viewId, title, priority })`
 */
export function RightPanelHost() {
  const tabs = useRightPanelStore((s) => s.tabs)
  const activeViewId = useRightPanelStore((s) => s.activeViewId)
  const setActive = useRightPanelStore((s) => s.setActive)
  const entries = useSlotStore((s) => s.slots.rightPanelContent)
  const toggleRightPanel = useLayoutStore((s) => s.toggleRightPanel)

  const outlineCount = useOutlineStore((s) => s.headings.length)
  const backlinksCount = useBacklinksStore((s) => s.links.length)

  const ordered = useMemo(() => {
    return Object.entries(tabs)
      .map(([viewId, meta]) => ({ viewId, ...meta }))
      .sort((a, b) => a.priority - b.priority)
  }, [tabs])

  const activeEntry =
    activeViewId != null ? entries.find((e) => e.id === activeViewId) : undefined

  const getCount = (viewId: string): number | null => {
    if (viewId === OUTLINE_VIEW_ID) return outlineCount
    if (viewId === BACKLINKS_VIEW_ID) return backlinksCount
    return null
  }

  return (
    <div
      style={{
        height: '100%',
        width: '100%',
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
      }}
    >
      {/* INSPECTOR header */}
      <div
        style={{
          height: 36,
          flex: '0 0 36px',
          display: 'flex',
          alignItems: 'center',
          padding: '0 6px 0 12px',
          background: 'var(--bg-raised)',
          borderBottom: '1px solid var(--line-soft)',
          gap: 4,
        }}
      >
        <span
          style={{
            flex: 1,
            fontSize: 11,
            fontWeight: 700,
            letterSpacing: '0.08em',
            color: 'var(--fg-muted)',
            userSelect: 'none',
          }}
        >
          INSPECTOR
        </span>
        <StarButton />
        <CloseButton onClick={toggleRightPanel} />
      </div>

      {/* Tab row */}
      <div
        style={{
          height: 32,
          flex: '0 0 32px',
          background: 'var(--bg-raised)',
          borderBottom: '1px solid var(--line-soft)',
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'stretch',
          overflowX: 'auto',
          overflowY: 'hidden',
          whiteSpace: 'nowrap',
        }}
      >
        {ordered.map((t) => {
          const isActive = t.viewId === activeViewId
          const count = getCount(t.viewId)
          return (
            <div
              key={t.viewId}
              onClick={() => setActive(t.viewId)}
              title={t.title}
              style={{
                flex: 1,
                height: '100%',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                gap: 5,
                cursor: 'pointer',
                fontSize: 12,
                color: isActive ? 'var(--fg)' : 'var(--fg-muted)',
                boxShadow: isActive ? 'inset 0 -2px 0 var(--accent)' : undefined,
                userSelect: 'none',
                position: 'relative',
              }}
              onMouseEnter={(e) => {
                if (!isActive) e.currentTarget.style.background = 'var(--bg-hover)'
              }}
              onMouseLeave={(e) => {
                if (!isActive) e.currentTarget.style.background = 'transparent'
              }}
            >
              {t.title}
              {count !== null && count > 0 && (
                <span
                  style={{
                    fontFamily: 'var(--f-mono)',
                    fontSize: 10,
                    color: 'var(--fg-dim)',
                  }}
                >
                  {count}
                </span>
              )}
            </div>
          )
        })}
      </div>

      {/* Body */}
      <div style={{ flex: '1 1 auto', overflow: 'auto', minHeight: 0 }}>
        {activeEntry ? (
          createElement(activeEntry.component)
        ) : (
          <div
            style={{
              padding: 16,
              color: 'var(--fg-dim)',
              fontSize: 12,
              textAlign: 'center',
            }}
          >
            No inspectors registered
          </div>
        )}
      </div>
    </div>
  )
}
