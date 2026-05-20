// Backlinks section for the Note Context accordion.
//
// Reads from `useBacklinksDataStore`, which is populated by an
// always-on subscriber in `noteContext/backlinksLoader.ts`. Lazy-load
// is partly forfeited for this section in exchange for an
// always-current backlinks-count indicator in `RightPanelFooter` /
// `FileStats` (see backlinksLoader.ts for the trade-off rationale).
//
// What's NOT in this section yet (follow-up commits):
//   - BL-049 phase 4 block-filter mode (toggle between `backlinks`
//     and `backlinks_to_block` IPCs to narrow to a specific block id).
//   - On-edit silent refresh — subscribe to editor session changes
//     and silently re-fetch.

import { useBacklinksDataStore } from '../backlinksDataStore'
import { useEventBus } from '../eventBus'

export function BacklinksSection() {
  const events = useEventBus()
  const currentRelpath = useBacklinksDataStore((s) => s.currentRelpath)
  const links = useBacklinksDataStore((s) => s.links)
  const loading = useBacklinksDataStore((s) => s.loading)
  const error = useBacklinksDataStore((s) => s.error)

  if (!currentRelpath) {
    return (
      <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>
        Open a file to see its backlinks.
      </div>
    )
  }
  if (error) {
    return <div style={{ padding: 16, fontSize: 12, color: 'var(--text-error)' }}>{error}</div>
  }
  if (loading) {
    return <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>Loading…</div>
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
