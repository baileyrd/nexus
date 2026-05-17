// BL-143 Phase 2.1 — Zustand store for the `nexus.collab` panel.
//
// Subscribes (via the plugin's `activate` wiring in index.ts) to four
// kernel bus topics:
//
//   com.nexus.collab.peers.joined   — { peer_id, display_name }
//   com.nexus.collab.peers.left     — { peer_id }
//   com.nexus.collab.presence       — { user_id, display_name, cursor? }
//   com.nexus.collab.connection     — { state }
//
// We keep peers as a Map keyed by user_id so a presence update before
// the corresponding peer-joined frame still surfaces in the UI (the
// relay always sends PeerJoined first, but the local-author's own
// presence is published directly on the local bus without a joined
// event — keying off the union of both topics avoids a special case).

import { create } from 'zustand'

// ── Wire payload types (mirror crates/nexus-collab/src/{presence,protocol}.rs) ──

export interface PresenceCursor {
  relpath: string
  block_id?: string
  /** Character offset of the caret (CodeMirror `EditorSelection.main.head`). */
  offset?: number
  /** Other end of the selection range when not a caret. */
  selection_end?: number
}

export interface PresenceEvent {
  user_id: string
  display_name: string
  cursor?: PresenceCursor
}

export interface PeerInfo {
  peer_id: string
  display_name: string
}

export interface PeerLeft {
  peer_id: string
}

export type ConnectionState = 'connecting' | 'connected' | 'disconnected' | 'idle'

export interface ConnectionPayload {
  state: 'connecting' | 'connected' | 'disconnected'
}

/// BL-143 Phase 2.3 — relay-host status. Mirrors
/// `crates/nexus-collab/src/core_plugin.rs::RelayStatus`. Carried
/// on the `com.nexus.collab.relay.started` and `…relay.stopped`
/// bus events, and returned by the `start_relay` / `stop_relay` /
/// `relay_status` IPC handlers.
export interface RelayStatus {
  running: boolean
  url?: string | null
  host?: string | null
  port?: number | null
  token?: string | null
}

// ── Store model ────────────────────────────────────────────────────────────────

export interface CollabPeer {
  user_id: string
  display_name: string
  cursor?: PresenceCursor
  /** Most recent time a presence/joined frame touched this peer (ms epoch). */
  last_seen_ms: number
}

interface CollabState {
  /** `'idle'` until the first `com.nexus.collab.connection` frame arrives.
   *  Distinguishes "collab is disabled / never wired" from "trying to connect". */
  connection: ConnectionState
  peers: Record<string, CollabPeer>
  /** BL-143 Phase 2.3 — `null` until the first `relay_status` / start /
   *  stop event lands; thereafter mirrors the backend. `running: false`
   *  is the steady "no share live" state. */
  relay: RelayStatus | null

  onPeerJoined(info: PeerInfo): void
  onPeerLeft(payload: PeerLeft): void
  onPresence(ev: PresenceEvent): void
  onConnection(payload: ConnectionPayload): void
  onRelayStatus(status: RelayStatus): void
  reset(): void
}

export const useCollabStore = create<CollabState>((set) => ({
  connection: 'idle',
  peers: {},
  relay: null,

  onPeerJoined: (info) =>
    set((s) => ({
      peers: {
        ...s.peers,
        [info.peer_id]: {
          user_id: info.peer_id,
          display_name: info.display_name,
          cursor: s.peers[info.peer_id]?.cursor,
          last_seen_ms: Date.now(),
        },
      },
    })),

  onPeerLeft: ({ peer_id }) =>
    set((s) => {
      if (!s.peers[peer_id]) return s
      const next = { ...s.peers }
      delete next[peer_id]
      return { peers: next }
    }),

  onPresence: (ev) =>
    set((s) => ({
      peers: {
        ...s.peers,
        [ev.user_id]: {
          user_id: ev.user_id,
          display_name: ev.display_name,
          cursor: ev.cursor,
          last_seen_ms: Date.now(),
        },
      },
    })),

  onConnection: ({ state }) => set({ connection: state }),

  onRelayStatus: (status) => set({ relay: status }),

  reset: () => set({ connection: 'idle', peers: {}, relay: null }),
}))
