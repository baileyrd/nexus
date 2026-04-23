import { useEffect, useMemo, useRef, useState } from 'react'
import { useFilesStore, type FilesDirEntry, type SortMode } from './filesStore'
import { useWorkspaceStore } from '../workspace/workspaceStore'
import { useEditorStore } from '../editor/editorStore'
import { createDir, createFile, loadChildren } from './kernelClient'
import { Icon } from '../../../icons'
import { getApi } from './runtime'
import { NavActionButton, NavButtonsContainer, NavHeader } from '../../../primitives/NavHeader'

interface FilesTreeProps {
  onFileActivate: (entry: FilesDirEntry) => void
}

const INDENT_PX = 14
const ROOT_RELPATH = ''

/** Directory extensions that should behave like documents in the
 *  tree: one click opens them as a leaf instead of expanding their
 *  contents. Currently just `.bases` (PRD-10 database bundle); add
 *  more here when other bundle formats land (`.excalidraw`, etc.). */
const BUNDLE_DIR_EXTS = new Set(['bases'])

function isBundleDir(entry: FilesDirEntry): boolean {
  if (!entry.isDir) return false
  const dot = entry.name.lastIndexOf('.')
  if (dot < 0) return false
  return BUNDLE_DIR_EXTS.has(entry.name.slice(dot + 1).toLowerCase())
}

/** Sort entries in-place by the user's chosen mode. Directories always
 *  come first (VSCode / Obsidian convention); the mode only orders
 *  within each bucket. Missing timestamps sink to the bottom. */
function sortEntries(entries: FilesDirEntry[], mode: SortMode): FilesDirEntry[] {
  const sorted = [...entries]
  sorted.sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1
    switch (mode) {
      case 'nameAsc':
        return a.name.toLowerCase().localeCompare(b.name.toLowerCase())
      case 'nameDesc':
        return b.name.toLowerCase().localeCompare(a.name.toLowerCase())
      case 'modifiedDesc':
        return compareNullableNumber(b.modifiedMs, a.modifiedMs, a.name, b.name)
      case 'modifiedAsc':
        return compareNullableNumber(a.modifiedMs, b.modifiedMs, a.name, b.name)
      case 'createdDesc':
        return compareNullableNumber(b.createdMs, a.createdMs, a.name, b.name)
      case 'createdAsc':
        return compareNullableNumber(a.createdMs, b.createdMs, a.name, b.name)
    }
  })
  return sorted
}

/** Numeric comparator that treats `undefined` as "worst" (pushed to
 *  the end) and breaks ties by case-insensitive name. */
function compareNullableNumber(
  a: number | undefined,
  b: number | undefined,
  nameA: string,
  nameB: string,
): number {
  if (a === undefined && b === undefined) {
    return nameA.toLowerCase().localeCompare(nameB.toLowerCase())
  }
  if (a === undefined) return 1
  if (b === undefined) return -1
  if (a !== b) return a - b
  return nameA.toLowerCase().localeCompare(nameB.toLowerCase())
}

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

export function FilesTree({ onFileActivate }: FilesTreeProps) {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const rootEntries = useFilesStore((s) => s.children[ROOT_RELPATH])
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

  useEffect(() => {
    if (!rootPath) return
    if (rootEntries) return
    loadChildren(ROOT_RELPATH).then((entries) =>
      setChildren(ROOT_RELPATH, entries),
    )
  }, [rootPath, rootEntries, setChildren])

  // Auto-reveal: whenever the active editor file changes and the user
  // has the flag on, expand every ancestor directory, select the file,
  // and scroll its row into view.
  useEffect(() => {
    if (!autoReveal) return
    if (!activeRelpath) return
    for (const dir of ancestors(activeRelpath)) {
      setExpanded(dir, true)
      // Fire-and-forget: unexpanded dirs haven't been listed yet, so
      // `loadChildren` populates them before TreeNode renders them.
      const cached = useFilesStore.getState().children[dir]
      if (!cached) {
        loadChildren(dir).then((entries) =>
          useFilesStore.getState().setChildren(dir, entries),
        )
      }
    }
    setSelected(activeRelpath)
  }, [autoReveal, activeRelpath, setExpanded, setSelected])

  if (!rootPath) {
    return (
      <div
        style={{
          padding: '12px 14px',
          color: 'var(--fg-dim)',
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
      console.warn('[nexus.files] create_file failed:', err)
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
      console.warn('[nexus.files] create_dir failed:', err)
      await api.input.confirm(`Failed to create "${trimmed}": ${(err as Error).message ?? err}`)
    }
  }

  const handleToggleAutoReveal = () => {
    setAutoReveal(!autoReveal)
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', width: '100%', height: '100%', overflow: 'hidden' }}>
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

      <div className="nav-files-container">
        {rootEntries ? (
          sortEntries(rootEntries, sortMode).map((entry) => (
            <TreeNode
              key={entry.relpath}
              entry={entry}
              depth={0}
              rootPath={rootPath}
              sortMode={sortMode}
              onFileActivate={onFileActivate}
            />
          ))
        ) : (
          <div style={{ padding: '12px 14px', color: 'var(--text-faint)' }}>Loading…</div>
        )}
      </div>
    </div>
  )
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
    <NavHeader style={{ position: 'relative' }}>
      <NavButtonsContainer hasSeparator>
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

  // Dismiss on outside click / Escape. Use capture so a click on a
  // toolbar button re-opening the same menu cleanly toggles rather
  // than re-opening after this handler fires.
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
        background: 'var(--bg-raised)',
        border: '1px solid var(--line)',
        borderRadius: 'var(--r)',
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
                style={{ height: 1, background: 'var(--line-soft)', margin: '4px 0' }}
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
        background: hover ? 'var(--bg-hover)' : 'transparent',
        color: selected ? 'var(--fg)' : 'var(--fg-muted)',
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
          style={{ position: 'absolute', left: 8, display: 'inline-flex', color: 'var(--fg)' }}
        >
          <Icon name="check" size={12} />
        </span>
      )}
      {label}
    </button>
  )
}

function TreeNode({
  entry,
  depth,
  rootPath,
  sortMode,
  onFileActivate,
}: {
  entry: FilesDirEntry
  depth: number
  rootPath: string
  sortMode: SortMode
  onFileActivate: (entry: FilesDirEntry) => void
}) {
  const expanded = useFilesStore((s) => s.expanded.has(entry.relpath))
  const children = useFilesStore((s) => s.children[entry.relpath])
  const toggleExpanded = useFilesStore((s) => s.toggleExpanded)
  const setChildren = useFilesStore((s) => s.setChildren)
  const selected = useFilesStore((s) => s.selected === entry.relpath)
  const setSelected = useFilesStore((s) => s.setSelected)
  const rowRef = useRef<HTMLButtonElement>(null)

  // Scroll-into-view when auto-reveal selected this row. We scroll on
  // every `selected` transition — the cost is a single smooth scroll
  // per click, which is cheap.
  useEffect(() => {
    if (selected && rowRef.current) {
      rowRef.current.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
    }
  }, [selected])

  const bundle = isBundleDir(entry)

  const handleClick = () => {
    if (entry.isDir && !bundle) {
      toggleExpanded(entry.relpath)
      if (!children) {
        loadChildren(entry.relpath).then((entries) =>
          setChildren(entry.relpath, entries),
        )
      }
    } else {
      setSelected(entry.relpath)
      onFileActivate(entry)
    }
  }

  const tooltip = entry.relpath ? `${rootPath}/${entry.relpath}` : rootPath

  // Bundle dirs render with the file wrapper so they don't get the
  // folder-expand affordance and their children are never listed.
  const wrapperClass = entry.isDir && !bundle ? 'nav-folder' : 'nav-file'
  return (
    <div className={wrapperClass}>
      <Row
        entry={entry}
        depth={depth}
        expanded={expanded}
        selected={selected}
        tooltip={tooltip}
        onClick={handleClick}
        buttonRef={rowRef}
      />
      {entry.isDir && !bundle && expanded && children && (
        <div className="nav-folder-children tree-item-children">
          {sortEntries(children, sortMode).map((child) => (
            <TreeNode
              key={child.relpath}
              entry={child}
              depth={depth + 1}
              rootPath={rootPath}
              sortMode={sortMode}
              onFileActivate={onFileActivate}
            />
          ))}
        </div>
      )}
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
  buttonRef,
}: {
  entry: FilesDirEntry
  depth: number
  expanded: boolean
  selected: boolean
  tooltip: string
  onClick: () => void
  buttonRef: React.RefObject<HTMLButtonElement>
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
      ref={buttonRef}
      onClick={onClick}
      onDoubleClick={onClick}
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
          <Icon name="doc" size={14} />
        )}
      </span>
      <span className={`tree-item-inner ${contentClass}`}>
        <span className="tree-item-inner-text">{entry.name}</span>
      </span>
    </button>
  )
}
