import { createElement } from 'react'
import { useSlotStore } from '../../../registry/SlotRegistry'
import { useLayoutStore } from '../../../stores/layoutStore'

export function SidebarHost() {
  const activeViewId = useLayoutStore((s) => s.sidebar.activeView)
  const entries = useSlotStore((s) => s.slots.sidebarContent)

  if (!activeViewId) return null
  const match = entries.find((e) => e.id === activeViewId)
  if (!match) return null

  return (
    <div style={{ height: '100%', width: '100%', overflow: 'auto' }}>
      {createElement(match.component)}
    </div>
  )
}
