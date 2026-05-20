// Backlinks section for the Note Context accordion.
//
// Reads from `useBacklinksDataStore`, which is populated by an
// always-on subscriber in `noteContext/backlinksLoader.ts`. Lazy-load
// is partly forfeited for this section in exchange for an
// always-current backlinks-count indicator in `RightPanelFooter` /
// `FileStats` (see backlinksLoader.ts for the trade-off rationale).
//
// BL-049 phase 4 — block-filter mode. Clicking a `^<block-id>`
// fragment chip on a row sets `blockFilter` in the store; the
// loader notices and re-issues with `backlinks_to_block`. The active
// filter renders as a header chip with `×` to clear. Heading-anchor
// fragments (e.g. `#some-heading`) are non-interactive — the kernel
// only narrows by block id, not by heading slug.
//
// What's NOT in this section yet (one follow-up commit):
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
  const blockFilter = useBacklinksDataStore((s) => s.blockFilter)
  const setBlockFilter = useBacklinksDataStore((s) => s.setBlockFilter)

  // Active filter chip — only render the header strip when a filter
  // is set, so unfiltered usage stays visually clean.
  const filterChip = blockFilter ? (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 6,
        padding: '6px 10px',
        borderBottom: '1px solid var(--background-modifier-border)',
        fontSize: 11,
        color: 'var(--text-muted)',
      }}
    >
      <span>Filtered to block</span>
      <span
        style={{
          padding: '2px 6px',
          borderRadius: 4,
          background: 'var(--background-secondary)',
          color: 'var(--text-normal)',
          fontFamily: 'var(--font-monospace)',
        }}
      >
        ^{blockFilter.slice(0, 8)}
      </span>
      <button
        type="button"
        onClick={() => setBlockFilter(null)}
        title="Clear block filter"
        aria-label="Clear block filter"
        style={{
          border: 0,
          background: 'transparent',
          color: 'var(--text-muted)',
          cursor: 'pointer',
          padding: '0 4px',
          fontSize: 14,
          lineHeight: 1,
        }}
      >
        ×
      </button>
    </div>
  ) : null

  if (!currentRelpath) {
    return (
      <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>
        Open a file to see its backlinks.
      </div>
    )
  }
  if (error) {
    return (
      <>
        {filterChip}
        <div style={{ padding: 16, fontSize: 12, color: 'var(--text-error)' }}>{error}</div>
      </>
    )
  }
  if (loading) {
    return (
      <>
        {filterChip}
        <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>Loading…</div>
      </>
    )
  }
  if (links.length === 0) {
    return (
      <>
        {filterChip}
        <div style={{ padding: 16, fontSize: 12, color: 'var(--text-faint)' }}>
          {blockFilter ? 'No backlinks for that block.' : 'No backlinks found.'}
        </div>
      </>
    )
  }
  return (
    <>
      {filterChip}
      <div style={{ padding: 4, fontSize: 13 }}>
        {links.map((link, idx) => {
          // BL-049 phase 4 — block-anchored fragments are clickable
          // to narrow the panel to that block id. Heading-anchored
          // fragments stay non-interactive (kernel only narrows by
          // block id).
          const isBlockAnchor =
            link.fragment !== null && link.fragment.startsWith('^')
          const blockId = isBlockAnchor && link.fragment
            ? link.fragment.slice(1)
            : null
          return (
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
                    <span
                      onClick={
                        blockId
                          ? (e) => {
                              e.stopPropagation()
                              setBlockFilter(blockId)
                            }
                          : undefined
                      }
                      role={blockId ? 'button' : undefined}
                      title={blockId ? 'Filter to this block' : undefined}
                      style={{
                        color: 'var(--text-faint)',
                        cursor: blockId ? 'pointer' : 'default',
                        textDecoration: blockId ? 'underline dotted' : 'none',
                      }}
                    >
                      {link.fragment.startsWith('^') ? link.fragment : `#${link.fragment}`}
                      {link.linkText ? ' · ' : ''}
                    </span>
                  )}
                  {link.linkText}
                </div>
              )}
            </div>
          )
        })}
      </div>
    </>
  )
}
