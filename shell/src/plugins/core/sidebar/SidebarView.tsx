// Sidebar host — delegates its entire surface to the active view.
// The active view is responsible for its own chrome (.leftpanel / .panel-head /
// .filter / etc.), so this component just picks the view.

import { useLayoutStore } from '../../../stores/layoutStore'
import { useSlotStore } from '../../../registry/SlotRegistry'

export function SidebarView() {
  const activeView = useLayoutStore(s => s.sidebar.activeView)
  const entries    = useSlotStore(s => s.slots.sidebarContent)

  const activeEntry = entries.find(e => e.id === activeView) ?? entries[0]

  if (!activeEntry) {
    return (
      <div className="leftpanel">
        <div className="panel-head"><span>Explorer</span></div>
        <div className="filter" />
        <div className="tree" style={{ color: 'var(--fg-dim)', fontSize: 12, padding: 16 }}>
          No sidebar views registered.
        </div>
        <div className="leftfoot" />
      </div>
    )
  }

  return <activeEntry.component />
}
