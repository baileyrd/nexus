# Terminal Broadcaster Refactor — Implementation Plan

**Status**: drafted, not started.
**Goal**: Remove the shell-side 5-second `read_raw_since` heartbeat by giving every PTY session a kernel-side broadcaster thread that publishes `com.nexus.terminal.output.<id>` events autonomously.

## Why

Today the kernel only emits output broadcast events as a side effect of the `pump` and `read_raw_since` IPC handlers (see `crates/nexus-terminal/src/core_plugin.rs` ~line 43). There is no autonomous PTY reader publishing on its own. So the shell has to call `read_raw_since` on a 5-second timer (`PTY_POLL_INTERVAL_MS = 5000` in `shell/src/plugins/nexus/terminal/TerminalView.tsx`) just to keep the kernel pumping the PTY at all — without it, input flows but output never returns and the terminal looks frozen.

After this refactor:
- `read_raw_since` and `pump` IPC handlers become pure ring readers (no `manager.drain()` step, no event side effect).
- Shell drops the heartbeat entirely.
- `terminalStore.recoverFn` keeps working — gap → `read_raw_since` snapshot from the ring.

## Architecture

### Today
```
PTY → reader_thread → mpsc::channel(rx held by Session)
                              ↓
                  manager.drain() ← called only by IPC handlers
                              ↓
                  appends to ring buffer
                              ↓
                  buffer_read_since() ← read by pump/read_raw_since
                              ↓
                  emit broadcast event (side effect inside handler)
```

### Goal
```
PTY → reader_thread → mpsc::channel
                              ↓
              NEW: broadcaster_thread (per session)
                ├→ writes to ring buffer
                └→ publishes broadcast event via EventBus

IPC handlers (read_raw_since, pump): pure recovery — read ring, no side effects
```

## Ordered file-by-file change list

Each step leaves the workspace compiling. Steps 1–4 are Rust-only and can ship in one PR; step 5 is the shell deletion.

### Step 1 — `crates/nexus-terminal/src/manager.rs`: expose ring-append + cursor primitives

Add two pub methods (no behaviour change for existing callers):

- `pub fn append_bytes(&mut self, id: &SessionId, bytes: &[u8])` — pushes into both `entry.buffer` and `entry.lines` (mirrors the `read_into` body without touching `session.read`). Used by the broadcaster after it pulls from the mpsc.
- `pub fn take_output_rx(&mut self, id: &SessionId) -> Option<Receiver<std::io::Result<Vec<u8>>>>` — moves the mpsc receiver out of `Session`. Requires session.rs changes (step 2).

Mark `drain` as deprecated internally (still works; broadcaster will become the sole writer).

### Step 2 — `crates/nexus-terminal/src/session.rs`: relinquish the receiver at spawn

Change `Session::spawn` to return `(Session, Receiver<io::Result<Vec<u8>>>)`. The session keeps `pending` for the legacy `read`/`read_into` path (still useful in unit tests), but in production the manager owns the receiver and forwards it to the broadcaster.

Update `SessionManager::spawn` (manager.rs ~120) to destructure the tuple and stash the `Receiver` in a new `Entry` field `output_rx: Option<Receiver<…>>` so step 4 can lift it out. All in-tree callers of `Session::spawn` (the `session.rs` test module + manager) update in lockstep.

`Session::Drop` (session.rs ~666) is unchanged: child kill propagates EOF to the reader thread, which closes its `tx` end. The broadcaster sees `RecvError::Disconnected` and exits.

### Step 3 — new file `crates/nexus-terminal/src/broadcaster.rs`

Standalone module owning one thread per session. API:

```rust
pub struct Broadcaster { handle: Option<JoinHandle<()>>, stop: Arc<AtomicBool> }
impl Broadcaster {
    pub fn spawn(
        id: SessionId,
        rx: Receiver<io::Result<Vec<u8>>>,
        sink: Arc<dyn OutputSink>,
    ) -> Self
    pub fn stop(&mut self)            // sets flag, joins
}
impl Drop for Broadcaster { fn drop(&mut self) { self.stop(); } }
```

`OutputSink` is a tiny trait the kernel-aware caller implements:

```rust
pub trait OutputSink: Send + Sync {
    fn on_chunk(&self, id: &SessionId, bytes: Vec<u8>);
    fn on_eof(&self, id: &SessionId);
}
```

Keeps `nexus-terminal` library kernel-free; the plugin layer (step 4) supplies the impl that ring-appends + publishes the event.

Loop body: `rx.recv_timeout(250ms)`; on `Ok(Ok(data))` non-empty → `sink.on_chunk`; on `Ok(Ok(empty))` / `Err(Disconnected)` → `sink.on_eof` then break; on `Err(Timeout)` → check `stop` flag, continue.

Add module declaration to `crates/nexus-terminal/src/lib.rs`.

### Step 4 — `crates/nexus-terminal/src/core_plugin.rs`: own broadcasters, slim handlers

Changes inside `TerminalCorePlugin`:

- Add field `broadcasters: Mutex<HashMap<SessionId, Broadcaster>>`.
- Implement `OutputSink` on a small `PluginSink { server: Weak<Mutex<InMemoryTerminalServer>>, bus: Option<Arc<EventBus>>, emitters: Weak<Mutex<HashMap<SessionId, EmitterState>>> }`. **Forces wrapping `server` in `Arc<Mutex<…>>`** — see locking section below. The sink:
  1. Locks `server`, calls `manager_mut().append_bytes(id, &bytes)` (step 1).
  2. Locks `emitters`, increments `next_seq`, advances `cursor` by `bytes.len()`.
  3. If `bus.is_some()`, publishes `OutputStreamPayload { data: bytes, seq, ts_ms }` on `com.nexus.terminal.output.<id>` — exact same shape as today.
- `dispatch_create_session`: after `create_session` succeeds, take the receiver from the manager entry and spawn a `Broadcaster` keyed by id.
- `dispatch_close_session`: after `request_shutdown`, remove the broadcaster from the map → its `Drop` joins the thread (or explicitly `.stop()` first to drain the EOF). Also clear the `emitters` entry.
- `dispatch_pump`: drop the publish path entirely — `pump` becomes "advance line cursor over the ring, emit `OutputReceived` for new lines, return delta in bytes". Critical fix: change `SessionManager::drain` to read **from the ring buffer and feed `LineBuffer`** rather than from the mpsc (the broadcaster now owns the mpsc). Specifically: `drain` keeps a per-entry `lines_byte_cursor: u64` and reads `buffer_read_since(id, lines_byte_cursor)` into `entry.lines`. Preserves `wait_for_pattern`'s semantics.
- `dispatch_read_raw_since`: drop the `manager.drain(id, timeout)` call entirely. Just `manager.buffer_read_since(id, cursor)`. Drop the `fetch_new_bytes` + `publish_output` calls — broadcaster-only now.

### Step 5 — `shell/src/plugins/nexus/terminal/TerminalView.tsx`: remove the heartbeat

Delete:

- `const PTY_POLL_INTERVAL_MS = 5000`.
- The `setInterval(..., PTY_POLL_INTERVAL_MS)` block, plus the corresponding `clearInterval` in the effect cleanup.
- Update the comment block to say "output arrives autonomously via the broadcaster; recoverFn handles gaps".

Keep the on-mount `tick()` call — still useful to drain pre-mount backlog.

`terminalStore.ts` is not modified. `recoverFn` keeps calling `read_raw_since` for gap repair — server-side that handler is still present, just no longer drains.

**Sequencing**: ship Rust + shell in the same PR. The Rust change alone is harmless (heartbeat keeps working but becomes redundant); shipping the shell change without the Rust change leaves the terminal frozen.

## Ownership & locking design

Today: `Mutex<InMemoryTerminalServer>` inside `TerminalCorePlugin`. Single dispatch thread + `&mut self` paths means contention is zero.

After: the broadcaster thread (one per session, N total) calls `server.manager_mut().append_bytes(...)` whenever a chunk arrives. Genuinely concurrent with IPC dispatches.

- Wrap server in `Arc<Mutex<InMemoryTerminalServer>>`. Plugin holds a strong `Arc`; each `PluginSink` holds a `Weak<Mutex<…>>`. `Weak` so a leaked broadcaster can't keep the server alive after the plugin is dropped.
- `emitters` similarly becomes `Arc<Mutex<HashMap<…>>>` with `Weak` in the sink.
- Broadcaster lock discipline: do `recv_timeout` **outside** the lock; once a chunk arrives, take the server lock, append, drop the lock, then take the emitters lock and publish. Never hold both simultaneously.
- IPC handlers continue locking server mutex normally. Broadcaster's lock hold is microseconds.
- The plugin's `Mutex<HashMap<SessionId, Broadcaster>>` is only touched on session create/close; never on the hot path.

## Shutdown ordering

Required order (per session):

1. `close_session` IPC arrives → `manager.request_shutdown(id, 500ms)` → child kill ladder.
2. portable-pty reader sees EOF → its inner `reader.read` returns `Ok(0)` → `tx.send(Ok(empty))` then break.
3. mpsc `rx.recv_timeout` in broadcaster sees `Ok(Ok(empty))` → calls `sink.on_eof` → `break` from loop → thread exits.
4. `dispatch_close_session` removes the `Broadcaster` from the map → `Broadcaster::Drop` joins the JoinHandle.

Plugin shutdown (no `close_session` called): `TerminalCorePlugin` drops → `broadcasters` map drops → each `Broadcaster::Drop` sets stop flag, then attempts join. If a child is wedged and not producing EOF, `recv_timeout(250ms)` polls the stop flag and exits within 250ms. Don't panic on join failure — log and move on.

## Test migration

Tests that today assume "events only come from `pump` / `read_raw_since` IPC handler calls":

| Test | Change |
|---|---|
| `pump_publishes_output_event_with_monotonic_seq` (`core_plugin.rs` ~962) | Delete the `dispatch(HANDLER_PUMP, …)` calls. Subscribe to the bus first, create session, then poll `sub.try_recv` in a deadline loop until first event lands. Same `seq=1` assertion. |
| `pump_without_event_bus_remains_silent_publish_path` (`core_plugin.rs` ~1054) | Rename to `broadcaster_without_event_bus_drops_chunks_silently`. Create plugin without `with_event_bus`, create session, sleep ~200ms, call `read_raw_since` and assert it returns the bytes correctly. Asserts the no-bus path doesn't panic. |
| `pump_emits_output_received_for_each_new_line` (`server.rs` ~655) | Unchanged — exercises `TerminalServer` library directly. |
| `read_raw_since_zero_cursor_returns_all_bytes` (`server.rs` ~820), `read_raw_since_advances_cursor_…` (`server.rs` ~843), `…cursor_past_end…` (`server.rs` ~904) | Need updating: today they rely on `read_raw_since` calling `manager.drain` to actually pull bytes. After the refactor, library-level `read_raw_since` no longer drains. Add a test helper `with_broadcaster_session(...)` that spawns a session AND a broadcaster wired to a sink that calls `manager.append_bytes`. |
| `pump_read_output_returns_structured_lines` (`core_plugin.rs` ~853), `search_output_via_dispatch_finds_matches` (~891) | With broadcaster auto-feeding the ring and `pump` re-derived to advance line cursor over the ring (step 4), these continue to work. May need a small grace sleep before the first `pump`. |
| `wait_for_pattern_*` (`server.rs` ~683, ~707; `core_plugin.rs` ~1080) | Same rewrite — relies on `pump` internally, which now reads from the ring. |

Add new tests:

- `broadcaster_publishes_chunk_within_50ms_of_pty_write` — end-to-end latency gate.
- `closing_session_drops_broadcaster_and_joins_thread`.
- `multiple_sessions_have_independent_broadcasters_and_seqs`.

## Risks & open questions

1. **Lock contention under burst output.** A `cargo build` produces tens of MB/s. Broadcaster grabs server lock per chunk (~8KB chunks, so ~1000 lock acquisitions/sec/session). Likely fine (microsecond holds) but worth a load test. Mitigation: per-session sub-mutex on just the `Entry`.
2. **`SessionManager::drain` semantic change.** Today `drain` pulls from the mpsc; after step 4 it reads the ring. Subtle difference: with the mpsc-based drain, a caller could observe bytes the broadcaster hadn't appended yet (race-free because there was no broadcaster). New shape relies entirely on broadcaster having run — adds ~1 broadcaster-recv-timeout of latency to `pump`-driven tests. Acceptable.
3. **Recovery semantics.** `recoverFn` calls `read_raw_since` which now reads only the ring. If the broadcaster is wedged (deadlocked, panicked thread), the ring stops growing and recovery returns no new bytes. Need a "broadcaster died" detection — perhaps a `tracing::error!` from the join handle on unexpected exit. Open question: auto-restart broadcasters? Probably not — a dead broadcaster signals a bug worth surfacing.
4. **`Drop` ordering for Arc cycles.** `PluginSink` holds `Weak<Mutex<Server>>`, broadcaster holds `Arc<dyn OutputSink>`, plugin holds `Broadcaster` and `Arc<Mutex<Server>>`. No cycle (sink is `Weak`-back). On plugin drop: broadcasters map drops → each broadcaster's `Drop` joins → sink ref count hits zero → no leak. Verify with a `weak_count` assertion in a test.
5. **Late subscribers miss bootstrap bytes.** A subscriber that connects after session-create misses the first chunk. Already true today (the bus has no replay). `recoverFn` gap detection only triggers on a seq jump, not on "no events ever seen". Subscribers must call `read_raw_since(cursor=0)` once on subscribe to seed. Worth confirming the shell already does this — `index.ts` recoverFn handles it on first gap, but the very first chunk has no prior seq to compare against. Not new to this refactor; flag for the shell author.
6. **Backpressure.** `EventBus::publish_plugin` is non-blocking (broadcast channel). If the kernel bus is overrun the publish drops silently and the seq increments. The shell sees a gap and recoverFn fires. Fine for steady state; pathological for burst output where every other chunk is dropped. Out of scope for this refactor but note for follow-up.

## Smoke test sketch

After deploying both changes:

1. **Heartbeat truly gone:** open a terminal session in the shell. Confirm via grep that `setInterval` is removed. Type `echo hello` and press enter. Expect output rendered within ~50ms.
2. **No-input idle stream:** in the PTY, run `for i in $(seq 1 5); do sleep 1; date; done`. Expect each `date` line to appear immediately (within a frame) — not batched into 5s heartbeat windows.
3. **Recovery path:** in the dev console, monkey-patch `useTerminalStore.getState().handleStreamChunk` to drop every 3rd chunk for 2 seconds. Type `yes | head -1000`. Watch console for "lag-recovery read_raw_since" debug lines. Confirm xterm output is contiguous.
4. **Close cleanup:** close the terminal tab. In a debug build with `RUST_LOG=nexus_terminal=trace`, expect to see broadcaster thread exit log within ~250ms of the close signal.

## Critical files

- `crates/nexus-terminal/src/core_plugin.rs`
- `crates/nexus-terminal/src/manager.rs`
- `crates/nexus-terminal/src/session.rs`
- `crates/nexus-terminal/src/server.rs`
- `crates/nexus-terminal/src/broadcaster.rs` (new)
- `shell/src/plugins/nexus/terminal/TerminalView.tsx`

**Estimated effort**: ~300-400 lines of Rust + ~20 lines TS deletion + test rewrites. One focused day of work for someone familiar with the crate.
