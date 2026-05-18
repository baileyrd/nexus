import { createRoot, type Root } from 'react-dom/client'
import { createElement, useEffect, useState } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ViewBase, workspace, type Leaf } from '../../../workspace'
import { useEditorStore } from '../editor/editorStore'
import { getKernel } from '../files/kernelClient'

const VIEW_TYPE = 'all-properties'
const COMMAND_FOCUS = 'nexus.allProperties.focus'
const STORAGE_PLUGIN_ID = 'com.nexus.storage'

interface FrontmatterReply {
  status?: unknown
  fields?: unknown
}

interface Properties {
  status: string | null
  fields: Array<[string, string]>
}

function decode(raw: unknown): Properties {
  const out: Properties = { status: null, fields: [] }
  if (!raw || typeof raw !== 'object') return out
  const r = raw as FrontmatterReply
  if (typeof r.status === 'string') out.status = r.status
  if (r.fields && typeof r.fields === 'object') {
    for (const [k, v] of Object.entries(r.fields as Record<string, unknown>)) {
      if (typeof v === 'string') out.fields.push([k, v])
    }
  }
  return out
}

function AllPropertiesView() {
  const activeRelpath = useEditorStore((s) => s.activeRelpath)
  const [props, setProps] = useState<Properties>({ status: null, fields: [] })
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    if (!activeRelpath) {
      setProps({ status: null, fields: [] })
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
    kernel
      .invoke<unknown>(STORAGE_PLUGIN_ID, 'read_frontmatter', { path: activeRelpath })
      .then((raw) => {
        if (cancelled) return
        setProps(decode(raw))
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
  const hasAny = props.status !== null || props.fields.length > 0
  if (!hasAny) {
    return (
      <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>
        No frontmatter properties.
      </div>
    )
  }
  return (
    <div style={{ padding: 8, fontSize: 12 }}>
      <table style={{ width: '100%', borderCollapse: 'collapse' }}>
        <tbody>
          {props.status !== null && (
            <tr>
              <td
                style={{
                  padding: '4px 8px',
                  color: 'var(--text-muted)',
                  verticalAlign: 'top',
                  width: '40%',
                }}
              >
                status
              </td>
              <td style={{ padding: '4px 8px', color: 'var(--text-normal)' }}>{props.status}</td>
            </tr>
          )}
          {props.fields.map(([k, v]) => (
            <tr key={k}>
              <td
                style={{
                  padding: '4px 8px',
                  color: 'var(--text-muted)',
                  verticalAlign: 'top',
                  width: '40%',
                }}
              >
                {k}
              </td>
              <td style={{ padding: '4px 8px', color: 'var(--text-normal)', wordBreak: 'break-word' }}>
                {v || <span style={{ color: 'var(--text-faint)' }}>—</span>}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

class AllPropertiesPaneView extends ViewBase {
  readonly viewType = VIEW_TYPE
  private root: Root | null = null

  constructor(leaf: Leaf) {
    super(leaf)
  }

  async onOpen(containerEl: HTMLElement): Promise<void> {
    this.root = createRoot(containerEl)
    this.root.render(createElement(AllPropertiesView))
  }

  async onClose(): Promise<void> {
    this.root?.unmount()
    this.root = null
  }
}

export const allPropertiesPlugin: Plugin = {
  manifest: {
    id: 'nexus.allProperties',
    name: 'All Properties',
    version: '0.1.0',
    core: false,
    activationEvents: [`onCommand:${COMMAND_FOCUS}`, `onView:${VIEW_TYPE}`],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus All Properties', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    api.viewRegistry.register(VIEW_TYPE, (leaf) => new AllPropertiesPaneView(leaf))
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'right')
      workspace.revealLeaf(leaf)
    })
  },
}
