// Forge-styled file explorer. Matches the markup used by [forge_panels.jsx]:
//   .leftpanel > .panel-head + .filter + .tree(.row/.children) + .leftfoot
// so the Forge CSS carries over unchanged.

import { useMemo, useState } from 'react'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { readDir } from '@tauri-apps/plugin-fs'
import { useContextKeyStore } from '../../../host/ContextKeyService'
import { useConfigValue } from '../../../stores/configStore'
import { Ic } from '../../../shell/icons'

interface TreeNode {
  name: string
  path: string
  isDirectory: boolean
  children?: TreeNode[]
  isExpanded?: boolean
}

export function FileExplorerView() {
  const [rootPath, setRootPath]         = useState<string | null>(null)
  const [tree, setTree]                 = useState<TreeNode[]>([])
  const [selectedPath, setSelectedPath] = useState<string | null>(null)
  const [filter, setFilter]             = useState('')
  const showHidden = useConfigValue('fileExplorer.showHidden', false) as boolean

  const sortNodes = (nodes: TreeNode[]) =>
    [...nodes].sort((a, b) => {
      if (a.isDirectory !== b.isDirectory) return a.isDirectory ? -1 : 1
      return a.name.localeCompare(b.name)
    })

  const filterNodes = (entries: { name?: string; isDirectory?: boolean }[], basePath: string): TreeNode[] =>
    entries
      .filter(e => e.name && (showHidden || !e.name.startsWith('.')))
      .map(e => ({
        name: e.name!,
        path: `${basePath}/${e.name}`,
        isDirectory: e.isDirectory ?? false,
      }))

  const handleOpenFolder = async () => {
    const selected = await openDialog({ directory: true, multiple: false })
    if (selected && typeof selected === 'string') {
      const entries = await readDir(selected).catch(() => [])
      setTree(sortNodes(filterNodes(entries, selected)))
      setRootPath(selected)
    }
  }

  const openFile = async (path: string) => {
    setSelectedPath(path)
    useContextKeyStore.getState().set('fileExplorerFocus', true)
    try {
      const { useEditorStore } = await import('../editorArea/index')
      const name = path.split(/[\\/]/).pop() ?? path
      useEditorStore.getState().openTab({ path, title: name, isDirty: false, isPinned: false, isPreview: true })
    } catch { /* editor area not loaded */ }
  }

  const toggleDir = async (node: TreeNode) => {
    if (!node.isDirectory) return
    if (node.isExpanded) {
      setTree(t => collapseNode(t, node.path))
    } else {
      const entries = await readDir(node.path).catch(() => [])
      const children = sortNodes(filterNodes(entries, node.path))
      setTree(t => expandNode(t, node.path, children))
    }
  }

  const rootLabel = useMemo(() => {
    if (!rootPath) return 'Explorer'
    return rootPath.split(/[\\/]/).filter(Boolean).pop() ?? rootPath
  }, [rootPath])

  return (
    <div
      className="leftpanel"
      onFocus={() => useContextKeyStore.getState().set('sidebarFocus', true)}
      onBlur={() => useContextKeyStore.getState().set('sidebarFocus', false)}
      tabIndex={0}
    >
      <div className="panel-head">
        <span>{rootLabel}</span>
        <div className="actions">
          <button className="icon-btn" title="Open folder" onClick={handleOpenFolder}><Ic.folder /></button>
          <button className="icon-btn" title="New file"><Ic.plus /></button>
          <button className="icon-btn" title="Collapse all"><Ic.min /></button>
        </div>
      </div>

      <div className="filter">
        <Ic.search style={{ width: 12, height: 12, color: 'var(--fg-dim)' }} />
        <input
          value={filter}
          onChange={e => setFilter(e.target.value)}
          placeholder="Filter files…"
        />
        <span className="kbd">⌘P</span>
      </div>

      <div className="tree">
        {!rootPath ? (
          <div style={{ padding: 20, textAlign: 'center', color: 'var(--fg-dim)', fontSize: 12 }}>
            <p style={{ marginBottom: 10 }}>No folder open</p>
            <button
              onClick={handleOpenFolder}
              style={{
                background: 'var(--accent)', color: 'var(--accent-ink)',
                border: 'none', borderRadius: 'var(--r)',
                padding: '6px 14px', fontSize: 12, cursor: 'pointer',
                fontWeight: 500,
              }}
            >
              Open Folder
            </button>
          </div>
        ) : (
          tree.map(node => (
            <TreeNodeRow
              key={node.path}
              node={node}
              depth={0}
              selectedPath={selectedPath}
              filter={filter.toLowerCase()}
              onFileClick={openFile}
              onDirClick={toggleDir}
            />
          ))
        )}
      </div>

      <div className="leftfoot">
        <div className="u">
          <div className="av">SH</div>
          <span>shell</span>
        </div>
        <div style={{ display: 'flex', gap: 4 }}>
          <button className="icon-btn" title="Help"><span style={{ fontSize: 11 }}>?</span></button>
          <button className="icon-btn" title="Settings"><Ic.settings /></button>
        </div>
      </div>
    </div>
  )
}

function TreeNodeRow({ node, depth, selectedPath, filter, onFileClick, onDirClick }: {
  node: TreeNode
  depth: number
  selectedPath: string | null
  filter: string
  onFileClick: (path: string) => void
  onDirClick: (node: TreeNode) => void
}) {
  if (filter) {
    const matches = deepMatch(node, filter)
    if (!matches) return null
  }

  const isFile = !node.isDirectory
  const FolderIc = node.isExpanded ? Ic.folderOpen : Ic.folder
  const IconComp = isFile ? Ic.doc : FolderIc
  const isActive = selectedPath === node.path

  return (
    <>
      <div
        className={'row ' + (node.isExpanded ? 'open ' : '') + (isActive ? 'active' : '')}
        style={{ paddingLeft: 6 + depth * 14 }}
        onClick={() => isFile ? onFileClick(node.path) : onDirClick(node)}
      >
        {!isFile
          ? <span className="caret"><Ic.chev style={{ width: 10, height: 10 }} /></span>
          : <span className="caret" />}
        <span className="icon"><IconComp style={{ width: 13, height: 13 }} /></span>
        <span className="name">{node.name}</span>
      </div>
      {node.isDirectory && node.isExpanded && node.children?.map(child => (
        <TreeNodeRow
          key={child.path}
          node={child}
          depth={depth + 1}
          selectedPath={selectedPath}
          filter={filter}
          onFileClick={onFileClick}
          onDirClick={onDirClick}
        />
      ))}
    </>
  )
}

function deepMatch(n: TreeNode, f: string): boolean {
  if (n.name.toLowerCase().includes(f)) return true
  return (n.children ?? []).some(c => deepMatch(c, f))
}

function expandNode(nodes: TreeNode[], path: string, children: TreeNode[]): TreeNode[] {
  return nodes.map(n => {
    if (n.path === path) return { ...n, isExpanded: true, children }
    if (n.children) return { ...n, children: expandNode(n.children, path, children) }
    return n
  })
}

function collapseNode(nodes: TreeNode[], path: string): TreeNode[] {
  return nodes.map(n => {
    if (n.path === path) return { ...n, isExpanded: false, children: [] }
    if (n.children) return { ...n, children: collapseNode(n.children, path) }
    return n
  })
}
