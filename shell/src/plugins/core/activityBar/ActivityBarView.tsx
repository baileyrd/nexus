// Forge-styled activity rail. The existing plugin contract keeps an
// item store + slot-based registration; icons are resolved via the
// shared Ic map so plugin authors reference icons by name.

import { useActivityBarStore } from './activityBarStore'
import { useLayoutStore } from '../../../stores/layoutStore'
import { getRegistry } from '../../../host/shellRegistry'
import { Ic, type IconName } from '../../../shell/icons'

function resolveIcon(name: string) {
  const key = name as IconName
  return Ic[key] ?? Ic.doc
}

export function ActivityBarView() {
  const { items, activeId, setActive } = useActivityBarStore()
  const { setActiveSidebarView, toggleSidebar, sidebar } = useLayoutStore()

  const handleClick = (item: typeof items[number]) => {
    if (activeId === item.id && sidebar.visible) {
      toggleSidebar()
    } else {
      setActive(item.id)
      setActiveSidebarView(item.viewId)
      if (!sidebar.visible) toggleSidebar()
    }
  }

  const Settings = Ic.settings

  return (
    <div className="rail">
      {items.map(item => {
        const Icon = resolveIcon(item.icon)
        return (
          <button
            key={item.id}
            className={'rail-btn ' + (activeId === item.id ? 'active' : '')}
            onClick={() => handleClick(item)}
            title={item.title}
          >
            <Icon />
          </button>
        )
      })}
      <div className="spacer" />
      <button
        className="rail-btn"
        onClick={() => getRegistry()?.commands.execute('workbench.action.openSettings')}
        title="Settings (⌘,)"
      >
        <Settings />
      </button>
    </div>
  )
}
