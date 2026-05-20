import { createRoot, type Root } from 'react-dom/client'
import { createElement, useEffect, useState } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ViewBase, workspace, type Leaf } from '../../../workspace'
import { useEditorStore } from '../editor/editorStore'
import { getKernel } from '../files/kernelClient'
import type { EventsAPI } from '../../../types/plugin'

let events: EventsAPI | null = null

const VIEW_TYPE = 'tags'
const COMMAND_FOCUS = 'nexus.tags.focus'
const STORAGE_PLUGIN_ID = 'com.nexus.storage'

interface FrontmatterReply {
  status?: unknown
  fields?: unknown
}

interface KernelTagResult {
  name?: unknown
  file_path?: unknown
  source?: unknown
}

interface TagUsage {
  name: string
  filePaths: string[]
}

function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

function extractTags(raw: unknown): string[] {
  if (!raw || typeof raw !== 'object') return []
  const r = raw as FrontmatterReply
  const fields = (r.fields ?? {}) as Record<string, unknown>
  const raw_tags = fields.tags
  if (typeof raw_tags !== 'string' || raw_tags.length === 0) return []
  // Frontmatter parser joins list values with ", " — split it back.
  // Single string values (e.g. `tags: foo`) pass through as one tag.
  return raw_tags
    .split(',')
    .map((s) => s.trim().replace(/^#/, ''))
    .filter((s) => s.length > 0)
}

function TagsView() {
  const activeRelpath = useEditorStore((s) => s.activeRelpath)
  const [tags, setTags] = useState<TagUsage[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [expanded, setExpanded] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    if (!activeRelpath) {
      setTags([])
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
    setExpanded(null)
    kernel
      .invoke<unknown>(STORAGE_PLUGIN_ID, 'read_frontmatter', { path: activeRelpath })
      .then(async (raw) => {
        if (cancelled) return
        const names = extractTags(raw)
        if (names.length === 0) {
          setTags([])
          setLoading(false)
          return
        }
        const usages = await Promise.all(
          names.map(async (name) => {
            try {
              const results = await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'query_tags', {
                name,
              })
              const filePaths = Array.isArray(results)
                ? (results as KernelTagResult[])
                    .map((r) => (typeof r.file_path === 'string' ? r.file_path : null))
                    .filter((p): p is string => p !== null)
                : []
              return { name, filePaths }
            } catch {
              return { name, filePaths: [] }
            }
          }),
        )
        if (cancelled) return
        setTags(usages)
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
  if (tags.length === 0) {
    return (
      <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>
        No tags on this note.
      </div>
    )
  }
  return (
    <div style={{ padding: 8, fontSize: 13 }}>
      {tags.map((tag) => {
        const isOpen = expanded === tag.name
        const others = tag.filePaths.filter((p) => p !== activeRelpath)
        return (
          <div key={tag.name} style={{ marginBottom: 4 }}>
            <div
              onClick={() => setExpanded(isOpen ? null : tag.name)}
              style={{
                padding: '4px 8px',
                cursor: 'pointer',
                color: 'var(--text-normal)',
                display: 'flex',
                justifyContent: 'space-between',
              }}
            >
              <span>#{tag.name}</span>
              <span style={{ color: 'var(--text-muted)', fontSize: 11 }}>
                {tag.filePaths.length} {tag.filePaths.length === 1 ? 'file' : 'files'}
              </span>
            </div>
            {isOpen && others.length > 0 && (
              <div style={{ paddingLeft: 16 }}>
                {others.map((p) => (
                  <div
                    key={p}
                    onClick={() =>
                      events?.emit('files:open', { relpath: p, name: basename(p) })
                    }
                    style={{
                      padding: '2px 8px',
                      cursor: 'pointer',
                      fontSize: 12,
                      color: 'var(--text-muted)',
                    }}
                    title={p}
                  >
                    {basename(p)}
                  </div>
                ))}
              </div>
            )}
          </div>
        )
      })}
    </div>
  )
}

class TagsPaneView extends ViewBase {
  readonly viewType = VIEW_TYPE
  private root: Root | null = null

  constructor(leaf: Leaf) {
    super(leaf)
  }

  async onOpen(containerEl: HTMLElement): Promise<void> {
    this.root = createRoot(containerEl)
    this.root.render(createElement(TagsView))
  }

  async onClose(): Promise<void> {
    this.root?.unmount()
    this.root = null
  }
}

export const tagsPlugin: Plugin = {
  manifest: {
    id: 'nexus.tags',
    name: 'Tags',
    version: '0.1.0',
    core: false,
    activationEvents: [`onCommand:${COMMAND_FOCUS}`, `onView:${VIEW_TYPE}`],
    // Imports `../editor/editorStore` and `../files/kernelClient` —
    // those plugins must be loaded first.
    dependsOn: ['nexus.editor', 'nexus.files'],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus Tags', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    events = api.events
    api.viewRegistry.register(VIEW_TYPE, (leaf) => new TagsPaneView(leaf))
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'right')
      workspace.revealLeaf(leaf)
    })
  },
}
