//! BL-140 Phase 2c — reconnect-on-drop wrapper over [`RemoteRuntime`].
//!
//! When the underlying transport dies (SSH child exits, network drops,
//! response router stops) the bare [`crate::remote::RemoteRuntime`]
//! reports every subsequent call as
//! [`IpcInvokerError::Transport`] forever. This module layers
//! reconnect-with-backoff on top: when a call fails with `Transport`,
//! the wrapper tears down the current runtime, asks a
//! [`ConnectionFactory`] for a fresh one, and retries the call.
//!
//! # Reconnect triggers
//!
//! Only `IpcInvokerError::Transport(_)` triggers a reconnect. Server-
//! reported errors (`Remote { code, message }`) and per-call
//! deadlines (`Timeout`) don't — they're not connection-death
//! signals.
//!
//! # Backoff
//!
//! Default schedule: `[100ms, 500ms, 2s, 10s, 30s]`. After the last
//! delay is consumed the next failure surfaces as a final
//! `Transport(_)` to the caller. The schedule is configurable via
//! [`ReconnectingRuntime::with_backoff`].
//!
//! # Subscription replay (BL-146)
//!
//! Callers that subscribe through [`ReconnectingRuntime::subscribe`]
//! have their `(subscription_id, filter, sink)` triples recorded in an
//! internal registry. A watchdog spawned per fresh client awaits
//! [`nexus_remote::RemoteClient::wait_for_disconnect`]; on a drop it
//! walks the reconnect schedule, builds a new client, and re-installs
//! every registered subscription against it before announcing the
//! new connection. The replay count is published on
//! [`ReconnectingRuntime::subscribe_replays`] so the Tauri bridge can
//! tell first-connect from reconnect-with-replay.
//!
//! Subscriptions installed by reaching past this wrapper (e.g. directly
//! through `ensure_client().subscribe(...)`) are NOT tracked — they die
//! with the transport. Always use the runtime-level methods.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde_json::Value;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::invoker::{IpcInvoker, IpcInvokerError};
use crate::remote::{build_remote_runtime_ssh, RemoteRuntime};
use nexus_remote::{EventDelivery, ForgeUri, RemoteClient};

/// Lifecycle state of the remote-forge connection, surfaced by
/// [`ReconnectingRuntime::subscribe_state`] so the Tauri bridge can
/// render a connection-state badge in the status bar.
///
/// State transitions are emitted on a `broadcast::Sender` so multiple
/// subscribers can observe them without contention. Each
/// `ReconnectingRuntime` starts in `Idle` (no connection built yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// No connection has been built yet (initial state after
    /// `ReconnectingRuntime::new`, or after `reset()` until the next
    /// dispatch).
    Idle,
    /// A connection is live + the last `ipc_call` succeeded.
    Connected,
    /// A `Transport` failure was observed; the wrapper is walking the
    /// backoff schedule trying to rebuild.
    Reconnecting,
    /// The backoff schedule exhausted without a successful retry. The
    /// next dispatch will start the cycle over.
    Disconnected,
}

impl ConnectionState {
    /// Stable wire-form string used by the Tauri bridge when forwarding
    /// state changes to the frontend. Lowercase + hyphenated so the
    /// frontend can `switch` on it without normalisation.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Connected => "connected",
            Self::Reconnecting => "reconnecting",
            Self::Disconnected => "disconnected",
        }
    }
}

/// Default reconnect schedule — five attempts spaced
/// `[100ms, 500ms, 2s, 10s, 30s]`. Matches the `nexus-mcp`
/// `ConnectionPool` pattern.
pub const DEFAULT_BACKOFF: [Duration; 5] = [
    Duration::from_millis(100),
    Duration::from_millis(500),
    Duration::from_secs(2),
    Duration::from_secs(10),
    Duration::from_secs(30),
];

/// Factory for building a fresh [`RemoteRuntime`] when the current
/// connection dies.
///
/// Returns a `Pin<Box<dyn Future>>` rather than `async fn` because
/// trait objects with async methods need an explicit Box+Pin wrapping
/// (object-safety) — same shape `nexus-mcp` uses for its connection
/// factory.
pub trait ConnectionFactory: Send + Sync {
    /// Build a fresh runtime. Called on the very first `invoker()`
    /// dispatch and on every reconnect attempt.
    fn build<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteRuntime>> + Send + 'a>>;
}

/// SSH-backed factory — spawns `ssh ... nexus serve --stdio` on each
/// build. The URI is captured by value so the factory can be cloned
/// into long-lived async tasks.
pub struct SshConnectionFactory {
    uri: ForgeUri,
}

impl SshConnectionFactory {
    /// Build a factory for the supplied `ssh://` URI.
    #[must_use]
    pub fn new(uri: ForgeUri) -> Self {
        Self { uri }
    }
}

impl ConnectionFactory for SshConnectionFactory {
    fn build<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<RemoteRuntime>> + Send + 'a>> {
        Box::pin(async move { build_remote_runtime_ssh(&self.uri) })
    }
}

/// One row in [`ReconnectingRuntime`]'s subscription registry — a
/// triple of `(filter, sink)` keyed by the externally-supplied
/// subscription id. The sink is cloneable (`mpsc::UnboundedSender`),
/// so each replay against a fresh client clones it rather than moving.
#[derive(Clone)]
struct SubscriptionEntry {
    filter: Value,
    sink: mpsc::UnboundedSender<EventDelivery>,
}

/// Registry of subscriptions installed through
/// [`ReconnectingRuntime::subscribe`]. The reconnect path iterates this
/// map to replay each subscription against a freshly-built client.
type SubscriptionRegistry = Arc<Mutex<HashMap<String, SubscriptionEntry>>>;

/// A self-reconnecting wrapper over a [`RemoteRuntime`]. Holds the
/// current connection behind a `Mutex` and lazily builds + rebuilds it
/// via a [`ConnectionFactory`].
pub struct ReconnectingRuntime {
    factory: Arc<dyn ConnectionFactory>,
    current: Arc<Mutex<Option<RemoteRuntime>>>,
    backoff: Vec<Duration>,
    /// Broadcast sender for [`ConnectionState`] transitions. Held
    /// inside an `Arc` so the invoker (which lives independently of
    /// the runtime) can share it.
    state_tx: Arc<broadcast::Sender<ConnectionState>>,
    /// BL-146 — every active subscription installed through
    /// [`Self::subscribe`]. Replayed against each freshly-built
    /// `RemoteClient` after a reconnect.
    subscriptions: SubscriptionRegistry,
    /// BL-146 — fires with the count of subscriptions replayed every
    /// time the watchdog (or `ipc_call`'s reconnect path) installs a
    /// new client. Held in an `Arc` for the same reason as `state_tx`.
    replay_tx: Arc<broadcast::Sender<usize>>,
    /// Watchdog task handle for the currently-installed client. Aborted
    /// when a new client is installed; spawned by `install_client`.
    watchdog: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl ReconnectingRuntime {
    /// Construct a reconnecting runtime backed by `factory`. The first
    /// call to [`Self::invoker`] + dispatch triggers the first
    /// `factory.build()`.
    #[must_use]
    pub fn new(factory: Arc<dyn ConnectionFactory>) -> Self {
        // Bounded channel — late subscribers can `Lagged` if a flurry
        // of reconnects fires before they catch up; we don't care
        // about per-event ordering for a UI badge, only the latest.
        let (state_tx, _state_rx) = broadcast::channel(16);
        let (replay_tx, _replay_rx) = broadcast::channel(16);
        Self {
            factory,
            current: Arc::new(Mutex::new(None)),
            backoff: DEFAULT_BACKOFF.to_vec(),
            state_tx: Arc::new(state_tx),
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
            replay_tx: Arc::new(replay_tx),
            watchdog: Arc::new(Mutex::new(None)),
        }
    }

    /// Override the reconnect schedule (mostly useful in tests — a
    /// tight schedule keeps test runtime down without changing
    /// semantics).
    #[must_use]
    pub fn with_backoff(mut self, schedule: Vec<Duration>) -> Self {
        self.backoff = schedule;
        self
    }

    /// Return an [`IpcInvoker`] trait object. Cheap — every dispatch
    /// re-acquires the current-runtime mutex internally so the same
    /// invoker survives reconnects.
    #[must_use]
    pub fn invoker(&self) -> Arc<dyn IpcInvoker + Send + Sync> {
        Arc::new(ReconnectingInvoker {
            factory: Arc::clone(&self.factory),
            current: Arc::clone(&self.current),
            backoff: self.backoff.clone(),
            state_tx: Arc::clone(&self.state_tx),
            subscriptions: Arc::clone(&self.subscriptions),
            replay_tx: Arc::clone(&self.replay_tx),
            watchdog: Arc::clone(&self.watchdog),
        })
    }

    /// Subscribe to [`ConnectionState`] transitions. The returned
    /// receiver fires whenever the wrapper notices a state change
    /// while servicing an `ipc_call`.
    ///
    /// BL-140 Phase 3c — the Tauri bridge subscribes at `boot_remote`
    /// time and forwards each transition to the frontend on the
    /// `kernel:connection-state` channel so the status-bar badge can
    /// reflect the current state.
    #[must_use]
    pub fn subscribe_state(&self) -> broadcast::Receiver<ConnectionState> {
        self.state_tx.subscribe()
    }

    /// BL-146 — subscribe to subscription-replay counts. Fires once
    /// per successful client install (initial connect + every
    /// reconnect) with the count of subscriptions that were re-
    /// installed. The first connect typically fires with `0` (registry
    /// empty); reconnects with N>0 distinguish "fresh connection" from
    /// "reconnected and replayed".
    #[must_use]
    pub fn subscribe_replays(&self) -> broadcast::Receiver<usize> {
        self.replay_tx.subscribe()
    }

    /// BL-146 — register a remote subscription with the reconnect
    /// wrapper. Records `(subscription_id, filter, sink)` in the
    /// internal registry AND installs the subscription against the
    /// current client. If no client is currently connected the
    /// registration is queued and the watchdog will install it on the
    /// next successful build.
    ///
    /// Same shape as [`nexus_remote::RemoteClient::subscribe`] but
    /// transparently survives reconnects. Callers who reach past this
    /// method via [`Self::ensure_client`] are not tracked.
    ///
    /// # Errors
    /// - [`IpcInvokerError::Transport`] if the underlying
    ///   `RemoteClient::subscribe` call fails. The registration is
    ///   rolled back so a failed first install doesn't get replayed
    ///   later. (If the client is offline, the call returns `Ok` — the
    ///   sub is queued for replay.)
    pub async fn subscribe(
        &self,
        subscription_id: &str,
        filter: Value,
        sink: mpsc::UnboundedSender<EventDelivery>,
    ) -> Result<(), IpcInvokerError> {
        // Record first so a fresh-client replay that races with this
        // call still picks up the entry.
        {
            let mut reg = self.subscriptions.lock().await;
            reg.insert(
                subscription_id.to_string(),
                SubscriptionEntry {
                    filter: filter.clone(),
                    sink: sink.clone(),
                },
            );
        }
        // Snapshot the current client (without holding the slot lock
        // across the await) and try to install. If there's no current
        // client, just leave the entry in the registry — the watchdog
        // /reconnect path will replay it.
        let maybe_client = {
            let slot = self.current.lock().await;
            slot.as_ref().map(|rt| Arc::clone(&rt.client))
        };
        let Some(client) = maybe_client else {
            return Ok(());
        };
        if let Err(e) = client.subscribe(subscription_id, filter, sink).await {
            // Roll back so a permanently-doomed sub (e.g. duplicate id
            // server-side) doesn't haunt every future reconnect.
            self.subscriptions.lock().await.remove(subscription_id);
            return Err(IpcInvokerError::Transport(format!(
                "remote subscribe failed: {e}"
            )));
        }
        Ok(())
    }

    /// BL-146 — cancel a subscription installed through
    /// [`Self::subscribe`]. Removes it from the registry so it doesn't
    /// get replayed on the next reconnect AND issues
    /// `event_unsubscribe` against the current client (if any).
    ///
    /// Returns `true` if the server acknowledged the unsubscribe;
    /// `false` if the registry knew the id but the server didn't, or
    /// no client is connected. Safe to call twice — second call returns
    /// `Ok(false)` instead of erroring on an unknown id.
    pub async fn unsubscribe(
        &self,
        subscription_id: &str,
    ) -> Result<bool, IpcInvokerError> {
        let removed = {
            let mut reg = self.subscriptions.lock().await;
            reg.remove(subscription_id).is_some()
        };
        let maybe_client = {
            let slot = self.current.lock().await;
            slot.as_ref().map(|rt| Arc::clone(&rt.client))
        };
        let Some(client) = maybe_client else {
            return Ok(false);
        };
        match client.unsubscribe(subscription_id).await {
            Ok(ok) => Ok(ok),
            Err(e) => {
                // Registry has already been cleared, so a second call
                // will be a no-op. The error itself surfaces — the
                // caller cares about whether the server saw it.
                let _ = removed;
                Err(IpcInvokerError::Transport(format!(
                    "remote unsubscribe failed: {e}"
                )))
            }
        }
    }

    /// Tear down the current connection (if any). Subsequent dispatches
    /// will rebuild on demand.
    ///
    /// BL-146 — also aborts the watchdog task so we don't race a
    /// caller-driven reset against an auto-reconnect; the next
    /// `ensure_connected` rebuilds + spawns a fresh watchdog. Existing
    /// subscriptions in the registry are preserved, so the rebuild
    /// will replay them.
    pub async fn reset(&self) {
        if let Some(handle) = self.watchdog.lock().await.take() {
            handle.abort();
        }
        let mut slot = self.current.lock().await;
        if let Some(runtime) = slot.take() {
            runtime.shutdown().await;
        }
    }

    /// Ensure a connection exists, building one through the factory
    /// if not, and return the underlying [`nexus_remote::RemoteClient`].
    ///
    /// BL-140 Phase 3 — needed by callers that want to drive
    /// [`nexus_remote::RemoteClient::subscribe`] directly (the
    /// `IpcInvoker` trait only covers `ipc_call`). The Tauri bridge
    /// uses this to wire `kernel_subscribe` over remote.
    ///
    /// The returned `Arc` clones the underlying client; the caller's
    /// reference outlives subsequent reconnects, but subscriptions
    /// installed *directly* against it are NOT replayed when the
    /// transport drops. Use [`Self::subscribe`] (which tracks the
    /// subscription in the registry + replays on reconnect) for any
    /// subscription that should survive a connection drop.
    ///
    /// # Errors
    /// Same shape as the first attempt of `ipc_call` — a build failure
    /// surfaces as `Transport("initial connection: ...")`.
    ///
    /// # Panics
    /// Panics if a successful build doesn't end up populating the
    /// slot (would indicate an internal logic bug, not a runtime
    /// condition).
    pub async fn ensure_client(
        &self,
    ) -> Result<Arc<nexus_remote::RemoteClient>, IpcInvokerError> {
        let mut slot = self.current.lock().await;
        if slot.is_none() {
            match self.factory.build().await {
                Ok(rt) => {
                    install_runtime_under_lock(
                        &mut slot,
                        rt,
                        &self.subscriptions,
                        &self.replay_tx,
                        &self.watchdog,
                        &self.factory,
                        &self.current,
                        &self.backoff,
                        &self.state_tx,
                    )
                    .await;
                }
                Err(e) => {
                    return Err(IpcInvokerError::Transport(format!(
                        "initial connection: {e}"
                    )));
                }
            }
        }
        Ok(Arc::clone(&slot.as_ref().expect("just set").client))
    }
}

struct ReconnectingInvoker {
    factory: Arc<dyn ConnectionFactory>,
    current: Arc<Mutex<Option<RemoteRuntime>>>,
    backoff: Vec<Duration>,
    state_tx: Arc<broadcast::Sender<ConnectionState>>,
    subscriptions: SubscriptionRegistry,
    replay_tx: Arc<broadcast::Sender<usize>>,
    watchdog: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl ReconnectingInvoker {
    /// Publish a state transition. Send errors (zero subscribers) are
    /// silently ignored — the wire-up doesn't care if anyone is
    /// listening.
    fn emit_state(&self, state: ConnectionState) {
        let _ = self.state_tx.send(state);
    }
}

impl ReconnectingInvoker {
    /// Ensure a connection exists. If `current` is `None`, build one
    /// through the factory. Errors during build surface as
    /// `Transport` — the caller is doing an IPC call and a connection
    /// failure is the closest semantic.
    async fn ensure_connected(&self) -> Result<(), IpcInvokerError> {
        let mut slot = self.current.lock().await;
        if slot.is_some() {
            return Ok(());
        }
        match self.factory.build().await {
            Ok(rt) => {
                install_runtime_under_lock(
                    &mut slot,
                    rt,
                    &self.subscriptions,
                    &self.replay_tx,
                    &self.watchdog,
                    &self.factory,
                    &self.current,
                    &self.backoff,
                    &self.state_tx,
                )
                .await;
                Ok(())
            }
            Err(e) => {
                // Initial build failure surfaces as Disconnected on
                // the state channel — the wrapper tried, couldn't get
                // a connection, and there's nothing more it will do
                // until the next dispatch. The `ipc_call` path's own
                // Reconnecting→Disconnected arc only fires when the
                // very first call succeeded; for "never connected"
                // sessions this is the only emit point.
                self.emit_state(ConnectionState::Disconnected);
                Err(IpcInvokerError::Transport(format!(
                    "initial connection: {e}"
                )))
            }
        }
    }

    /// Capture an `Arc<dyn IpcInvoker>` for the current runtime so the
    /// caller can dispatch a single call without holding the mutex
    /// across an `await` on the underlying ipc layer.
    async fn snapshot_invoker(
        &self,
    ) -> Option<Arc<dyn IpcInvoker + Send + Sync>> {
        let slot = self.current.lock().await;
        slot.as_ref().map(RemoteRuntime::invoker)
    }

    /// Tear down whatever connection sits in `current` (if any). The
    /// next `ensure_connected` will rebuild.
    async fn tear_down_current(&self) {
        let taken = {
            let mut slot = self.current.lock().await;
            slot.take()
        };
        if let Some(runtime) = taken {
            runtime.shutdown().await;
        }
    }
}

#[async_trait::async_trait]
impl IpcInvoker for ReconnectingInvoker {
    async fn ipc_call(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value, IpcInvokerError> {
        self.ensure_connected().await?;
        let first_attempt = match self.snapshot_invoker().await {
            Some(inv) => {
                inv.ipc_call(target_plugin_id, command_id, args.clone(), timeout)
                    .await
            }
            None => {
                return Err(IpcInvokerError::Transport(
                    "ensure_connected reported ok but invoker is missing".to_string(),
                ));
            }
        };
        // Only Transport failures trigger reconnect. Server errors +
        // timeouts surface as-is.
        let Err(IpcInvokerError::Transport(first_err)) = first_attempt else {
            // Non-Transport result (Ok, Remote, or Timeout) — the
            // connection itself is healthy. Mark Connected if we
            // weren't already (covers the first successful call after
            // an Idle/Reconnecting state).
            self.emit_state(ConnectionState::Connected);
            return first_attempt;
        };
        tracing::warn!(
            error = %first_err,
            plugin_id = %target_plugin_id,
            command_id = %command_id,
            "remote ipc_call transport failure; will attempt reconnect"
        );
        self.emit_state(ConnectionState::Reconnecting);
        self.tear_down_current().await;

        // Walk the backoff schedule once. Each tick: sleep, rebuild,
        // retry. First successful retry wins. After the schedule
        // exhausts surface the last error.
        let mut last_err = first_err;
        for (idx, delay) in self.backoff.iter().enumerate() {
            tokio::time::sleep(*delay).await;
            let build_result = self.factory.build().await;
            let rt = match build_result {
                Ok(rt) => rt,
                Err(e) => {
                    last_err = format!("reconnect attempt {}: {e}", idx + 1);
                    tracing::warn!(
                        attempt = idx + 1,
                        error = %last_err,
                        "reconnect build failed"
                    );
                    continue;
                }
            };
            // Take the lock, install the new runtime + spawn watchdog
            // + replay subscriptions, snapshot its invoker, drop the
            // lock before awaiting the retry call.
            let inv = {
                let mut slot = self.current.lock().await;
                install_runtime_under_lock(
                    &mut slot,
                    rt,
                    &self.subscriptions,
                    &self.replay_tx,
                    &self.watchdog,
                    &self.factory,
                    &self.current,
                    &self.backoff,
                    &self.state_tx,
                )
                .await;
                slot.as_ref().expect("just set").invoker()
            };
            match inv
                .ipc_call(target_plugin_id, command_id, args.clone(), timeout)
                .await
            {
                Ok(v) => {
                    tracing::info!(
                        attempt = idx + 1,
                        plugin_id = %target_plugin_id,
                        command_id = %command_id,
                        "remote ipc_call recovered after reconnect"
                    );
                    self.emit_state(ConnectionState::Connected);
                    return Ok(v);
                }
                Err(IpcInvokerError::Transport(e)) => {
                    last_err = format!("reconnect attempt {}: {e}", idx + 1);
                    self.tear_down_current().await;
                }
                Err(other) => {
                    // Server-reported error or timeout after reconnect
                    // — the transport is healthy again even though
                    // this specific call failed.
                    self.emit_state(ConnectionState::Connected);
                    return Err(other);
                }
            }
        }
        self.emit_state(ConnectionState::Disconnected);
        Err(IpcInvokerError::Transport(format!(
            "reconnect schedule exhausted: {last_err}"
        )))
    }
}

/// Install `rt` into the supplied current-slot guard, replay every
/// registered subscription against its client, publish the replay
/// count, and spawn a fresh watchdog. Aborts any prior watchdog first
/// so we don't end up with two competing reconnect loops.
///
/// Called from every code path that installs a `RemoteRuntime` into
/// `current` — initial build (`ensure_connected` / `ensure_client`) +
/// every iteration of the `ipc_call` reconnect loop — so subscription
/// replay + watchdog spawn are guaranteed to follow each install
/// exactly once.
///
/// The slot guard is borrowed mutably (so the caller stays in the
/// critical section) but the replay + watchdog setup itself doesn't
/// touch the slot.
#[allow(clippy::too_many_arguments)]
async fn install_runtime_under_lock(
    slot: &mut Option<RemoteRuntime>,
    rt: RemoteRuntime,
    subscriptions: &SubscriptionRegistry,
    replay_tx: &Arc<broadcast::Sender<usize>>,
    watchdog: &Arc<Mutex<Option<JoinHandle<()>>>>,
    factory: &Arc<dyn ConnectionFactory>,
    current: &Arc<Mutex<Option<RemoteRuntime>>>,
    backoff: &[Duration],
    state_tx: &Arc<broadcast::Sender<ConnectionState>>,
) {
    let client_for_replay = Arc::clone(&rt.client);
    *slot = Some(rt);
    let replayed = replay_subscriptions(subscriptions, &client_for_replay).await;
    let _ = replay_tx.send(replayed);
    spawn_watchdog(
        Arc::clone(&client_for_replay),
        Arc::clone(subscriptions),
        Arc::clone(replay_tx),
        Arc::clone(watchdog),
        Arc::clone(factory),
        Arc::clone(current),
        backoff.to_vec(),
        Arc::clone(state_tx),
    )
    .await;
}

/// Iterate the subscription registry and install each entry against
/// the freshly-built client. Returns the count of subscriptions
/// installed. Errors per-subscription are logged but don't abort the
/// loop — one bad sub shouldn't take down the others.
///
/// The registry snapshot is taken under-lock + cloned so the
/// individual subscribe awaits don't hold the registry lock (a
/// concurrent `subscribe()` / `unsubscribe()` would otherwise block).
async fn replay_subscriptions(
    subscriptions: &SubscriptionRegistry,
    client: &RemoteClient,
) -> usize {
    let entries: Vec<(String, SubscriptionEntry)> = {
        let reg = subscriptions.lock().await;
        reg.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    };
    let mut replayed = 0_usize;
    for (id, entry) in entries {
        match client.subscribe(&id, entry.filter, entry.sink).await {
            Ok(_) => replayed += 1,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    subscription_id = %id,
                    "subscription replay failed"
                );
            }
        }
    }
    replayed
}

/// Spawn a watchdog task that loops on
/// `client.wait_for_disconnect()` → reconnect-with-replay → repeat.
/// Aborts any prior watchdog first so we don't end up with two
/// competing loops if `install_runtime_under_lock` is called twice in
/// rapid succession.
///
/// The loop continues until either the backoff schedule exhausts (the
/// task ends, emitting `Disconnected`) or the task is explicitly
/// aborted (typically by `install_runtime_under_lock` spawning a
/// successor watchdog).
#[allow(clippy::too_many_arguments)]
async fn spawn_watchdog(
    client: Arc<RemoteClient>,
    subscriptions: SubscriptionRegistry,
    replay_tx: Arc<broadcast::Sender<usize>>,
    watchdog: Arc<Mutex<Option<JoinHandle<()>>>>,
    factory: Arc<dyn ConnectionFactory>,
    current: Arc<Mutex<Option<RemoteRuntime>>>,
    backoff: Vec<Duration>,
    state_tx: Arc<broadcast::Sender<ConnectionState>>,
) {
    {
        let mut guard = watchdog.lock().await;
        if let Some(prev) = guard.take() {
            prev.abort();
        }
    }
    let handle = tokio::spawn(async move {
        let mut watched = client;
        loop {
            watched.wait_for_disconnect().await;
            // Possible race: ipc_call (or another install path) might
            // have already replaced `current` with a fresh runtime.
            // Pivot to watching the new client instead of rebuilding.
            {
                let slot = current.lock().await;
                if let Some(rt) = slot.as_ref() {
                    if !Arc::ptr_eq(&rt.client, &watched) {
                        watched = Arc::clone(&rt.client);
                        continue;
                    }
                }
            }
            tracing::warn!("remote watchdog: connection lost; reconnecting");
            let _ = state_tx.send(ConnectionState::Reconnecting);
            if let Some(new_rt) = watchdog_rebuild(&factory, &current, &backoff).await {
                let new_client = Arc::clone(&new_rt.client);
                {
                    let mut slot = current.lock().await;
                    if let Some(old) = slot.take() {
                        old.shutdown().await;
                    }
                    *slot = Some(new_rt);
                }
                let replayed = replay_subscriptions(&subscriptions, &new_client).await;
                let _ = replay_tx.send(replayed);
                let _ = state_tx.send(ConnectionState::Connected);
                tracing::info!(replayed, "remote watchdog: reconnect succeeded");
                watched = new_client;
            } else {
                let _ = state_tx.send(ConnectionState::Disconnected);
                tracing::warn!("remote watchdog: backoff exhausted");
                break;
            }
        }
    });
    *watchdog.lock().await = Some(handle);
}

/// Walk the backoff schedule, asking the factory for a new runtime on
/// each tick. Returns `Some(runtime)` on first success, `None` if the
/// schedule exhausted without one succeeding.
///
/// Doesn't touch `current` itself — the caller is responsible for
/// installing the returned runtime under the slot lock (so the
/// install + replay + watchdog-spawn happen as one atomic step).
async fn watchdog_rebuild(
    factory: &Arc<dyn ConnectionFactory>,
    current: &Arc<Mutex<Option<RemoteRuntime>>>,
    backoff: &[Duration],
) -> Option<RemoteRuntime> {
    // Tear down whatever stale runtime is sitting in the slot so the
    // next slot.lock observer sees the right state.
    if let Some(stale) = current.lock().await.take() {
        stale.shutdown().await;
    }
    for (idx, delay) in backoff.iter().enumerate() {
        tokio::time::sleep(*delay).await;
        match factory.build().await {
            Ok(rt) => return Some(rt),
            Err(e) => {
                tracing::warn!(
                    attempt = idx + 1,
                    error = %e,
                    "remote watchdog: rebuild attempt failed"
                );
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_remote::SshForgeUri;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A factory that always fails. Used to verify the backoff
    /// schedule + the "schedule exhausted" surfacing.
    struct AlwaysFailFactory {
        attempts: Arc<AtomicUsize>,
    }

    impl ConnectionFactory for AlwaysFailFactory {
        fn build<'a>(
            &'a self,
        ) -> Pin<Box<dyn Future<Output = Result<RemoteRuntime>> + Send + 'a>> {
            let attempts = Arc::clone(&self.attempts);
            Box::pin(async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(anyhow::anyhow!("simulated build failure"))
            })
        }
    }

    #[tokio::test]
    async fn initial_connection_failure_surfaces_as_transport_error() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let factory = Arc::new(AlwaysFailFactory {
            attempts: Arc::clone(&attempts),
        });
        let rt = ReconnectingRuntime::new(factory);
        let inv = rt.invoker();
        let err = inv
            .ipc_call("com.x", "y", Value::Null, Duration::from_secs(1))
            .await
            .unwrap_err();
        match err {
            IpcInvokerError::Transport(msg) => {
                assert!(msg.contains("initial connection"), "got: {msg}");
            }
            other => panic!("expected Transport, got: {other}"),
        }
        // One build attempt — the initial connect. The reconnect path
        // never engages because we never got a first connection.
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn default_backoff_is_the_documented_schedule() {
        assert_eq!(
            DEFAULT_BACKOFF,
            [
                Duration::from_millis(100),
                Duration::from_millis(500),
                Duration::from_secs(2),
                Duration::from_secs(10),
                Duration::from_secs(30),
            ]
        );
    }

    #[test]
    fn ssh_connection_factory_is_constructible() {
        // Smoke test — the factory just captures the URI. Real spawn
        // behaviour is covered by the integration tests over duplex
        // pairs.
        let uri = ForgeUri::Ssh(SshForgeUri {
            user: Some("alice".to_string()),
            host: "host".to_string(),
            port: Some(22),
            path: "/srv/forge".to_string(),
        });
        let _factory = SshConnectionFactory::new(uri);
    }
}
