import { useCallback, useMemo, useState, type CSSProperties } from 'react'
import {
  useCollabStore,
  type CollabPeer,
  type ConnectionState,
  type RelayStatus,
} from './collabStore'
import { getCollabApi } from './collabRuntime'

const COLLAB_PLUGIN_ID = 'com.nexus.collab'

// ── Connection-state pill ─────────────────────────────────────────────────────

interface StatusDesc {
  label: string
  fg: string
  bg: string
}

function describeConnection(state: ConnectionState): StatusDesc {
  switch (state) {
    case 'connected':
      return { label: 'Connected',     fg: '#0a6f1a', bg: 'rgba(35,180,80,0.18)' }
    case 'connecting':
      return { label: 'Connecting…',   fg: '#7a5a00', bg: 'rgba(240,180,30,0.22)' }
    case 'disconnected':
      return { label: 'Disconnected',  fg: '#a83232', bg: 'rgba(220,70,70,0.18)' }
    case 'idle':
      return { label: 'Not configured', fg: 'var(--text-muted)', bg: 'transparent' }
  }
}

// ── Styles ────────────────────────────────────────────────────────────────────

const PANEL: CSSProperties = {
  height: '100%',
  display: 'flex',
  flexDirection: 'column',
  background: 'var(--background-primary)',
  color: 'var(--text-normal)',
}

const HEADER: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'space-between',
  padding: '8px 12px',
  borderBottom: '1px solid var(--background-modifier-border)',
  flexShrink: 0,
}

const TITLE: CSSProperties = {
  fontSize: 12,
  fontWeight: 600,
  textTransform: 'uppercase',
  letterSpacing: 0.5,
  color: 'var(--text-muted)',
}

function statusStyle(desc: StatusDesc): CSSProperties {
  return {
    fontSize: 11,
    padding: '2px 8px',
    borderRadius: 10,
    color: desc.fg,
    background: desc.bg,
  }
}

const EMPTY: CSSProperties = {
  padding: '16px 12px',
  fontSize: 12,
  color: 'var(--text-muted)',
}

const LIST: CSSProperties = {
  listStyle: 'none',
  margin: 0,
  padding: 0,
  overflowY: 'auto',
  flex: 1,
}

const ROW: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 10,
  padding: '8px 12px',
  borderBottom: '1px solid var(--background-modifier-border-hover, transparent)',
}

const AVATAR: CSSProperties = {
  width: 24,
  height: 24,
  borderRadius: 12,
  flexShrink: 0,
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  fontSize: 11,
  fontWeight: 600,
  background: 'var(--background-modifier-hover)',
  color: 'var(--text-normal)',
}

const META: CSSProperties = {
  minWidth: 0,
  flex: 1,
}

const NAME: CSSProperties = {
  fontSize: 13,
  fontWeight: 500,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
}

const FOCUS: CSSProperties = {
  fontSize: 11,
  color: 'var(--text-muted)',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
}

// ── Share-this-forge styles ──────────────────────────────────────────────────

const SHARE_BAR: CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
  padding: '10px 12px',
  borderBottom: '1px solid var(--background-modifier-border)',
  flexShrink: 0,
}

const SHARE_ROW: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  gap: 8,
}

const BTN: CSSProperties = {
  padding: '4px 10px',
  fontSize: 12,
  borderRadius: 4,
  border: '1px solid var(--background-modifier-border)',
  background: 'var(--background-modifier-form-field)',
  color: 'var(--text-normal)',
  cursor: 'pointer',
}

const BTN_PRIMARY: CSSProperties = {
  ...BTN,
  background: 'var(--interactive-accent)',
  borderColor: 'var(--interactive-accent)',
  color: 'var(--text-on-accent)',
}

const URL_PILL: CSSProperties = {
  flex: 1,
  minWidth: 0,
  fontFamily: 'var(--font-monospace, monospace)',
  fontSize: 11,
  padding: '4px 8px',
  background: 'var(--background-secondary)',
  borderRadius: 4,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
  userSelect: 'all',
}

const ERROR_TEXT: CSSProperties = {
  fontSize: 11,
  color: 'var(--text-error, #c44)',
}

// ── Components ────────────────────────────────────────────────────────────────

// ── Share-this-forge controls ────────────────────────────────────────────────

function ShareBar({ relay }: { relay: RelayStatus | null }) {
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)

  const start = useCallback(async () => {
    setBusy(true)
    setError(null)
    try {
      const status = await getCollabApi().kernel.invoke<RelayStatus>(
        COLLAB_PLUGIN_ID,
        'start_relay',
        {},
      )
      useCollabStore.getState().onRelayStatus(status)
    } catch (e) {
      setError(String((e as Error)?.message ?? e))
    } finally {
      setBusy(false)
    }
  }, [])

  const stop = useCallback(async () => {
    setBusy(true)
    setError(null)
    try {
      const status = await getCollabApi().kernel.invoke<RelayStatus>(
        COLLAB_PLUGIN_ID,
        'stop_relay',
        {},
      )
      useCollabStore.getState().onRelayStatus(status)
    } catch (e) {
      setError(String((e as Error)?.message ?? e))
    } finally {
      setBusy(false)
    }
  }, [])

  const copyUrl = useCallback(async () => {
    if (!relay?.url) return
    try {
      await navigator.clipboard.writeText(relay.url)
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch (e) {
      setError(`copy failed: ${String((e as Error)?.message ?? e)}`)
    }
  }, [relay?.url])

  if (relay?.running && relay.url) {
    return (
      <div style={SHARE_BAR}>
        <div style={SHARE_ROW}>
          <span style={URL_PILL} title={relay.url}>{relay.url}</span>
          <button type="button" style={BTN} disabled={busy} onClick={() => void copyUrl()}>
            {copied ? 'Copied' : 'Copy'}
          </button>
        </div>
        <div style={SHARE_ROW}>
          <span style={{ fontSize: 11, color: 'var(--text-muted)', flex: 1 }}>
            Share this URL with peers on your network to let them join the forge.
          </span>
          <button type="button" style={BTN} disabled={busy} onClick={() => void stop()}>
            Stop sharing
          </button>
        </div>
        {error ? <div style={ERROR_TEXT}>{error}</div> : null}
      </div>
    )
  }

  return (
    <div style={SHARE_BAR}>
      <div style={SHARE_ROW}>
        <span style={{ fontSize: 12, color: 'var(--text-muted)', flex: 1 }}>
          Start a local relay so peers on your network can join this forge.
        </span>
        <button type="button" style={BTN_PRIMARY} disabled={busy} onClick={() => void start()}>
          {busy ? 'Starting…' : 'Share this forge'}
        </button>
      </div>
      {error ? <div style={ERROR_TEXT}>{error}</div> : null}
    </div>
  )
}

/**
 * C64 — jump to where a peer is working: opens their file (if not
 * already the active tab) and scrolls to their caret offset. No-op
 * when the peer has no cursor (nothing to jump to).
 */
function jumpToPeer(cursor: NonNullable<CollabPeer['cursor']>): void {
  const api = getCollabApi()
  const name = cursor.relpath.split(/[\\/]/).filter((s) => s.length > 0).pop() ?? cursor.relpath
  api.events.emit('files:open', { relpath: cursor.relpath, name })
  if (cursor.offset !== undefined) {
    api.events.emit('nexus.editor:reveal-offset', {
      relpath: cursor.relpath,
      offset: cursor.offset,
    })
  }
}

function PeerRow({ peer }: { peer: CollabPeer }) {
  const cursor = peer.cursor
  const subtitle = cursor
    ? cursor.block_id
      ? `${cursor.relpath} · #${cursor.block_id}`
      : cursor.relpath
    : 'not focused'
  const initial = peer.display_name.slice(0, 1).toUpperCase() || '?'
  const clickable = cursor !== undefined
  return (
    <li
      style={clickable ? { ...ROW, cursor: 'pointer' } : ROW}
      onClick={clickable ? () => jumpToPeer(cursor) : undefined}
      onKeyDown={
        clickable
          ? (e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault()
                jumpToPeer(cursor)
              }
            }
          : undefined
      }
      role={clickable ? 'button' : undefined}
      tabIndex={clickable ? 0 : undefined}
      title={clickable ? `Jump to ${peer.display_name}` : undefined}
    >
      <div style={AVATAR} aria-hidden>{initial}</div>
      <div style={META}>
        <div style={NAME}>{peer.display_name}</div>
        <div style={FOCUS} title={subtitle}>{subtitle}</div>
      </div>
    </li>
  )
}

export function CollabPanel() {
  const connection = useCollabStore((s) => s.connection)
  const peersMap   = useCollabStore((s) => s.peers)
  const relay      = useCollabStore((s) => s.relay)

  const peers = useMemo(
    () => Object.values(peersMap).sort((a, b) => a.display_name.localeCompare(b.display_name)),
    [peersMap],
  )
  const desc = describeConnection(connection)

  return (
    <div style={PANEL}>
      <header style={HEADER}>
        <div style={TITLE}>Collaboration</div>
        <div style={statusStyle(desc)}>{desc.label}</div>
      </header>

      <ShareBar relay={relay} />

      {peers.length === 0 ? (
        <div style={EMPTY}>
          {connection === 'connected'
            ? 'No other peers connected.'
            : connection === 'idle'
              ? 'Set [collab] in .forge/config.toml to share this forge.'
              : 'Waiting for peers…'}
        </div>
      ) : (
        <ul style={LIST}>
          {peers.map((p) => <PeerRow key={p.user_id} peer={p} />)}
        </ul>
      )}
    </div>
  )
}
