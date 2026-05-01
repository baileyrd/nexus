// Minimal positioned context menu for the files tree (WI-21).
//
// Why a local copy instead of `shell/src/shell/ContextMenu.tsx`?
// That module imports from `host/shellRegistry`, which the WI-23
// import-hygiene guardrail forbids in this plugin. Rather than add
// nexus.files to the host-internals allowlist for one menu, this
// keeps the plugin clean. The menu is intentionally small — six
// items max, no submenus, no command-palette integration — so a
// 50-line standalone component is the right size.
//
// Behavior:
//   - Positioned at (x, y) in viewport coords; flipped to fit if it
//     would overflow the right or bottom edge.
//   - Click outside or Escape dismisses.
//   - Up/Down arrow keys move highlight; Enter activates; Tab also
//     moves highlight (keyboard-only users can drive it).
//   - First non-disabled item is auto-focused on mount.

import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from 'react'
import { zIndex } from '../../../shell/zIndex'

export interface FilesContextMenuItem {
  id: string
  label: string
  onSelect: () => void | Promise<void>
  disabled?: boolean
  separatorBefore?: boolean
}

export interface FilesContextMenuProps {
  x: number
  y: number
  items: FilesContextMenuItem[]
  onClose: () => void
}

const PANEL_STYLE: CSSProperties = {
  position: 'fixed',
  zIndex: zIndex.dropdown,
  background: 'var(--background-primary, var(--bg-raised, #1e1e1e))',
  border: '1px solid var(--divider-color, var(--line, #333))',
  borderRadius: 6,
  boxShadow: '0 6px 20px rgba(0,0,0,0.35)',
  padding: 4,
  fontSize: 12,
  fontFamily: 'var(--f-ui, inherit)',
  minWidth: 180,
}

const SEPARATOR_STYLE: CSSProperties = {
  height: 1,
  background: 'var(--divider-color, var(--line, #333))',
  margin: '4px 0',
}

export function FilesContextMenu({ x, y, items, onClose }: FilesContextMenuProps) {
  const panelRef = useRef<HTMLDivElement | null>(null)
  const [pos, setPos] = useState<{ left: number; top: number }>({ left: x, top: y })
  // Index into items[] of the highlighted row. Starts on the first
  // non-disabled item; -1 if all are disabled.
  const initialIndex = items.findIndex((i) => !i.disabled)
  const [activeIndex, setActiveIndex] = useState<number>(initialIndex)

  // Flip into the viewport once we know the rendered size.
  useLayoutEffect(() => {
    const el = panelRef.current
    if (!el) return
    const rect = el.getBoundingClientRect()
    let left = x
    let top = y
    if (left + rect.width > window.innerWidth - 4) {
      left = Math.max(4, window.innerWidth - rect.width - 4)
    }
    if (top + rect.height > window.innerHeight - 4) {
      top = Math.max(4, window.innerHeight - rect.height - 4)
    }
    setPos({ left, top })
  }, [x, y])

  // Outside-click + Escape dismiss. Keyboard nav (Up/Down/Enter/Tab).
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node | null
      if (!t) return
      if (panelRef.current?.contains(t)) return
      onClose()
    }
    const moveActive = (delta: number) => {
      if (items.length === 0) return
      let next = activeIndex
      for (let i = 0; i < items.length; i++) {
        next = (next + delta + items.length) % items.length
        if (!items[next].disabled) {
          setActiveIndex(next)
          return
        }
      }
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        onClose()
        return
      }
      if (e.key === 'ArrowDown' || (e.key === 'Tab' && !e.shiftKey)) {
        e.preventDefault()
        moveActive(1)
        return
      }
      if (e.key === 'ArrowUp' || (e.key === 'Tab' && e.shiftKey)) {
        e.preventDefault()
        moveActive(-1)
        return
      }
      if (e.key === 'Enter' || e.key === ' ') {
        if (activeIndex < 0) return
        const item = items[activeIndex]
        if (!item || item.disabled) return
        e.preventDefault()
        onClose()
        void item.onSelect()
      }
    }
    document.addEventListener('mousedown', onDown, true)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDown, true)
      document.removeEventListener('keydown', onKey)
    }
  }, [activeIndex, items, onClose])

  return (
    <div
      ref={panelRef}
      role="menu"
      style={{ ...PANEL_STYLE, left: pos.left, top: pos.top }}
    >
      {items.map((item, i) => (
        <div key={item.id}>
          {item.separatorBefore && <div aria-hidden style={SEPARATOR_STYLE} />}
          <MenuRow
            item={item}
            active={i === activeIndex}
            onMouseEnter={() => {
              if (!item.disabled) setActiveIndex(i)
            }}
            onClick={() => {
              if (item.disabled) return
              onClose()
              void item.onSelect()
            }}
          />
        </div>
      ))}
    </div>
  )
}

function MenuRow({
  item,
  active,
  onMouseEnter,
  onClick,
}: {
  item: FilesContextMenuItem
  active: boolean
  onMouseEnter: () => void
  onClick: () => void
}) {
  return (
    <button
      type="button"
      role="menuitem"
      aria-disabled={item.disabled || undefined}
      onClick={onClick}
      onMouseEnter={onMouseEnter}
      style={{
        display: 'block',
        width: '100%',
        textAlign: 'left',
        padding: '5px 10px',
        border: 0,
        borderRadius: 4,
        background: active && !item.disabled
          ? 'var(--background-modifier-hover, var(--bg-hover, rgba(255,255,255,0.06)))'
          : 'transparent',
        color: item.disabled
          ? 'var(--text-faint, var(--fg-dim, #666))'
          : 'var(--text-normal, var(--fg, #ddd))',
        cursor: item.disabled ? 'default' : 'pointer',
        font: 'inherit',
      }}
    >
      {item.label}
    </button>
  )
}
