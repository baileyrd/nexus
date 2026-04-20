import { useState, useEffect } from 'react'
import { useFilesStore, type FilesDirEntry } from './filesStore'
import { useWorkspaceStore } from '../workspace/workspaceStore'
import { loadChildren } from './kernelClient'

interface FilesTreeProps {
  onFileActivate: (entry: FilesDirEntry) => void
}

const INDENT_PX = 14
const ROOT_RELPATH = ''

export function FilesTree({ onFileActivate }: FilesTreeProps) {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const rootEntries = useFilesStore((s) => s.children[ROOT_RELPATH])
  const setChildren = useFilesStore((s) => s.setChildren)
  const [filter, setFilter] = useState('')

  useEffect(() => {
    if (!rootPath) return
    if (rootEntries) return
    loadChildren(ROOT_RELPATH).then((entries) =>
      setChildren(ROOT_RELPATH, entries),
    )
  }, [rootPath, rootEntries, setChildren])

  // Clear filter when workspace changes
  useEffect(() => {
    setFilter('')
  }, [rootPath])

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

  if (!rootEntries) {
    return (
      <div
        style={{
          padding: '12px 14px',
          color: 'var(--fg-dim)',
          fontSize: 'var(--ui-size, 12px)',
        }}
      >
        Loading…
      </div>
    )
  }

  const normalizedFilter = filter.trim().toLowerCase()

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', overflow: 'hidden' }}>
      {/* Filter row */}
      <div
        style={{
          flexShrink: 0,
          padding: '4px 8px',
          borderBottom: '1px solid var(--line-soft)',
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            background: 'var(--bg)',
            border: '1px solid var(--line-soft)',
            borderRadius: 'var(--r)',
            padding: '3px 8px',
          }}
        >
          <svg
            width={12}
            height={12}
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth={1.75}
            strokeLinecap="round"
            strokeLinejoin="round"
            style={{ color: 'var(--fg-dim)', flexShrink: 0 }}
            aria-hidden
          >
            <circle cx="11" cy="11" r="7" />
            <path d="m20 20-3-3" />
          </svg>
          <input
            type="text"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Filter files…"
            aria-label="Filter files"
            style={{
              flex: 1,
              background: 'transparent',
              border: 0,
              outline: 'none',
              color: 'var(--fg)',
              fontSize: 'var(--ui-size, 12px)',
              fontFamily: 'var(--f-ui)',
              padding: 0,
              lineHeight: '20px',
            }}
          />
          {filter && (
            <button
              type="button"
              aria-label="Clear filter"
              onClick={() => setFilter('')}
              style={{
                background: 'transparent',
                border: 0,
                color: 'var(--fg-dim)',
                cursor: 'pointer',
                padding: 0,
                display: 'inline-flex',
                alignItems: 'center',
                flexShrink: 0,
              }}
            >
              <svg width={12} height={12} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
                <path d="M18 6L6 18M6 6l12 12" />
              </svg>
            </button>
          )}
        </div>
      </div>

      {/* Tree */}
      <div style={{ flex: 1, overflow: 'auto', padding: '4px 0', fontSize: 'var(--ui-size, 13px)' }}>
        {rootEntries.map((entry) => (
          <TreeNode
            key={entry.relpath}
            entry={entry}
            depth={0}
            rootPath={rootPath}
            filter={normalizedFilter}
            onFileActivate={onFileActivate}
          />
        ))}
      </div>
    </div>
  )
}

function TreeNode({
  entry,
  depth,
  rootPath,
  filter,
  onFileActivate,
}: {
  entry: FilesDirEntry
  depth: number
  rootPath: string
  filter: string
  onFileActivate: (entry: FilesDirEntry) => void
}) {
  const expanded = useFilesStore((s) => s.expanded.has(entry.relpath))
  const children = useFilesStore((s) => s.children[entry.relpath])
  const toggleExpanded = useFilesStore((s) => s.toggleExpanded)
  const setChildren = useFilesStore((s) => s.setChildren)
  const selected = useFilesStore((s) => s.selected === entry.relpath)
  const setSelected = useFilesStore((s) => s.setSelected)

  // When a filter is active, hide non-matching files (keep all dirs visible)
  if (filter && !entry.isDir && !entry.name.toLowerCase().includes(filter)) {
    return null
  }

  const handleClick = () => {
    if (entry.isDir) {
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

  return (
    <div>
      <Row
        entry={entry}
        depth={depth}
        expanded={expanded}
        selected={selected}
        tooltip={tooltip}
        onClick={handleClick}
      />
      {entry.isDir && expanded && children && (
        <div>
          {children.map((child) => (
            <TreeNode
              key={child.relpath}
              entry={child}
              depth={depth + 1}
              rootPath={rootPath}
              filter={filter}
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
}: {
  entry: FilesDirEntry
  depth: number
  expanded: boolean
  selected: boolean
  tooltip: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      onDoubleClick={onClick}
      title={tooltip}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 6,
        width: '100%',
        textAlign: 'left',
        border: 'none',
        background: selected ? 'var(--accent-soft)' : 'transparent',
        color: selected ? 'var(--fg)' : 'var(--fg-muted)',
        cursor: 'pointer',
        font: 'inherit',
        padding: `3px 8px 3px ${8 + depth * INDENT_PX}px`,
        height: 24,
        lineHeight: '18px',
        transition: 'background 0.06s',
      }}
      onMouseEnter={(e) => {
        if (!selected) e.currentTarget.style.background = 'var(--bg-hover)'
      }}
      onMouseLeave={(e) => {
        if (!selected) e.currentTarget.style.background = 'transparent'
      }}
    >
      <span
        aria-hidden
        style={{
          width: 12,
          display: 'inline-flex',
          justifyContent: 'center',
          color: 'var(--fg-dim)',
        }}
      >
        {entry.isDir ? (expanded ? <ChevronDown /> : <ChevronRight />) : null}
      </span>
      <span
        aria-hidden
        style={{ display: 'inline-flex', alignItems: 'center' }}
      >
        {entry.isDir ? (
          expanded ? <FolderOpenIcon /> : <FolderIcon />
        ) : (
          <FileIcon />
        )}
      </span>
      <span
        style={{
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {entry.name}
      </span>
    </button>
  )
}

function svgProps() {
  return {
    width: 14,
    height: 14,
    viewBox: '0 0 24 24',
    fill: 'none',
    stroke: 'currentColor',
    strokeWidth: 1.75,
    strokeLinecap: 'round' as const,
    strokeLinejoin: 'round' as const,
  }
}

function ChevronRight() {
  return (
    <svg {...svgProps()} width={10} height={10}>
      <path d="M9 6l6 6-6 6" />
    </svg>
  )
}

function ChevronDown() {
  return (
    <svg {...svgProps()} width={10} height={10}>
      <path d="M6 9l6 6 6-6" />
    </svg>
  )
}

function FolderIcon() {
  return (
    <svg {...svgProps()}>
      <path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2z" />
    </svg>
  )
}

function FolderOpenIcon() {
  return (
    <svg {...svgProps()}>
      <path d="M6 14l1.45-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6A2 2 0 0 1 18.46 20H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.93a2 2 0 0 1 1.66.9l.82 1.2A2 2 0 0 0 12.07 6H18a2 2 0 0 1 2 2v2" />
    </svg>
  )
}

function FileIcon() {
  return (
    <svg {...svgProps()}>
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6" />
    </svg>
  )
}
