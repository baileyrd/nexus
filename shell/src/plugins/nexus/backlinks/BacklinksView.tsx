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

  const header = currentRelpath ? (
    <div
      style={{
        padding: '8px 14px',
        borderBottom: '1px solid var(--line-soft)',
        fontSize: 11,
        fontFamily: 'var(--f-ui)',
        color: 'var(--fg-dim)',
        whiteSpace: 'nowrap',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
      }}
      title={currentRelpath}
    >
      Backlinks to{' '}
      <span style={{ color: 'var(--fg)' }}>{basename(currentRelpath)}</span>
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
    body = <StateMessage color="var(--fg-dim)">No backlinks.</StateMessage>
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
