# Remote forge (BL-140)

A **remote forge** is a Nexus runtime running on a different machine
than the one you're typing on. The local CLI proxies every IPC call
over a transport (SSH child today) to a headless `nexus serve` on the
other end. Same kernel, same plugins, same IPC verbs — they just live
elsewhere.

BL-140 is feature-complete. Every published phase (1 / 2a / 2b / 2c /
3a / 3b / 3c) is shipped: both the CLI and the Tauri shell can open a
remote forge, the SSH connection auto-reconnects on drop, and the
shell's workspace status item renders a live connection-state badge
(connected / reconnecting / disconnected) that reflects the underlying
SSH transport health.

---

## When to use it

- You have your forge on a server / NAS / dev VM but want to drive it
  from your laptop.
- You want to keep the index (`.forge/index.db`, Tantivy, vectorstore)
  on a machine that's always on while the CLI is ephemeral.
- You want a single forge accessible from multiple machines without
  cross-syncing the index — file-as-truth + index-rebuilt-on-server is
  the natural shape.

If you want **two people editing the same forge concurrently**, that's
BL-143 (live collaboration network transport), not this. Remote forge
is one user, two machines.

---

## URI syntax

```
ssh://[user@]host[:port]/abs/path
```

- **`user@`** is optional. Falls back to the local SSH default (often
  the current user; what `ssh host` would do).
- **`:port`** is optional. Falls back to the SSH config / library
  default (22).
- **`/abs/path`** is **required** and **must be absolute**. The remote
  `nexus serve` opens this path as its forge root.
- IPv6 literals must be bracketed: `ssh://root@[2001:db8::1]:22/srv/forge`.

Pass via `--forge-path` or `NEXUS_FORGE_PATH`:

```bash
nexus --forge-path ssh://alice@host.example.com/srv/forge content list

# or via env
export NEXUS_FORGE_PATH=ssh://alice@host.example.com/srv/forge
nexus content list
```

The CLI detects `://` in the path and routes through the remote
runtime constructor. Anything without `://` is a local path.

The **Tauri shell** has an "Open remote forge…" entry in the launcher
that prompts for the same URI shape. Once entered, the URI is
persisted to recents identically to local paths and reopens on
subsequent shell launches; clicking it from the recents list works the
same as a local forge would.

---

## What works over a remote forge

Subcommands that only need `ipc_call` route through the remote
transport transparently. That covers the bulk of Nexus's surface:

| Plugin | Works over remote |
|---|---|
| `com.nexus.ai` | ✅ chat, ask, stream, planning, predict |
| `com.nexus.agent` (non-interactive) | ✅ run, plan, list, get, delegate |
| `com.nexus.skills` | ✅ list, show |
| `com.nexus.workflow` | ✅ list, show, run, validate |
| `com.nexus.database` | ✅ all bases ops |
| `com.nexus.notifications` | ✅ send |
| `com.nexus.security` | ✅ audit log queries |
| `com.nexus.terminal` | ✅ saved-command + ad-hoc surfaces |
| `com.nexus.mcp.host` | ✅ host-side MCP ops |
| `com.nexus.storage::import_forge` | ✅ |
| `nexus graph status` / `unresolved` / `neighbors` / `entity *` / `dream-cycle run` | ✅ — BL-147 |
| `nexus forge status` / `reindex` / `doctor` | ✅ — BL-147 |
| `nexus content *` / `nexus tags list` / `nexus config show|reset` / `nexus canvas *` / `nexus bases *` | ✅ — BL-147 |
| `nexus crdt enable-transport` | ✅ — BL-147 (gitignore step; merge-driver registration is still a local-tree op) |

Local-only by design — these need the local kernel handle, event bus
subscription, or stdio that the remote shape can't tunnel:

| Subcommand | Why local-only |
|---|---|
| `nexus forge init` | Creates a local `.forge/` directory; no IPC analogue. |
| `nexus serve` / `nexus acp serve` / `nexus mcp serve` | Spawn local servers. Spinning one up against a remote forge is nonsensical. |
| `nexus agent run --interactive` | Subscribes to the local kernel bus for approval prompts. The non-interactive path works fine over remote. |
| `nexus ai chat` (streaming) | Subscribes to `com.nexus.ai.stream_*` events; pump needs a bus the CLI can read. |

Running a local-only subcommand against an `ssh://` URI exits with:

```
Error: this operation requires a local forge; remote (ssh://) forges only support IPC-based subcommands
```

If you hit that for something that *should* work over remote, the
subcommand is probably reaching for `app.runtime()` instead of
`app.invoker()` — switch the call site (most use a central `fn call`
helper).

---

## Architecture

```
┌──────────────────────────────┐         ┌──────────────────────────────┐
│  Local machine               │         │  Remote machine              │
│                              │         │                              │
│  nexus (CLI)                 │         │  ssh (sshd)                  │
│   │                          │         │   │                          │
│   ↓                          │         │   ↓                          │
│  App::invoker()              │         │  nexus serve --stdio         │
│   ↓                          │         │   ↓                          │
│  ReconnectingRuntime         │         │  RemoteServer                │
│   ↓                          │         │   ↓                          │
│  SshConnectionFactory        │         │  KernelPluginContext         │
│   ↓                          │         │   ↓                          │
│  RemoteRuntime ──── SSH ──── │ ▶◀▶◀▶◀▶ │  Kernel + plugins + storage │
│   │ Arc<RemoteClient>        │         │                              │
└──────────────────────────────┘         └──────────────────────────────┘
```

The wire format is line-delimited **JSON-RPC 2.0** — one message per
line, terminated by `\n`. No `Content-Length:` header. Matches the
`nexus-acp` framing.

### Methods

| Method | Body | Returns |
|---|---|---|
| `ipc_call` | `{ plugin_id, command, args, timeout_ms? }` | The IPC handler's return value (any JSON) |
| `event_subscribe` | `{ subscription_id, filter }` | `{ subscription_id }` (echo) |
| `event_unsubscribe` | `{ subscription_id }` | `{ ok: bool, reason?: string }` |

`filter` is one of:

- `{ "kind": "all" }`
- `{ "kind": "variant", "name": "PluginStarted" }`
- `{ "kind": "custom_prefix", "prefix": "com.nexus.editor." }`
- `{ "kind": "custom_exact", "type_id": "com.nexus.editor.saved" }`

### Server-pushed notifications

Each delivered subscription event arrives as a JSON-RPC notification:

```json
{ "jsonrpc": "2.0", "method": "event",
  "params": { "subscription_id": "...", "event": <PublishedEvent> } }
```

`<PublishedEvent>` is the same serde shape the kernel publishes on its
local bus, so subscribers can reuse existing decoding.

### Errors

| Code | When |
|---|---|
| `-32601` | Method not found (only `ipc_call` / `event_subscribe` / `event_unsubscribe` are valid) |
| `-32602` | Invalid params (missing required field, unknown filter kind) |
| `-32000` | Underlying `ipc_call` failure on the server side |

Client-side these map to `IpcInvokerError::{Remote, Transport, Timeout}`.

### Trust posture

There is **no method allow-list** — the remote-forge server exposes
the *whole* IPC surface. Trust lives at the transport layer: SSH auth
proves the client is allowed to drive the remote kernel. This is the
deliberate split from `nexus-acp`, which curates a narrow agent
surface for arbitrary external clients.

---

## Reconnect (Phase 2c)

The `ReconnectingRuntime` wraps the SSH-spawning runtime so a dropped
connection rebuilds on the next call.

- **Trigger**: only `IpcInvokerError::Transport(_)`. Server errors
  (`Remote { code }`) and timeouts (`Timeout`) surface as-is — they're
  not connection-death signals.
- **Schedule**: `[100ms, 500ms, 2s, 10s, 30s]`. After the last delay
  the next failure surfaces as `Transport("reconnect schedule
  exhausted: ...")`.
- **State carried across reconnect**: subscriptions are replayed
  (BL-146). Every subscription installed via
  `ReconnectingRuntime::subscribe(id, filter, sink)` is recorded in an
  internal `SubscriptionRegistry`. A per-client watchdog awaits
  `RemoteClient::wait_for_disconnect`; on a drop it walks the backoff
  schedule, builds a fresh client, and re-installs every registered
  subscription against it. Subscribers see uninterrupted event flow.
  Use `ReconnectingRuntime::subscribe_replays` to observe per-replay
  counts.

---

## Operational tips

### Make sure `nexus` is on `$PATH` remotely

The CLI shell command launched on the remote side is literally `nexus
serve --forge-path /path --stdio`. If `nexus` isn't on the SSH-side
non-interactive `$PATH` (typical when shelling in non-login), spawn
will succeed but the binary won't be found:

```
remote server error (code -32000): ...nexus: command not found...
```

Fix it by either:

- Symlinking `nexus` into `/usr/local/bin/` on the remote host.
- Setting `Environment="PATH=..."` for non-interactive SSH via
  `/etc/ssh/sshrc` or `~/.ssh/environment`.

### SSH config

The local CLI invokes `ssh` exactly as you'd invoke it manually:

```bash
ssh -T [-p <port>] [user@]host -- nexus serve --forge-path /path --stdio
```

So **everything you'd put in `~/.ssh/config`** still works — jump
hosts, control sockets, identity files, custom `Host` aliases:

```sshconfig
Host devbox
  HostName devbox.internal
  User alice
  Port 2222
  IdentityFile ~/.ssh/devbox_ed25519
  ControlMaster auto
  ControlPath /tmp/ssh-%r@%h:%p
  ControlPersist 10m
```

Then:

```bash
nexus --forge-path ssh://devbox/srv/forge content list
```

**`ControlMaster` + `ControlPersist`** in particular is a big win for
remote forge — without it, every CLI invocation pays the full SSH
handshake (≈0.5–2s typical). With it, repeated commands reuse the
master connection.

### Auth prompts

Stderr inherits from the parent shell, so SSH password / passphrase
prompts reach your terminal. Set up key-based auth or `ssh-agent` for
non-interactive flows.

### Latency budget

Every `ipc_call` round-trip is one SSH frame each way. Bulk-write
subcommands that issue many small calls (like `nexus forge import`)
will feel slower over remote than against a local forge. Targeted
single-call subcommands (`nexus content read`, `nexus ai ask`) feel
about as fast as the underlying latency.

---

## Programmatic use

If you're building tooling on top of remote forge directly (not via
CLI), the relevant crate is `nexus-remote`:

```rust
use nexus_remote::{ForgeUri, RemoteClient};
use nexus_bootstrap::reconnect::{ReconnectingRuntime, SshConnectionFactory};
use std::sync::Arc;

let uri = ForgeUri::parse("ssh://alice@host/srv/forge")?;
let factory = Arc::new(SshConnectionFactory::new(uri));
let runtime = ReconnectingRuntime::new(factory);
let invoker = runtime.invoker();

let response = invoker
    .ipc_call(
        "com.nexus.storage",
        "list_dir",
        serde_json::json!({ "path": "" }),
        std::time::Duration::from_secs(10),
    )
    .await?;
```

To wire a non-SSH transport (in-process tests, custom protocol),
implement the `ConnectionFactory` trait — it returns a `RemoteRuntime`
built via `build_remote_runtime_over_pipes(reader, writer, guard)`.

---

## Limitations

- **No `event_subscribe` consumer in the CLI yet.** The wire shape is
  there, but no in-tree subcommand currently uses it over remote. The
  shell (`kernel_subscribe`) is the one in-tree consumer today.
- **Plugin scaffolding** (`nexus plugin scaffold`, `nexus plugin
  install`) is local-only by design — community plugins live on the
  remote machine, alongside their `nexus serve`.
- **No connection pooling across CLI invocations.** Each `nexus
  --forge-path ssh://...` invocation spawns a fresh SSH child. Use
  SSH `ControlMaster` to amortise.

---

## References

- **Code**:
  [`crates/nexus-remote/`](../../crates/nexus-remote/) (transport +
  client + server),
  [`crates/nexus-bootstrap/src/remote.rs`](../../crates/nexus-bootstrap/src/remote.rs)
  (SSH spawn + `RemoteRuntime`),
  [`crates/nexus-bootstrap/src/reconnect.rs`](../../crates/nexus-bootstrap/src/reconnect.rs)
  (reconnect wrapper),
  [`crates/nexus-bootstrap/src/invoker.rs`](../../crates/nexus-bootstrap/src/invoker.rs)
  (transport-agnostic trait).
- **Tests**:
  [`crates/nexus-remote/tests/end_to_end.rs`](../../crates/nexus-remote/tests/end_to_end.rs),
  [`crates/nexus-remote/tests/client_server_loop.rs`](../../crates/nexus-remote/tests/client_server_loop.rs),
  [`crates/nexus-bootstrap/tests/remote_runtime_loop.rs`](../../crates/nexus-bootstrap/tests/remote_runtime_loop.rs),
  [`crates/nexus-bootstrap/tests/reconnect_loop.rs`](../../crates/nexus-bootstrap/tests/reconnect_loop.rs).
- **Backlog entry**:
  [BL-140 in BACKLOG.md](../PRDs/BACKLOG.md).
