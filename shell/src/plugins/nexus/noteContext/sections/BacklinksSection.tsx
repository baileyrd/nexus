// Backlinks section for the Note Context accordion.
//
// Ported from the standalone `nexus.backlinks` plugin. The legacy
// plugin had three load paths layered on top of each other:
//   1. Active-file-change fetch (matches our section's main effect).
//   2. On-edit silent refresh subscribed to editor session changes —
//      documented in the original code as "largely a no-op" because
//      editing file A doesn't change file A's *incoming* backlinks
//      (those live on other files). Skipped here; pick it up if
//      the storage layer ever publishes a cross-file reindex event.
//   3. Block-filter mode (BL-049 phase 4) — toggling between the
//      `backlinks` and `backlinks_to_block` IPCs to narrow the list
//      to a specific block id. Niche power feature; skipped in v1.
//
// What remains is the core surface most users actually see: the
// inbound-link list for the active file, refreshed on tab switch.
// Hard-lazy semantics fall out from the accordion — the section's
// useEffect only runs while expanded; collapse tears it down and a
// later expand re-fetches.

import { useActiveFileQuery } from '../../_lib/useActiveFileQuery'
import { useEventBus } from '../eventBus'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const BACKLINKS_COMMAND = 'backlinks'

interface KernelBacklink {
  source_path?: unknown
  link_text?: unknown
  link_type?: unknown
  fragment?: unknown
}

interface Backlink {
  sourceRelpath: string
  sourceName: string
  linkText: string
  linkType: string
  fragment: string | null
}

function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

function decode(raw: unknown, currentRelpath: string): Backlink[] {
  if (!Array.isArray(raw)) return []
  const out: Backlink[] = []
  for (const item of raw as KernelBacklink[]) {
    if (!item || typeof item !== 'object') continue
    const sourceRelpath = typeof item.source_path === 'string' ? item.source_path : null
    if (!sourceRelpath) continue
    // Defensive: a file can in principle link to itself; the inspector
    // is more useful when those are excluded.
    if (sourceRelpath === currentRelpath) continue
    out.push({
      sourceRelpath,
      sourceName: basename(sourceRelpath) || sourceRelpath,
      linkText: typeof item.link_text === 'string' ? item.link_text : '',
      linkType: typeof item.link_type === 'string' ? item.link_type : '',
      fragment:
        typeof item.fragment === 'string' && item.fragment.length > 0
          ? item.fragment
          : null,
    })
  }
  return out
}

export function BacklinksSection() {
  const events = useEventBus()
  const { data: links, loading, error, activeRelpath } = useActiveFileQuery<Backlink[]>({
    fetch: async (kernel, relpath) => {
      const raw = await kernel.invoke<KernelBacklink[]>(STORAGE_PLUGIN_ID, BACKLINKS_COMMAND, {
        path: relpath,
      })
      return decode(raw, relpath)
    },
    initial: [],
  })

  if (!activeRelpath) {
    return (
      <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>
        Open a file to see its backlinks.
      </div>
    )
  }
  if (loading) {
    return <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>Loading…</div>
  }
  if (error) {
    return <div style={{ padding: 16, fontSize: 12, color: 'var(--text-error)' }}>{error}</div>
  }
  if (links.length === 0) {
    return (
      <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>
        No backlinks found.
      </div>
    )
  }
  return (
    <div style={{ padding: 4, fontSize: 13 }}>
      {links.map((link, idx) => (
        <div
          key={`${link.sourceRelpath}::${idx}`}
          onClick={() =>
            events?.emit('files:open', {
              relpath: link.sourceRelpath,
              name: link.sourceName,
            })
          }
          title={link.sourceRelpath}
          style={{
            padding: '4px 8px',
            cursor: 'pointer',
            color: 'var(--text-normal)',
            display: 'flex',
            flexDirection: 'column',
            gap: 2,
          }}
        >
          <div
            style={{
              fontWeight: 500,
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
            }}
          >
            {link.sourceName}
          </div>
          {(link.linkText || link.fragment) && (
            <div
              style={{
                fontSize: 11,
                color: 'var(--text-muted)',
                whiteSpace: 'nowrap',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
              }}
            >
              {link.fragment && (
                <span style={{ color: 'var(--text-faint)' }}>
                  {link.fragment.startsWith('^') ? link.fragment : `#${link.fragment}`}
                  {link.linkText ? ' · ' : ''}
                </span>
              )}
              {link.linkText}
            </div>
          )}
        </div>
      ))}
    </div>
  )
}
