// Editor tab strip + "more" menu — ported from the deleted
// `nexus.titleBar` plugin when we distributed top-strip content into
// per-column top rows (Task 6).
//
// The surrounding `.workspace-tab-header-container` row in EditorView
// provides the Tauri drag region via `data-tauri-drag-region`; tab
// elements themselves must NOT carry the attribute (on Windows, Tauri 2
// eats click events inside a drag region).
//
// Naming note: the editor store already exports an `EditorTab` type
// (the data model). The header-row components are suffixed `Chip` /
// `Strip` to avoid collision with that type.
import { useEffect, useRef, useState } from 'react'
import { isDirty, type EditorTab } from './editorStore'
import { getEditorRuntime } from './runtime'
import { Icon } from '../../../icons'

export function EditorTabStrip({
  tabs,
  activeRelpath,
  onSelect,
  onClose,
  onNewTab,
}: {
  tabs: EditorTab[]
  activeRelpath: string | null
  onSelect: (relpath: string) => void
  onClose: (relpath: string) => void
  onNewTab: () => void
}) {
  return (
    <div
      style={{
        flex: '1 1 auto',
        display: 'flex',
        alignItems: 'stretch',
        minWidth: 0,
        height: '100%',
        zIndex: 1,
        paddingLeft: 8,
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'stretch',
          minWidth: 0,
          overflowX: 'auto',
          overflowY: 'hidden',
          scrollbarWidth: 'none',
          flex: '0 1 auto',
        }}
      >
        {tabs.map((tab) => (
          <EditorTabChip
            key={tab.relpath}
            tab={tab}
            active={tab.relpath === activeRelpath}
            onSelect={onSelect}
            onClose={onClose}
          />
        ))}
        <NewTabButton onClick={onNewTab} />
      </div>
      {/* Empty gap that eats remaining space so the tab menu stays
          flush right. Not marked draggable itself — the parent
          `.workspace-tab-header-container` row carries `data-tauri-drag-region`,
          which covers this empty area because a plain <div> with no
          click handler doesn't intercept pointer events above the
          parent's drag surface. Tabs above also carry no drag
          attribute so Tauri 2 on Windows doesn't swallow clicks. */}
      <div style={{ flex: '1 1 auto', minWidth: 12 }} />
      <EditorTabMenu
        tabs={tabs}
        activeRelpath={activeRelpath}
        onSelect={onSelect}
      />
    </div>
  )
}

function EditorTabChip({
  tab,
  active,
  onSelect,
  onClose,
}: {
  tab: EditorTab
  active: boolean
  onSelect: (relpath: string) => void
  onClose: (relpath: string) => void
}) {
  const dirty = isDirty(tab)
  return (
    <div
      role="tab"
      aria-selected={active}
      title={tab.relpath}
      onClick={() => onSelect(tab.relpath)}
      className={`workspace-tab-header${active ? ' is-active' : ''}`}
      data-type="markdown"
    >
      <div className="workspace-tab-header-inner">
        <div className="workspace-tab-header-inner-icon">
          <Icon name="doc" size={14} />
        </div>
        <div className="workspace-tab-header-inner-title">
          {tab.name}
          {dirty && (
            <span
              aria-hidden
              title="Unsaved changes"
              style={{
                width: 6,
                height: 6,
                borderRadius: '50%',
                background: 'var(--text-normal)',
                marginLeft: 6,
                display: 'inline-block',
              }}
            />
          )}
        </div>
        <button
          type="button"
          aria-label="Close"
          className="workspace-tab-header-inner-close-button"
          onClick={(e) => {
            e.stopPropagation()
            onClose(tab.relpath)
          }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          <Icon name="x" size={12} />
        </button>
      </div>
    </div>
  )
}

function NewTabButton({ onClick }: { onClick: () => void }) {
  const [hover, setHover] = useState(false)
  return (
    <button
      type="button"
      aria-label="New tab"
      title="New tab"
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        width: 28,
        height: '100%',
        padding: 0,
        border: 0,
        background: hover ? 'var(--bg-hover)' : 'transparent',
        color: hover ? 'var(--fg)' : 'var(--fg-muted)',
        cursor: 'pointer',
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        flexShrink: 0,
      }}
    >
      <Icon name="plus" size={14} />
    </button>
  )
}

function EditorTabMenu({
  tabs,
  activeRelpath,
  onSelect,
}: {
  tabs: EditorTab[]
  activeRelpath: string | null
  onSelect: (relpath: string) => void
}) {
  const [open, setOpen] = useState(false)
  const anchorRef = useRef<HTMLButtonElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node | null
      if (!t) return
      if (menuRef.current?.contains(t)) return
      if (anchorRef.current?.contains(t)) return
      setOpen(false)
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', onDown, true)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDown, true)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  const [hover, setHover] = useState(false)

  const stackTabs = () => {
    console.log('[nexus.editor] Stack tabs — not yet implemented')
    setOpen(false)
  }
  const bookmarkTabs = () => {
    console.log(`[nexus.editor] Bookmark ${tabs.length} tabs — not yet implemented`)
    setOpen(false)
  }
  const closeAll = () => {
    const rt = getEditorRuntime()
    setOpen(false)
    if (!rt) return
    void rt.closeAll()
  }
  const pickTab = (relpath: string) => {
    onSelect(relpath)
    setOpen(false)
  }

  return (
    <div style={{ position: 'relative', display: 'inline-flex', alignItems: 'center', flexShrink: 0 }}>
      <button
        ref={anchorRef}
        type="button"
        aria-label="Tab menu"
        title="Tab menu"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        onMouseEnter={() => setHover(true)}
        onMouseLeave={() => setHover(false)}
        style={{
          width: 28,
          height: 26,
          padding: 0,
          background: open ? 'var(--bg)' : hover ? 'var(--bg-hover)' : 'transparent',
          border: 0,
          color: open ? 'var(--fg)' : hover ? 'var(--fg)' : 'var(--fg-muted)',
          cursor: 'pointer',
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          borderRadius: 'var(--r)',
          marginRight: 4,
          alignSelf: 'center',
        }}
      >
        <span style={{ display: 'inline-flex', transform: 'rotate(90deg)' }}>
          <Icon name="chev" size={14} />
        </span>
      </button>
      {open && (
        <div
          ref={menuRef}
          role="menu"
          style={{
            position: 'absolute',
            top: '100%',
            right: 0,
            marginTop: 4,
            minWidth: 240,
            background: 'var(--bg-raised)',
            border: '1px solid var(--line)',
            borderRadius: 'var(--r)',
            boxShadow: '0 6px 24px rgba(0,0,0,0.4)',
            padding: '4px 0',
            zIndex: 20,
            fontSize: 12,
            color: 'var(--fg)',
          }}
        >
          <TabMenuItem label="Stack tabs" onClick={stackTabs} />
          <TabMenuItem label={`Bookmark ${tabs.length} tabs…`} onClick={bookmarkTabs} />
          <MenuDivider />
          <TabMenuItem label="Close all" onClick={closeAll} />
          {tabs.length > 0 && <MenuDivider />}
          {tabs.map((tab) => (
            <TabMenuItem
              key={tab.relpath}
              label={tab.name}
              selected={tab.relpath === activeRelpath}
              onClick={() => pickTab(tab.relpath)}
            />
          ))}
        </div>
      )}
    </div>
  )
}

function MenuDivider() {
  return (
    <div
      aria-hidden
      style={{ height: 1, background: 'var(--line-soft)', margin: '4px 0' }}
    />
  )
}

function TabMenuItem({
  label,
  selected,
  onClick,
}: {
  label: string
  selected?: boolean
  onClick: () => void
}) {
  const [hover, setHover] = useState(false)
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        width: '100%',
        border: 0,
        background: hover ? 'var(--bg-hover)' : 'transparent',
        color: selected ? 'var(--fg)' : 'var(--fg-muted)',
        textAlign: 'left',
        padding: '6px 10px 6px 24px',
        cursor: 'pointer',
        font: 'inherit',
        position: 'relative',
        whiteSpace: 'nowrap',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
      }}
    >
      {selected && (
        <span
          aria-hidden
          style={{ position: 'absolute', left: 8, display: 'inline-flex', color: 'var(--fg)' }}
        >
          <Icon name="check" size={12} />
        </span>
      )}
      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{label}</span>
    </button>
  )
}
