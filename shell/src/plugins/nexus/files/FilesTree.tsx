import { useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useFilesStore, type FilesDirEntry } from './filesStore'
import { useWorkspaceStore } from '../workspace/workspaceStore'

interface FilesTreeProps {
  onFileActivate: (entry: FilesDirEntry) => void
}

const INDENT_PX = 14

async function loadChildren(path: string): Promise<FilesDirEntry[]> {
  try {
    return await invoke<FilesDirEntry[]>('read_dir', { path })
  } catch (err) {
    console.warn('[nexus.files] read_dir failed for', path, err)
    return []
  }
}

export function FilesTree({ onFileActivate }: FilesTreeProps) {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const children = useFilesStore((s) => s.children)
  const setChildren = useFilesStore((s) => s.setChildren)

  useEffect(() => {
    if (!rootPath) return
    if (children[rootPath]) return
    loadChildren(rootPath).then((entries) => setChildren(rootPath, entries))
  }, [rootPath, children, setChildren])

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

  const rootEntries = children[rootPath]

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

  return (
    <div style={{ padding: '4px 0', fontSize: 'var(--ui-size, 13px)' }}>
      {rootEntries.map((entry) => (
        <TreeNode
          key={entry.path}
          entry={entry}
          depth={0}
          onFileActivate={onFileActivate}
        />
      ))}
    </div>
  )
}

function TreeNode({
  entry,
  depth,
  onFileActivate,
}: {
  entry: FilesDirEntry
  depth: number
  onFileActivate: (entry: FilesDirEntry) => void
}) {
  const expanded = useFilesStore((s) => s.expanded.has(entry.path))
  const children = useFilesStore((s) => s.children[entry.path])
  const toggleExpanded = useFilesStore((s) => s.toggleExpanded)
  const setChildren = useFilesStore((s) => s.setChildren)
  const selected = useFilesStore((s) => s.selected === entry.path)
  const setSelected = useFilesStore((s) => s.setSelected)

  const handleClick = () => {
    if (entry.isDirectory) {
      toggleExpanded(entry.path)
      if (!children) {
        loadChildren(entry.path).then((entries) => setChildren(entry.path, entries))
      }
    } else {
      setSelected(entry.path)
      onFileActivate(entry)
    }
  }

  return (
    <div>
      <Row
        entry={entry}
        depth={depth}
        expanded={expanded}
        selected={selected}
        onClick={handleClick}
      />
      {entry.isDirectory && expanded && children && (
        <div>
          {children.map((child) => (
            <TreeNode
              key={child.path}
              entry={child}
              depth={depth + 1}
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
  onClick,
}: {
  entry: FilesDirEntry
  depth: number
  expanded: boolean
  selected: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      onDoubleClick={onClick}
      title={entry.path}
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
        {entry.isDirectory ? (expanded ? <ChevronDown /> : <ChevronRight />) : null}
      </span>
      <span
        aria-hidden
        style={{ display: 'inline-flex', alignItems: 'center' }}
      >
        {entry.isDirectory ? (
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
