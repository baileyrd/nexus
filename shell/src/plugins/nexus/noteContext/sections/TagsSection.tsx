// Tags section for the Note Context accordion.
//
// Ported from `nexus.tags/index.tsx`. Same data path:
//   1. `read_frontmatter` for the active file → list of tag names.
//   2. For each tag name, `query_tags` → list of file paths that
//      also carry it.
// The standalone `nexus.tags` plugin remains live until step 6;
// this duplicate keeps users with the legacy plugin enabled
// unaffected during the rollout.
//
// Click a tag header to expand its co-occurrence list; click a file
// name in that list to open it.

import { useEffect, useState } from 'react'
import { useActiveFileQuery } from '../../_lib/useActiveFileQuery'
import { useEventBus } from '../eventBus'

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

export function TagsSection() {
  const events = useEventBus()
  const { data: tags, loading, error, activeRelpath } = useActiveFileQuery<TagUsage[]>({
    fetch: async (kernel, relpath) => {
      const raw = await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'read_frontmatter', { path: relpath })
      const names = extractTags(raw)
      if (names.length === 0) return []
      return Promise.all(
        names.map(async (name) => {
          try {
            const results = await kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'query_tags', { name })
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
    },
    initial: [],
  })
  const [expanded, setExpanded] = useState<string | null>(null)
  // Drop the expanded-tag selection when the active file changes —
  // a tag name from file A isn't meaningful for file B's tag set.
  useEffect(() => { setExpanded(null) }, [activeRelpath])

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
