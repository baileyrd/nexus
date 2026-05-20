import { createRoot, type Root } from 'react-dom/client'
import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { ViewBase, workspace, type Leaf } from '../../../workspace'
import { useActiveFileQuery } from '../_lib/useActiveFileQuery'
import type { EventsAPI } from '../../../types/plugin'

let events: EventsAPI | null = null

const VIEW_TYPE = 'outgoing-links'
const COMMAND_FOCUS = 'nexus.outgoingLinks.focus'
const STORAGE_PLUGIN_ID = 'com.nexus.storage'

interface KernelOutgoingLink {
  target_path?: unknown
  link_text?: unknown
  link_type?: unknown
  is_resolved?: unknown
}

interface OutgoingLink {
  targetPath: string
  linkText: string
  linkType: string
  isResolved: boolean
}

function decode(raw: unknown): OutgoingLink[] {
  if (!Array.isArray(raw)) return []
  const out: OutgoingLink[] = []
  for (const item of raw as KernelOutgoingLink[]) {
    if (!item || typeof item !== 'object') continue
    const targetPath = typeof item.target_path === 'string' ? item.target_path : null
    if (!targetPath) continue
    out.push({
      targetPath,
      linkText: typeof item.link_text === 'string' ? item.link_text : targetPath,
      linkType: typeof item.link_type === 'string' ? item.link_type : '',
      isResolved: item.is_resolved === true,
    })
  }
  return out
}

function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

function OutgoingLinksView() {
  const { data: links, loading, error, activeRelpath } = useActiveFileQuery<OutgoingLink[]>({
    fetch: async (kernel, relpath) => {
      const raw = await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'outgoing_links', { path: relpath })
      return decode(raw)
    },
    initial: [],
  })

  if (!activeRelpath) {
    return (
      <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>
        No active note.
      </div>
    )
  }
  if (loading) {
    return (
      <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>Loading…</div>
    )
  }
  if (error) {
    return (
      <div style={{ padding: 16, fontSize: 12, color: 'var(--text-error)' }}>{error}</div>
    )
  }
  if (links.length === 0) {
    return (
      <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>
        No outgoing links.
      </div>
    )
  }
  return (
    <div style={{ padding: 8, fontSize: 13 }}>
      {links.map((link, i) => {
        const label = link.linkText || basename(link.targetPath)
        const onClick = () => {
          if (!link.isResolved) return
          events?.emit('files:open', { relpath: link.targetPath, name: basename(link.targetPath) })
        }
        return (
          <div
            key={`${link.targetPath}-${i}`}
            onClick={onClick}
            title={link.targetPath}
            style={{
              padding: '4px 8px',
              cursor: link.isResolved ? 'pointer' : 'default',
              color: link.isResolved ? 'var(--text-normal)' : 'var(--text-muted)',
              fontStyle: link.isResolved ? 'normal' : 'italic',
            }}
          >
            {label}
          </div>
        )
      })}
    </div>
  )
}

class OutgoingLinksPaneView extends ViewBase {
  readonly viewType = VIEW_TYPE
  private root: Root | null = null

  constructor(leaf: Leaf) {
    super(leaf)
  }

  async onOpen(containerEl: HTMLElement): Promise<void> {
    this.root = createRoot(containerEl)
    this.root.render(createElement(OutgoingLinksView))
  }

  async onClose(): Promise<void> {
    this.root?.unmount()
    this.root = null
  }
}

export const outgoingLinksPlugin: Plugin = {
  manifest: {
    id: 'nexus.outgoingLinks',
    name: 'Outgoing Links',
    version: '0.1.0',
    core: false,
    activationEvents: [`onCommand:${COMMAND_FOCUS}`, `onView:${VIEW_TYPE}`],
    // Imports `../editor/editorStore` and `../files/kernelClient` —
    // those plugins must be loaded first.
    dependsOn: ['nexus.editor', 'nexus.files'],
    contributes: {
      commands: [{ id: COMMAND_FOCUS, title: 'Focus Outgoing Links', category: 'View' }],
    },
  },

  activate(api: PluginAPI) {
    events = api.events
    api.viewRegistry.register(VIEW_TYPE, (leaf) => new OutgoingLinksPaneView(leaf))
    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'right')
      workspace.revealLeaf(leaf)
    })
  },
}
