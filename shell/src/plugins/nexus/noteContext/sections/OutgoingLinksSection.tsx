// Outgoing Links section for the Note Context accordion.
//
// Ported from `nexus.outgoingLinks/index.tsx` — same React component
// shape (data via `useActiveFileQuery`, identical decode + render),
// adapted only insofar as it now lives inside an accordion section
// rather than its own right-panel tab. The standalone
// `nexus.outgoingLinks` plugin remains functional until step 6
// retires it; this duplicates the component so users with the legacy
// plugin enabled continue to see it.
//
// Click on a resolved link emits the shell `files:open` event so the
// editor switches to the target file. Unresolved links render greyed
// and are inert (you can still copy the label).

import { useActiveFileQuery } from '../../_lib/useActiveFileQuery'
import { useEventBus } from '../eventBus'

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

export function OutgoingLinksSection() {
  const events = useEventBus()
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
