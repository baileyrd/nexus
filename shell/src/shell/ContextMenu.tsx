import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from 'react'
import { Icon, type IconName } from '../icons'
import { getRegistry } from '../host/shellRegistry'
import { zIndex } from './zIndex'

export type ContextMenuItem =
  | {
      kind: 'item'
      label: string
      commandId?: string
      onSelect?: () => void | Promise<void>
      disabled?: boolean
      tooltip?: string
      iconName?: IconName
      submenu?: ContextMenuItem[]
    }
  | { kind: 'separator' }
  | { kind: 'header'; label: string }

export interface ContextMenuProps {
  open: boolean
  anchorRect: DOMRect | null
  items: ContextMenuItem[]
  onClose: () => void
  align?: 'start' | 'end'
  minWidth?: number
}

const PANEL_STYLE: CSSProperties = {
  position: 'fixed',
  zIndex: zIndex.dropdown,
  background: 'var(--background-primary, #1e1e1e)',
  border: '1px solid var(--divider-color, var(--background-modifier-border, #333))',
  borderRadius: 6,
  boxShadow: '0 6px 20px rgba(0,0,0,0.35)',
  padding: 4,
  fontSize: 12,
  fontFamily: 'var(--font-interface)',
}

const SEPARATOR_STYLE: CSSProperties = {
  height: 1,
  background: 'var(--divider-color, var(--background-modifier-border, #333))',
  margin: '4px 0',
}

const HEADER_STYLE: CSSProperties = {
  padding: '4px 8px',
  color: 'var(--text-faint, #777)',
  fontSize: 11,
  textTransform: 'uppercase',
  letterSpacing: 0.5,
}

function MenuRow({
  item,
  onClose,
  onOpenSubmenu,
  onRequestCloseSubmenu,
  onCancelCloseSubmenu,
}: {
  item: Extract<ContextMenuItem, { kind: 'item' }>
  onClose: () => void
  onOpenSubmenu?: (rect: DOMRect, items: ContextMenuItem[]) => void
  onRequestCloseSubmenu?: () => void
  onCancelCloseSubmenu?: () => void
}) {
  const ref = useRef<HTMLButtonElement | null>(null)
  const hasSubmenu = Array.isArray(item.submenu) && item.submenu.length > 0
  const disabled = !!item.disabled
  const [hover, setHover] = useState(false)

  const handleClick = () => {
    if (disabled) return
    if (hasSubmenu) {
      const r = ref.current?.getBoundingClientRect()
      if (r && onOpenSubmenu) onOpenSubmenu(r, item.submenu!)
      return
    }
    onClose()
    if (item.commandId) {
      const reg = getRegistry()
      if (reg) void reg.commands.execute(item.commandId)
    }
    if (item.onSelect) void item.onSelect()
  }

  return (
    <button
      ref={ref}
      type="button"
      role="menuitem"
      title={item.tooltip}
      disabled={disabled}
      onClick={handleClick}
      onMouseEnter={() => {
        setHover(true)
        if (hasSubmenu && !disabled) {
          onCancelCloseSubmenu?.()
          const r = ref.current?.getBoundingClientRect()
          if (r && onOpenSubmenu) onOpenSubmenu(r, item.submenu!)
        } else {
          // Hovering a non-submenu row should retire any open submenu
          // from a sibling row, with the same grace window so the
          // user can briefly cross other rows on the way to it.
          onRequestCloseSubmenu?.()
        }
      }}
      onMouseLeave={() => {
        setHover(false)
        if (hasSubmenu && !disabled) onRequestCloseSubmenu?.()
      }}
      style={{
        display: 'flex',
        alignItems: 'center',
        width: '100%',
        padding: '6px 8px',
        background:
          hover && !disabled
            ? 'var(--background-modifier-hover, #2a2a2a)'
            : 'transparent',
        border: 'none',
        color: disabled
          ? 'var(--text-faint, #777)'
          : 'var(--text-normal, #ccc)',
        cursor: disabled ? 'default' : 'pointer',
        opacity: disabled ? 0.55 : 1,
        textAlign: 'left',
        borderRadius: 4,
        gap: 8,
        fontSize: 12,
      }}
    >
      <span style={{ width: 14, display: 'inline-flex', alignItems: 'center' }}>
        {item.iconName ? <Icon name={item.iconName} size={12} /> : null}
      </span>
      <span
        style={{
          flex: 1,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {item.label}
      </span>
      {hasSubmenu ? (
        <span style={{ display: 'inline-flex' }}>
          <Icon name="chev" size={12} />
        </span>
      ) : null}
    </button>
  )
}

interface SubmenuState {
  parentRect: DOMRect
  items: ContextMenuItem[]
}

export function ContextMenu({
  open,
  anchorRect,
  items,
  onClose,
  align = 'end',
  minWidth = 240,
}: ContextMenuProps) {
  const panelRef = useRef<HTMLDivElement | null>(null)
  const submenuRef = useRef<HTMLDivElement | null>(null)
  const [submenu, setSubmenu] = useState<SubmenuState | null>(null)
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null)
  const closeTimerRef = useRef<number | null>(null)

  const cancelCloseSubmenu = () => {
    if (closeTimerRef.current !== null) {
      window.clearTimeout(closeTimerRef.current)
      closeTimerRef.current = null
    }
  }
  const requestCloseSubmenu = () => {
    cancelCloseSubmenu()
    closeTimerRef.current = window.setTimeout(() => {
      closeTimerRef.current = null
      setSubmenu(null)
    }, 150)
  }

  useEffect(() => () => cancelCloseSubmenu(), [])

  useEffect(() => {
    if (!open) {
      cancelCloseSubmenu()
      setSubmenu(null)
    }
  }, [open])

  useEffect(() => {
    if (!open) return
    const onDocClick = (e: MouseEvent) => {
      const target = e.target as Node
      if (panelRef.current?.contains(target)) return
      if (submenuRef.current?.contains(target)) return
      onClose()
    }
    const onEscape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('mousedown', onDocClick)
    document.addEventListener('keydown', onEscape)
    return () => {
      document.removeEventListener('mousedown', onDocClick)
      document.removeEventListener('keydown', onEscape)
    }
  }, [open, onClose])

  useLayoutEffect(() => {
    if (!open || !anchorRect) {
      setPos(null)
      return
    }
    const margin = 4
    const panel = panelRef.current
    const panelW = panel?.offsetWidth ?? minWidth
    const panelH = panel?.offsetHeight ?? 0
    const top = Math.min(
      window.innerHeight - panelH - margin,
      anchorRect.bottom + margin,
    )
    const left =
      align === 'end'
        ? Math.max(margin, Math.min(window.innerWidth - panelW - margin, anchorRect.right - panelW))
        : Math.max(margin, Math.min(window.innerWidth - panelW - margin, anchorRect.left))
    setPos({ top, left })
  }, [open, anchorRect, items, align, minWidth])

  if (!open || !anchorRect) return null

  return (
    <>
      <div
        ref={panelRef}
        role="menu"
        style={{
          ...PANEL_STYLE,
          top: pos?.top ?? -9999,
          left: pos?.left ?? -9999,
          minWidth,
          visibility: pos ? 'visible' : 'hidden',
        }}
      >
        {items.map((item, i) => {
          if (item.kind === 'separator') {
            return <div key={`sep-${i}`} style={SEPARATOR_STYLE} />
          }
          if (item.kind === 'header') {
            return (
              <div key={`hdr-${i}`} style={HEADER_STYLE}>
                {item.label}
              </div>
            )
          }
          return (
            <MenuRow
              key={`item-${i}`}
              item={item}
              onClose={onClose}
              onOpenSubmenu={(rect, sub) => {
                cancelCloseSubmenu()
                setSubmenu({ parentRect: rect, items: sub })
              }}
              onRequestCloseSubmenu={requestCloseSubmenu}
              onCancelCloseSubmenu={cancelCloseSubmenu}
            />
          )
        })}
      </div>
      {submenu ? (
        <Submenu
          panelRef={submenuRef}
          parentRect={submenu.parentRect}
          items={submenu.items}
          onClose={onClose}
          onMouseEnter={cancelCloseSubmenu}
          onMouseLeave={requestCloseSubmenu}
        />
      ) : null}
    </>
  )
}

function Submenu({
  parentRect,
  items,
  onClose,
  panelRef,
  onMouseEnter,
  onMouseLeave,
}: {
  parentRect: DOMRect
  items: ContextMenuItem[]
  onClose: () => void
  panelRef: React.MutableRefObject<HTMLDivElement | null>
  onMouseEnter?: () => void
  onMouseLeave?: () => void
}) {
  const minWidth = 220
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null)

  useLayoutEffect(() => {
    const margin = 4
    const w = panelRef.current?.offsetWidth ?? minWidth
    const h = panelRef.current?.offsetHeight ?? 0
    let left = parentRect.right - 2
    if (left + w > window.innerWidth - margin) {
      left = Math.max(margin, parentRect.left - w + 2)
    }
    const top = Math.min(window.innerHeight - h - margin, parentRect.top)
    setPos({ top, left })
  }, [parentRect, items, panelRef])

  return (
    <div
      ref={panelRef}
      role="menu"
      onMouseEnter={onMouseEnter}
      onMouseLeave={onMouseLeave}
      style={{
        ...PANEL_STYLE,
        top: pos?.top ?? -9999,
        left: pos?.left ?? -9999,
        minWidth,
        visibility: pos ? 'visible' : 'hidden',
      }}
    >
      {items.map((item, i) => {
        if (item.kind === 'separator') {
          return <div key={`sep-${i}`} style={SEPARATOR_STYLE} />
        }
        if (item.kind === 'header') {
          return (
            <div key={`hdr-${i}`} style={HEADER_STYLE}>
              {item.label}
            </div>
          )
        }
        return <MenuRow key={`item-${i}`} item={item} onClose={onClose} />
      })}
    </div>
  )
}
