# ADR 0034: Live Collaboration Network Transport — Direct WebSocket Relay

**Date:** 2026-05-23
**Status:** Accepted (Phase 1–2 shipped)
**Related:** BL-143 (collaboration transport), BL-144 (ACP outbound), BL-145 (MCP inbound), ADR 0026 (CRDT layer), PRD-08 §8 (collaborative editing)

## Context

PRD-08 §8 requires that two Nexus sessions on the same forge converge to identical state when editing the same document. ADR 0026 established the in-process CRDT layer (`nexus-crdt` — op-log, RGA, version vectors) and the in-process sync infrastructure (`nexus-collab` — relay server, presence, client bridge). 

The remaining problem: how do two **processes** (potentially on different machines, connected over a network) exchange CRDT operations, presence data, and cursor information reliably?

The CRDT layer requires an envelope for serialising operations between processes. A direct WebSocket relay between the two Nexus runtimes (host and guest) was chosen.

## Decision

A **direct peer-to-peer WebSocket relay** model:

### Phase 1 — Relay server

- `nexus collab serve` — starts a `RelayServer` that listens on `0.0.0.0:<port>` (or a free OS-assigned port)
- Generates a **UUID-v4 authentication token** — clients must prove they have this token via the `nexus collab join` command
- Detects the LAN IP via a UDP-connect probe (falls back to `127.0.0.1` for loopback-only scenarios)
- Idempotent: calling `serve` while a relay is active returns the existing status (prevents window-vs-popout port conflicts)

### Phase 1.5 — Client bridge

- `nexus collab join <relay_url>` — connects the local runtime to the relay
- Registers a `CollabClient` that:
  - Maintains an `EventSubscription` on `com.nexus.editor.ops.` from the local kernel bus
  - Serialises `OpEnvelope { op: CrdtOp }` as JSON
  - Writes to the relay's WebSocket connection
  - Receives broadcasted ops from peers and pushes them into the local kernel bus
- Handles reconnection with a **reconnect buffer** — ops received during disconnection are queued and replayed on reconnect (see `tests/reconnect.rs::reconnect_replays_buffered_events_after_relay_outage`)

### Phase 2 — Presence and cursor

- The relay publishes presence events (`PeerInfo { name, cursor_pos, active_files }`) every ~200ms
- The shell's `cursorPublisher` debounces cursor updates to ≤150ms intervals
- The `CollabPanel` lists connected peers and their active files
- Remote cursors are rendered in the editor within 200ms of the sender's update

### Phase 2.3 — Share UI

- Three new IPC handlers on `com.nexus.collab`:
  - `start_relay({ port? })` — start the relay, return status
  - `stop_relay()` — shutdown the relay, broadcast `com.nexus.collab.relay.stopped`
  - `join_relay({ url, token })` — connect this runtime to the relay

### Security

- **TLS is deferred** — the relay uses bare `ws://` only (insecure). This is noted in `Cargo.toml` and tracked as a separate requirement. The relay should not be exposed to untrusted networks in production until TLS is added.
- **Authentication**: UUID-v4 token, generated per-serve. Validated during handshake. Bad tokens are rejected (tested in `tests/dod.rs::dod_handshake_with_bad_token_is_rejected`).
- **LAN-only by default**: the relay binds `0.0.0.0` but the URL returned is typically a LAN address. No internet-facing deployment without TLS.

## Consequences

### Positive

- **Simple architecture.** No external services required for collaboration. The relay is a first-class feature of the Nexus binary — "nexus collab serve" starts it.
- **Low latency.** Direct WebSocket between peers avoids the round-trip through a third-party service. Cursor updates arrive within 150ms.
- **CRDT-native.** The relay just shuffles `OpEnvelope` bytes. No transformation, no conflict resolution at the relay layer — all logic stays in `nexus-crdt`.
- **Composable.** The relay can serve any number of peers, not just two. Future features (multi-user canvases, group editing) can piggyback on the same transport.

### Negative / costs

- **No TLS yet.** Bare WebSocket means the relay should not run on untrusted networks. The Cargo.toml explicitly notes this.
- **Firewall / NAT traversal.** On complex networks, peers may not be able to connect to the LAN-hosted relay. No STUN/TURN support yet.
- **Process-local transport.** The relay runs inside the host Nimbus runtime, not as a separate daemon. If the host crashes, the relay goes down too.

## Alternatives Considered

### A. Third-party collaboration service (e.g., SharedCanvas, CRDT.cloud)

**Rejected** because it introduces an external dependency, latency, and potential single-point-of-failure. The peer-to-peer approach is simpler to deploy and maintain.

### B. Git-based sync (CRDT-over-Git)

**Deferred.** ADR 0026 mentions op-log compaction and merge driver support for git-based sync (BL-007). This is the right approach for "share a forge over git" but not for live, low-latency collaboration. The transport is separate from the persistence layer.

### C. WebRTC data channels

**Rejected for now.** WebRTC would handle NAT traversal better and offer P2P connections directly, but adds significant complexity for a feature whose primary use case is LAN-based collaboration today.

## Open follow-ups

1. **TLS for the relay.** Add `rustls` TLS support to the relay server. The Cargo.toml note about deferred TLS should be migrated to a backlog ticket.
2. **STUN/TURN for NAT traversal.** Required for collaboration over public networks.
3. **Test the TLS path.** The audit flagged `nexus-collab` as one of the seven critical crates with no tests. Add TLS-aware collaboration tests once the TLS stack is added.
4. **Consider a dedicated relay daemon** in the long term for better resilience (crash recovery, scalability to many peers).
