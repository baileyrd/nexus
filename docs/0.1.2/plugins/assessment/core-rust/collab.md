# com.nexus.collab

- **Path:** `crates/nexus-collab/`
- **Tier:** Core Rust
- **Bootstrap order:** 23

## Architecture

- Entry point `crates/nexus-collab/src/lib.rs` re-exports `RelayServer`, `Token`, `CollabClient`, `ReconnectingClient`, `PresenceEvent`, the protocol envelopes, and URL helpers. Registered by `crates/nexus-bootstrap/src/plugins/collab.rs` with `LifecycleFlags::NONE` (no lifecycle hooks — the plugin is purely a request handler + bus publisher).
- Key modules: `core_plugin` (4 IPC handlers + relay state), `server` (`RelayServer` — WebSocket relay binding via `tokio-tungstenite`), `client` (`CollabClient` — kernel-side bridge that subscribes to local bus topics and ferries envelopes to the relay), `reconnect_client` (`ReconnectingClient` — auto-reconnect wrapper, exposes `CONNECTION_STATE_TOPIC`), `presence` (`PresenceEvent` + `PRESENCE_TOPIC`, `PEER_JOINED_TOPIC`, `PEER_LEFT_TOPIC`), `protocol` (client/server message types), `auth` (`Token` newtype with strength check), `url` (`ws://…/?token=…` builder/parser).
- BL-143 Phase 2.3: the plugin can host an in-process relay. `start_relay` binds a free port (or honours `args.port`), generates a UUID-v4 token, spawns the accept task on the ambient tokio runtime, and broadcasts `RELAY_STARTED_TOPIC` (`com.nexus.collab.relay.started`). `stop_relay` shuts down and broadcasts `RELAY_STOPPED_TOPIC`. `relay_status` reports a snapshot. Detects LAN IP via a UDP-route probe (`detect_lan_ip` in `core_plugin.rs:205`), falls back to `127.0.0.1`.
- Identity is read at bootstrap time from `[collab]` in `<forge>/.forge/config.toml` via `crates/nexus-bootstrap/src/collab.rs::load_config`; `LocalPeer { user_id, display_name }` is stamped onto outgoing `publish_presence` events. When `[collab]` is absent or incomplete, `publish_presence` returns the stable error `"publish_presence: collab not configured"` so the shell can stop calling.
- Persistence: read-only — `[collab]` in `<forge>/.forge/config.toml` (`docs/0.1.2/settings/forge-config.md:170`). No SQLite, no derived index. Relay token + bound port live in-memory only.
- Settings owned: `[collab]` block via `nexus-bootstrap`'s `CollabConfig` (`crates/nexus-bootstrap/src/collab.rs:61`) — the crate itself just consumes `LocalPeer`. Not directly an owner.
- External dependencies: `tokio-tungstenite` (WebSocket server + client), `futures-util`, `uuid`. Opens listening TCP sockets (capability `network`) when `start_relay` is invoked; opens outbound TCP/WebSocket connections when the bridge `CollabClient` joins a remote relay.

## Surface

- IPC handlers (from `IPC_HANDLERS` in `core_plugin.rs:57`, all sync):
  - `publish_presence` (1) — stamps `LocalPeer` identity onto a partial `PresenceCursor` and publishes on `PRESENCE_TOPIC`.
  - `start_relay` (2) — bind + spawn accept loop, return `RelayStatus { running, url, host, port, token }`. Idempotent.
  - `stop_relay` (3) — graceful shutdown + accept-task abort.
  - `relay_status` (4) — snapshot.
- Bus events: `com.nexus.collab.presence` (the `PRESENCE_TOPIC` constant), `com.nexus.collab.peer.joined`, `com.nexus.collab.peer.left`, `com.nexus.collab.relay.started`, `com.nexus.collab.relay.stopped`, plus the bridge's connection-state topic (`CONNECTION_STATE_TOPIC`).

## Necessity

- **Verdict:** Optional
- **Required for basic capabilities?** No. The basic single-user workflow does not involve real-time collaboration; the plugin is dormant when `[collab]` is absent (`publish_presence` reports `not configured`; `start_relay` works but no one is calling it).
- **Depended on by:** shell `nexus.collab` plugin (`shell/src/plugins/nexus/collab/CollabPanel.tsx`, `collabStore.ts`, `cursorPublisher.ts`, `index.ts`). The CRDT plugin family bridges editor ops via the `com.nexus.editor.ops.<relpath>` topic family. Tests: `crates/nexus-collab/tests/presence_bridge.rs`, `reconnect.rs`.
- **Depends on:** `nexus-kernel`, `nexus-plugins`. The bridge (`CollabClient`) subscribes to events authored by `com.nexus.editor` (CRDT ops) and republishes them; there is a runtime data-flow dependency on the editor plugin's ops topics.
- **What breaks if removed:** the Share panel (start a relay), live presence cursors, and the WebSocket bridge that ferries CRDT ops between peers all go offline. Local CRDT editing (via `nexus-crdt`) keeps working — collab only adds the network leg. Markdown / search / git unaffected.

## Notes

- The crate hosts both relay server and reconnect client under one roof — the bootstrap can host a quick share without depending on a separate process. UDP-route trick for LAN-IP detection is a deliberate "best effort, cheap to call" decision (`core_plugin.rs:205`).
- `stop_relay` aborts the accept task rather than awaiting it (block_on is illegal inside the tokio runtime); the comment at `core_plugin.rs:398` calls out that an explicit-port re-bind a few ms later relies on OS port-table tolerance.
- No flat-TOML loader — settings live in `<forge>/.forge/config.toml::[collab]` only.
- Test coverage is solid: handler-level tests in `core_plugin.rs` plus dedicated `presence_bridge.rs` and `reconnect.rs` integration tests.
