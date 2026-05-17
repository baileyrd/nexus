# Live collaboration (BL-143)

Two (or more) Nexus runtimes on different machines can edit the same
forge concurrently. Each runtime keeps its own local copy of the
markdown files and its own SQLite index; the network transport just
relays CRDT edit ops and cursor presence so the two copies converge.

The CRDT layer (BL-074) is the merge engine. BL-143 is the wire — a
WebSocket relay, a kernel-bus bridge, and a Share-this-forge UI.

---

## When to use it

- **Two people editing the same notes at the same time** — pair note-
  taking, live meeting minutes, drafting a doc with a colleague.
- **One person on multiple machines** — laptop + desktop, swapping
  mid-session without first having to sync a git remote.
- **Read-along sessions** — a presenter publishes cursor and edits,
  one or more viewers see them live.

If you want **one user, two machines, single forge on a server**, that
is BL-140 (remote forge), not this. Remote forge proxies IPC; collab
runs two full local copies that converge.

---

## Architecture in one paragraph

Every runtime keeps a `CollabClient` open to a relay. The client
subscribes to the local kernel bus for `com.nexus.editor.ops.*` (CRDT
ops) and `com.nexus.collab.*` (presence + future collab topics);
matching events are forwarded over the WebSocket. Inbound envelopes
are re-published on the local bus under the `com.nexus.collab.bridge`
sub-namespace so the editor's CRDT consumer applies the op the same
way it applies a local one. The relay itself is topic-agnostic — it
authenticates the handshake with a shared token, gives every peer the
initial peer list, then fans every envelope out to everyone else.

Cursor presence rides the same wire: the CM6 editor publishes a
`PresenceEvent` whenever the caret moves (debounced 150 ms); peers
receive it on `com.nexus.collab.presence` and the editor's
`remoteCursorsExt` decoration layer paints a coloured caret + display
name tooltip at the right offset.

---

## Configure `[collab]`

Edit `<forge>/.forge/config.toml`:

```toml
[collab]
enabled = true
relay_url = "ws://192.168.1.42:7700/"
token = "shared-secret"
peer_id = "alice@laptop"
display_name = "Alice"

# Optional reconnect tuning (defaults shown):
# initial_delay_ms = 500
# max_delay_ms     = 30000
# buffer_capacity  = 256
```

- **`peer_id`** must be unique on the relay. The handshake rejects a
  second peer with the same id.
- **`display_name`** is what the peers panel and remote-cursor tooltip
  show.
- **`token`** is the shared secret the relay checks on every join.
  Prefer the keyring (next section) so the token isn't on disk.

`enabled = false` or a missing block disables collab entirely — no
WebSocket, no bus bridge, no Share button affordance.

---

## CLI

```bash
nexus collab --help      # subcommand index
nexus collab serve --help
nexus collab join --help
nexus collab token --help
```

### `nexus collab serve [--port <p>] [--token <t>] [--save-token]`

Run a WebSocket relay on `0.0.0.0:<p>` (default 7700). Token resolution
order:

1. `--token <value>` if given.
2. The `nexus.collab.token` keyring entry if not.

`--save-token` writes `--token` into the keyring so future invocations
don't need it.

The command runs until SIGINT; on shutdown it broadcasts a close frame
to every connected peer so reconnecting clients see a clean disconnect.

### `nexus collab join <ws-url> [--token <t>] [--peer-id <id>] [--display-name <n>] [--save-token]`

Open a `CollabClient` against the relay at `<ws-url>`. The URL accepts
the `?token=` query suffix.

```bash
nexus collab join 'ws://192.168.1.42:7700/?token=hunter2' \
  --peer-id bob@desk --display-name 'Bob'
```

`peer_id` defaults to `$USER`; `display_name` defaults to the title-
cased `peer_id`. Token precedence: `--token` > `?token=` > keyring.

### `nexus collab token { set <value> | clear }`

Manage the `nexus.collab.token` keyring entry. Missing-entry on `clear`
is not an error.

---

## Share this forge from the shell

The Tauri shell's **Collaboration** activity-bar entry opens the peers
panel. The header carries a **Share this forge** button:

1. Click **Share this forge** — picks a free port, generates a fresh
   token, binds an in-process relay on `0.0.0.0:<port>`, and shows the
   `ws://<your-lan-ip>:<port>/?token=…` URL.
2. Hit **Copy** to put the URL on the clipboard.
3. The other peer pastes it into `nexus collab join` (or their own
   shell's join flow when 2.3 follow-ups land), or sets it as their
   `[collab].relay_url`.
4. Click **Stop sharing** to tear the relay down and free the port.

LAN-IP detection uses a UDP-connect probe; if it fails (sandboxed
machine, no default route) the URL falls back to `127.0.0.1` and the
peer has to substitute the real IP by hand.

The Share state is bus-broadcast, so a popout window or the main shell
both reflect the running relay without polling.

---

## Peers panel + remote cursors

While collab is active:

- The peers panel renders every connected peer with their display name
  and current file/block. A connection-state pill (`Connecting…`,
  `Connected`, `Disconnected`) lives in the panel header.
- The editor decorates remote peers' carets with a coloured marker.
  The colour is a stable FNV-1a hash of the peer's `user_id`, so the
  same peer keeps the same colour across sessions.
- Hovering a remote caret shows the peer's display name.

Untitled buffers don't publish presence (there's no shared `relpath`
to match against), and the cursor publisher short-circuits silently
when `[collab]` is not configured so per-keystroke IPC calls aren't
issued in a single-user forge.

---

## Troubleshooting

| Symptom | Likely cause / fix |
|---|---|
| Shell panel says **Not configured** | `[collab]` is missing or `enabled = false` in `.forge/config.toml`. |
| Status pill stuck on **Connecting…** | Relay URL is wrong, the relay isn't running, or a firewall is dropping the WebSocket. Check `relay_url` and try `nexus collab serve` locally to confirm the relay can bind. |
| Status pill flapping between **Disconnected** ↔ **Connecting…** | Token mismatch. The handshake fails immediately with `ERR_AUTH` and the supervisor retries with exponential backoff. |
| Share button errors with `Address already in use` | A relay is already bound to that port. Stop the previous share or `nexus collab serve` instance, or pass a different `--port`. |
| Remote cursors don't appear | Check that both peers see the **Connected** pill *and* that the peers panel lists the remote peer. The CM6 decorator only renders for peers whose `cursor.relpath` matches the active editor's relpath. |

---

## Limits & non-goals

- **Single-relay topology.** Phase 1 ships one in-memory broadcast
  channel per relay process. Hosted multi-channel relays and
  persistent rooms are deferred (BL-143 Phase 3).
- **No voice / screen share.** Zed-style channels with WebRTC are not
  on the BL-143 roadmap; see the parent backlog item for the
  scope-decision rationale.
- **No server-side persistence.** The relay forgets every envelope
  the moment it's fanned out. A late-joiner gets the live presence
  snapshot but does *not* get an op-log replay. CRDT convergence still
  works because every peer keeps its own log — the missing pieces
  arrive when the laggard re-connects (Phase 1.5).

---

## Test surface

| DoD bullet | Where it's pinned |
|---|---|
| Two runtimes exchange editor ops over the relay | `crates/nexus-collab/tests/dod.rs::dod_two_runtimes_exchange_editor_ops_over_relay` (+ `client_bridge.rs`) |
| Presence event round-trips with cursor | `tests/dod.rs::dod_presence_event_round_trips_with_cursor` (+ `presence_bridge.rs`) |
| Auth token rejection | `tests/dod.rs::dod_handshake_with_bad_token_is_rejected` (+ `relay_server.rs`) |
| Disconnect / reconnect handled gracefully (buffered ops replay) | `tests/reconnect.rs::reconnect_replays_buffered_events_after_relay_outage` |
| `publish_presence` handler stamps identity end-to-end | `tests/dod.rs::dod_publish_presence_handler_stamps_identity_end_to_end` |
| Connection-state transitions surface on the bus | `tests/dod.rs::dod_disconnect_is_observable_on_bus_via_connection_topic` |

Shell-side unit tests for the cursor publisher and remote-cursor
projection live under
`shell/src/plugins/nexus/collab/{cursorPublisher,remoteCursors}.test.ts`.
