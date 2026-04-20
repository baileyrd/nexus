// src/plugins/nexus/sidebar/SidebarTabStrip.tsx
// Header row for the left sidebar's multi-leaf model. Mirrors Obsidian's
// per-split tab container — one tab per open leaf, click to activate,
// hover to reveal the close chip. Empty space doubles as a Tauri drag
// region so the sidebar column participates in window dragging the same
// way the editor/right-panel tab rows do.

import { useState } from 'react'
import { Icon } from '../../../icons'
import type { SidebarLeaf } from './sidebarSplitStore'

export interface SidebarTabStripProps {
  leaves: SidebarLeaf[]
  activeLeafId: string | null
  onSelect: (id: string) => void
  onClose: (id: string) => void
  /** Resolve a display title for a leaf's view type. Falls back to the
   *  raw type string when no contributed title is found. */
  getTitle: (type: string) => string
}

export function SidebarTabStrip(props: SidebarTabStripProps) {
  const { leaves, activeLeafId, onSelect, onClose, getTitle } = props
  const [hoverId, setHoverId] = useState<string | null>(null)

  return (
    <div
      data-tauri-drag-region
      style={{
        height: 'var(--header-height)',
        flex: '0 0 var(--header-height)',
        display: 'flex',
        alignItems: 'stretch',
        background: 'var(--tab-container-background)',
        borderBottom: 'var(--tab-outline-width) solid var(--tab-outline-color)',
        overflow: 'hidden',
      }}
    >
      {leaves.map((leaf) => {
        const isActive = leaf.id === activeLeafId
        const isHovered = hoverId === leaf.id
        return (
          <div
            key={leaf.id}
            role="tab"
            aria-selected={isActive}
            onClick={() => onSelect(leaf.id)}
            onMouseEnter={() => setHoverId(leaf.id)}
            onMouseLeave={() => setHoverId((h) => (h === leaf.id ? null : h))}
            title={getTitle(leaf.type)}
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              gap: 6,
              padding: '0 8px 0 10px',
              maxWidth: 160,
              minWidth: 0,
              cursor: 'pointer',
              color: isActive ? 'var(--text-normal)' : 'var(--text-muted)',
              fontSize: 12,
              userSelect: 'none',
              boxShadow: isActive
                ? 'inset 0 -2px 0 var(--interactive-accent)'
                : 'none',
              background: isActive
                ? 'var(--tab-background-active, transparent)'
                : 'transparent',
            }}
          >
            <span
              style={{
                flex: 1,
                minWidth: 0,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              {getTitle(leaf.type)}
            </span>
            {/* Close chip — visible on hover or when active so the user can
                always dismiss the current tab without fishing for it. */}
            <button
              type="button"
              aria-label={`Close ${getTitle(leaf.type)}`}
              onClick={(e) => {
                e.stopPropagation()
                onClose(leaf.id)
              }}
              style={{
                width: 16,
                height: 16,
                padding: 0,
                border: 0,
                background: 'transparent',
                color: 'inherit',
                cursor: 'pointer',
                display: 'inline-flex',
                alignItems: 'center',
                justifyContent: 'center',
                borderRadius: 'var(--r)',
                flexShrink: 0,
                opacity: isActive || isHovered ? 0.75 : 0,
                transition: 'opacity 80ms linear',
              }}
              onMouseEnter={(e) => {
                ;(e.currentTarget as HTMLButtonElement).style.opacity = '1'
                ;(e.currentTarget as HTMLButtonElement).style.background =
                  'var(--bg-hover)'
              }}
              onMouseLeave={(e) => {
                ;(e.currentTarget as HTMLButtonElement).style.opacity =
                  isActive || isHovered ? '0.75' : '0'
                ;(e.currentTarget as HTMLButtonElement).style.background =
                  'transparent'
              }}
            >
              <Icon name="x" size={12} />
            </button>
          </div>
        )
      })}
      {/* Trailing flex filler — inherits data-tauri-drag-region from parent
          because it has no pointer-events-changing children, so empty
          space drags the window. */}
      <div style={{ flex: 1 }} />
    </div>
  )
}
