import { createElement, useMemo } from 'react'
import { useSlotStore } from '../../../registry/SlotRegistry'
import { useRightPanelStore } from './rightPanelStore'
import { useLayoutStore } from '../../../stores/layoutStore'
import { useOutlineStore } from '../outline/outlineStore'
import { useBacklinksStore } from '../backlinks/backlinksStore'
import { Icon } from '../../../icons'
import { WindowControls } from '../../../shell/WindowControls'

const OUTLINE_VIEW_ID = 'nexus.outline.view'
const BACKLINKS_VIEW_ID = 'nexus.backlinks.view'

/**
 * Right-panel toggle — the `x` that collapses the panel. Lives at the
 * right edge of the tab row, immediately left of WindowControls.
 * Mirrors the hover behaviour of the old INSPECTOR-header close.
 */
function RightPanelToggleButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      aria-label="Hide inspector"
      title="Hide inspector"
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
        width: 28,
        height: 'var(--header-height)',
        padding: 0,
        border: 0,
        background: 'transparent',
        color: 'var(--fg-muted)',
        cursor: 'pointer',
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        flexShrink: 0,
      }}
    >
      <Icon name="x" size={14} />
    </button>
  )
}

/**
 * The view the rightPanel host registers into the `rightPanel` slot.
 * Single top row (--header-height, 30px Obsidian-faithful): tabs +
 * right-panel-toggle + window controls (rightmost column gets the
 * window controls). The old INSPECTOR label + StarButton were dropped
 * during the column-refactor (Task 6); the tab titles themselves
 * carry enough identity.
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
      {/* Top row — tab strip, right-panel-toggle, window controls.
          RightPanelHost only renders when rightPanel.visible, so this
          column is always the rightmost when present and is where the
          window controls live. */}
      <div
        className="rp-top-row"
        data-tauri-drag-region
        style={{
          height: 'var(--header-height)',
          flex: '0 0 var(--header-height)',
          flexShrink: 0,
          display: 'flex',
          alignItems: 'stretch',
          background: 'var(--bg-raised)',
          borderBottom: '1px solid var(--line-soft)',
        }}
      >
        <div
          className="rp-tabs"
          style={{
            flex: '1 1 auto',
            display: 'flex',
            alignItems: 'stretch',
            overflowX: 'auto',
            overflowY: 'hidden',
            whiteSpace: 'nowrap',
            minWidth: 0,
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
                  flex: '0 0 auto',
                  height: '100%',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 5,
                  padding: '0 var(--size-4-2)',
                  cursor: 'pointer',
                  fontSize: 12,
                  color: isActive ? 'var(--text-normal)' : 'var(--text-muted)',
                  boxShadow: isActive ? 'inset 0 -2px 0 var(--interactive-accent)' : undefined,
                  userSelect: 'none',
                  position: 'relative',
                  whiteSpace: 'nowrap',
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
        <RightPanelToggleButton onClick={toggleRightPanel} />
        <WindowControls />
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
