import { createRoot, type Root } from 'react-dom/client'
import { createElement, useEffect, useState } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ViewBase, workspace, type Leaf } from '../../../workspace'
import { useEditorStore } from '../editor/editorStore'
import { getKernel } from '../files/kernelClient'

const VIEW_TYPE = 'file-properties'
const COMMAND_FOCUS = 'nexus.fileProperties.focus'
const STORAGE_PLUGIN_ID = 'com.nexus.storage'

interface FrontmatterReply {
  status?: unknown
  fields?: unknown
}

interface FileRecord {
  path?: unknown
  file_type?: unknown
  size_bytes?: unknown
  created_at?: unknown
  modified_at?: unknown
}

interface FileProps {
  status: string | null
  title: string | null
  tags: string | null
  fields: Record<string, string>
}

function decodeFrontmatter(raw: unknown): FileProps {
  const out: FileProps = { status: null, title: null, tags: null, fields: {} }
  if (!raw || typeof raw !== 'object') return out
  const r = raw as FrontmatterReply
  if (typeof r.status === 'string') out.status = r.status
  if (r.fields && typeof r.fields === 'object') {
    for (const [k, v] of Object.entries(r.fields as Record<string, unknown>)) {
      if (typeof v !== 'string') continue
      if (k === 'title') out.title = v
      else if (k === 'tags') out.tags = v
      else out.fields[k] = v
    }
  }
  return out
}

function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / (1024 * 1024)).toFixed(2)} MB`
}

function formatTimestamp(secs: number): string {
  if (!Number.isFinite(secs) || secs <= 0) return '—'
  try {
    return new Date(secs * 1000).toLocaleString()
  } catch {
    return '—'
  }
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <tr>
      <td
        style={{
          padding: '4px 8px',
          color: 'var(--text-muted)',
          verticalAlign: 'top',
          width: '40%',
        }}
      >
        {label}
      </td>
      <td style={{ padding: '4px 8px', color: 'var(--text-normal)', wordBreak: 'break-word' }}>
        {value || <span style={{ color: 'var(--text-faint)' }}>—</span>}
      </td>
    </tr>
  )
}

function FilePropertiesView() {
  const activeRelpath = useEditorStore((s) => s.activeRelpath)
  const [props, setProps] = useState<FileProps | null>(null)
  const [meta, setMeta] = useState<FileRecord | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    if (!activeRelpath) {
      setProps(null)
      setMeta(null)
      setError(null)
      setLoading(false)
      return
    }
    const kernel = getKernel()
    if (!kernel) {
      setError('Kernel not ready.')
      return
    }
    setLoading(true)
    setError(null)
    Promise.all([
      kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'read_frontmatter', { path: activeRelpath }),
      kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'query_files', {
        prefix: activeRelpath,
        include_deleted: false,
      }),
    ])
      .then(([rawFm, rawFiles]) => {
        if (cancelled) return
        setProps(decodeFrontmatter(rawFm))
        if (Array.isArray(rawFiles)) {
          const match = (rawFiles as FileRecord[]).find((r) => r.path === activeRelpath) ?? null
          setMeta(match)
        } else {
          setMeta(null)
        }
        setLoading(false)
      })
      .catch((err: unknown) => {
        if (cancelled) return
        setError(err instanceof Error ? err.message : String(err))
        setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [activeRelpath])

  if (!activeRelpath) {
    return (
      <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>
        No active note.
      </div>
    )
  }
  if (loading) {
    return <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>Loading…</div>
  }
  if (error) {
    return <div style={{ padding: 16, fontSize: 12, color: 'var(--text-error)' }}>{error}</div>
  }
  const p = props ?? { status: null, title: null, tags: null, fields: {} }
  const fileType = typeof meta?.file_type === 'string' ? meta.file_type : ''
  const size = typeof meta?.size_bytes === 'number' ? formatBytes(meta.size_bytes) : ''
  const created = typeof meta?.created_at === 'number' ? formatTimestamp(meta.created_at) : ''
  const modified =
    typeof meta?.modified_at === 'number' ? formatTimestamp(meta.modified_at) : ''
  return (
    <div style={{ padding: 8, fontSize: 12 }}>
      <table style={{ width: '100%', borderCollapse: 'collapse' }}>
        <tbody>
          <Row label="name" value={basename(activeRelpath)} />
          <Row label="path" value={activeRelpath} />
          <Row label="type" value={fileType} />
          <Row label="size" value={size} />
          <Row label="created" value={created} />
          <Row label="modified" value={modified} />
          {p.title !== null && <Row label="title" value={p.title} />}
          {p.tags !== null && <Row label="tags" value={p.tags} />}
          {p.status !== null && <Row label="status" value={p.status} />}
          {Object.entries(p.fields).map(([k, v]) => (
            <Row key={k} label={k} value={v} />
          ))}
        </tbody>
      </table>
    </div>
  )
}

class FilePropertiesPaneView extends ViewBase {
  readonly viewType = VIEW_TYPE
  private root: Root | null = null

  constructor(leaf: Leaf) {
    super(leaf)
  }

  getIcon(): string {
    return 'info'
  }

  async onOpen(containerEl: HTMLElement): Promise<void> {
    this.root = createRoot(containerEl)
    this.root.render(createElement(FilePropertiesView))
  }

  async onClose(): Promise<void> {
    this.root?.unmount()
    this.root = null
  }
}

export const filePropertiesPlugin: Plugin = {
  manifest: {
    id: 'nexus.fileProperties',
    name: 'File Properties',
    version: '0.1.0',
    core: false,
    activationEvents: [`onCommand:${COMMAND_FOCUS}`, `onView:${VIEW_TYPE}`],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus File Properties', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    api.viewRegistry.register(VIEW_TYPE, (leaf) => new FilePropertiesPaneView(leaf))
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'right')
      workspace.revealLeaf(leaf)
    })
  },
}
