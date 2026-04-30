import { useBacklinksStore, type Backlink } from './backlinksStore'
import { eventBus } from '../../../host/EventBus'

const EVENT_FILE_OPEN = 'files:open'

/** Basename of a forge-relative path. Forward-slash only. */
function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

/**
 * Right-panel inspector listing every file that links TO the
 * currently-open editor tab. Rows mirror the search plugin's shape so
 * the inspector feels consistent with other file-list surfaces.
 *
 * Row click emits `files:open` on the shell bus — the editor plugin
 * picks it up and opens (or raises) the corresponding tab.
 */
export function BacklinksView() {
  const currentRelpath = useBacklinksStore((s) => s.currentRelpath)
  const links = useBacklinksStore((s) => s.links)
  const loading = useBacklinksStore((s) => s.loading)
  const error = useBacklinksStore((s) => s.error)
  const blockFilter = useBacklinksStore((s) => s.blockFilter)
  const setBlockFilter = useBacklinksStore((s) => s.setBlockFilter)

  const header = currentRelpath ? (
    <div
      style={{
        padding: '8px 14px',
        borderBottom: '1px solid var(--line-soft)',
        fontSize: 11,
        fontFamily: 'var(--f-ui)',
        color: 'var(--fg-dim)',
        display: 'flex',
        flexDirection: 'column',
        gap: 4,
      }}
      title={currentRelpath}
    >
      <div
        style={{
          whiteSpace: 'nowrap',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
        }}
      >
        Backlinks to{' '}
        <span style={{ color: 'var(--fg)' }}>{basename(currentRelpath)}</span>
      </div>
      {blockFilter && (
        <ActiveBlockFilterChip
          blockId={blockFilter}
          onClear={() => setBlockFilter(null)}
        />
      )}
    </div>
  ) : null

  // Empty-state precedence: no active file > error > loading >
  // no matches > results. Errors win over loading so a stale
  // loading flag never hides a surfaced failure.
  let body: React.ReactNode
  if (!currentRelpath) {
    body = (
      <StateMessage color="var(--fg-dim)">
        Open a file to see its backlinks.
      </StateMessage>
    )
  } else if (error) {
    body = <StateMessage color="var(--risk)">{error}</StateMessage>
  } else if (loading) {
    body = <StateMessage color="var(--fg-muted)">Loading…</StateMessage>
  } else if (links.length === 0) {
    body = <StateMessage color="var(--fg-dim)">No backlinks found.</StateMessage>
  } else {
    body = (
      <div style={{ overflowY: 'auto', flex: 1 }}>
        {links.map((link, idx) => (
          <BacklinkRow
            key={`${link.sourceRelpath}::${idx}`}
            link={link}
            onPick={() =>
              eventBus.emit(EVENT_FILE_OPEN, {
                relpath: link.sourceRelpath,
                name: link.sourceName,
              })
            }
          />
        ))}
      </div>
    )
  }

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        width: '100%',
      }}
    >
      {header}
      {body}
    </div>
  )
}

function StateMessage({
  children,
  color,
}: {
  children: React.ReactNode
  color: string
}) {
  return (
    <div
      style={{
        padding: '12px 14px',
        color,
        fontFamily: 'var(--f-ui)',
        fontSize: 12,
      }}
    >
      {children}
    </div>
  )
}

interface BacklinkRowProps {
  link: Backlink
  onPick: () => void
}

/** BL-049 phase 4 — chip rendered in the header when a per-block filter
 *  is active. Truncates the UUID to 8 chars to match `FragmentPill`'s
 *  legibility convention; the `×` clears the filter, which the store
 *  subscription in `index.ts` picks up to re-issue an unfiltered load. */
export function ActiveBlockFilterChip({
  blockId,
  onClear,
}: {
  blockId: string
  onClear: () => void
}) {
  const label = `^${blockId.slice(0, 8)}…`
  return (
    <span
      title={`Filtered to block ${blockId}`}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 4,
        padding: '0 4px 0 6px',
        borderRadius: 999,
        border: '1px solid var(--line-soft)',
        background: 'var(--accent-soft, var(--bg-raised))',
        color: 'var(--fg)',
        fontSize: 10,
        fontFamily: 'var(--f-mono)',
        lineHeight: '16px',
        alignSelf: 'flex-start',
      }}
    >
      <span>{label}</span>
      <button
        type="button"
        onClick={onClear}
        aria-label="Clear block filter"
        style={{
          all: 'unset',
          cursor: 'pointer',
          padding: '0 4px',
          fontSize: 11,
          lineHeight: '14px',
          color: 'var(--fg-dim)',
        }}
      >
        ×
      </button>
    </span>
  )
}

function BacklinkRow({ link, onPick }: BacklinkRowProps) {
  // We don't have a content excerpt — the kernel returns graph-edge
  // metadata only. Surface the inbound link text instead; if a note
  // wrote `[[My Note|display]]` we render "display" here, clamped to
  // three lines for long display texts.
  const showExcerpt = link.linkText.trim().length > 0

  return (
    <div
      role="button"
      tabIndex={-1}
      onClick={onPick}
      style={{
        padding: '8px 14px',
        cursor: 'pointer',
        background: 'transparent',
        transition: 'background 0.06s',
        fontFamily: 'var(--f-ui)',
        borderBottom: '1px solid var(--line-soft)',
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = 'var(--bg-hover)'
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = 'transparent'
      }}
    >
      <div
        style={{
          color: 'var(--fg)',
          fontSize: 13,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {link.sourceName || link.sourceRelpath}
      </div>
      <div
        style={{
          color: 'var(--fg-dim)',
          fontSize: 11,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
          marginTop: 1,
        }}
      >
        {link.sourceRelpath}
      </div>
      {link.fragment && (
        <div style={{ marginTop: 4 }}>
          <FragmentPill fragment={link.fragment} />
        </div>
      )}
      {showExcerpt && (
        <div
          style={{
            color: 'var(--fg-muted)',
            fontSize: 12,
            marginTop: 4,
            display: '-webkit-box',
            WebkitLineClamp: 3,
            WebkitBoxOrient: 'vertical',
            overflow: 'hidden',
          }}
        >
          {link.linkText}
        </div>
      )}
    </div>
  )
}

/** Compact pill rendering the source link's anchor fragment. Two
 *  visual variants — a "block" pill for `^<uuid>` BL-049 anchors
 *  (truncated to the first 8 hex characters so the pill stays
 *  legible) and a "heading" pill for plain heading slugs. Pure
 *  function so the view tests can pin the rendered text without a
 *  full DOM snapshot.
 *
 *  BL-049 phase 4 — when rendered inside a `BacklinkRow` the pill is
 *  clickable for block anchors: clicking sets the per-block filter on
 *  `useBacklinksStore`. Heading anchors stay non-interactive (no
 *  matching IPC surface yet). The click handler stops propagation so
 *  the surrounding row's "open file" handler doesn't also fire. */
export function FragmentPill({ fragment }: { fragment: string }) {
  const isBlock = fragment.startsWith('^')
  const label = isBlock ? `^${fragment.slice(1, 9)}…` : `#${fragment}`
  const titleBase = isBlock
    ? `Block anchor ${fragment}`
    : `Heading anchor ${fragment}`
  const setBlockFilter = useBacklinksStore((s) => s.setBlockFilter)
  const onClick = isBlock
    ? (event: React.MouseEvent) => {
        event.stopPropagation()
        setBlockFilter(fragment.slice(1))
      }
    : undefined

  return (
    <span
      role={isBlock ? 'button' : undefined}
      tabIndex={isBlock ? 0 : undefined}
      onClick={onClick}
      title={isBlock ? `${titleBase} — click to filter` : titleBase}
      style={{
        display: 'inline-block',
        padding: '0 6px',
        borderRadius: 999,
        border: '1px solid var(--line-soft)',
        background: isBlock ? 'var(--accent-soft, var(--bg-raised))' : 'var(--bg-raised)',
        color: 'var(--fg-muted)',
        fontSize: 10,
        fontFamily: 'var(--f-mono)',
        lineHeight: '16px',
        cursor: isBlock ? 'pointer' : 'default',
      }}
    >
      {label}
    </span>
  )
}
