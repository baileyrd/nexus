// src/plugins/nexus/sidebar/SidebarTabStrip.tsx
// Header row for the left sidebar's multi-leaf model. Migrated to
// Obsidian-faithful DOM: `.workspace-tab-header-container` →
// `.workspace-tab-header-container-inner` → `.workspace-tab-header` →
// `.workspace-tab-header-inner > .workspace-tab-header-inner-icon`.
// Sidebar tabs are icon-only (app.css:6351) — the title goes in the
// tooltip. CSS handles styling, hover, and active states.

import { Icon, type IconName } from '../../../icons'
import type { SidebarLeaf } from './sidebarSplitStore'

export interface SidebarTabStripProps {
  leaves: SidebarLeaf[]
  activeLeafId: string | null
  onSelect: (id: string) => void
  onClose: (id: string) => void
  /** Resolve display metadata (title + iconName) for a leaf's view type. */
  getMeta: (type: string) => { title: string; iconName?: string }
}

export function SidebarTabStrip(props: SidebarTabStripProps) {
  const { leaves, activeLeafId, onSelect, getMeta } = props

  return (
    <div className="workspace-tab-header-container" data-tauri-drag-region>
      <div className="workspace-tab-header-container-inner" data-tauri-drag-region>
        {leaves.map((leaf) => {
          const isActive = leaf.id === activeLeafId
          const meta = getMeta(leaf.type)
          const iconName = (meta.iconName ?? 'filePlus') as IconName
          return (
            <div
              key={leaf.id}
              className={`workspace-tab-header${isActive ? ' is-active' : ''}`}
              role="tab"
              aria-selected={isActive}
              onClick={() => onSelect(leaf.id)}
              title={meta.title}
            >
              <div className="workspace-tab-header-inner">
                <div className="workspace-tab-header-inner-icon">
                  <Icon name={iconName} size={18} />
                </div>
                <div className="workspace-tab-header-inner-title">{meta.title}</div>
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}
