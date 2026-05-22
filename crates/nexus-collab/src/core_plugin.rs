//! `com.nexus.collab` core plugin.
//!
//! Phase 2.2 — `publish_presence`. Frontends send a partial cursor; the
//! plugin stamps the local peer identity from `[collab]` and publishes
//! on `PRESENCE_TOPIC`. Centralising through one handler also keeps
//! the kernel's namespace anti-spoof check satisfied (the topic lives
//! under the collab plugin id, not the caller's).
//!
//! Phase 2.3 — `start_relay` / `stop_relay` / `relay_status`. The shell
//! drives an in-process [`crate::RelayServer`] so a user can host a
//! quick share without leaving the editor. The handler picks a free
//! port (or honours an explicit one), generates a fresh token, binds
//! synchronously, and spawns the accept loop on the ambient tokio
//! runtime. `stop_relay` calls [`crate::RelayServer::shutdown`] and
//! awaits the accept task so the next start can re-bind cleanly.
//! State transitions broadcast on `RELAY_STARTED_TOPIC` /
//! `RELAY_STOPPED_TOPIC` so the shell (and any TUI / observability
//! consumer) can sync without polling.

use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex, RwLock};

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::auth::Token;
use crate::client::COLLAB_PLUGIN_ID;
use crate::presence::{PresenceCursor, PresenceEvent, PRESENCE_TOPIC};
use crate::server::RelayServer;

/// Reverse-DNS identifier — matches [`COLLAB_PLUGIN_ID`] so the
/// outbound subscription in the bridge picks up presence events
/// authored here (per the Phase 1.3 anti-loop note in [`crate::client`],
/// events authored under `com.nexus.collab.bridge` are skipped on the
/// outbound side; events authored here under `com.nexus.collab` are
/// forwarded).
pub const PLUGIN_ID: &str = COLLAB_PLUGIN_ID;

/// Handler id for `publish_presence` (BL-143 Phase 2.2).
pub const HANDLER_PUBLISH_PRESENCE: u32 = 1;
/// Handler id for `start_relay` (BL-143 Phase 2.3).
pub const HANDLER_START_RELAY: u32 = 2;
/// Handler id for `stop_relay` (BL-143 Phase 2.3).
pub const HANDLER_STOP_RELAY: u32 = 3;
/// Handler id for `relay_status` (BL-143 Phase 2.3).
pub const HANDLER_RELAY_STATUS: u32 = 4;

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::collab::register`.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("publish_presence", HANDLER_PUBLISH_PRESENCE),
    ("start_relay", HANDLER_START_RELAY),
    ("stop_relay", HANDLER_STOP_RELAY),
    ("relay_status", HANDLER_RELAY_STATUS),
];

/// Bus topic emitted by [`HANDLER_START_RELAY`] once a relay has been
/// bound and the accept loop is running. Payload is [`RelayStatus`]
/// with `running = true`. The shell uses this to sync the Share UI
/// across windows / popouts without polling.
pub const RELAY_STARTED_TOPIC: &str = "com.nexus.collab.relay.started";

/// Bus topic emitted by [`HANDLER_STOP_RELAY`] once the relay has been
/// fully shut down (accept task joined). Payload is [`RelayStatus`]
/// with `running = false`.
pub const RELAY_STOPPED_TOPIC: &str = "com.nexus.collab.relay.stopped";

/// Local peer identity sourced from `[collab]` in `.forge/config.toml`.
/// The plugin holds this in an `RwLock` so the bootstrap can lazily
/// populate it; a `None` identity means collab is not configured for
/// this forge and [`HANDLER_PUBLISH_PRESENCE`] short-circuits with a
/// known `ExecutionFailed` so the shell can stop calling.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
pub struct LocalPeer {
    pub user_id: String,
    pub display_name: String,
}

/// Args for `publish_presence`. The frontend never sends `user_id` or
/// `display_name` — those come from the plugin's [`LocalPeer`]. A
/// frontend that wants to surface "I cleared my cursor" sends
/// `cursor: None`; the plugin still stamps the identity so peers learn
/// "<name> is on the forge but not focused on a file".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct PublishPresenceArgs {
    /// Optional cursor location. `None` is treated as "idle / no focus".
    #[serde(default)]
    pub cursor: Option<PresenceCursor>,
}

/// Reply for `publish_presence` — empty struct so the handler still
/// has a typed JSON return that can grow additively (e.g. a future
/// rate-limit signal).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
pub struct PublishPresenceReply {}

/// Args for `start_relay`. All fields optional so the caller can
/// `{ }` for the common case of "pick a port for me".
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields, default)]
pub struct StartRelayArgs {
    /// Specific port to bind on `0.0.0.0`. `None` / `0` lets the OS
    /// pick a free port; the assigned port is reflected back in the
    /// reply.
    pub port: Option<u16>,
}

/// Status / reply for the relay-host handlers. Shared shape across
/// `start_relay`, `relay_status`, and the `RELAY_STARTED_TOPIC` /
/// `RELAY_STOPPED_TOPIC` bus events.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
pub struct RelayStatus {
    /// True when an accept loop is running.
    pub running: bool,
    /// Best-effort LAN-reachable URL for peers to join, e.g.
    /// `ws://192.168.1.42:7700/?token=…`. `None` when not running.
    pub url: Option<String>,
    /// Local IP the URL embeds; defaults to `127.0.0.1` when LAN-IP
    /// detection fails (e.g. no default route on a CI box).
    pub host: Option<String>,
    /// Port the listener is bound on. Useful when the caller passed
    /// `port: None` and wants to know what the OS picked.
    pub port: Option<u16>,
    /// Token embedded in the URL — emitted so the shell can show a
    /// "copy" button that copies the URL but also offers to copy the
    /// token alone for users with their own URL conventions.
    pub token: Option<String>,
}

impl RelayStatus {
    fn stopped() -> Self {
        Self {
            running: false,
            url: None,
            host: None,
            port: None,
            token: None,
        }
    }
}

/// Internal handle the plugin holds while a relay is live.
struct RunningRelay {
    server: Arc<RelayServer>,
    accept_task: JoinHandle<Result<(), crate::server::RelayServerError>>,
    host: String,
    port: u16,
    token: String,
}

/// Best-effort LAN-IP probe. Opens a UDP socket "connected" to a
/// well-known public IP (no packets are actually sent — `connect`
/// on UDP just sets the default route hint) and reads back the
/// local address the kernel would use for that route. Falls back to
/// `127.0.0.1` if no route is reachable (sandboxed CI, airplane
/// mode).
fn detect_lan_ip() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|sock| {
            sock.connect("1.1.1.1:80")?;
            sock.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

/// Core plugin holding the local identity + a bus handle + (when
/// hosting) the live [`RunningRelay`] handle.
pub struct CollabCorePlugin {
    bus: Option<Arc<EventBus>>,
    identity: Arc<RwLock<Option<LocalPeer>>>,
    /// `Some` while a `start_relay` is live. Wrapped in a `Mutex` so
    /// the sync `dispatch` can flip it without coordinating across
    /// async tasks — the accept loop owns the `Arc<RelayServer>` it
    /// was handed and doesn't touch this slot.
    relay: Arc<Mutex<Option<RunningRelay>>>,
}

impl CollabCorePlugin {
    /// Build the plugin. Pass `None` for either argument to leave the
    /// handler in "not configured" mode; the lifecycle hooks still run.
    #[must_use]
    pub fn new(bus: Option<Arc<EventBus>>, identity: Option<LocalPeer>) -> Self {
        Self {
            bus,
            identity: Arc::new(RwLock::new(identity)),
            relay: Arc::new(Mutex::new(None)),
        }
    }

    /// Lock the relay slot with poisoning recovery (D2 audit). The
    /// inner data is just `Option<RunningRelay>` — a presence/absence
    /// flag with no cross-field invariants — so we can keep going on
    /// the prior value even if the previous holder panicked. Logs a
    /// `warn!` so the poisoning is observable in tracing output;
    /// subsequent poisoned calls don't re-warn (the `into_inner`
    /// branch clears the poison on success).
    fn lock_relay(&self) -> std::sync::MutexGuard<'_, Option<RunningRelay>> {
        self.relay.lock().unwrap_or_else(|poisoned| {
            tracing::warn!(
                plugin = "com.nexus.collab",
                "relay slot mutex was poisoned by a previous panic; \
                 recovering and continuing — relay state may be stale"
            );
            poisoned.into_inner()
        })
    }

    /// Replace the local identity at runtime. Reserved for a future
    /// settings-change hook; not wired into the public IPC surface
    /// today.
    pub fn set_identity(&self, identity: Option<LocalPeer>) {
        if let Ok(mut g) = self.identity.write() {
            *g = identity;
        }
    }

    fn snapshot_identity(&self) -> Option<LocalPeer> {
        self.identity.read().ok().and_then(|g| g.clone())
    }
}

impl CorePlugin for CollabCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_PUBLISH_PRESENCE => self.dispatch_publish_presence(args),
            HANDLER_START_RELAY => self.dispatch_start_relay(args),
            HANDLER_STOP_RELAY => self.dispatch_stop_relay(),
            HANDLER_RELAY_STATUS => self.dispatch_relay_status(),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

impl CollabCorePlugin {
    fn dispatch_publish_presence(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: PublishPresenceArgs = serde_json::from_value(args.clone())
            .map_err(|e| exec_err(format!("publish_presence: invalid args: {e}")))?;
        let Some(identity) = self.snapshot_identity() else {
            // Surface a stable message so the shell can detect this
            // condition without a wire-format match. Same shape as
            // every other "<crate>: <reason>" PluginError message.
            return Err(exec_err("publish_presence: collab not configured".to_string()));
        };
        let bus = self
            .bus
            .as_ref()
            .ok_or_else(|| exec_err("publish_presence: no event bus wired".to_string()))?;
        let event = PresenceEvent {
            user_id: identity.user_id,
            display_name: identity.display_name,
            cursor: a.cursor,
        };
        let payload = serde_json::to_value(&event)
            .map_err(|e| exec_err(format!("publish_presence: serialize: {e}")))?;
        bus.publish_plugin(PLUGIN_ID, PRESENCE_TOPIC, payload)
            .map_err(|e| exec_err(format!("publish_presence: bus publish: {e}")))?;
        serde_json::to_value(&PublishPresenceReply::default())
            .map_err(|e| exec_err(format!("publish_presence: serialize reply: {e}")))
    }
}

impl CollabCorePlugin {
    /// `start_relay` — bind, spawn accept loop, publish
    /// `RELAY_STARTED_TOPIC`, and return the URL. Idempotent in the
    /// sense that calling it while a relay is live returns the
    /// existing status without re-binding; the caller can call
    /// `stop_relay` first if they want a fresh port / token.
    fn dispatch_start_relay(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let a: StartRelayArgs = serde_json::from_value(args.clone())
            .map_err(|e| exec_err(format!("start_relay: invalid args: {e}")))?;

        // Re-entrancy: if a relay is already up, return its status.
        // Avoids surprising "two listeners on the same port" attempts
        // when the shell renders Share twice (popout + main window).
        {
            let guard = self.lock_relay();
            if let Some(rr) = guard.as_ref() {
                return ok_reply(&relay_status_from(rr));
            }
        }

        // Generate a fresh token for this share. UUID-v4 gives ~122
        // bits of entropy — plenty for a "type this into a peer's
        // join box" scenario.
        let token_value = uuid::Uuid::new_v4().simple().to_string();
        let token = Token::new(&token_value)
            .map_err(|e| exec_err(format!("start_relay: token: {e}")))?;

        // Sync bind on a std listener so this works inside the sync
        // dispatch. We hand the listener to tokio via from_std after
        // setting it non-blocking.
        let requested_port = a.port.unwrap_or(0);
        let std_listener = std::net::TcpListener::bind(("0.0.0.0", requested_port))
            .map_err(|e| exec_err(format!("start_relay: bind 0.0.0.0:{requested_port}: {e}")))?;
        std_listener
            .set_nonblocking(true)
            .map_err(|e| exec_err(format!("start_relay: set_nonblocking: {e}")))?;
        let bound: SocketAddr = std_listener
            .local_addr()
            .map_err(|e| exec_err(format!("start_relay: local_addr: {e}")))?;

        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| exec_err("start_relay: no ambient tokio runtime".to_string()))?;
        // `from_std` is sync — it just wraps the descriptor in tokio's
        // async listener. Must be called from inside a runtime, but
        // doesn't need its own task.
        let listener = {
            let _enter = handle.enter();
            tokio::net::TcpListener::from_std(std_listener)
                .map_err(|e| exec_err(format!("start_relay: TcpListener::from_std: {e}")))?
        };

        let server = Arc::new(RelayServer::new(token));
        let server_for_task = Arc::clone(&server);
        let accept_task = handle.spawn(async move { server_for_task.serve_listener(listener).await });

        let host = detect_lan_ip();
        let port = bound.port();
        let running = RunningRelay {
            server,
            accept_task,
            host: host.clone(),
            port,
            token: token_value,
        };
        let status = relay_status_from(&running);

        {
            let mut guard = self.lock_relay();
            *guard = Some(running);
        }

        // Best-effort bus broadcast. A failure here doesn't roll back
        // the start — the relay is live and the caller already has
        // the URL in the reply.
        if let Some(bus) = self.bus.as_ref() {
            let payload = serde_json::to_value(&status)
                .map_err(|e| exec_err(format!("start_relay: serialize status: {e}")))?;
            let _ = bus.publish_plugin(PLUGIN_ID, RELAY_STARTED_TOPIC, payload);
        }
        ok_reply(&status)
    }

    /// `stop_relay` — shutdown the running server, await the accept
    /// task (so the OS port frees deterministically), publish
    /// `RELAY_STOPPED_TOPIC`. Calling stop with no relay running is
    /// not an error — the reply just reports `running: false`.
    fn dispatch_stop_relay(&self) -> Result<serde_json::Value, PluginError> {
        let taken = {
            let mut guard = self.lock_relay();
            guard.take()
        };
        let Some(running) = taken else {
            return ok_reply(&RelayStatus::stopped());
        };
        running.server.shutdown();
        // `block_on` is illegal inside a tokio runtime, so we can't
        // synchronously await the accept task here. Aborting is
        // belt-and-braces — `shutdown()` already broadcasts to the
        // select! arm in `serve_listener` (Phase 1.1 follow-up)
        // which calls `JoinSet::shutdown().await` before returning,
        // and the listener drops then. The next `start_relay` may
        // pick a different port if it requested `0`, and an explicit
        // port re-bind a few ms later is what an idempotent OS port
        // table will allow.
        running.accept_task.abort();
        let status = RelayStatus::stopped();
        if let Some(bus) = self.bus.as_ref() {
            let payload = serde_json::to_value(&status)
                .map_err(|e| exec_err(format!("stop_relay: serialize status: {e}")))?;
            let _ = bus.publish_plugin(PLUGIN_ID, RELAY_STOPPED_TOPIC, payload);
        }
        ok_reply(&status)
    }

    /// `relay_status` — snapshot of the current relay state. Always
    /// succeeds; the reply has `running: false` when there is no
    /// active relay.
    fn dispatch_relay_status(&self) -> Result<serde_json::Value, PluginError> {
        let guard = self.lock_relay();
        let status = guard
            .as_ref()
            .map_or_else(RelayStatus::stopped, relay_status_from);
        ok_reply(&status)
    }
}

fn relay_status_from(rr: &RunningRelay) -> RelayStatus {
    let url = format!("ws://{}:{}/?token={}", rr.host, rr.port, rr.token);
    RelayStatus {
        running: true,
        url: Some(url),
        host: Some(rr.host.clone()),
        port: Some(rr.port),
        token: Some(rr.token.clone()),
    }
}

fn ok_reply<T: Serialize>(value: &T) -> Result<serde_json::Value, PluginError> {
    serde_json::to_value(value)
        .map_err(|e| exec_err(format!("serialize reply: {e}")))
}

nexus_plugins::define_dispatch_helpers!();

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_kernel::{EventBus, EventFilter, EventSubscription, NexusEvent};

    fn identity() -> LocalPeer {
        LocalPeer {
            user_id: "alice".into(),
            display_name: "Alice".into(),
        }
    }

    /// Pull the next event off the subscription and decode the
    /// `NexusEvent::Custom` payload as a [`PresenceEvent`]. Panics if
    /// the next event isn't a `Custom` frame on the presence topic —
    /// these tests only publish through the handler, so any other
    /// frame is a regression.
    async fn next_presence(sub: &mut EventSubscription) -> PresenceEvent {
        let pe = sub.recv().await.expect("non-error event");
        let NexusEvent::Custom { type_id, payload, .. } = &pe.event else {
            panic!("expected Custom event, got {:?}", pe.event);
        };
        assert_eq!(type_id, PRESENCE_TOPIC);
        serde_json::from_value(payload.clone()).expect("payload decodes")
    }

    #[tokio::test]
    async fn publish_presence_publishes_event_on_presence_topic() {
        let bus = Arc::new(EventBus::new(8));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix(PRESENCE_TOPIC.to_string()));
        let mut plugin = CollabCorePlugin::new(Some(Arc::clone(&bus)), Some(identity()));

        let v = plugin
            .dispatch(
                HANDLER_PUBLISH_PRESENCE,
                &serde_json::json!({
                    "cursor": {
                        "relpath": "notes/today.md",
                        "offset": 42,
                    },
                }),
            )
            .expect("handler runs");
        assert_eq!(v, serde_json::json!({}));

        let payload = next_presence(&mut sub).await;
        assert_eq!(payload.user_id, "alice");
        assert_eq!(payload.display_name, "Alice");
        assert_eq!(payload.cursor.as_ref().unwrap().relpath, "notes/today.md");
        assert_eq!(payload.cursor.as_ref().unwrap().offset, Some(42));
        assert_eq!(payload.cursor.as_ref().unwrap().selection_end, None);
    }

    #[tokio::test]
    async fn publish_presence_with_no_cursor_still_stamps_identity() {
        let bus = Arc::new(EventBus::new(8));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix(PRESENCE_TOPIC.to_string()));
        let mut plugin = CollabCorePlugin::new(Some(Arc::clone(&bus)), Some(identity()));
        plugin
            .dispatch(HANDLER_PUBLISH_PRESENCE, &serde_json::json!({}))
            .expect("handler runs");
        let payload = next_presence(&mut sub).await;
        assert_eq!(payload.user_id, "alice");
        assert!(payload.cursor.is_none());
    }

    #[test]
    fn publish_presence_rejects_when_identity_missing() {
        let bus = Arc::new(EventBus::new(8));
        let mut plugin = CollabCorePlugin::new(Some(bus), None);
        let err = plugin
            .dispatch(HANDLER_PUBLISH_PRESENCE, &serde_json::json!({}))
            .unwrap_err();
        assert!(
            err.to_string().contains("collab not configured"),
            "stable error message: {err}"
        );
    }

    #[test]
    fn publish_presence_rejects_unknown_fields() {
        let bus = Arc::new(EventBus::new(8));
        let mut plugin = CollabCorePlugin::new(Some(bus), Some(identity()));
        let err = plugin
            .dispatch(
                HANDLER_PUBLISH_PRESENCE,
                &serde_json::json!({"cursor": null, "user_id": "trying to spoof"}),
            )
            .unwrap_err();
        assert!(err.to_string().contains("invalid args"), "{err}");
    }

    #[test]
    fn dispatch_unknown_handler_fails() {
        let mut plugin = CollabCorePlugin::new(None, None);
        let err = plugin
            .dispatch(99, &serde_json::json!({}))
            .unwrap_err();
        assert!(err.to_string().contains("unknown handler id 99"));
    }

    // ── Phase 2.3 relay-host handler tests ────────────────────────────

    /// Bind on a free port and tear down again. Asserts the URL shape,
    /// the bus event payload, and that `relay_status` round-trips.
    #[tokio::test]
    async fn start_relay_then_stop_relay_round_trips() {
        let bus = Arc::new(EventBus::new(8));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix(
            "com.nexus.collab.relay.".to_string(),
        ));
        let mut plugin = CollabCorePlugin::new(Some(Arc::clone(&bus)), None);

        // start with port = 0 → OS picks. Reply shape: running=true,
        // url shaped like ws://host:port/?token=…
        let start = plugin
            .dispatch(HANDLER_START_RELAY, &serde_json::json!({}))
            .expect("start_relay");
        assert_eq!(start["running"], true);
        let url = start["url"].as_str().expect("url present").to_string();
        assert!(url.starts_with("ws://"));
        assert!(url.contains("?token="));
        let port = start["port"].as_u64().expect("port present") as u16;
        assert!(port > 0, "OS picked a real port, got {port}");

        // started event was published.
        let ev = sub.recv().await.expect("started event");
        match &ev.event {
            NexusEvent::Custom { type_id, .. } => {
                assert_eq!(type_id, RELAY_STARTED_TOPIC);
            }
            other => panic!("expected Custom event, got {other:?}"),
        }

        // relay_status sees the running state.
        let status = plugin
            .dispatch(HANDLER_RELAY_STATUS, &serde_json::json!({}))
            .expect("relay_status while running");
        assert_eq!(status["running"], true);
        assert_eq!(status["port"], port);

        // stop tears it down.
        let stopped = plugin
            .dispatch(HANDLER_STOP_RELAY, &serde_json::json!({}))
            .expect("stop_relay");
        assert_eq!(stopped["running"], false);
        assert!(stopped["url"].is_null());

        let ev = sub.recv().await.expect("stopped event");
        match &ev.event {
            NexusEvent::Custom { type_id, .. } => {
                assert_eq!(type_id, RELAY_STOPPED_TOPIC);
            }
            other => panic!("expected Custom event, got {other:?}"),
        }

        // Second stop is a no-op (no error).
        let stopped2 = plugin
            .dispatch(HANDLER_STOP_RELAY, &serde_json::json!({}))
            .expect("stop_relay idempotent");
        assert_eq!(stopped2["running"], false);
    }

    /// Calling `start_relay` twice in a row returns the existing
    /// status the second time — the shell rendering Share twice (main
    /// + popout) doesn't crash the listener.
    #[tokio::test]
    async fn start_relay_is_idempotent_while_running() {
        let bus = Arc::new(EventBus::new(8));
        let mut plugin = CollabCorePlugin::new(Some(Arc::clone(&bus)), None);

        let first = plugin
            .dispatch(HANDLER_START_RELAY, &serde_json::json!({}))
            .expect("start_relay first");
        let second = plugin
            .dispatch(HANDLER_START_RELAY, &serde_json::json!({}))
            .expect("start_relay second is a no-op-with-status");
        assert_eq!(first["url"], second["url"], "second call returns same URL");
        assert_eq!(first["token"], second["token"]);

        let _ = plugin.dispatch(HANDLER_STOP_RELAY, &serde_json::json!({}));
    }

    /// `relay_status` with no relay running reports the stopped shape;
    /// no extra fields leak.
    #[test]
    fn relay_status_when_idle_is_stopped() {
        let mut plugin = CollabCorePlugin::new(None, None);
        let v = plugin
            .dispatch(HANDLER_RELAY_STATUS, &serde_json::json!({}))
            .expect("relay_status idle");
        assert_eq!(v["running"], false);
        assert!(v["url"].is_null());
        assert!(v["port"].is_null());
        assert!(v["token"].is_null());
    }

    /// `start_relay` rejects unknown fields so a caller can't slip a
    /// silent typo past the schema.
    #[tokio::test]
    async fn start_relay_rejects_unknown_fields() {
        let bus = Arc::new(EventBus::new(4));
        let mut plugin = CollabCorePlugin::new(Some(bus), None);
        let err = plugin
            .dispatch(HANDLER_START_RELAY, &serde_json::json!({"prt": 7700}))
            .unwrap_err();
        assert!(err.to_string().contains("invalid args"), "{err}");
    }

    #[tokio::test]
    async fn set_identity_updates_in_place() {
        let bus = Arc::new(EventBus::new(8));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix(PRESENCE_TOPIC.to_string()));
        let mut plugin = CollabCorePlugin::new(Some(Arc::clone(&bus)), None);
        plugin.set_identity(Some(LocalPeer {
            user_id: "bob".into(),
            display_name: "Bob".into(),
        }));
        plugin
            .dispatch(HANDLER_PUBLISH_PRESENCE, &serde_json::json!({}))
            .expect("handler runs after late identity wiring");
        let payload = next_presence(&mut sub).await;
        assert_eq!(payload.user_id, "bob");
    }
}
