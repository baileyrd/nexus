//! Tauri managed state holder for the Nexus kernel runtime + the Tauri
//! commands that drive it.
//!
//! The shell boots with no kernel — `KernelRuntime::new()` yields an empty
//! slot. The lifecycle commands are:
//!
//! * [`init_forge`] — prepare an on-disk forge (mkdir + SQLite schema).
//! * [`boot_kernel`] — run `nexus_bootstrap::build_cli_runtime` and swap the
//!   resulting runtime pieces into the slot.
//! * [`shutdown_kernel`] — drain the runtime (kernel + plugin loader).
//!
//! The bridge surface (added in Phase 0 step 4) is:
//!
//! * [`kernel_invoke`] — JSON-envelope over `context.ipc_call`, with an
//!   optional timeout (default 30s). The shared `Arc<KernelPluginContext>`
//!   is cloned out of the guard before `.await` so the outer mutex is never
//!   held across the dispatch.
//! * [`kernel_subscribe`] / [`kernel_unsubscribe`] — subscribe spawns a
//!   tokio task that `.recv()`s on an `EventSubscription` with
//!   `EventFilter::CustomPrefix(prefix)` and forwards every matching
//!   `NexusEvent::Custom` to the frontend via `app.emit("kernel:event", ..)`.
//!   The `JoinHandle` is tracked so `kernel_unsubscribe` (and shutdown) can
//!   abort it.
//! * [`kernel_is_booted`] — cheap sync boolean, backed by an `AtomicBool`
//!   kept in sync with the runtime slot, so frontend `available()` polls
//!   don't need to take the async mutex.
//!
//! On window close, `lib.rs` fires `shutdown_kernel` via
//! [`KernelRuntime::shutdown`] so background tokio tasks and SQLite handles
//! don't leak across restarts.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use nexus_bootstrap::invoker::{IpcInvoker, IpcInvokerError};
use nexus_bootstrap::reconnect::{
    ConnectionState, ReconnectingRuntime, SshConnectionFactory,
};
use nexus_bootstrap::Runtime;
use nexus_kernel::{
    EventFilter, Events, IpcErrorEnvelope, IpcErrorKind, Kernel, KernelPluginContext, NexusEvent,
    RecvError,
};
use nexus_plugins::SharedPluginLoader;
use nexus_remote::ForgeUri;
use serde::Serialize;
use tauri::{AppHandle, Emitter, WebviewWindow};
use tauri::async_runtime::JoinHandle;
use tokio::sync::{mpsc, Mutex};

// Invoker plugin id note: we currently call
// `nexus_bootstrap::build_cli_runtime`, which registers the invoker as
// `com.nexus.cli`. The private `build(forge_root, invoker_id, invoker_name)`
// is the only place that parameter is configurable, and we're not modifying
// nexus-bootstrap in this task — so the shell rides on the CLI identity until
// a `build_shell_runtime` entry is added upstream.

/// Default `kernel_invoke` timeout when the caller doesn't supply one.
const DEFAULT_INVOKE_TIMEOUT_MS: u64 = 30_000;

/// Event channel name used for every forwarded kernel event. The frontend
/// disambiguates by `subscription_id` inside the envelope payload.
const KERNEL_EVENT_CHANNEL: &str = "kernel:event";

/// BL-140 Phase 3c — channel name for `ConnectionState` transitions
/// forwarded from the [`ReconnectingRuntime`]'s state broadcast to the
/// frontend. Only fires for remote forges; the status-bar badge plugin
/// subscribes to render the badge.
const CONNECTION_STATE_CHANNEL: &str = "kernel:connection-state";

/// BL-146 — channel name for subscription-replay notifications. Fires
/// after the reconnect wrapper re-installs N subscriptions against a
/// freshly-built client so the frontend can render a "reconnected, N
/// subscriptions restored" toast (or distinguish first-connect from
/// reconnect on the status badge).
const SUBSCRIPTIONS_REPLAYED_CHANNEL: &str = "kernel:subscriptions-replayed";

/// Payload shape emitted to the webview for every bridged kernel event.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct KernelEventEnvelope {
    subscription_id: String,
    topic: String,
    payload: serde_json::Value,
}

/// Payload shape for `kernel:connection-state` events. `state` is the
/// stable wire string from [`ConnectionState::as_str`] (one of
/// `"idle"` / `"connected"` / `"reconnecting"` / `"disconnected"`).
#[derive(Serialize, Clone)]
struct ConnectionStateEnvelope {
    state: &'static str,
}

/// Payload shape for `kernel:subscriptions-replayed` events (BL-146).
/// `replayed` is the number of subscriptions re-installed against the
/// freshly-built client. `0` for the initial connect; >0 for
/// reconnect-with-replay.
#[derive(Serialize, Clone)]
struct SubscriptionsReplayedEnvelope {
    replayed: usize,
}

/// Booted runtime — either a local kernel + plugin loader, or a remote
/// SSH-backed proxy. The discriminant decides which path
/// [`kernel_invoke`] + [`kernel_subscribe`] follow.
///
/// BL-140 Phase 3 — the `Remote` variant carries a [`ReconnectingRuntime`]
/// so a dropped SSH connection rebuilds transparently on the next
/// `kernel_invoke`. Subscription replay across reconnect is filed as a
/// separate follow-up (today a transport drop silently kills all
/// active subscriptions — the frontend has to re-subscribe).
enum BootedRuntime {
    Local {
        kernel: Kernel,
        context: Arc<KernelPluginContext>,
        loader: Arc<SharedPluginLoader>,
    },
    Remote {
        runtime: Arc<ReconnectingRuntime>,
        invoker: Arc<dyn IpcInvoker + Send + Sync>,
        /// Background task forwarding [`ConnectionState`] transitions
        /// from the reconnecting runtime onto the
        /// `kernel:connection-state` Tauri channel. Aborted on
        /// shutdown.
        state_forwarder: JoinHandle<()>,
        /// BL-146 — background task forwarding subscription-replay
        /// counts from the reconnecting runtime onto the
        /// `kernel:subscriptions-replayed` Tauri channel. Aborted on
        /// shutdown.
        replay_forwarder: JoinHandle<()>,
        /// Latest [`ConnectionState`] observed. Read by the
        /// `kernel_connection_state` Tauri command so the frontend
        /// can render the badge before any transition has fired.
        current_state: Arc<StdMutex<ConnectionState>>,
    },
}

impl BootedRuntime {
    fn invoker(&self) -> Arc<dyn IpcInvoker + Send + Sync> {
        match self {
            Self::Local { context, .. } => Arc::new(
                nexus_bootstrap::invoker::LocalIpcInvoker::new((**context).clone()),
            ),
            Self::Remote { invoker, .. } => Arc::clone(invoker),
        }
    }

    #[allow(dead_code)] // Phase 3b consumer in the workspace plugin path.
    fn is_remote(&self) -> bool {
        matches!(self, Self::Remote { .. })
    }
}

/// Live subscription tracked by [`KernelRuntime::subscriptions`].
/// The `window_label` tag lets `cancel_window` find every
/// subscription belonging to a closing window without having to
/// reach into the spawned forwarder task.
struct SubscriptionEntry {
    handle: JoinHandle<()>,
    window_label: String,
}

/// Tauri-managed holder for the (optionally-booted) kernel runtime.
///
/// The inner `Option<BootedRuntime>` starts as `None` and is populated by
/// [`boot_kernel`] once a forge root is known. A parallel `AtomicBool`
/// mirrors `is_some()` so `kernel_is_booted` stays a sync, lock-free call
/// — otherwise every frontend availability probe would have to take the
/// async mutex.
pub struct KernelRuntime {
    inner: Arc<Mutex<Option<BootedRuntime>>>,
    /// Kept in sync with `inner.is_some()`. Writes happen only under the
    /// mutex so the two never drift from a reader's point of view.
    booted: Arc<AtomicBool>,
    /// Active event subscriptions, tagged with the window label that
    /// created them so `cancel_window` can find and abort every
    /// subscription belonging to a closing window. Uses
    /// `std::sync::Mutex` because we only hold it for quick map
    /// operations (insert / remove / drain) — never across an await.
    ///
    /// Pre-fix this was `HashMap<sub_id, JoinHandle>`; the forwarder
    /// task knew its window_label but the map did not, so a closing
    /// window left its subscriptions running. They would silently
    /// emit_to a dead window forever (Tauri no-ops the emit), leaking
    /// a tokio task per popout-with-subscription.
    subscriptions: Arc<StdMutex<HashMap<String, SubscriptionEntry>>>,
    /// Per-window cancellation tokens, keyed by Tauri webview label.
    /// Lazily populated on the first `kernel_invoke` from each window;
    /// the window's token is fired by [`cancel_window`] when the user
    /// closes that window so in-flight calls from that window receive
    /// `IpcError::Cancelled` and the kernel releases their resources
    /// (rather than letting popout closes leak the work).
    window_cancels: Arc<StdMutex<HashMap<String, nexus_kernel::cancel::CancellationToken>>>,
}

impl KernelRuntime {
    /// Create an empty runtime slot. No kernel is booted yet.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            booted: Arc::new(AtomicBool::new(false)),
            subscriptions: Arc::new(StdMutex::new(HashMap::new())),
            window_cancels: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    /// Cancellation token for the given Tauri webview label. Creates a
    /// new root token on first request from that window; returns the
    /// same token on subsequent calls so every `kernel_invoke` from
    /// the window shares a cancel scope. Cloned so the caller can
    /// derive child tokens / scope it without holding the map lock.
    pub fn window_cancel_token(&self, label: &str) -> nexus_kernel::cancel::CancellationToken {
        let mut map = self
            .window_cancels
            .lock()
            .expect("window_cancels mutex poisoned");
        map.entry(label.to_string())
            .or_insert_with(nexus_kernel::cancel::CancellationToken::new)
            .clone()
    }

    /// Fire the cancellation token for `label`, drop the token map
    /// entry, and abort every subscription forwarder still running
    /// for that window.
    ///
    /// Any in-flight `kernel_invoke` from that window observes the
    /// cancel via the kernel's IPC dispatch race and returns
    /// `IpcError::Cancelled`. Any active subscription (created via
    /// `kernel_subscribe`) has its forwarder task aborted so it stops
    /// `emit_to`-ing a now-dead window and stops draining its
    /// `EventSubscription` from the kernel bus.
    ///
    /// No-op for windows that never invoked or subscribed.
    pub fn cancel_window(&self, label: &str) {
        // Token side — see KernelRuntime::window_cancel_token.
        let token = self
            .window_cancels
            .lock()
            .expect("window_cancels mutex poisoned")
            .remove(label);
        if let Some(token) = token {
            token.cancel();
        }

        // Subscription side — find every entry tagged with this label,
        // remove them from the map, and abort their forwarder tasks.
        // Held briefly: the abort itself is sync-fire-and-forget.
        let aborted: Vec<SubscriptionEntry> = {
            let mut subs = self
                .subscriptions
                .lock()
                .expect("subscriptions mutex poisoned");
            let matching: Vec<String> = subs
                .iter()
                .filter(|(_, e)| e.window_label == label)
                .map(|(id, _)| id.clone())
                .collect();
            matching
                .into_iter()
                .filter_map(|id| subs.remove(&id))
                .collect()
        };
        for entry in aborted {
            entry.handle.abort();
        }
    }

    /// Returns `true` once a `Runtime` has been swapped into the slot.
    ///
    /// Reads the atomic flag — cheap, lock-free, safe to call from a sync
    /// Tauri command.
    pub fn is_booted(&self) -> bool {
        self.booted.load(Ordering::Acquire)
    }

    /// Boot a kernel runtime rooted at `path` and stash it in the slot.
    ///
    /// Extracted from the `#[tauri::command] boot_kernel` body so the
    /// `setup` hook in `lib.rs` can boot the runtime before any webview
    /// IPC is in play (needed for the e2e harness, which can't reliably
    /// invoke commands through the BiDi bridge — Tauri v2 rejects the
    /// webdriver-injected Origin header).
    pub async fn boot_at(&self, path: &std::path::Path) -> Result<(), String> {
        let mut guard = self.inner.lock().await;
        if guard.is_some() {
            return Err("kernel already booted".to_string());
        }
        let Runtime {
            kernel,
            context,
            loader,
        } = nexus_bootstrap::build_cli_runtime(path.to_path_buf())
            .map_err(|e| format!("{e:#}"))?;
        *guard = Some(BootedRuntime::Local {
            kernel,
            context: Arc::new(context),
            loader,
        });
        self.booted.store(true, Ordering::Release);
        Ok(())
    }

    /// BL-140 Phase 3 — boot a remote-forge runtime against the
    /// supplied `ssh://` URI. The SSH child isn't spawned eagerly —
    /// the first `kernel_invoke` triggers
    /// [`SshConnectionFactory::build`].
    ///
    /// Spawns a background forwarder that observes
    /// [`ConnectionState`] transitions on the reconnecting runtime
    /// and re-emits them onto the `kernel:connection-state` Tauri
    /// channel so the status-bar badge can render the current state.
    ///
    /// Rejects when a runtime is already booted, same posture as
    /// [`Self::boot_at`].
    pub async fn boot_remote_at(
        &self,
        uri_str: &str,
        app: AppHandle,
    ) -> Result<(), String> {
        let uri = ForgeUri::parse(uri_str)
            .map_err(|e| format!("invalid forge URI '{uri_str}': {e}"))?;
        let mut guard = self.inner.lock().await;
        if guard.is_some() {
            return Err("kernel already booted".to_string());
        }
        let factory = Arc::new(SshConnectionFactory::new(uri));
        let runtime = Arc::new(ReconnectingRuntime::new(factory));
        let invoker = runtime.invoker();
        let current_state: Arc<StdMutex<ConnectionState>> =
            Arc::new(StdMutex::new(ConnectionState::Idle));

        // Subscribe BEFORE storing the runtime so we don't miss any
        // emit fired by a racy first dispatch.
        let mut state_rx = runtime.subscribe_state();
        let mut replay_rx = runtime.subscribe_replays();
        let state_for_task = Arc::clone(&current_state);
        let app_for_state = app.clone();
        let state_forwarder = tauri::async_runtime::spawn(async move {
            loop {
                match state_rx.recv().await {
                    Ok(state) => {
                        if let Ok(mut guard) = state_for_task.lock() {
                            *guard = state;
                        }
                        // Broadcast to every window — connection state
                        // is global to the workspace, not per-window
                        // (every popout shares the same runtime).
                        let envelope = ConnectionStateEnvelope {
                            state: state.as_str(),
                        };
                        if let Err(e) =
                            app_for_state.emit(CONNECTION_STATE_CHANNEL, envelope)
                        {
                            eprintln!(
                                "[boot_remote] connection-state emit failed: {e}"
                            );
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        let replay_forwarder = tauri::async_runtime::spawn(async move {
            while let Ok(replayed) = replay_rx.recv().await {
                let envelope = SubscriptionsReplayedEnvelope { replayed };
                if let Err(e) = app.emit(SUBSCRIPTIONS_REPLAYED_CHANNEL, envelope) {
                    eprintln!(
                        "[boot_remote] subscriptions-replayed emit failed: {e}"
                    );
                }
            }
        });

        *guard = Some(BootedRuntime::Remote {
            runtime,
            invoker,
            state_forwarder,
            replay_forwarder,
            current_state,
        });
        self.booted.store(true, Ordering::Release);
        Ok(())
    }

    /// Snapshot of the current connection state. Returns
    /// `ConnectionState::Idle` for local forges + when no runtime is
    /// booted (the frontend renders no badge in those cases).
    pub async fn connection_state(&self) -> ConnectionState {
        let guard = self.inner.lock().await;
        match guard.as_ref() {
            Some(BootedRuntime::Remote { current_state, .. }) => current_state
                .lock()
                .map(|s| *s)
                .unwrap_or(ConnectionState::Idle),
            _ => ConnectionState::Idle,
        }
    }

    /// BL-096 follow-up — live-revoke a previously-granted HIGH-risk
    /// capability from a loaded plugin. Routes through
    /// `SharedPluginLoader::revoke_capability` which mutates the
    /// running plugin's wired context cap set, persists to
    /// `granted_caps.json`, audits, and publishes
    /// `com.nexus.kernel.capability_revoked` on the bus.
    ///
    /// The capability is parsed from the dotted kernel form
    /// (`fs.read`, `process.spawn`, …); unknown strings are rejected
    /// before the loader is touched so a buggy frontend can't spam
    /// the audit log with garbage.
    ///
    /// Returns the kind / message of any [`PluginError`] as a string —
    /// matching the rest of the Tauri-command surface, which renders
    /// `Result<_, String>` rather than typed envelopes for the host
    /// commands. Caller branches on the message when it needs to
    /// distinguish "plugin not loaded" from "non-revocable
    /// capability"; the loader's error variants are preserved in the
    /// rendered `Display`.
    pub async fn revoke_plugin_capability(
        &self,
        plugin_id: &str,
        capability: &str,
    ) -> Result<(), String> {
        let cap = nexus_plugin_api::Capability::from_str(capability).map_err(|_| {
            format!(
                "revoke_plugin_capability: '{capability}' is not a recognised \
                 capability — wire form is the dotted kernel name (e.g. 'fs.read', \
                 'process.spawn')."
            )
        })?;
        let loader = {
            let guard = self.inner.lock().await;
            match guard.as_ref() {
                Some(BootedRuntime::Local { loader, .. }) => Arc::clone(loader),
                Some(BootedRuntime::Remote { .. }) => {
                    return Err(
                        "revoke_plugin_capability is local-only — community plugins live on the remote host"
                            .to_string(),
                    );
                }
                None => return Err("kernel not booted".to_string()),
            }
        };
        // `SharedPluginLoader::revoke_capability` takes `&self` and
        // does its own internal locking; we drop the runtime guard
        // before calling so concurrent kernel work doesn't serialise
        // on the outer mutex.
        loader
            .revoke_capability(plugin_id, cap)
            .map_err(|e| format!("{e}"))
    }

    /// Drain the runtime: pull it out of the slot, abort every active
    /// subscription forwarder, call `kernel.shutdown().await`, then unload
    /// every plugin.
    ///
    /// Idempotent — shutting down an empty slot returns `Ok(())`.
    ///
    /// Errors from individual steps are aggregated so a failing plugin
    /// doesn't prevent the kernel's shutdown flag from flipping or prevent
    /// other plugins from unloading. The joined error string is returned at
    /// the end if any step failed.
    pub async fn shutdown(&self) -> Result<(), String> {
        // Abort active subscriptions first so their background tasks stop
        // touching the kernel before we tear it down.
        {
            let mut subs = self.subscriptions.lock().expect("subscriptions mutex poisoned");
            for (_id, entry) in subs.drain() {
                entry.handle.abort();
            }
        }

        let runtime_opt = {
            let mut guard = self.inner.lock().await;
            let taken = guard.take();
            if taken.is_some() {
                self.booted.store(false, Ordering::Release);
            }
            taken
        };
        let Some(runtime) = runtime_opt else {
            return Ok(());
        };

        let mut errors: Vec<String> = Vec::new();

        match runtime {
            BootedRuntime::Local {
                kernel, loader, ..
            } => {
                if let Err(e) = kernel.shutdown().await {
                    errors.push(format!("kernel shutdown: {e:#}"));
                }

                // `PluginLoader` has no `shutdown` — only
                // `PluginManager::shutdown` does, and nexus-bootstrap
                // does not wrap its loader in a manager. Replicate
                // the manager's drain-in-reverse-registration-order
                // logic here so every plugin's `on_stop` hook fires
                // and a failing plugin doesn't prevent its siblings
                // from unloading.
                let mut loader_guard = loader.lock();
                let mut ids = loader_guard.registration_order();
                ids.reverse();
                for id in ids {
                    if let Err(e) = loader_guard.unload(&id) {
                        errors.push(format!("unload {id}: {e}"));
                    }
                }
            }
            BootedRuntime::Remote {
                runtime,
                state_forwarder,
                replay_forwarder,
                ..
            } => {
                // The remote kernel + plugins live on the server side
                // and shut themselves down when the SSH transport
                // closes. Locally we only need to reset the
                // reconnecting wrapper (drops the current client +
                // router) and abort the state + replay forwarders.
                state_forwarder.abort();
                replay_forwarder.abort();
                runtime.reset().await;
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

impl Default for KernelRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Prepare an on-disk forge at `path`.
///
/// Creates `notes/`, `attachments/`, and `.forge/` (idempotent via
/// `create_dir_all`) then calls [`nexus_bootstrap::init_forge`] which runs
/// `StorageEngine::init` to set up the SQLite schema + search index.
///
/// Optionally applies a scaffold `template` (BL-054 Phase 1: `"os"`
/// lays down the Agentic OS directory layout + memory-map `CLAUDE.md`).
/// Idempotent: re-running against an already-templated forge is a
/// no-op that never overwrites pre-existing user files.
///
/// Does NOT boot a kernel — the caller is expected to follow up with
/// [`boot_kernel`]. Safe to call against an already-initialised forge.
#[tauri::command]
pub async fn init_forge(path: String, template: Option<String>) -> Result<(), String> {
    let root = PathBuf::from(&path);
    // Mirrors the legacy shell's `forge.rs::init_layout` ordering
    // (retired Phase 4 WI-37): create the top-level skeleton first so
    // `init_forge` below never trips over a missing `.forge/` parent.
    for sub in ["notes", "attachments", ".forge"] {
        let dir = root.join(sub);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create '{}': {e}", dir.display()))?;
    }
    nexus_bootstrap::init_forge(&root).map_err(|e| format!("{e:#}"))?;
    if let Some(name) = template {
        let kind = nexus_bootstrap::forge_template::ForgeTemplate::from_str(&name)
            .ok_or_else(|| format!("unknown forge template '{name}' (expected: os)"))?;
        nexus_bootstrap::forge_template::apply(&root, kind).map_err(|e| format!("{e:#}"))?;
    }
    Ok(())
}

/// Boot a kernel runtime rooted at `path` and stash it in managed state.
///
/// Rejects with `"kernel already booted"` if a runtime is already in the
/// slot. Callers must shut the old one down first via [`shutdown_kernel`]
/// before booting against a different forge — keeping the two operations
/// explicit avoids hidden re-entrancy during a workspace swap.
///
/// On any error mid-boot the slot stays `None`: `build_cli_runtime` either
/// succeeds and returns a `Runtime`, or fails and leaves nothing behind to
/// clean up.
#[tauri::command]
pub async fn boot_kernel(
    path: String,
    runtime: tauri::State<'_, KernelRuntime>,
) -> Result<(), String> {
    // `nexus_bootstrap::build` is private; `build_cli_runtime` is the public
    // entry that forwards to `build(forge_root, "com.nexus.cli", "Nexus
    // CLI")`. Using it here means the shell's invoker identity is
    // `com.nexus.cli` for now — acceptable during Phase 0 since no kernel
    // plugin gates on invoker id. A dedicated `build_shell_runtime` can be
    // added to nexus-bootstrap later.
    runtime.boot_at(&PathBuf::from(&path)).await
}

/// BL-140 Phase 3 — boot a remote-forge runtime against an `ssh://`
/// URI. The SSH child is spawned lazily on the first `kernel_invoke`,
/// not at boot time, so a transient network blip doesn't block the
/// shell's startup. Subsequent transport failures are retried by the
/// [`ReconnectingRuntime`] wrapper per BL-140 Phase 2c semantics.
///
/// Same posture as [`boot_kernel`] otherwise: rejects when a runtime
/// is already booted. Frontend swaps forges via
/// `shutdown_kernel` → `boot_*` in the workspace plugin.
#[tauri::command]
pub async fn boot_remote(
    uri: String,
    app: AppHandle,
    runtime: tauri::State<'_, KernelRuntime>,
) -> Result<(), String> {
    runtime.boot_remote_at(&uri, app).await
}

/// BL-140 Phase 3c — return the current [`ConnectionState`] as its
/// wire-form string. `"idle"` for local forges + un-booted state.
/// The frontend uses this for the initial badge render before any
/// `kernel:connection-state` event has fired.
#[tauri::command]
pub async fn kernel_connection_state(
    runtime: tauri::State<'_, KernelRuntime>,
) -> Result<&'static str, String> {
    Ok(runtime.connection_state().await.as_str())
}

/// Tear down the kernel runtime if one is booted. Idempotent.
#[tauri::command]
pub async fn shutdown_kernel(
    runtime: tauri::State<'_, KernelRuntime>,
) -> Result<(), String> {
    runtime.shutdown().await
}

/// BL-096 follow-up — live-revoke a HIGH-risk capability from a
/// loaded plugin. Routes through
/// [`KernelRuntime::revoke_plugin_capability`] which mutates the
/// running plugin's wired context cap set, persists to
/// `granted_caps.json`, audits, and publishes
/// `com.nexus.kernel.capability_revoked` on the kernel bus.
///
/// Companion to the existing file-only `set_plugin_granted_capabilities`
/// (which writes `granted_caps.json` without touching the live kernel
/// state, so the change only takes effect at next boot). The "Revoke"
/// button in the shell calls *this* verb when a kernel is booted so
/// the plugin loses the cap immediately, falling back to the
/// file-write command when no kernel is around.
///
/// SECURITY: routes through `Capability::from_str` for input
/// validation — same posture as `set_plugin_granted_capabilities`.
/// Non-HIGH-risk caps are silently no-ops at the loader layer
/// (auto-granted from manifest, can't be revoked at runtime).
#[tauri::command]
pub async fn revoke_plugin_capability(
    plugin_id: String,
    capability: String,
    runtime: tauri::State<'_, KernelRuntime>,
) -> Result<(), String> {
    runtime
        .revoke_plugin_capability(&plugin_id, &capability)
        .await
}

/// Invoke a kernel plugin command through `context.ipc_call`.
///
/// The outer runtime mutex is released before the dispatch `.await` so
/// concurrent invokes don't serialize and re-entrant IPC calls don't
/// deadlock against the mutex.
///
/// The call is scoped under the calling window's CancellationToken
/// (see [`KernelRuntime::window_cancel_token`]) so a popout close
/// fires the token and any in-flight dispatch from that window
/// returns `IpcError::Cancelled`. Other windows' in-flight calls are
/// unaffected because each window owns its own token.
#[tauri::command]
pub async fn kernel_invoke(
    plugin_id: String,
    command_id: String,
    args: serde_json::Value,
    timeout_ms: Option<u64>,
    webview: tauri::Webview,
    runtime: tauri::State<'_, KernelRuntime>,
) -> Result<serde_json::Value, IpcErrorEnvelope> {
    let invoker = {
        let guard = runtime.inner.lock().await;
        let Some(booted) = guard.as_ref() else {
            // No `IpcError` variant matches "kernel hasn't been booted yet"
            // (that's a shell-side state, not a kernel-side dispatch failure).
            // Surface it as DispatchFailed so the frontend can branch on
            // `kind` rather than parse `message`.
            return Err(IpcErrorEnvelope {
                kind: IpcErrorKind::DispatchFailed,
                plugin_id: plugin_id.clone(),
                command: command_id.clone(),
                message: "kernel not booted".to_string(),
                retryable: false,
            });
        };
        booted.invoker()
    };

    let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_INVOKE_TIMEOUT_MS));
    let window_token = runtime.window_cancel_token(webview.label());

    let fut = invoker.ipc_call(&plugin_id, &command_id, args, timeout);
    nexus_kernel::cancel::scope_async(window_token, fut)
        .await
        .map_err(|e| invoker_error_to_envelope(&e, &plugin_id, &command_id))
}

/// BL-140 Phase 3 — map [`IpcInvokerError`] (covers local + remote
/// shapes) to the IPC envelope the frontend already knows how to
/// decode. Remote-only variants degrade to existing kinds:
/// `Remote { code }` → `DispatchFailed` (server error), `Transport` →
/// `DispatchFailed` with `retryable=true` (transient connection blip),
/// `Timeout` → `Timeout` with the carried fields, `Local` →
/// unwraps to the existing path.
fn invoker_error_to_envelope(
    e: &IpcInvokerError,
    plugin_id: &str,
    command_id: &str,
) -> IpcErrorEnvelope {
    match e {
        IpcInvokerError::Local(inner) => {
            IpcErrorEnvelope::from_ipc_error_in_context(inner, plugin_id, command_id)
        }
        IpcInvokerError::Remote { code, message } => IpcErrorEnvelope {
            kind: IpcErrorKind::DispatchFailed,
            plugin_id: plugin_id.to_string(),
            command: command_id.to_string(),
            message: format!("remote server error (code {code}): {message}"),
            retryable: false,
        },
        IpcInvokerError::Transport(msg) => IpcErrorEnvelope {
            kind: IpcErrorKind::DispatchFailed,
            plugin_id: plugin_id.to_string(),
            command: command_id.to_string(),
            message: format!("transport error: {msg}"),
            retryable: true,
        },
        IpcInvokerError::Timeout {
            plugin_id: pid,
            command,
            timeout_ms,
        } => IpcErrorEnvelope {
            kind: IpcErrorKind::Timeout,
            plugin_id: pid.clone(),
            command: command.clone(),
            message: format!("IPC call to '{pid}'.'{command}' timed out after {timeout_ms}ms"),
            retryable: true,
        },
    }
}

/// Subscribe to kernel custom events whose `type_id` starts with
/// `topic_prefix`. Returns a subscription id that the frontend passes back
/// to [`kernel_unsubscribe`].
///
/// A tokio task is spawned to pump the `EventSubscription` and forward
/// every match onto the `kernel:event` Tauri channel with the subscription
/// id baked into the payload.
///
/// **Per-window scope (issue #86):** events are emitted to the calling
/// window only via `webview.emit_to(label, ...)`. Pre-fix this used
/// `app.emit(...)` which broadcasts to every webview in the app —
/// a popout opened on a different forge, or a settings/help window
/// that happened to be alive, would receive every kernel event from
/// every subscription. Now each subscription's events are scoped to
/// the window that asked for them.
///
/// **BL-074 ops topic (popout forwarding):** plugin code running
/// inside a popout window can subscribe to
/// `EventFilter::CustomPrefix("com.nexus.editor.ops.")` through this
/// command and receive `OpEnvelope` events scoped to that popout's
/// webview. No additional shell-side wiring is needed because every
/// popout shares the same `KernelRuntime`/`EventBus` as the main
/// window; the per-window scoping above is exactly what BL-074
/// wants. If you change this command's emit target, update
/// `docs/adr/0026-collaborative-editing-crdt-layer.md` so the
/// follow-up plan stays in sync.
#[tauri::command]
pub async fn kernel_subscribe(
    topic_prefix: String,
    app: AppHandle,
    webview: WebviewWindow,
    runtime: tauri::State<'_, KernelRuntime>,
) -> Result<String, String> {
    let subscription_id = uuid::Uuid::new_v4().to_string();
    let subscription_id_for_task = subscription_id.clone();
    let subscriptions_for_task = Arc::clone(&runtime.subscriptions);
    let window_label = webview.label().to_string();

    let handle = {
        let guard = runtime.inner.lock().await;
        let Some(booted) = guard.as_ref() else {
            return Err("kernel not booted".to_string());
        };

        match booted {
            BootedRuntime::Local { context, .. } => {
                let subscription = context
                    .subscribe(EventFilter::CustomPrefix(topic_prefix.clone()));
                spawn_local_forwarder(
                    subscription_id_for_task,
                    window_label,
                    app,
                    subscriptions_for_task,
                    subscription,
                )
            }
            BootedRuntime::Remote { runtime, .. } => {
                // BL-146 — route through `runtime.subscribe`, which
                // records the `(id, filter, sink)` triple in the
                // reconnect wrapper's registry. When the transport
                // drops, the runtime's watchdog rebuilds + replays
                // the subscription against the new client, so the
                // forwarder's `rx` keeps receiving events without
                // the frontend re-subscribing.
                let (tx, rx) =
                    mpsc::unbounded_channel::<nexus_remote::EventDelivery>();
                let filter_json = serde_json::json!({
                    "kind": "custom_prefix",
                    "prefix": topic_prefix,
                });
                runtime
                    .subscribe(&subscription_id, filter_json, tx)
                    .await
                    .map_err(|e| format!("remote subscribe failed: {e}"))?;

                spawn_remote_forwarder(
                    subscription_id_for_task,
                    window_label,
                    app,
                    subscriptions_for_task,
                    rx,
                )
            }
        }
    };

    {
        let mut subs = runtime
            .subscriptions
            .lock()
            .expect("subscriptions mutex poisoned");
        subs.insert(
            subscription_id.clone(),
            SubscriptionEntry {
                handle,
                window_label: webview.label().to_string(),
            },
        );
    }

    Ok(subscription_id)
}

/// Spawn the local-side forwarder task that drains an
/// [`nexus_kernel::EventSubscription`] and pushes each
/// `NexusEvent::Custom` onto the Tauri event channel scoped to one
/// window.
fn spawn_local_forwarder(
    subscription_id: String,
    window_label: String,
    app: AppHandle,
    subscriptions: Arc<StdMutex<HashMap<String, SubscriptionEntry>>>,
    mut subscription: nexus_kernel::EventSubscription,
) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        loop {
            match subscription.recv().await {
                Ok(published) => {
                    if let NexusEvent::Custom {
                        type_id, payload, ..
                    } = &published.event
                    {
                        emit_kernel_event(
                            &app,
                            &window_label,
                            &subscription_id,
                            type_id.clone(),
                            payload.clone(),
                        );
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    eprintln!(
                        "[kernel_subscribe] sub {subscription_id} lagged ({n} events dropped)"
                    );
                }
                Err(RecvError::Closed) => break,
            }
        }
        if let Ok(mut subs) = subscriptions.lock() {
            subs.remove(&subscription_id);
        }
    })
}

/// Spawn the remote-side forwarder task that drains the
/// `mpsc::UnboundedReceiver<EventDelivery>` returned by
/// `RemoteClient::subscribe` and emits to the Tauri event channel.
fn spawn_remote_forwarder(
    subscription_id: String,
    window_label: String,
    app: AppHandle,
    subscriptions: Arc<StdMutex<HashMap<String, SubscriptionEntry>>>,
    mut rx: mpsc::UnboundedReceiver<nexus_remote::EventDelivery>,
) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        while let Some(delivery) = rx.recv().await {
            // The remote wraps the `PublishedEvent` JSON whole; extract
            // `type_id` + `payload` from inside its serde shape so the
            // frontend envelope stays identical to the local path.
            let event = delivery.event;
            let custom = event.get("Custom").or_else(|| event.get("custom"));
            let Some(custom) = custom else {
                // Non-Custom variants don't match CustomPrefix filters,
                // so the server shouldn't emit them here. Skip
                // defensively.
                continue;
            };
            let type_id = custom
                .get("type_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .unwrap_or_default();
            let payload = custom
                .get("payload")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            emit_kernel_event(&app, &window_label, &subscription_id, type_id, payload);
        }
        if let Ok(mut subs) = subscriptions.lock() {
            subs.remove(&subscription_id);
        }
    })
}

/// Shared emit + window-scoping helper. Errors are logged but not
/// surfaced — the calling window may already be closed.
fn emit_kernel_event(
    app: &AppHandle,
    window_label: &str,
    subscription_id: &str,
    topic: String,
    payload: serde_json::Value,
) {
    let envelope = KernelEventEnvelope {
        subscription_id: subscription_id.to_string(),
        topic,
        payload,
    };
    if let Err(e) = app.emit_to(
        tauri::EventTarget::webview_window(window_label),
        KERNEL_EVENT_CHANNEL,
        envelope,
    ) {
        eprintln!(
            "[kernel_subscribe] emit failed for sub {subscription_id} → window '{window_label}': {e}"
        );
    }
}

/// Cancel a subscription created by [`kernel_subscribe`]. Idempotent: an
/// unknown id is a silent no-op so races between a natural task exit (bus
/// closed) and an explicit unsubscribe don't surface as errors.
///
/// For remote subscriptions, also calls `ReconnectingRuntime::unsubscribe`
/// to clear the registry entry — otherwise the subscription would be
/// replayed against every future reconnect (BL-146).
#[tauri::command]
pub async fn kernel_unsubscribe(
    subscription_id: String,
    runtime: tauri::State<'_, KernelRuntime>,
) -> Result<(), String> {
    // Tell the remote runtime to drop the registry entry + issue
    // event_unsubscribe against the current client. Best-effort: a
    // transport error during teardown shouldn't fail the unsubscribe.
    {
        let guard = runtime.inner.lock().await;
        if let Some(BootedRuntime::Remote { runtime, .. }) = guard.as_ref() {
            if let Err(e) = runtime.unsubscribe(&subscription_id).await {
                eprintln!(
                    "[kernel_unsubscribe] remote unsubscribe {subscription_id} failed: {e}"
                );
            }
        }
    }
    let entry = {
        let mut subs = runtime
            .subscriptions
            .lock()
            .expect("subscriptions mutex poisoned");
        subs.remove(&subscription_id)
    };
    if let Some(entry) = entry {
        entry.handle.abort();
    }
    Ok(())
}

/// Cheap boolean probe exposed to the frontend via `api.kernel.available()`.
///
/// Sync and lock-free — reads the `AtomicBool` mirror of the runtime slot
/// rather than taking the async mutex. Frontend plugin init paths poll this
/// on every workspace change; dodging the lock keeps that hot.
#[tauri::command]
pub fn kernel_is_booted(runtime: tauri::State<'_, KernelRuntime>) -> bool {
    runtime.is_booted()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_kernel::IpcError;

    #[test]
    fn from_ipc_error_in_context_preserves_timeout_kind() {
        let err = IpcError::Timeout {
            plugin_id: "com.real".to_string(),
            command: "ping".to_string(),
            timeout_ms: 100,
        };
        let env = IpcErrorEnvelope::from_ipc_error_in_context(&err, "fallback", "fallback");
        assert_eq!(env.kind, IpcErrorKind::Timeout);
        assert!(env.retryable);
        assert_eq!(env.plugin_id, "com.real");
        assert_eq!(env.command, "ping");
    }

    #[test]
    fn from_ipc_error_in_context_fills_serialization_fallbacks() {
        let err = IpcError::SerializationFailed {
            reason: "boom".to_string(),
        };
        let env =
            IpcErrorEnvelope::from_ipc_error_in_context(&err, "com.caller", "do_thing");
        assert_eq!(env.kind, IpcErrorKind::Serialization);
        assert_eq!(env.plugin_id, "com.caller");
        assert_eq!(env.command, "do_thing");
    }

    // ── Per-window cancellation token map ────────────────────────────────────

    #[test]
    fn window_cancel_token_returns_same_token_per_window() {
        let rt = KernelRuntime::new();
        let a1 = rt.window_cancel_token("main");
        let a2 = rt.window_cancel_token("main");
        // Cancelling via the first reference must be observable via the
        // second — proves both calls handed back clones of the same
        // underlying token, not fresh roots per call.
        assert!(!a1.is_cancelled());
        a1.cancel();
        assert!(a2.is_cancelled());
    }

    #[test]
    fn window_cancel_token_is_independent_per_window() {
        let rt = KernelRuntime::new();
        let main = rt.window_cancel_token("main");
        let popout = rt.window_cancel_token("popout-1");
        rt.cancel_window("popout-1");
        assert!(popout.is_cancelled(), "popout-1 must be cancelled");
        assert!(
            !main.is_cancelled(),
            "main must NOT be cancelled when only popout-1 was closed",
        );
    }

    #[test]
    fn cancel_window_drops_map_entry_and_subsequent_lookup_creates_fresh_token() {
        let rt = KernelRuntime::new();
        let first = rt.window_cancel_token("popout-1");
        rt.cancel_window("popout-1");
        assert!(first.is_cancelled());
        // Re-creating a window with the same label (rare but possible
        // if Tauri reuses labels) must hand back a FRESH token so the
        // new window's in-flight calls aren't born already-cancelled.
        let second = rt.window_cancel_token("popout-1");
        assert!(!second.is_cancelled(), "fresh token after cancel + reuse");
    }

    #[test]
    fn cancel_window_is_a_no_op_for_unknown_label() {
        let rt = KernelRuntime::new();
        // Must not panic or poison anything when the window never invoked.
        rt.cancel_window("never-invoked");
        let token = rt.window_cancel_token("never-invoked");
        assert!(!token.is_cancelled());
    }

    // ── Subscription-cleanup on window close ─────────────────────────────────
    //
    // cancel_window must abort forwarder tasks belonging to the closing
    // window and leave forwarders from other windows alone. Without this
    // sweep, a popout that subscribed would leak its tokio task forever
    // (it would keep draining the kernel bus and emit_to a dead window,
    // which Tauri silently no-ops).

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancel_window_aborts_subscriptions_for_the_closing_window_only() {
        let rt = KernelRuntime::new();

        // Helper: spawn a forwarder-shaped task that parks on a oneshot
        // until either the test releases it OR cancel_window aborts it.
        // Returns the JoinHandle + the sender so the test can verify which
        // tasks were aborted vs which received the release.
        async fn spawn_pretend_forwarder() -> (JoinHandle<&'static str>, tokio::sync::oneshot::Sender<()>) {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let h = tauri::async_runtime::spawn(async move {
                match rx.await {
                    Ok(()) => "released",
                    Err(_) => "released-on-drop",
                }
            });
            (h, tx)
        }

        // Two subscriptions on the closing popout + one on the main window.
        let (popout_h_1, _popout_tx_1) = spawn_pretend_forwarder().await;
        let (popout_h_2, _popout_tx_2) = spawn_pretend_forwarder().await;
        let (main_h, main_tx) = spawn_pretend_forwarder().await;

        // The spawn_pretend_forwarder above returns &'static str but the
        // real subscriptions map stores JoinHandle<()>. Cast via a no-op
        // adapter task so we can drop them into the map.
        async fn into_unit_handle(
            h: JoinHandle<&'static str>,
        ) -> JoinHandle<()> {
            tauri::async_runtime::spawn(async move {
                let _ = h.await;
            })
        }
        let popout_h_1 = into_unit_handle(popout_h_1).await;
        let popout_h_2 = into_unit_handle(popout_h_2).await;
        let main_h = into_unit_handle(main_h).await;

        {
            let mut subs = rt.subscriptions.lock().unwrap();
            subs.insert(
                "sub-popout-1".into(),
                SubscriptionEntry { handle: popout_h_1, window_label: "popout-1".into() },
            );
            subs.insert(
                "sub-popout-2".into(),
                SubscriptionEntry { handle: popout_h_2, window_label: "popout-1".into() },
            );
            subs.insert(
                "sub-main".into(),
                SubscriptionEntry { handle: main_h, window_label: "main".into() },
            );
        }
        assert_eq!(rt.subscriptions.lock().unwrap().len(), 3);

        rt.cancel_window("popout-1");

        // The two popout entries must be removed; main must remain.
        let surviving: Vec<String> = rt
            .subscriptions
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            surviving,
            vec!["sub-main".to_string()],
            "cancel_window must remove only the closing window's subscriptions; survivors {surviving:?}",
        );

        // Sanity: the main subscription's forwarder is still alive — we
        // can release it via its oneshot and it completes normally
        // rather than aborting prematurely.
        let _ = main_tx.send(());
    }
}
