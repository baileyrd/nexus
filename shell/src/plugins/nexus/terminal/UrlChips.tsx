import { clientLogger } from '../../../clientLogger'
import type { UrlMatch } from './urls'

/**
 * BL-058: pill chip strip for the terminal output's most recent URLs.
 *
 * Renders above the xterm container, single line, horizontally
 * scrollable on overflow. Click a chip → `openExternal(target)` opens
 * the URL in the user's default handler (browser, file manager, SSH
 * client). Empty list → strip is hidden so it doesn't claim vertical
 * space when nothing's been detected.
 *
 * `openExternal` is supplied by the plugin host (it routes through
 * `api.platform.shell.openExternal`) so this component never imports
 * `@tauri-apps/*` directly — the plugin-import-hygiene guardrail
 * keeps everything on the platform-API path.
 */
export interface UrlChipsProps {
  urls: UrlMatch[]
  openExternal: (target: string) => Promise<void>
  onDismiss?: () => void
}

export function UrlChips({ urls, openExternal, onDismiss }: UrlChipsProps) {
  if (urls.length === 0) return null

  const handleClick = (m: UrlMatch) => {
    void openExternal(m.resolved).catch((err) => {
      clientLogger.warn('[Terminal] open URL failed:', m.resolved, err)
    })
  }

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 6,
        padding: '4px 8px',
        background: 'var(--background-secondary)',
        borderBottom: '1px solid var(--background-modifier-border)',
        overflowX: 'auto',
        overflowY: 'hidden',
        flexShrink: 0,
        scrollbarWidth: 'thin',
      }}
    >
      <span
        style={{
          fontFamily: 'var(--font-interface)',
          fontSize: 11,
          color: 'var(--text-muted)',
          flexShrink: 0,
          marginRight: 2,
        }}
      >
        Links
      </span>
      {urls.map((m) => (
        <UrlChip key={`${m.resolved}#${m.start}`} match={m} onClick={() => handleClick(m)} />
      ))}
      {onDismiss && (
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Clear URL chips"
          title="Clear"
          style={{
            marginLeft: 'auto',
            background: 'transparent',
            border: 'none',
            color: 'var(--text-muted)',
            cursor: 'pointer',
            fontSize: 12,
            padding: '2px 6px',
            flexShrink: 0,
          }}
        >
          ×
        </button>
      )}
    </div>
  )
}

function UrlChip({ match, onClick }: { match: UrlMatch; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={match.resolved}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 4,
        padding: '2px 8px',
        fontSize: 11,
        fontFamily: 'var(--font-interface)',
        background: 'var(--interactive-normal)',
        color: 'var(--text-normal)',
        border: '1px solid var(--background-modifier-border)',
        borderRadius: 999,
        cursor: 'pointer',
        whiteSpace: 'nowrap',
        flexShrink: 0,
        maxWidth: 280,
        overflow: 'hidden',
        textOverflow: 'ellipsis',
      }}
    >
      <span style={{ opacity: 0.65, fontSize: 10 }}>{kindIcon(match.kind)}</span>
      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{displayText(match)}</span>
    </button>
  )
}

function kindIcon(kind: UrlMatch['kind']): string {
  switch (kind) {
    case 'HttpHttps':
      return '↗'
    case 'File':
      return '📄'
    case 'Localhost':
      return '⌂'
  }
}

function displayText(m: UrlMatch): string {
  // Trim the scheme for compactness; the full target is in the title.
  if (m.kind === 'HttpHttps') {
    return m.raw.replace(/^https?:\/\//, '')
  }
  if (m.kind === 'File') {
    return m.raw.replace(/^file:\/\//, '')
  }
  return m.raw
}
