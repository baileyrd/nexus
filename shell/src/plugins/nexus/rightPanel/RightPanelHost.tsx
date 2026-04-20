import { createElement, useMemo } from 'react'
import { useSlotStore } from '../../../registry/SlotRegistry'
import { useRightPanelStore } from './rightPanelStore'
import { useLayoutStore } from '../../../stores/layoutStore'
import { Icon, type IconName } from '../../../icons'
import { WindowControls } from '../../../shell/WindowControls'

/**
 * Right-panel toggle — the `x` that collapses the panel. Lives at the
 * right edge of the tab row, immediately left of WindowControls.
 */
function RightPanelToggleButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      aria-label="Hide inspector"
      title="Hide inspector"
      onClick={onClick}
      className="clickable-icon sidebar-toggle-button mod-right"
      style={{
        width: 28,
        height: 'var(--header-height)',
        padding: 0,
        border: 0,
        background: 'transparent',
        color: 'var(--icon-color)',
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
 * The right-panel host renders the Obsidian `.workspace-split.mod-right-split`
 * interior — a `.workspace-tabs.mod-top` column whose tab-header is
 * icon-only (per app.css:6351) with the title shown via tooltip.
 * Plugins contribute a tab by:
 *   1. `api.views.register(viewId, { slot: 'rightPanelContent', component, priority })`
 *   2. `api.events.emit('rightPanel:registerTab', { viewId, title, priority, iconName })`
 */
export function RightPanelHost() {
  const tabs = useRightPanelStore((s) => s.tabs)
  const activeViewId = useRightPanelStore((s) => s.activeViewId)
  const setActive = useRightPanelStore((s) => s.setActive)
  const entries = useSlotStore((s) => s.slots.rightPanelContent)
  const toggleRightPanel = useLayoutStore((s) => s.toggleRightPanel)

  const ordered = useMemo(() => {
    return Object.entries(tabs)
      .map(([viewId, meta]) => ({ viewId, ...meta }))
      .sort((a, b) => a.priority - b.priority)
  }, [tabs])

  const activeEntry =
    activeViewId != null ? entries.find((e) => e.id === activeViewId) : undefined

  return (
    <div
      className="workspace-tabs mod-top"
      style={{
        height: '100%',
        width: '100%',
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
      }}
    >
      {/* Tab header — Obsidian structure. The window controls live at the
          far right of this row so min/max/close stays reachable when the
          right panel is the rightmost column. */}
      <div className="workspace-tab-header-container" data-tauri-drag-region>
        <div className="workspace-tab-header-container-inner" data-tauri-drag-region>
          {ordered.map((t) => {
            const isActive = t.viewId === activeViewId
            const iconName = (t.iconName ?? 'filePlus') as IconName
            return (
              <div
                key={t.viewId}
                className={`workspace-tab-header${isActive ? ' is-active' : ''}`}
                role="tab"
                aria-selected={isActive}
                onClick={() => setActive(t.viewId)}
                title={t.title}
              >
                <div className="workspace-tab-header-inner">
                  <div className="workspace-tab-header-inner-icon">
                    <Icon name={iconName} size={18} />
                  </div>
                  <div className="workspace-tab-header-inner-title">{t.title}</div>
                </div>
              </div>
            )
          })}
        </div>
        <RightPanelToggleButton onClick={toggleRightPanel} />
        <WindowControls />
      </div>

      {/* Body */}
      <div
        className="workspace-leaf"
        style={{ flex: '1 1 auto', overflow: 'auto', minHeight: 0 }}
      >
        <div className="workspace-leaf-content view-content">
          {activeEntry ? (
            createElement(activeEntry.component)
          ) : (
            <div
              style={{
                padding: 16,
                color: 'var(--text-faint)',
                fontSize: 12,
                textAlign: 'center',
              }}
            >
              No inspectors registered
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
