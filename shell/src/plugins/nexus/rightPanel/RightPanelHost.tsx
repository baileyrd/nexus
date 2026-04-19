import { createElement, useMemo } from 'react'
import { useSlotStore } from '../../../registry/SlotRegistry'
import { useRightPanelStore } from './rightPanelStore'

/**
 * The view the rightPanel host registers into the `rightPanel` slot.
 * Renders a tab row (metadata from `rightPanelStore`) plus the active
 * tab's body component (resolved from the `rightPanelContent` slot).
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

  // Tabs in priority order — lower number first, matching SlotRegistry
  // sort convention. Ties fall back to insertion order via entry index.
  const ordered = useMemo(() => {
    return Object.entries(tabs)
      .map(([viewId, meta]) => ({ viewId, ...meta }))
      .sort((a, b) => a.priority - b.priority)
  }, [tabs])

  const activeEntry =
    activeViewId != null ? entries.find((e) => e.id === activeViewId) : undefined

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
          return (
            <div
              key={t.viewId}
              onClick={() => setActive(t.viewId)}
              title={t.title}
              style={{
                padding: '0 14px',
                height: '100%',
                display: 'flex',
                alignItems: 'center',
                cursor: 'pointer',
                fontSize: 12,
                color: isActive ? 'var(--fg)' : 'var(--fg-muted)',
                boxShadow: isActive ? 'inset 0 -2px 0 var(--accent)' : undefined,
                userSelect: 'none',
              }}
              onMouseEnter={(e) => {
                if (!isActive) e.currentTarget.style.background = 'var(--bg-hover)'
              }}
              onMouseLeave={(e) => {
                if (!isActive) e.currentTarget.style.background = 'transparent'
              }}
            >
              {t.title}
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
