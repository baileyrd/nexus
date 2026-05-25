# nexus-collab

> Kind: lib · IPC plugin id: com.nexus.collab · CorePlugin: yes · Has settings: yes (`[collab]` in `.forge/config.toml`) · As of: 2026-05-25

## Overview

`nexus-collab` is Nexus's live-collaboration network transport — the BL-143 work item. It ferries CRDT-op envelopes and presence updates between peers running on different machines over a WebSocket relay. The relay is deliberately **topic-agnostic**: every message on the wire is an opaque `payload` JSON tagged with a kernel-bus `topic` string, and the relay routes those envelopes between connected peers without ever inspecting the payload. The CRDT merge semantics (`com.nexus.editor.ops.<relpath>`) and presence semantics (`com.nexus.collab.presence`) live entirely in the consumers, which keeps this crate stable as those consumers grow new topics.

The crate bundles both halves of the connection. The **relay server** (`server.rs`, `RelayServer`) owns a `TcpListener`, accepts WebSocket peers, validates a shared-secret handshake, and broadcasts each peer's envelopes to all the others (never echoing a peer's own envelope back). The **client bridge** (`client.rs`, `CollabClient`) opens one WebSocket connection to a relay, subscribes to the local kernel `EventBus`, ships matching events outbound as envelopes, and re-publishes inbound envelopes back onto the bus — turning a remote relay into a transparent extension of the local event bus. A self-supervising variant (`reconnect_client.rs`, `ReconnectingClient`) wraps the bridge with exponential-backoff reconnection and a bounded outbound replay buffer so events published during an outage are not lost.

It composes with `nexus-crdt` and `nexus-editor` purely by convention, not by Cargo dependency. The editor's `CrdtPublisher` (wired in bootstrap) publishes op envelopes on `com.nexus.editor.ops.*`; the collab outbound bridge subscribes to that prefix and forwards them; the inbound bridge republishes received ops on the same topics so the editor's CRDT layer merges them. `nexus-collab` duplicates the `OPS_TOPIC_PREFIX` constant locally rather than depending on `nexus-crdt` (which would transitively pull `nexus-editor`), preserving microkernel leaf-isolation. Site-based self-echo dedup (dropping inbound ops whose `op.id.site` matches the local site) belongs to the consumer side and is configurable but left disabled by default in bootstrap.

Microkernel fit: the crate is a subsystem core plugin (`CollabCorePlugin`, id `com.nexus.collab`) registered by `nexus-bootstrap`. It depends only on `nexus-kernel` and `nexus-plugins`, never the reverse. All frontend-reachable capability flows through IPC handlers (`publish_presence`, `start_relay`, `stop_relay`, `relay_status`); the bridge and relay tasks talk to the rest of the system exclusively through the kernel `EventBus`. **Phase 1 is plain `ws://` only — no TLS (`wss://`), no per-user credentials, just a single static shared-secret token.** Those, plus multi-channel/hosted relays, are explicitly deferred to a later phase.

## Position in the dependency graph

- **Direct `nexus-*` deps:** `nexus-kernel` (EventBus, EventFilter, NexusEvent, RecvError, publish paths), `nexus-plugins` (CorePlugin trait, PluginError, `define_dispatch_helpers!`).
- **Notable external deps:** `tokio-tungstenite` (WebSocket client + server framing), `futures-util` (`SinkExt`/`StreamExt`, split sink/stream), `tokio` (net, sync, task), `uuid` (fresh per-share relay token in `start_relay`), `serde`/`serde_json`, `thiserror`, `tracing`. Optional `ts-rs` + `schemars` behind the `ts-export` feature emit TypeScript bindings + JSON Schema for the wire types (presence payloads and the IPC arg/reply structs).
- **Crates depending on it:** `nexus-bootstrap` — registers the core plugin (`src/plugins/collab.rs`) and wires the optional reconnecting bridge from `[collab]` config (`src/collab.rs`). No other crate links it; CLI/TUI/shell reach it only through the registered IPC handlers and bus topics.
- It deliberately does **not** depend on `nexus-crdt` or `nexus-editor` (constant duplication keeps it a leaf-adjacent subsystem). It references `nexus_remote::transport::MAX_LINE_BYTES` only in a doc-comment to explain the 16 MiB frame cap, not as a code dep.

## Public API surface

| Module | Item(s) | Purpose |
|--------|---------|---------|
| `protocol` | `ClientMessage` (`Hello`, `Envelope`), `ServerMessage` (`Hello`, `Envelope`, `PeerJoined`, `PeerLeft`, `Error`), `PeerInfo`, `ERR_AUTH`/`ERR_HANDSHAKE`/`ERR_BAD_FRAME` | The JSON wire protocol — one tagged message per WebSocket text frame; handshake + topic-tagged envelope relaying. |
| `auth` | `Token`, `TokenError` | Newtype wrapping the static shared secret; constant-time `verify`. Rejects empty tokens at construction. |
| `server` | `RelayServer`, `RelayServerError` | In-process topic-agnostic WebSocket relay: accept loop, per-peer read/write tasks, peer registry, echo suppression. |
| `client` | `CollabClient`, `CollabClientConfig`, `ConnectParams`, `ConnectError`, plugin-id consts (`COLLAB_PLUGIN_ID`, `COLLAB_BRIDGE_PLUGIN_ID`, `EDITOR_PLUGIN_ID`), `OPS_TOPIC_PREFIX`, `DEFAULT_HANDSHAKE_TIMEOUT` | One-shot relay connection + the local-bus⇄wire bridge tasks (outbound subscribe→ship, inbound receive→republish). |
| `reconnect_client` | `ReconnectingClient`, `ReconnectConfig`, `ConnectionState`, `CONNECTION_STATE_TOPIC` | Self-supervising bridge with exponential backoff + bounded outbound replay buffer; emits connection-state transitions on the bus. |
| `presence` | `PresenceEvent`, `PresenceCursor`, topic consts (`PRESENCE_TOPIC`, `PEER_JOINED_TOPIC`, `PEER_LEFT_TOPIC`, `COLLAB_TOPIC_PREFIX`) | Presence wire shape (peer id + display name + optional cursor) and the `com.nexus.collab.*` bus-topic vocabulary. |
| `url` | `WsEndpoint`, `parse` (re-exported as `parse_ws_url`) | Minimal `ws://host:port[/path][?token=…]` parser (no full URL dep). |
| `core_plugin` | `CollabCorePlugin`, `LocalPeer`, `PublishPresenceArgs`/`PublishPresenceReply`, `StartRelayArgs`, `RelayStatus`, `PLUGIN_ID`, `IPC_HANDLERS`, `HANDLER_*` consts, `RELAY_STARTED_TOPIC`/`RELAY_STOPPED_TOPIC` | The `com.nexus.collab` CorePlugin: presence stamping + in-process relay-host lifecycle. |

## IPC handlers

Registered by `nexus_bootstrap::plugins::collab::register` via `IPC_HANDLERS` (SD-06 single source of truth). All four also get `.v1` aliases (ADR 0021) pointing at the same handler id.

| command | handler id | args | returns | capability | description |
|---------|-----------|------|---------|------------|-------------|
| `publish_presence` | 1 | `PublishPresenceArgs { cursor: Option<PresenceCursor> }` (`deny_unknown_fields`; caller never sends identity) | `PublishPresenceReply { published: bool }` | none (core plugin, full native access) | Stamps the configured `[collab]` `LocalPeer` identity onto a `PresenceEvent` and publishes it on `PRESENCE_TOPIC`. When collab is unconfigured (no identity) returns a successful no-op with `published: false` rather than an error, so the shell's per-keystroke cursor publisher stops pinging without logging failures. |
| `start_relay` | 2 | `StartRelayArgs { port: Option<u16> }` (`deny_unknown_fields, default`; `None`/`0` ⇒ OS-picked free port) | `RelayStatus` (`running`, `url`, `host`, `port`, `token`) | none | Generates a fresh UUID-v4 token, binds `0.0.0.0:<port>`, spawns the accept loop on the ambient tokio runtime, broadcasts `RELAY_STARTED_TOPIC`, and returns a LAN-reachable `ws://host:port/?token=…` URL. Idempotent: a second call while running returns the existing status without re-binding. Errors with `ExecutionFailed` if no ambient tokio runtime. |
| `stop_relay` | 3 | none (`{}`) | `RelayStatus` (stopped shape: `running: false`, all fields `None`) | none | Calls `RelayServer::shutdown`, aborts the accept task, broadcasts `RELAY_STOPPED_TOPIC`. No-op (not an error) when no relay is running. |
| `relay_status` | 4 | none (`{}`) | `RelayStatus` | none | Snapshot of the current relay state; `running: false` shape when idle. Always succeeds. |

`dispatch` returns `PluginError::ExecutionFailed` for an unknown handler id and for arg-decode / serialize / bus-publish failures.

## Capabilities

The plugin is a **core (native) plugin**, so its manifest declares no capability list — core plugins run with full native access and are not capability-gated the way community WASM/JS plugins are (see `docs/0.1.2/capabilities.md`). The kernel-mediated operations it performs are bus publishes via `EventBus::publish_plugin` (namespace-checked: the topic must lie in the plugin's own `com.nexus.collab` namespace) and `EventBus::publish_core` (the trusted-core escape hatch used by the inbound bridge to republish on the editor's `com.nexus.editor.ops.*` namespace). Network access (outbound `ws://` connect, inbound `TcpListener::bind`) is performed directly by tokio in the bridge/relay tasks and is not mediated by a `net.*` capability in Phase 1 — there is no capability gate on the relay endpoint or the bind address.

## Settings / Config

The optional bridge is configured by a `[collab]` block in `<forge>/.forge/config.toml`, read by `nexus_bootstrap::collab::load_config` (parse failure / missing file / missing block all collapse to disabled). The same block is the single source of truth for both the bridge connection and the `publish_presence` identity stamp.

```toml
[collab]
enabled = true                       # master switch; default false
relay_url = "ws://127.0.0.1:7700/"   # ws://host:port[/path][?token=…]; required when enabled
token = "shared-secret"              # matches the relay's static token; required
peer_id = "alice@laptop"             # unique on the relay; required
display_name = "Alice"               # peers-panel label; required
# optional ReconnectConfig overrides:
initial_delay_ms = 1000              # default 1000 (ReconnectConfig::initial_delay = 1s)
max_delay_ms = 30000                 # default 30000 (max_delay = 30s)
buffer_capacity = 256                # outbound replay queue depth; default 256
backoff_factor = 2.0                 # default 2.0
handshake_timeout_secs = 10          # CollabClient handshake budget; default 10s
```

`enabled=true` with any of the four required fields blank ⇒ warn log + spawn skipped (`fields_complete`). A `relay_url` that doesn't parse as `ws://host:port[/...]` ⇒ warn + skip. No ambient tokio runtime (e.g. CLI single-shot) ⇒ debug + skip. In every skip case the runtime keeps booting; collaboration is opt-in and never fatal.

Other defaults defined in-crate: `DEFAULT_HANDSHAKE_TIMEOUT = 10s`; relay `BROADCAST_CAPACITY = 1024` entries; `MAX_FRAME_BYTES = 16 MiB` (client and server); `start_relay` generates a fresh UUID-v4 token per share and binds `0.0.0.0`.

**Phase 1 constraints:** plain `ws://` only — no `wss://`/TLS (the URL parser explicitly rejects `wss://`), a single static shared-secret token (no per-user credentials), and a single in-process broadcast channel (no multi-room/hosted relay). Site-based self-echo dedup is wired to `None` in bootstrap pending `CrdtPublisher::site()` plumbing.

## Events

The bridge connects local kernel-bus topics to the remote relay in both directions.

**Outbound (bus → relay).** `CollabClient`/`ReconnectingClient` subscribe to a configurable set of `EventFilter`s; the default (and bootstrap) set is two prefixes:
- `com.nexus.editor.ops.` (`OPS_TOPIC_PREFIX`) — CRDT op envelopes from the editor's `CrdtPublisher`.
- `com.nexus.collab.` (`COLLAB_TOPIC_PREFIX`) — presence + peer events.

Each matching `NexusEvent::Custom` is shipped to the relay as `ClientMessage::Envelope { topic = type_id, payload }`. Events whose `emitting_plugin == com.nexus.collab.bridge` are skipped (anti-loop — see below).

**Inbound (relay → bus).** Received `ServerMessage` frames are republished via `EventBus::publish_core(COLLAB_BRIDGE_PLUGIN_ID, …)`:
- `Envelope { topic, payload }` → republished verbatim on `topic` (after optional site-based self-echo drop on `op.id.site`).
- `PeerJoined { peer }` → `com.nexus.collab.peers.joined` (`PEER_JOINED_TOPIC`), payload = serialized `PeerInfo`.
- `PeerLeft { peer_id }` → `com.nexus.collab.peers.left` (`PEER_LEFT_TOPIC`), payload `{ "peer_id": … }`.

**Locally published by the core plugin / supervisor:**
- `PRESENCE_TOPIC` = `com.nexus.collab.presence` — `PublishPresenceArgs` handler emits a `PresenceEvent` here (peer-authored cursor/focus).
- `RELAY_STARTED_TOPIC` = `com.nexus.collab.relay.started` / `RELAY_STOPPED_TOPIC` = `com.nexus.collab.relay.stopped` — `RelayStatus` payloads from `start_relay`/`stop_relay` so the shell syncs the Share UI across windows without polling.
- `CONNECTION_STATE_TOPIC` = `com.nexus.collab.connection` — `ReconnectingClient` emits `{ "state": "connecting" | "connected" | "disconnected" }` per transition for a "reconnecting" badge.

## Internals & notable implementation details

**Handshake.** Client connects, sends `ClientMessage::Hello { token, peer_id, display_name }` as the mandatory first text frame. The server validates the token (constant-time compare), rejects a non-`Hello` first frame with `ERR_HANDSHAKE`, an empty `peer_id` with `ERR_HANDSHAKE`, a duplicate live `peer_id` with `ERR_HANDSHAKE`, a bad token with `ERR_AUTH`, and a non-JSON frame with `ERR_BAD_FRAME` — sending the error then closing. On success it replies `ServerMessage::Hello { peer_id, peers }` (snapshot of already-connected peers, excluding the newcomer) and broadcasts `PeerJoined` to the others.

**Relay framing & routing.** `RelayServer` holds one process-wide `broadcast::channel<Routed>` (`BROADCAST_CAPACITY = 1024`) plus a `Mutex<PeerRegistry>`. Each connection runs a **read task** (`pump_reads`: parse frames, drop bad ones, turn `Envelope`s into `ServerMessage::Envelope { from, topic, payload }` pushed to the broadcast channel) and a **write task** (drains the channel, **skips frames whose `from` equals this peer** — the relay's only routing rule, halving wire chatter; per-op self-echo dedup is left to consumers). `Routed.from = None` is used for relay-authored `PeerJoined`/`PeerLeft` so every peer receives them. A slow peer that lags the broadcast channel is dropped (`RecvError::Lagged → continue`). The accept loop tracks per-peer tasks in a `JoinSet`; `shutdown()` broadcasts a one-shot signal and the loop calls `JoinSet::shutdown().await` so all child sockets are aborted before `serve_listener` returns (lets a caller re-bind the port without racing). Frame size is capped at 16 MiB on both `accept_async_with_config` and `client_async_with_config`.

**Bridge anti-loop.** The inbound bridge republishes everything under `emitting_plugin = com.nexus.collab.bridge` (`COLLAB_BRIDGE_PLUGIN_ID`) via `publish_core` (which bypasses the namespace anti-spoof check so the bridge can legitimately stamp the editor's `com.nexus.editor.ops.*` topics). The outbound subscriber then skips any event whose `emitting_plugin` is that bridge id — one check breaks the relay loop uniformly for ops, presence, and peer events without per-topic special-casing. Locally-authored collab events use `com.nexus.collab` (`COLLAB_PLUGIN_ID`) and are forwarded normally.

**Self-echo by site.** `drop_for_self_echo` inspects an op-shaped payload (`{"op":{"id":{"site":…}}}`) and drops it when `site == local_site_id`. Non-CRDT payloads (presence, peer events) don't fit the shape and pass through untouched. Default `local_site_id` is `None` (disabled).

**Reconnect supervisor.** `ReconnectingClient::start` spawns a `supervise` task: one `run_feeder` per outbound filter pushes `(topic, payload)` tuples into a shared bounded `OutboundBuffer` (a `VecDeque` + `Notify`; drops the **oldest** entry on overflow at `buffer_capacity`). Each session connects, handshakes (no per-attempt timeout — the outer backoff bounds total wait), spawns a `run_drain` task that pops the buffer and writes to the sink (on send failure it **pushes the in-flight item back** for the next session, giving best-effort replay durability), and runs the inbound loop inline. On disconnect the supervisor sleeps `next_backoff` (multiply by `backoff_factor`, capped at `max_delay`; backoff resets to `initial_delay` after a session that actually ran), then reconnects. Drop/`shutdown` aborts feeders and supervisor.

**Relay-host handler.** `start_relay` binds a `std::net::TcpListener` synchronously (so it works inside the sync `dispatch`), sets it non-blocking, converts via `TcpListener::from_std` inside `handle.enter()`, and spawns the accept loop on the ambient runtime. `detect_lan_ip` opens a UDP socket "connected" to `1.1.1.1:80` (no packets sent) to read the kernel's chosen local address, falling back to `127.0.0.1`. The live `RunningRelay` lives behind a `std::sync::Mutex`; `relay_lock` recovers a poisoned guard rather than panicking (audit gap D2 — the relay slot's invariants are restored on the next start/stop edge). `stop_relay` can't `block_on` inside the runtime, so it `shutdown()`s (which awaits the JoinSet inside `serve_listener`) and `abort()`s the accept task as belt-and-braces.

**Wire compatibility.** `PresenceCursor` shipped `relpath` + `block_id` in Phase 1.3; Phase 2.2 added `offset` + `selection_end` as `#[serde(default, skip_serializing_if = "Option::is_none")]` so frames decode in both directions across versions. `PublishPresenceReply.published` is `#[serde(default)]` so an older `{}` reply decodes as `false`.

## Tests

In-module unit tests cover: protocol round-trips + unknown-kind rejection (`protocol`); token verify / empty / length-mismatch / constant-time (`auth`); URL parsing incl. token query, missing-port/wrong-scheme/empty-host rejection (`url`); `drop_for_self_echo` and `peer_info_payload` (`client`); backoff doubling + cap, `OutboundBuffer` overflow/notify, connection-state wire codes (`reconnect_client`); and the four IPC handlers incl. identity stamping, no-op-when-unconfigured, `deny_unknown_fields`, relay start/stop round-trip + idempotency + idle-stopped shape, and `set_identity` (`core_plugin`).

Integration tests (`tests/`):
- `relay_server.rs` — two/three raw `tokio-tungstenite` clients against a real `RelayServer`: handshake + mutual peer visibility, envelope broadcast to others only, `PeerLeft` on disconnect, bad-token / first-frame-must-be-Hello / duplicate-peer-id / empty-peer-id rejection, three-peer fan-out.
- `client_bridge.rs` — end-to-end `CollabClient ⇄ relay ⇄ CollabClient`: handshake + echoed peer id, initial-peers snapshot, outbound editor-op reaches the other bus, site-based self-echo drop, other-site op passes through, outbound filter scope, clean shutdown.
- `presence_bridge.rs` — presence round-trip (with/without cursor), `PeerJoined`/`PeerLeft` surfaced on each peer's bus, and bridge-authored events not looping back through outbound.
- `reconnect.rs` — `connecting→connected` on initial handshake, buffered-event replay after a relay outage (relay dropped and re-bound on the same port), backoff retry on bad-token handshake failure.
- `dod.rs` — BL-143 Definition-of-Done punch list: two-runtime op exchange, presence cursor round-trip, bad-token rejection, end-to-end `publish_presence` identity stamping, and disconnect observability via `CONNECTION_STATE_TOPIC`.
