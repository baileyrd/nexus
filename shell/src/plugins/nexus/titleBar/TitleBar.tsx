import { useEffect, useRef, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useEditorStore, isDirty, type EditorTab } from '../editor/editorStore'
import { getEditorRuntime } from '../editor/runtime'
import { useLayoutStore } from '../../../stores/layoutStore'
import { Icon } from '../../../icons'
import { getApi } from './runtime'

const FILES_FOCUS_COMMAND = 'nexus.files.focus'
const SEARCH_FOCUS_COMMAND = 'nexus.search.focus'
const BOOKMARKS_FOCUS_COMMAND = 'nexus.bookmarks.focus'
const BACKLINKS_FOCUS_COMMAND = 'nexus.backlinks.focus'
const OUTGOING_LINKS_FOCUS_COMMAND = 'nexus.outgoingLinks.focus'
const TAGS_FOCUS_COMMAND = 'nexus.tags.focus'
const ALL_PROPERTIES_FOCUS_COMMAND = 'nexus.allProperties.focus'
const OUTLINE_FOCUS_COMMAND = 'nexus.outline.focus'
const NEW_UNTITLED_COMMAND = 'nexus.editor.newUntitled'

const baseControlStyle: React.CSSProperties = {
  width: 40,
  height: 36,
  background: 'transparent',
  border: 'none',
  color: 'var(--fg-muted)',
  cursor: 'pointer',
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  padding: 0,
  transition: 'background 0.08s, color 0.08s',
}

function ControlButton({
  onClick,
  label,
  closeAccent,
  children,
}: {
  onClick: () => void
  label: string
  closeAccent?: boolean
  children: React.ReactNode
}) {
  const [hover, setHover] = useState(false)
  // Windows convention: close button hovers to red (literal so it lands
  // unambiguously regardless of theme-token availability); other controls
  // use a subtle raised background from the token palette.
  const hoverBg = closeAccent ? '#e81123' : 'var(--bg-hover)'
  const hoverFg = closeAccent ? '#ffffff' : 'var(--fg)'
  const style: React.CSSProperties = {
    ...baseControlStyle,
    background: hover ? hoverBg : 'transparent',
    color: hover ? hoverFg : 'var(--fg-muted)',
  }
  return (
    <button
      type="button"
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      aria-label={label}
      title={label}
      style={style}
    >
      {children}
    </button>
  )
}

function MinimizeIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
      <rect x="0" y="4.5" width="10" height="1" fill="currentColor" />
    </svg>
  )
}

function MaximizeIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
      <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" />
    </svg>
  )
}

function RestoreIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
      <rect x="2.5" y="0.5" width="7" height="7" fill="none" stroke="currentColor" />
      <rect x="0.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" />
    </svg>
  )
}

function CloseIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
      <line x1="0" y1="0" x2="10" y2="10" stroke="currentColor" />
      <line x1="10" y1="0" x2="0" y2="10" stroke="currentColor" />
    </svg>
  )
}

function ClusterButton({
  onClick,
  label,
  active,
  children,
}: {
  onClick: () => void
  label: string
  active?: boolean
  children: React.ReactNode
}) {
  const [hover, setHover] = useState(false)
  return (
    <button
      type="button"
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      aria-label={label}
      title={label}
      aria-pressed={active}
      style={{
        width: 28,
        height: 26,
        padding: 0,
        background: active ? 'var(--bg)' : hover ? 'var(--bg-hover)' : 'transparent',
        border: 0,
        color: active ? 'var(--fg)' : hover ? 'var(--fg)' : 'var(--fg-muted)',
        cursor: 'pointer',
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        borderRadius: 'var(--r)',
        transition: 'background 0.08s, color 0.08s',
      }}
    >
      {children}
    </button>
  )
}

export function TitleBar() {
  const tabs = useEditorStore((s) => s.tabs)
  const activeRelpath = useEditorStore((s) => s.activeRelpath)
  const setActive = useEditorStore((s) => s.setActive)
  const sidebarVisible = useLayoutStore((s) => s.sidebar.visible)
  const toggleSidebar = useLayoutStore((s) => s.toggleSidebar)
  const rightPanelVisible = useLayoutStore((s) => s.rightPanel.visible)
  const toggleRightPanel = useLayoutStore((s) => s.toggleRightPanel)
  const [maximized, setMaximized] = useState(false)

  useEffect(() => {
    const w = getCurrentWindow()
    let unlisten: (() => void) | undefined
    let cancelled = false
    ;(async () => {
      try {
        const current = await w.isMaximized()
        if (!cancelled) setMaximized(current)
        unlisten = await w.onResized(async () => {
          const now = await w.isMaximized()
          setMaximized(now)
        })
      } catch (err) {
        console.warn('[nexus.titleBar] failed to wire maximize listener:', err)
      }
    })()
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const minimize = () => getCurrentWindow().minimize()
  const toggleMaximize = () => getCurrentWindow().toggleMaximize()
  const close = () => getCurrentWindow().close()

  const execute = (commandId: string) => () => {
    const api = getApi()
    if (!api) return
    void api.commands.execute(commandId)
  }

  const requestCloseTab = (relpath: string) => {
    const rt = getEditorRuntime()
    if (!rt) return
    void rt.confirmAndClose(relpath)
  }

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        height: '100%',
        width: '100%',
        position: 'relative',
        userSelect: 'none',
        color: 'var(--fg-muted)',
        fontSize: 'var(--ui-size, 12px)',
        paddingLeft: 6,
      }}
    >
      {/* Left cluster — non-draggable */}
      <div style={{ flexShrink: 0, zIndex: 1 }}>
        <LeftCluster
          sidebarVisible={sidebarVisible}
          onToggleSidebar={toggleSidebar}
          onFiles={execute(FILES_FOCUS_COMMAND)}
          onSearch={execute(SEARCH_FOCUS_COMMAND)}
          onBookmarks={execute(BOOKMARKS_FOCUS_COMMAND)}
        />
      </div>

      {/* Tab strip — flex-grows to fill the middle. */}
      <TabStrip
        tabs={tabs}
        activeRelpath={activeRelpath}
        onSelect={setActive}
        onClose={requestCloseTab}
        onNewTab={execute(NEW_UNTITLED_COMMAND)}
      />

      {/* Right cluster — non-draggable */}
      <div style={{ flexShrink: 0, display: 'flex', alignItems: 'center', zIndex: 1 }}>
        <TabMenu
          tabs={tabs}
          activeRelpath={activeRelpath}
          onSelect={setActive}
        />
        <RightCluster
          rightPanelVisible={rightPanelVisible}
          onToggleRightPanel={toggleRightPanel}
          onBacklinks={execute(BACKLINKS_FOCUS_COMMAND)}
          onOutgoingLinks={execute(OUTGOING_LINKS_FOCUS_COMMAND)}
          onTags={execute(TAGS_FOCUS_COMMAND)}
          onAllProperties={execute(ALL_PROPERTIES_FOCUS_COMMAND)}
          onOutline={execute(OUTLINE_FOCUS_COMMAND)}
        />
        <ControlButton onClick={minimize} label="Minimize">
          <MinimizeIcon />
        </ControlButton>
        <ControlButton
          onClick={toggleMaximize}
          label={maximized ? 'Restore' : 'Maximize'}
        >
          {maximized ? <RestoreIcon /> : <MaximizeIcon />}
        </ControlButton>
        <ControlButton onClick={close} label="Close" closeAccent>
          <CloseIcon />
        </ControlButton>
      </div>
    </div>
  )
}

function LeftCluster({
  sidebarVisible,
  onToggleSidebar,
  onFiles,
  onSearch,
  onBookmarks,
}: {
  sidebarVisible: boolean
  onToggleSidebar: () => void
  onFiles: () => void
  onSearch: () => void
  onBookmarks: () => void
}) {
  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 2,
        padding: '0 4px',
        background: 'var(--bg-raised)',
        border: '1px solid var(--line-soft)',
        borderRadius: 'var(--r)',
        height: 28,
      }}
    >
      <ClusterButton
        onClick={onToggleSidebar}
        label={sidebarVisible ? 'Hide sidebar' : 'Show sidebar'}
        active={sidebarVisible}
      >
        <Icon name="panelLeft" size={14} />
      </ClusterButton>
      {sidebarVisible && (
        <>
          <ClusterButton onClick={onFiles} label="Files">
            <Icon name="folder" size={14} />
          </ClusterButton>
          <ClusterButton onClick={onSearch} label="Search">
            <Icon name="search" size={14} />
          </ClusterButton>
          <ClusterButton onClick={onBookmarks} label="Bookmarks">
            <Icon name="book" size={14} />
          </ClusterButton>
        </>
      )}
    </div>
  )
}

function TabStrip({
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
          <TitleBarTab
            key={tab.relpath}
            tab={tab}
            active={tab.relpath === activeRelpath}
            onSelect={onSelect}
            onClose={onClose}
          />
        ))}
        <NewTabButton onClick={onNewTab} />
      </div>
      {/* Empty gap that eats remaining space — draggable. Tabs above
          must NOT carry the drag-region attribute: on Windows, Tauri 2
          swallows click events inside a drag region so the tab onClick
          would never fire. */}
      <div data-tauri-drag-region style={{ flex: '1 1 auto', minWidth: 12 }} />
    </div>
  )
}

function TitleBarTab({
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
  const [hover, setHover] = useState(false)
  const dirty = isDirty(tab)
  const bg = active ? 'var(--bg)' : hover ? 'var(--bg-hover)' : 'transparent'
  const fg = active ? 'var(--fg)' : 'var(--fg-muted)'

  return (
    <div
      role="tab"
      aria-selected={active}
      title={tab.relpath}
      onClick={() => onSelect(tab.relpath)}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 6,
        padding: '0 8px 0 10px',
        height: '100%',
        borderRight: '1px solid var(--line-soft)',
        cursor: 'pointer',
        whiteSpace: 'nowrap',
        flexShrink: 0,
        maxWidth: 220,
        minWidth: 80,
        background: bg,
        color: fg,
        position: 'relative',
      }}
    >
      {active && (
        <span
          aria-hidden
          style={{
            position: 'absolute',
            left: 0,
            right: 0,
            bottom: 0,
            height: 2,
            background: 'var(--accent)',
          }}
        />
      )}
      <span
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          fontWeight: active ? 500 : 400,
          minWidth: 0,
        }}
      >
        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{tab.name}</span>
        {dirty && (
          <span
            aria-hidden
            title="Unsaved changes"
            style={{
              width: 6,
              height: 6,
              borderRadius: '50%',
              background: 'var(--fg)',
              marginLeft: 6,
              flexShrink: 0,
            }}
          />
        )}
      </span>
      {active ? (
        <TabCloseButton onClick={(e) => {
          e.stopPropagation()
          onClose(tab.relpath)
        }} />
      ) : (
        <span style={{ width: 16, height: 16, flexShrink: 0 }} />
      )}
    </div>
  )
}

function TabCloseButton({ onClick }: { onClick: (e: React.MouseEvent) => void }) {
  const [hover, setHover] = useState(false)
  return (
    <button
      type="button"
      aria-label="Close"
      onClick={onClick}
      onMouseDown={(e) => e.stopPropagation()}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        width: 16,
        height: 16,
        padding: 0,
        border: 0,
        background: hover ? 'var(--bg-hover)' : 'transparent',
        color: 'inherit',
        cursor: 'pointer',
        borderRadius: 'var(--r)',
        flexShrink: 0,
      }}
    >
      <Icon name="x" size={12} />
    </button>
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

function TabMenu({
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
    console.log('[nexus.titleBar] Stack tabs — not yet implemented')
    setOpen(false)
  }
  const bookmarkTabs = () => {
    console.log(`[nexus.titleBar] Bookmark ${tabs.length} tabs — not yet implemented`)
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
    <div style={{ position: 'relative', display: 'inline-flex', alignItems: 'center' }}>
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

function RightCluster({
  rightPanelVisible,
  onToggleRightPanel,
  onBacklinks,
  onOutgoingLinks,
  onTags,
  onAllProperties,
  onOutline,
}: {
  rightPanelVisible: boolean
  onToggleRightPanel: () => void
  onBacklinks: () => void
  onOutgoingLinks: () => void
  onTags: () => void
  onAllProperties: () => void
  onOutline: () => void
}) {
  return (
    <div
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 2,
        padding: '0 4px',
        height: 28,
        marginRight: 6,
      }}
    >
      <ClusterButton
        onClick={onToggleRightPanel}
        label={rightPanelVisible ? 'Hide right panel' : 'Show right panel'}
        active={rightPanelVisible}
      >
        <Icon name="panel" size={14} />
      </ClusterButton>
      {rightPanelVisible && (
        <>
          <ClusterButton onClick={onBacklinks} label="Backlinks">
            <Icon name="linkIn" size={14} />
          </ClusterButton>
          <ClusterButton onClick={onOutgoingLinks} label="Outgoing links">
            <Icon name="linkOut" size={14} />
          </ClusterButton>
          <ClusterButton onClick={onTags} label="Tags">
            <Icon name="tag" size={14} />
          </ClusterButton>
          <ClusterButton onClick={onAllProperties} label="All properties">
            <Icon name="archive" size={14} />
          </ClusterButton>
          <ClusterButton onClick={onOutline} label="Outline">
            <Icon name="list" size={14} />
          </ClusterButton>
        </>
      )}
    </div>
  )
}
