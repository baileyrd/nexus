// Forge-styled activity rail. The existing plugin contract keeps an
// item store + slot-based registration; icons are resolved via the
// shared Ic map so plugin authors reference icons by name.

import { useActivityBarStore } from './activityBarStore'
import { workspace } from '../../../workspace'
import { getRegistry } from '../../../host/shellRegistry'
import { Ic, type IconName } from '../../../shell/icons'

function resolveIcon(name: string) {
  const key = name as IconName
  return Ic[key] ?? Ic.doc
}

// Legacy template view — retained on disk but NOT loaded from main.tsx.
// The active activity-bar component is at plugins/nexus/activityBar.
export function ActivityBarView() {
  const { items, activeId, setActive } = useActivityBarStore()

  const handleClick = (item: typeof items[number]) => {
    const sidebarVisible = !workspace.leftSplit.collapsed
    if (activeId === item.id && sidebarVisible) {
      workspace.setSidedockCollapsed('left', true)
    } else {
      setActive(item.id)
      // Phase 7 removed the active-sidebar-view concept; consumers that
      // need a specific view now call `workspace.ensureLeafOfType +
      // revealLeaf` directly from their focus command.
      if (!sidebarVisible) workspace.setSidedockCollapsed('left', false)
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
