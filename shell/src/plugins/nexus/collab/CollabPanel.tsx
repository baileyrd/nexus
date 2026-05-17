import { useMemo, type CSSProperties } from 'react'
import { useCollabStore, type CollabPeer, type ConnectionState } from './collabStore'

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

// ── Components ────────────────────────────────────────────────────────────────

function PeerRow({ peer }: { peer: CollabPeer }) {
  const cursor = peer.cursor
  const subtitle = cursor
    ? cursor.block_id
      ? `${cursor.relpath} · #${cursor.block_id}`
      : cursor.relpath
    : 'not focused'
  const initial = peer.display_name.slice(0, 1).toUpperCase() || '?'
  return (
    <li style={ROW}>
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
