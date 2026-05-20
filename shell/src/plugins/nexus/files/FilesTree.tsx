import { useEffect, useMemo, useRef, useState, type DragEvent as ReactDragEvent, type MouseEvent as ReactMouseEvent } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useFilesStore, type FilesDirEntry, type SortMode } from './filesStore'
import { clientLogger } from '../../../clientLogger'
import { useWorkspaceStore } from '../workspace/workspaceStore'
import { useEditorStore } from '../editor/editorStore'
import { createDir, createFile, getKernel, loadChildren, renameEntry } from './kernelClient'
import { StatusDot } from './status/StatusPill'
import { useFileStatus } from './status/useFileStatus'
import { Icon } from '../../../icons'
import { getFileIcon } from './fileIcon'
import { getApi } from './runtime'
import { NavActionButton, NavButtonsContainer, NavHeader } from '../../../primitives/NavHeader'
import { FilesContextMenu, type FilesContextMenuItem } from './ContextMenu'
import { flattenTree, isBundleDir } from './flattenTree'

const DRAG_MIME = 'application/x-nexus-relpath'
const CONTEXT_KEY_FOCUSED = 'nexus.files.focused'

interface FilesTreeProps {
  onFileActivate: (entry: FilesDirEntry) => void
}

const INDENT_PX = 14
const ROOT_RELPATH = ''
// Row height matches the rendered row: padding 3+3 + line-height 18.
// Kept fixed so the virtualizer doesn't need per-row measurement.
const ROW_HEIGHT = 24

/** Forge-relative parent of a relpath. `""` → `""`. Forward-slash only. */
function parentRelpath(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? '' : relpath.slice(0, i)
}

/** All ancestor relpaths of `relpath`, outermost first, excluding the
 *  root sentinel `""` (which is always "expanded" implicitly). */
function ancestors(relpath: string): string[] {
  const out: string[] = []
  let cur = parentRelpath(relpath)
  while (cur !== '') {
    out.unshift(cur)
    cur = parentRelpath(cur)
  }
  return out
}

/** What was right-clicked: a specific entry, or the empty container
 *  (target=null = "add to root"). */
interface MenuTarget {
  entry: FilesDirEntry | null
  x: number
  y: number
}

/**
 * Closure plumbed down to each TreeRow so it can request the menu
 * without prop-drilling. Uses module-scope React state via a singleton
 * setter installed by `<FilesTree>` on mount; this avoids a context
 * provider for a one-component-deep concern.
 */
let openMenuRef: ((t: MenuTarget) => void) | null = null

export function FilesTree({ onFileActivate }: FilesTreeProps) {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const childrenCache = useFilesStore((s) => s.children)
  const expandedSet = useFilesStore((s) => s.expanded)
  const setChildren = useFilesStore((s) => s.setChildren)
  const sortMode = useFilesStore((s) => s.sortMode)
  const autoReveal = useFilesStore((s) => s.autoReveal)
  const selected = useFilesStore((s) => s.selected)
  const setSelected = useFilesStore((s) => s.setSelected)
  const setExpanded = useFilesStore((s) => s.setExpanded)
  const collapseAll = useFilesStore((s) => s.collapseAll)
  const setSortMode = useFilesStore((s) => s.setSortMode)
  const setAutoReveal = useFilesStore((s) => s.setAutoReveal)
  const activeRelpath = useEditorStore((s) => s.activeRelpath)

  const rootEntries = childrenCache[ROOT_RELPATH]

  const [menu, setMenu] = useState<MenuTarget | null>(null)
  const containerRef = useRef<HTMLDivElement | null>(null)
  const scrollRef = useRef<HTMLDivElement | null>(null)

  // Install the menu-open callback while this tree is mounted. Pair
  // with cleanup so a remount doesn't leave a stale closure that
  // updates an unmounted component's state.
  useEffect(() => {
    openMenuRef = (t) => setMenu(t)
    return () => {
      openMenuRef = null
    }
  }, [])

  // Track focus to drive the `nexus.files.focused` context key. The
  // keybindings registered in the plugin manifest gate Del/F2 on this
  // key, so when the user is typing in the editor the shortcuts stay
  // out of the way. We watch DOM focus on the container with capture,
  // covering inner buttons too.
  useEffect(() => {
    const api = getApi()
    if (!api) return
    const el = containerRef.current
    if (!el) return
    const onFocusIn = () => api.context.set(CONTEXT_KEY_FOCUSED, true)
    const onFocusOut = (e: FocusEvent) => {
      const next = e.relatedTarget as Node | null
      if (next && el.contains(next)) return
      api.context.set(CONTEXT_KEY_FOCUSED, false)
    }
    el.addEventListener('focusin', onFocusIn)
    el.addEventListener('focusout', onFocusOut)
    return () => {
      el.removeEventListener('focusin', onFocusIn)
      el.removeEventListener('focusout', onFocusOut)
      api.context.set(CONTEXT_KEY_FOCUSED, false)
    }
  }, [])

  useEffect(() => {
    if (!rootPath) return
    if (rootEntries) return
    loadChildren(ROOT_RELPATH).then((entries) =>
      setChildren(ROOT_RELPATH, entries),
    )
  }, [rootPath, rootEntries, setChildren])

  // Auto-reveal: whenever the active editor file changes and the user
  // has the flag on, expand every ancestor directory and select the
  // file. The scroll-to is handled by the virtualizer effect below
  // once the flat list rebuilds with the file in place.
  useEffect(() => {
    if (!autoReveal) return
    if (!activeRelpath) return
    for (const dir of ancestors(activeRelpath)) {
      setExpanded(dir, true)
      const cached = useFilesStore.getState().children[dir]
      if (!cached) {
        loadChildren(dir).then((entries) =>
          useFilesStore.getState().setChildren(dir, entries),
        )
      }
    }
    setSelected(activeRelpath)
  }, [autoReveal, activeRelpath, setExpanded, setSelected])

  const flatRows = useMemo(
    () =>
      rootEntries
        ? flattenTree(rootEntries, childrenCache, expandedSet, sortMode)
        : [],
    [rootEntries, childrenCache, expandedSet, sortMode],
  )

  const virtualizer = useVirtualizer({
    count: flatRows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 8,
  })

  // Scroll the selected row into view through the virtualizer.
  // Runs on every selection change (matches the pre-virtualization
  // `scrollIntoView` behaviour) and after auto-reveal repopulates the
  // flat list with the freshly-revealed file.
  useEffect(() => {
    if (!selected) return
    const idx = flatRows.findIndex((r) => r.entry.relpath === selected)
    if (idx < 0) return
    virtualizer.scrollToIndex(idx, { align: 'auto' })
  }, [selected, flatRows, virtualizer])

  if (!rootPath) {
    return (
      <div
        style={{
          padding: '12px 14px',
          color: 'var(--text-faint)',
          fontSize: 'var(--ui-size, 12px)',
        }}
      >
        No workspace open. Press Ctrl+O to pick a folder.
      </div>
    )
  }

  // Determine the parent dir for new-file / new-folder: the selected
  // directory itself, the selected file's parent, or the root.
  const parentForNew = (): string => {
    if (!selected) return ''
    const entries = rootEntries ?? []
    const match = findEntry(entries, selected, useFilesStore.getState().children)
    if (match?.isDir) return match.relpath
    return parentRelpath(selected)
  }

  const refreshParent = async (parent: string) => {
    const entries = await loadChildren(parent)
    setChildren(parent, entries)
  }

  const handleNewFile = async () => {
    const api = getApi()
    if (!api) return
    const name = await api.input.prompt('New note name')
    if (!name) return
    const trimmed = name.trim()
    if (!trimmed) return
    const withExt = /\.[^/\\]+$/.test(trimmed) ? trimmed : `${trimmed}.md`
    const parent = parentForNew()
    const relpath = parent ? `${parent}/${withExt}` : withExt
    try {
      await createFile(relpath)
      setExpanded(parent, true)
      await refreshParent(parent)
      setSelected(relpath)
    } catch (err) {
      clientLogger.warn('[nexus.files] create_file failed:', err)
      await api.input.confirm(`Failed to create "${withExt}": ${(err as Error).message ?? err}`)
    }
  }

  const handleNewBase = async () => {
    const api = getApi()
    if (!api) return
    await api.commands.execute('nexus.bases.new', { parent: parentForNew() })
  }

  const handleNewFolder = async () => {
    const api = getApi()
    if (!api) return
    const name = await api.input.prompt('New folder name')
    if (!name) return
    const trimmed = name.trim()
    if (!trimmed) return
    const parent = parentForNew()
    const relpath = parent ? `${parent}/${trimmed}` : trimmed
    try {
      await createDir(relpath)
      setExpanded(parent, true)
      setExpanded(relpath, true)
      await refreshParent(parent)
    } catch (err) {
      clientLogger.warn('[nexus.files] create_dir failed:', err)
      await api.input.confirm(`Failed to create "${trimmed}": ${(err as Error).message ?? err}`)
    }
  }

  const handleToggleAutoReveal = () => {
    setAutoReveal(!autoReveal)
  }

  // Right-click on the empty area (not on a row) → "root" menu. Rows
  // call `e.stopPropagation()` in their own context-menu handler, so
  // this only fires when the click landed outside any row.
  const handleContainerContextMenu = (e: ReactMouseEvent<HTMLDivElement>) => {
    e.preventDefault()
    setMenu({ entry: null, x: e.clientX, y: e.clientY })
  }

  // Drop on the empty area = move into the root. Rows that catch a
  // drop call `e.stopPropagation()` so this handler only sees drops
  // that landed outside any row.
  const handleContainerDragOver = (e: ReactDragEvent<HTMLDivElement>) => {
    if (!e.dataTransfer.types.includes(DRAG_MIME)) return
    e.preventDefault()
    e.dataTransfer.dropEffect = 'move'
  }
  const handleContainerDrop = async (e: ReactDragEvent<HTMLDivElement>) => {
    const from = e.dataTransfer.getData(DRAG_MIME)
    if (!from) return
    e.preventDefault()
    await moveEntry(from, '')
  }

  const items = menu ? buildMenuItems(menu.entry) : []

  const virtualRows = virtualizer.getVirtualItems()
  const totalHeight = virtualizer.getTotalSize()

  return (
    <div
      ref={containerRef}
      style={{ display: 'flex', flexDirection: 'column', width: '100%', height: '100%', overflow: 'hidden' }}
    >
      <Toolbar
        sortMode={sortMode}
        autoReveal={autoReveal}
        onNewFile={handleNewFile}
        onNewFolder={handleNewFolder}
        onNewBase={handleNewBase}
        onPickSort={setSortMode}
        onToggleAutoReveal={handleToggleAutoReveal}
        onCollapseAll={collapseAll}
      />

      <div
        ref={scrollRef}
        className="nav-files-container"
        onContextMenu={handleContainerContextMenu}
        onDragOver={handleContainerDragOver}
        onDrop={handleContainerDrop}
      >
        {!rootEntries ? (
          <div style={{ padding: '12px 14px', color: 'var(--text-faint)' }}>Loading…</div>
        ) : (
          <div style={{ position: 'relative', height: totalHeight, width: '100%' }}>
            {virtualRows.map((vr) => {
              const row = flatRows[vr.index]
              if (!row) return null
              return (
                <div
                  key={row.entry.relpath}
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    right: 0,
                    height: ROW_HEIGHT,
                    transform: `translateY(${vr.start}px)`,
                  }}
                >
                  <TreeRow
                    entry={row.entry}
                    depth={row.depth}
                    rootPath={rootPath}
                    onFileActivate={onFileActivate}
                  />
                </div>
              )
            })}
          </div>
        )}
      </div>

      {menu && (
        <FilesContextMenu
          x={menu.x}
          y={menu.y}
          items={items}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  )
}

function buildMenuItems(target: FilesDirEntry | null): FilesContextMenuItem[] {
  const api = getApi()
  if (!api) return []
  const parent =
    target === null
      ? ''
      : target.isDir
        ? target.relpath
        : parentRelpath(target.relpath)

  const items: FilesContextMenuItem[] = [
    {
      id: 'new-file',
      label: 'New note',
      onSelect: () => void api.commands.execute('nexus.files.create.file', { parent }),
    },
    {
      id: 'new-folder',
      label: 'New folder',
      onSelect: () => void api.commands.execute('nexus.files.create.folder', { parent }),
    },
    {
      id: 'new-canvas',
      label: 'New canvas',
      onSelect: () => void api.commands.execute('nexus.canvas.new', { parent }),
    },
    {
      id: 'new-base',
      label: 'New base',
      onSelect: () => void api.commands.execute('nexus.bases.new', { parent }),
    },
  ]

  if (target) {
    items.push({
      id: 'rename',
      label: 'Rename',
      separatorBefore: true,
      onSelect: () => void api.commands.execute('nexus.files.rename', { relpath: target.relpath }),
    })
    items.push({
      id: 'delete',
      label: target.isDir ? 'Delete Folder' : 'Delete',
      onSelect: () => void api.commands.execute('nexus.files.delete', { relpath: target.relpath }),
    })
  }

  items.push({
    id: 'reveal',
    label: 'Reveal in OS',
    separatorBefore: true,
    onSelect: () =>
      void api.commands.execute(
        'nexus.files.reveal',
        target ? { relpath: target.relpath } : {},
      ),
  })

  if (target) {
    items.push({
      id: 'copy-path',
      label: 'Copy Path',
      onSelect: () => void api.commands.execute('nexus.files.copyPath', { relpath: target.relpath }),
    })
  }

  return items
}

/**
 * Drag-drop move helper. Validates the move against three legal-edge
 * constraints (mirrors the legacy tree's behavior + common-sense),
 * then calls `rename_entry`. Returns silently for no-op moves.
 */
async function moveEntry(from: string, targetDir: string): Promise<void> {
  if (!from) return
  if (from === targetDir) return
  const fromParent = parentRelpath(from)
  if (fromParent === targetDir) return
  if (targetDir === from || targetDir.startsWith(`${from}/`)) return

  const name = from.slice(from.lastIndexOf('/') + 1) || from
  const dst = targetDir ? `${targetDir}/${name}` : name
  if (dst === from) return

  try {
    await renameEntry(from, dst)
    const entries1 = await loadChildren(fromParent)
    useFilesStore.getState().setChildren(fromParent, entries1)
    if (targetDir !== fromParent) {
      const entries2 = await loadChildren(targetDir)
      useFilesStore.getState().setChildren(targetDir, entries2)
    }
    useFilesStore.getState().setSelected(dst)
  } catch (err) {
    const api = getApi()
    api?.notifications.show({
      type: 'error',
      message: `Move failed: ${err instanceof Error ? err.message : String(err)}`,
    })
  }
}

/** Walk the cached tree to resolve a relpath to its entry. Returns
 *  null when any segment along the path is missing from the cache. */
function findEntry(
  rootEntries: FilesDirEntry[],
  relpath: string,
  cache: Record<string, FilesDirEntry[]>,
): FilesDirEntry | null {
  if (!relpath) return null
  const segments = relpath.split('/')
  let current: FilesDirEntry[] | undefined = rootEntries
  let path = ''
  for (let i = 0; i < segments.length; i++) {
    const seg = segments[i]
    if (!current) return null
    const next: FilesDirEntry | undefined = current.find((e) => e.name === seg)
    if (!next) return null
    if (i === segments.length - 1) return next
    path = path ? `${path}/${seg}` : seg
    current = cache[path]
  }
  return null
}

function Toolbar({
  sortMode,
  autoReveal,
  onNewFile,
  onNewFolder,
  onNewBase,
  onPickSort,
  onToggleAutoReveal,
  onCollapseAll,
}: {
  sortMode: SortMode
  autoReveal: boolean
  onNewFile: () => void
  onNewFolder: () => void
  onNewBase: () => void
  onPickSort: (mode: SortMode) => void
  onToggleAutoReveal: () => void
  onCollapseAll: () => void
}) {
  const [sortMenuOpen, setSortMenuOpen] = useState(false)
  const sortBtnRef = useRef<HTMLButtonElement>(null)

  return (
    <NavHeader
      style={{
        position: 'relative',
        padding: '0 var(--size-4-2)',
        height: 'var(--header-height)',
        flexShrink: 0,
        flexDirection: 'row',
        alignItems: 'center',
        borderBottom: '1px solid var(--divider-color)',
      }}
    >
      <NavButtonsContainer>
        <NavActionButton
          label="New note"
          icon={<Icon name="filePlus" size={14} />}
          onClick={onNewFile}
        />
        <NavActionButton
          label="New folder"
          icon={<Icon name="folderPlus" size={14} />}
          onClick={onNewFolder}
        />
        <NavActionButton
          label="New base"
          icon={<Icon name="db" size={14} />}
          onClick={onNewBase}
        />
        <NavActionButton
          ref={sortBtnRef}
          label="Change sort order"
          icon={<Icon name="sortAZ" size={14} />}
          active={sortMenuOpen}
          onClick={() => setSortMenuOpen((v) => !v)}
        />
        <NavActionButton
          label={autoReveal ? 'Auto-reveal: on' : 'Auto-reveal current file'}
          icon={<Icon name="crosshair" size={14} />}
          active={autoReveal}
          onClick={onToggleAutoReveal}
        />
        <NavActionButton
          label="Collapse all"
          icon={<Icon name="collapseAll" size={14} />}
          onClick={onCollapseAll}
        />
      </NavButtonsContainer>

      {sortMenuOpen && (
        <SortMenu
          sortMode={sortMode}
          anchorRef={sortBtnRef}
          onPick={(mode) => {
            onPickSort(mode)
            setSortMenuOpen(false)
          }}
          onDismiss={() => setSortMenuOpen(false)}
        />
      )}
    </NavHeader>
  )
}

const SORT_OPTIONS: ReadonlyArray<{ mode: SortMode; label: string; group: number }> = [
  { mode: 'nameAsc', label: 'File name (A to Z)', group: 0 },
  { mode: 'nameDesc', label: 'File name (Z to A)', group: 0 },
  { mode: 'modifiedDesc', label: 'Modified time (new to old)', group: 1 },
  { mode: 'modifiedAsc', label: 'Modified time (old to new)', group: 1 },
  { mode: 'createdDesc', label: 'Created time (new to old)', group: 2 },
  { mode: 'createdAsc', label: 'Created time (old to new)', group: 2 },
]

function SortMenu({
  sortMode,
  anchorRef,
  onPick,
  onDismiss,
}: {
  sortMode: SortMode
  anchorRef: React.RefObject<HTMLButtonElement>
  onPick: (mode: SortMode) => void
  onDismiss: () => void
}) {
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node | null
      if (!t) return
      if (menuRef.current?.contains(t)) return
      if (anchorRef.current?.contains(t)) return
      onDismiss()
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onDismiss()
    }
    document.addEventListener('mousedown', onDown, true)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDown, true)
      document.removeEventListener('keydown', onKey)
    }
  }, [anchorRef, onDismiss])

  return (
    <div
      ref={menuRef}
      role="menu"
      style={{
        position: 'absolute',
        top: '100%',
        right: 8,
        marginTop: 4,
        minWidth: 220,
        background: 'var(--background-secondary)',
        border: '1px solid var(--background-modifier-border)',
        borderRadius: 'var(--radius-s)',
        boxShadow: '0 6px 24px rgba(0,0,0,0.4)',
        padding: '4px 0',
        zIndex: 10,
        fontSize: 12,
      }}
    >
      {SORT_OPTIONS.map((opt, i) => {
        const prev = SORT_OPTIONS[i - 1]
        const divider = prev && prev.group !== opt.group
        return (
          <div key={opt.mode}>
            {divider && (
              <div
                aria-hidden
                style={{ height: 1, background: 'var(--divider-color)', margin: '4px 0' }}
              />
            )}
            <SortMenuItem
              label={opt.label}
              selected={sortMode === opt.mode}
              onClick={() => onPick(opt.mode)}
            />
          </div>
        )
      })}
    </div>
  )
}

function SortMenuItem({
  label,
  selected,
  onClick,
}: {
  label: string
  selected: boolean
  onClick: () => void
}) {
  const [hover, setHover] = useState(false)
  return (
    <button
      type="button"
      role="menuitemradio"
      aria-checked={selected}
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        width: '100%',
        border: 0,
        background: hover ? 'var(--background-modifier-hover)' : 'transparent',
        color: selected ? 'var(--text-normal)' : 'var(--text-muted)',
        textAlign: 'left',
        padding: '6px 10px 6px 24px',
        cursor: 'pointer',
        font: 'inherit',
        position: 'relative',
      }}
    >
      {selected && (
        <span
          aria-hidden
          style={{ position: 'absolute', left: 8, display: 'inline-flex', color: 'var(--text-normal)' }}
        >
          <Icon name="check" size={12} />
        </span>
      )}
      {label}
    </button>
  )
}

/** Single virtualized row. Owns its own drop-hover state and handles
 *  click / context-menu / drag-drop directly — no recursion into
 *  children, since the parent flattens the whole visible tree before
 *  handing it to the virtualizer. */
function TreeRow({
  entry,
  depth,
  rootPath,
  onFileActivate,
}: {
  entry: FilesDirEntry
  depth: number
  rootPath: string
  onFileActivate: (entry: FilesDirEntry) => void
}) {
  const expanded = useFilesStore((s) => s.expanded.has(entry.relpath))
  const cachedChildren = useFilesStore((s) => s.children[entry.relpath])
  const toggleExpanded = useFilesStore((s) => s.toggleExpanded)
  const setChildren = useFilesStore((s) => s.setChildren)
  const selected = useFilesStore((s) => s.selected === entry.relpath)
  const setSelected = useFilesStore((s) => s.setSelected)

  const bundle = isBundleDir(entry)
  const isDropTarget = entry.isDir && !bundle
  const [dropHover, setDropHover] = useState(false)

  const handleClick = () => {
    if (entry.isDir && !bundle) {
      toggleExpanded(entry.relpath)
      if (!cachedChildren) {
        loadChildren(entry.relpath).then((entries) =>
          setChildren(entry.relpath, entries),
        )
      }
    } else {
      setSelected(entry.relpath)
      onFileActivate(entry)
    }
  }

  const handleContextMenu = (e: ReactMouseEvent) => {
    e.preventDefault()
    e.stopPropagation()
    setSelected(entry.relpath)
    openMenuRef?.({ entry, x: e.clientX, y: e.clientY })
  }

  const handleDragStart = (e: ReactDragEvent<HTMLElement>) => {
    e.dataTransfer.setData(DRAG_MIME, entry.relpath)
    e.dataTransfer.effectAllowed = 'move'
  }

  const handleDragOver = (e: ReactDragEvent<HTMLElement>) => {
    if (!isDropTarget) return
    if (!e.dataTransfer.types.includes(DRAG_MIME)) return
    e.preventDefault()
    e.stopPropagation()
    e.dataTransfer.dropEffect = 'move'
    if (!dropHover) setDropHover(true)
  }

  const handleDragLeave = () => {
    if (dropHover) setDropHover(false)
  }

  const handleDrop = async (e: ReactDragEvent<HTMLElement>) => {
    if (!isDropTarget) return
    const from = e.dataTransfer.getData(DRAG_MIME)
    if (!from) return
    e.preventDefault()
    e.stopPropagation()
    setDropHover(false)
    await moveEntry(from, entry.relpath)
    useFilesStore.getState().setExpanded(entry.relpath, true)
    if (!useFilesStore.getState().children[entry.relpath]) {
      loadChildren(entry.relpath).then((entries) =>
        useFilesStore.getState().setChildren(entry.relpath, entries),
      )
    }
  }

  const tooltip = entry.relpath ? `${rootPath}/${entry.relpath}` : rootPath
  const wrapperClass = entry.isDir && !bundle ? 'nav-folder' : 'nav-file'

  return (
    <div
      className={wrapperClass}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      style={
        dropHover
          ? {
              outline: '1px solid var(--interactive-accent)',
              outlineOffset: -1,
              borderRadius: 3,
              height: '100%',
            }
          : { height: '100%' }
      }
    >
      <Row
        entry={entry}
        depth={depth}
        expanded={expanded}
        selected={selected}
        tooltip={tooltip}
        onClick={handleClick}
        onContextMenu={handleContextMenu}
        onDragStart={handleDragStart}
      />
    </div>
  )
}

function Row({
  entry,
  depth,
  expanded,
  selected,
  tooltip,
  onClick,
  onContextMenu,
  onDragStart,
}: {
  entry: FilesDirEntry
  depth: number
  expanded: boolean
  selected: boolean
  tooltip: string
  onClick: () => void
  onContextMenu: (e: ReactMouseEvent) => void
  onDragStart: (e: ReactDragEvent<HTMLElement>) => void
}) {
  const bundle = isBundleDir(entry)
  const showAsDir = entry.isDir && !bundle
  const titleClass = showAsDir ? 'nav-folder-title' : 'nav-file-title'
  const contentClass = showAsDir ? 'nav-folder-title-content' : 'nav-file-title-content'
  const selfClass =
    `tree-item-self ${titleClass} is-clickable` +
    (showAsDir ? ' mod-collapsible' : '') +
    (selected ? ' is-active' : '')
  return (
    <button
      type="button"
      draggable
      onDragStart={onDragStart}
      onClick={onClick}
      onDoubleClick={onClick}
      onContextMenu={onContextMenu}
      title={tooltip}
      className={selfClass}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 6,
        width: '100%',
        textAlign: 'left',
        background: 'transparent',
        border: 'none',
        color: 'inherit',
        font: 'inherit',
        padding: `3px 8px 3px ${8 + depth * INDENT_PX}px`,
        lineHeight: '18px',
      }}
    >
      <span
        aria-hidden
        className="tree-item-icon collapse-icon"
        style={{
          width: 12,
          display: 'inline-flex',
          justifyContent: 'center',
          position: 'static',
          margin: 0,
          color: 'var(--icon-color)',
        }}
      >
        {showAsDir ? (expanded ? <Icon name="chev" size={10} style={{ transform: 'rotate(90deg)' }} /> : <Icon name="chev" size={10} />) : null}
      </span>
      <span aria-hidden className={showAsDir ? '' : 'nav-file-icon'} style={{ display: 'inline-flex', alignItems: 'center', position: 'static', margin: 0 }}>
        {showAsDir ? (
          <Icon name={expanded ? 'folderOpen' : 'folder'} size={14} />
        ) : (
          <Icon name={getFileIcon(entry.name)} size={14} />
        )}
      </span>
      <span className={`tree-item-inner ${contentClass}`}>
        <span className="tree-item-inner-text">{entry.name}</span>
      </span>
      <RowStatusDot entry={entry} />
    </button>
  )
}

/** BL-053 Phase 4 — status dot rendered at the right edge of each
 *  markdown row when the file's `status:` frontmatter is set. */
function RowStatusDot({ entry }: { entry: FilesDirEntry }) {
  const status = useFileStatus(entry.relpath, entry.isDir, entry.name, getKernel())
  if (status == null) return null
  return (
    <span style={{ marginLeft: 'auto', paddingLeft: 6, display: 'inline-flex' }}>
      <StatusDot status={status} />
    </span>
  )
}
