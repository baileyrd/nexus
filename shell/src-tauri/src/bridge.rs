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

use nexus_bootstrap::Runtime;
use nexus_kernel::{
    EventFilter, IpcErrorEnvelope, IpcErrorKind, Kernel, KernelPluginContext, NexusEvent,
    PluginContext, RecvError,
};
use nexus_plugins::SharedPluginLoader;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri::async_runtime::JoinHandle;
use tokio::sync::Mutex;

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

/// Payload shape emitted to the webview for every bridged kernel event.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct KernelEventEnvelope {
    subscription_id: String,
    topic: String,
    payload: serde_json::Value,
}

/// Booted runtime pieces, destructured out of `nexus_bootstrap::Runtime` so
/// the plugin context can be shared (Arc) across concurrent Tauri command
/// tasks without holding the outer `Mutex` across await points.
struct BootedRuntime {
    kernel: Kernel,
    context: Arc<KernelPluginContext>,
    loader: Arc<SharedPluginLoader>,
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
    /// Active event subscriptions. Uses `std::sync::Mutex` because we only
    /// hold it for quick map operations (insert / remove / drain) — never
    /// across an await.
    subscriptions: Arc<StdMutex<HashMap<String, JoinHandle<()>>>>,
}

impl KernelRuntime {
    /// Create an empty runtime slot. No kernel is booted yet.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
            booted: Arc::new(AtomicBool::new(false)),
            subscriptions: Arc::new(StdMutex::new(HashMap::new())),
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
        *guard = Some(BootedRuntime {
            kernel,
            context: Arc::new(context),
            loader,
        });
        self.booted.store(true, Ordering::Release);
        Ok(())
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
            for (_id, handle) in subs.drain() {
                handle.abort();
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

        if let Err(e) = runtime.kernel.shutdown().await {
            errors.push(format!("kernel shutdown: {e:#}"));
        }

        // `PluginLoader` has no `shutdown` — only `PluginManager::shutdown`
        // does, and nexus-bootstrap does not wrap its loader in a manager.
        // Replicate the manager's drain-in-reverse-registration-order logic
        // here so every plugin's `on_stop` hook fires and a failing plugin
        // doesn't prevent its siblings from unloading.
        {
            let mut loader_guard = runtime.loader.lock();
            let mut ids = loader_guard.registration_order();
            ids.reverse();
            for id in ids {
                if let Err(e) = loader_guard.unload(&id) {
                    errors.push(format!("unload {id}: {e}"));
                }
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
/// Does NOT boot a kernel — the caller is expected to follow up with
/// [`boot_kernel`]. Safe to call against an already-initialised forge.
#[tauri::command]
pub async fn init_forge(path: String) -> Result<(), String> {
    let root = PathBuf::from(&path);
    // Mirrors `crates/nexus-app/src/forge.rs::init_layout` ordering: create
    // the top-level skeleton first so `init_forge` below never trips over
    // a missing `.forge/` parent.
    for sub in ["notes", "attachments", ".forge"] {
        let dir = root.join(sub);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create '{}': {e}", dir.display()))?;
    }
    nexus_bootstrap::init_forge(&root).map_err(|e| format!("{e:#}"))?;
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

/// Tear down the kernel runtime if one is booted. Idempotent.
#[tauri::command]
pub async fn shutdown_kernel(
    runtime: tauri::State<'_, KernelRuntime>,
) -> Result<(), String> {
    runtime.shutdown().await
}

/// Invoke a kernel plugin command through `context.ipc_call`.
///
/// The outer runtime mutex is released before the dispatch `.await` so
/// concurrent invokes don't serialize and re-entrant IPC calls don't
/// deadlock against the mutex.
#[tauri::command]
pub async fn kernel_invoke(
    plugin_id: String,
    command_id: String,
    args: serde_json::Value,
    timeout_ms: Option<u64>,
    runtime: tauri::State<'_, KernelRuntime>,
) -> Result<serde_json::Value, IpcErrorEnvelope> {
    let context = {
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
        Arc::clone(&booted.context)
    };

    let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_INVOKE_TIMEOUT_MS));

    context
        .ipc_call(&plugin_id, &command_id, args, timeout)
        .await
        .map_err(|e| IpcErrorEnvelope::from_ipc_error_in_context(&e, &plugin_id, &command_id))
}

/// Subscribe to kernel custom events whose `type_id` starts with
/// `topic_prefix`. Returns a subscription id that the frontend passes back
/// to [`kernel_unsubscribe`].
///
/// A tokio task is spawned to pump the `EventSubscription` and forward
/// every match onto the `kernel:event` Tauri channel with the subscription
/// id baked into the payload.
#[tauri::command]
pub async fn kernel_subscribe(
    topic_prefix: String,
    app: AppHandle,
    runtime: tauri::State<'_, KernelRuntime>,
) -> Result<String, String> {
    let mut subscription = {
        let guard = runtime.inner.lock().await;
        let Some(booted) = guard.as_ref() else {
            return Err("kernel not booted".to_string());
        };
        booted
            .context
            .subscribe(EventFilter::CustomPrefix(topic_prefix.clone()))
    };

    let subscription_id = uuid::Uuid::new_v4().to_string();
    let subscription_id_for_task = subscription_id.clone();
    let subscriptions_for_task = Arc::clone(&runtime.subscriptions);

    let handle = tauri::async_runtime::spawn(async move {
        loop {
            match subscription.recv().await {
                Ok(published) => {
                    if let NexusEvent::Custom {
                        type_id, payload, ..
                    } = &published.event
                    {
                        let envelope = KernelEventEnvelope {
                            subscription_id: subscription_id_for_task.clone(),
                            topic: type_id.clone(),
                            payload: payload.clone(),
                        };
                        if let Err(e) = app.emit(KERNEL_EVENT_CHANNEL, envelope) {
                            eprintln!(
                                "[kernel_subscribe] emit failed for sub \
                                 {subscription_id_for_task}: {e}"
                            );
                        }
                    }
                    // Non-Custom events don't match a CustomPrefix filter, so
                    // we shouldn't normally see them — but skip defensively.
                }
                Err(RecvError::Lagged(n)) => {
                    eprintln!(
                        "[kernel_subscribe] sub {subscription_id_for_task} lagged \
                         ({n} events dropped)"
                    );
                    // Keep looping — the broadcast receiver recovers after a lag.
                }
                Err(RecvError::Closed) => break,
            }
        }
        // Clean the entry on natural exit so stale JoinHandles don't pile up
        // if the bus closes before an explicit unsubscribe.
        if let Ok(mut subs) = subscriptions_for_task.lock() {
            subs.remove(&subscription_id_for_task);
        }
    });

    {
        let mut subs = runtime
            .subscriptions
            .lock()
            .expect("subscriptions mutex poisoned");
        subs.insert(subscription_id.clone(), handle);
    }

    Ok(subscription_id)
}

/// Cancel a subscription created by [`kernel_subscribe`]. Idempotent: an
/// unknown id is a silent no-op so races between a natural task exit (bus
/// closed) and an explicit unsubscribe don't surface as errors.
#[tauri::command]
pub async fn kernel_unsubscribe(
    subscription_id: String,
    runtime: tauri::State<'_, KernelRuntime>,
) -> Result<(), String> {
    let handle = {
        let mut subs = runtime
            .subscriptions
            .lock()
            .expect("subscriptions mutex poisoned");
        subs.remove(&subscription_id)
    };
    if let Some(h) = handle {
        h.abort();
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
}
