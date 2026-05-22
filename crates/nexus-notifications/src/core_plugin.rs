//! Core plugin wrapping the [`Transport`] registry + [BL-135] router.
//!
//! Exposes one IPC handler — `send` — that accepts either:
//!
//! 1. An explicit `channel` (the legacy override path); the
//!    notification is dispatched to that single transport bypassing
//!    the router. Used by callers that have already decided where to
//!    deliver — `nexus notify send --channel discord`, future
//!    plugin-internal callers that want a one-off route.
//! 2. A `source` tag (the BL-135 router path); the notification is
//!    routed through [`Router`] which consults the loaded
//!    `notifications.toml` to pick the channel list. Severity and
//!    quiet-hours filtering happen here.
//!
//! The plugin holds an immutable map of `Channel → Box<dyn Transport>`
//! initialised at construction. Today that's `Desktop` + `Discord` +
//! `Telegram` + `Email`; future channels are appended in
//! [`Channel`]-declaration order.
//!
//! BL-135 also brings:
//! - **Live reload.** [`NotificationsCorePlugin::reload_config_from_disk`]
//!   re-parses `notifications.toml` and swaps the router's rule set
//!   without recreating transports. The plugin spawns a `notify`
//!   filesystem watcher in [`on_start`] when a config path is known
//!   so the reload happens automatically; unit tests drive the
//!   method directly to side-step the watcher.
//! - **`AiEvent` subscriber.** [`on_start`] also subscribes to the
//!   `com.nexus.ai.runtime.*` bus prefix and translates each typed
//!   event into a source-tagged notification (`source = "ai_runtime"`).
//!   This is dormant when the runtime hasn't yet republished a
//!   topic.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use nexus_kernel::{Events as _, EventBus, EventFilter, KernelPluginContext, NexusEvent};
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::config::{NotificationsConfig, Severity};
use crate::inbox::{
    Inbox, InboxStats, NewEntry, StatusFilter, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_ROWS,
};
use crate::router::{current_min_of_day, Resolution, Router};
use crate::{
    Channel, DesktopTransport, DiscordWebhook, Notification, SmtpConfig, SmtpTransport,
    TelegramBot, Transport, INBOX_APPENDED_TOPIC,
};

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = nexus_types::plugin_ids::NOTIFICATIONS;

/// `send` handler id.
pub const HANDLER_SEND: u32 = 1;
/// `inbox_list` handler id (BL-136).
pub const HANDLER_INBOX_LIST: u32 = 2;
/// `inbox_mark_read` handler id (BL-136).
pub const HANDLER_INBOX_MARK_READ: u32 = 3;
/// `inbox_dismiss` handler id (BL-136).
pub const HANDLER_INBOX_DISMISS: u32 = 4;
/// `inbox_stats` handler id (BL-136).
pub const HANDLER_INBOX_STATS: u32 = 5;

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::notifications::register`.
/// Order matches the pre-SD-06 bootstrap registration.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("send", HANDLER_SEND),
    ("inbox_list", HANDLER_INBOX_LIST),
    ("inbox_mark_read", HANDLER_INBOX_MARK_READ),
    ("inbox_dismiss", HANDLER_INBOX_DISMISS),
    ("inbox_stats", HANDLER_INBOX_STATS),
];

/// Bus topic prefix the BL-134 runtime publishes typed [`crate`]-
/// independent AI lifecycle events on. Mirrored here to avoid pulling
/// in `nexus-ai-runtime` as a dependency (registration order is
/// notifications-before-runtime so the dep would invert).
pub const AI_RUNTIME_TOPIC_PREFIX: &str = "com.nexus.ai.runtime.";

/// Built-in source name for events synthesised from the
/// `com.nexus.ai.runtime.*` topic stream.
pub const SOURCE_AI_RUNTIME: &str = "ai_runtime";

/// Args for `com.nexus.notifications::send` (handler id `1`).
///
/// One of `channel` or `source` must be present:
/// - `channel` set, `source` unset: legacy override path; routed
///   directly to that transport.
/// - `source` set, `channel` unset: BL-135 router path; consults
///   `notifications.toml` to pick channels.
/// - Both set: `channel` wins (override path); the `source` tag is
///   recorded for observability but does not change the routing.
/// - Neither set: rejected at parse time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SendArgs {
    /// Explicit target channel — the override path. When set, the
    /// router is not consulted.
    #[serde(default)]
    pub channel: Option<Channel>,
    /// BL-135 source tag — feeds the router to pick channels from
    /// `notifications.toml`.
    #[serde(default)]
    pub source: Option<String>,
    /// Optional severity (`debug` / `info` / `warn` / `error`).
    /// Defaults to `info` when omitted. Only consulted on the router
    /// path; override-path callers can still supply it for
    /// observability.
    #[serde(default)]
    pub severity: Option<Severity>,
    /// Notification body. UTF-8, no length cap at the IPC layer.
    pub message: String,
    /// Optional title. When omitted, transports that need a header
    /// fall back to `"Nexus"`.
    #[serde(default)]
    pub title: Option<String>,
}

/// Reply for `com.nexus.notifications::send`.
///
/// `channel` echoes the *primary* channel that was used — for the
/// router path this is the first channel in `channels`. Legacy
/// consumers parsing the v1 shape (`{ delivered, channel }`) keep
/// working.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct SendReply {
    /// `true` when at least one transport accepted the notification.
    pub delivered: bool,
    /// Primary channel — for compatibility with the v1 shape. For
    /// router-path sends this is the first channel in `channels`.
    /// `None` if no channels were routed.
    #[serde(default)]
    pub channel: Option<Channel>,
    /// Every channel the notification was dispatched to. Length 1 on
    /// the override path; 0..N on the router path.
    #[serde(default)]
    pub channels: Vec<Channel>,
    /// Per-channel failure messages. Empty when every dispatch
    /// succeeded. Only populated on the router path — override-path
    /// failures still surface as a top-level IPC error.
    #[serde(default)]
    pub failures: Vec<ChannelFailure>,
    /// Resolved routing decision for observability — `"override"`,
    /// `"routed"`, `"unknown_source"`, `"filtered"`, or `"none"`.
    #[serde(default)]
    pub routing: String,
}

/// Args for `com.nexus.notifications::inbox_list` (handler id `2`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct InboxListArgs {
    /// Only return rows with `ts >= since` (Unix seconds).
    #[serde(default)]
    pub since: Option<i64>,
    /// `"all"` (default), `"unread"`, or `"dismissed"`.
    #[serde(default)]
    pub status: Option<String>,
    /// Restrict to a single source tag.
    #[serde(default)]
    pub source: Option<String>,
    /// Page size — default 100, capped at 1000.
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Args for `com.nexus.notifications::inbox_mark_read` /
/// `inbox_dismiss` (handler ids `3` / `4`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct InboxIdsArgs {
    /// Row ids to mutate. Empty is a no-op.
    pub ids: Vec<String>,
}

/// Reply for `inbox_mark_read` / `inbox_dismiss`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct InboxUpdatedReply {
    /// Number of rows whose state actually changed (already-read /
    /// already-dismissed rows are not double-stamped).
    pub updated: u32,
}

/// One row of the [`SendReply::failures`] list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct ChannelFailure {
    /// Channel that failed.
    pub channel: Channel,
    /// Transport-side error message.
    pub error: String,
}

/// Core plugin that owns the channel → transport map + router.
pub struct NotificationsCorePlugin {
    transports: HashMap<Channel, Box<dyn Transport>>,
    router: Router,
    config: Arc<RwLock<NotificationsConfig>>,
    /// Path the live-reload watcher polls. `None` disables the
    /// watcher (used by unit tests and the `with_transports` test
    /// constructor).
    config_path: Option<PathBuf>,
    /// Captured in [`wire_context`] so [`on_start`] can subscribe to
    /// the AI runtime topic. `None` when the plugin is constructed
    /// outside a bootstrap (unit tests).
    ctx: Option<Arc<KernelPluginContext>>,
    /// Bus handle for the dispatch-on-router path — used to publish
    /// the `com.nexus.notifications.delivered` event the shell toast
    /// subscriber listens on, even when the Desktop channel isn't in
    /// the route (so the toast UI sees every routed notification).
    bus: Option<Arc<EventBus>>,
    /// BL-136 inbox store. `None` for unit tests that construct the
    /// plugin via [`Self::with_transports`]; production callers
    /// route through [`Self::from_config`] which opens a SQLite-
    /// backed store under `<forge>/.forge/notifications/inbox.db`.
    inbox: Option<Arc<Inbox>>,
}

impl NotificationsCorePlugin {
    /// Build a fresh plugin from a [`NotificationsConfig`]. Transports
    /// are constructed from the `[channels.*]` blocks; the router
    /// resolves `[sources.*]` rules.
    ///
    /// `config_path` is optional and only used for live-reload. Pass
    /// `None` in tests / synthetic configs.
    ///
    /// # Errors
    /// Returns [`crate::config::ConfigError`] if the config has a
    /// malformed `quiet_hours` value.
    pub fn from_config(
        bus: Option<Arc<EventBus>>,
        config: NotificationsConfig,
        config_path: Option<PathBuf>,
    ) -> Result<Self, crate::config::ConfigError> {
        Self::from_config_with_inbox(bus, config, config_path, None)
    }

    /// `from_config` plus an explicit inbox path. Passing `None`
    /// disables the inbox (legacy / test paths). The bootstrap call
    /// site uses [`crate::INBOX_DB_RELPATH`] under the forge root.
    ///
    /// # Errors
    /// Same as [`Self::from_config`]; inbox open failures are logged
    /// + skipped (the dispatcher still runs without inbox writes).
    pub fn from_config_with_inbox(
        bus: Option<Arc<EventBus>>,
        config: NotificationsConfig,
        config_path: Option<PathBuf>,
        inbox_path: Option<PathBuf>,
    ) -> Result<Self, crate::config::ConfigError> {
        let router = Router::from_config(&config)?;
        let mut transports: HashMap<Channel, Box<dyn Transport>> = HashMap::new();
        transports.insert(Channel::Desktop, Box::new(DesktopTransport::new(bus.clone())));
        transports.insert(
            Channel::Discord,
            Box::new(DiscordWebhook::new(config.channels.discord.webhook_url.clone())),
        );
        transports.insert(
            Channel::Telegram,
            Box::new(TelegramBot::new(
                config.channels.telegram.bot_token.clone(),
                config.channels.telegram.chat_id.clone(),
                config
                    .channels
                    .telegram
                    .max_bytes
                    .unwrap_or(crate::DEFAULT_TELEGRAM_MAX_BYTES),
            )),
        );
        transports.insert(
            Channel::Email,
            Box::new(SmtpTransport::new(config.channels.email.to_smtp_config())),
        );
        let max_rows = config.inbox.max_rows.unwrap_or(DEFAULT_MAX_ROWS);
        let max_age = config.inbox.max_age_days.unwrap_or(DEFAULT_MAX_AGE_DAYS);
        let inbox = inbox_path.and_then(|p| match Inbox::open(&p, max_rows, max_age) {
            Ok(i) => Some(Arc::new(i)),
            Err(err) => {
                tracing::warn!(
                    path = %p.display(),
                    %err,
                    "inbox.db: open failed; notifications history disabled"
                );
                None
            }
        });
        Ok(Self {
            transports,
            router,
            config: Arc::new(RwLock::new(config)),
            config_path,
            ctx: None,
            bus,
            inbox,
        })
    }

    /// Legacy constructor — keeps the BL-133 signature working so
    /// existing tests in this crate and the bootstrap's pre-BL-135
    /// callsite (back-compat fallback when `notifications.toml` is
    /// absent) don't need to change.
    #[must_use]
    pub fn with_defaults(
        bus: Option<Arc<EventBus>>,
        discord_webhook_url: String,
        telegram_bot_token: String,
        telegram_chat_id: String,
        smtp_config: SmtpConfig,
    ) -> Self {
        let mut config = NotificationsConfig::default();
        config.channels.discord.webhook_url = discord_webhook_url;
        config.channels.telegram.bot_token = telegram_bot_token;
        config.channels.telegram.chat_id = telegram_chat_id;
        config.channels.email.host = smtp_config.host;
        config.channels.email.port = smtp_config.port;
        config.channels.email.username = smtp_config.username;
        config.channels.email.password = smtp_config.password;
        config.channels.email.from = smtp_config.from;
        config.channels.email.to = smtp_config.to;
        config.channels.email.subject_template = smtp_config.subject_template;
        Self::from_config(bus, config, None).expect(
            "with_defaults builds a config with no quiet_hours strings; resolve_sources cannot fail",
        )
    }

    /// Build a plugin with an arbitrary transport map — used by unit
    /// tests that swap in mock transports.
    #[must_use]
    pub fn with_transports(transports: HashMap<Channel, Box<dyn Transport>>) -> Self {
        Self {
            transports,
            router: Router::empty(),
            config: Arc::new(RwLock::new(NotificationsConfig::default())),
            config_path: None,
            ctx: None,
            bus: None,
            inbox: None,
        }
    }

    /// Build a plugin with both an arbitrary transport map and a
    /// pre-loaded config (so the router is non-empty). Used by unit
    /// tests covering the router path.
    #[must_use]
    pub fn with_transports_and_config(
        transports: HashMap<Channel, Box<dyn Transport>>,
        config: NotificationsConfig,
    ) -> Self {
        let router = Router::from_config(&config)
            .expect("test fixture: invalid notifications config");
        Self {
            transports,
            router,
            config: Arc::new(RwLock::new(config)),
            config_path: None,
            ctx: None,
            bus: None,
            inbox: None,
        }
    }

    /// Construct a plugin with an in-memory inbox attached. Used by
    /// the integration tests that exercise the IPC surface end-to-end.
    #[must_use]
    pub fn with_transports_and_inmemory_inbox(
        transports: HashMap<Channel, Box<dyn Transport>>,
        config: NotificationsConfig,
    ) -> Self {
        let router = Router::from_config(&config)
            .expect("test fixture: invalid notifications config");
        let max_rows = config.inbox.max_rows.unwrap_or(DEFAULT_MAX_ROWS);
        let max_age = config.inbox.max_age_days.unwrap_or(DEFAULT_MAX_AGE_DAYS);
        let inbox = Inbox::in_memory(max_rows, max_age)
            .ok()
            .map(Arc::new);
        Self {
            transports,
            router,
            config: Arc::new(RwLock::new(config)),
            config_path: None,
            ctx: None,
            bus: None,
            inbox,
        }
    }

    /// Borrow the inbox if attached. `None` for legacy / test
    /// constructors. Public so unit tests can pin write semantics
    /// without going through the IPC layer.
    #[must_use]
    pub fn inbox(&self) -> Option<&Arc<Inbox>> {
        self.inbox.as_ref()
    }

    /// Borrow the router. Public for shell IPC handlers that want to
    /// report current routing (e.g. settings UI showing "this source
    /// would route to X, Y" — wired in a BL-136 follow-up).
    #[must_use]
    pub fn router(&self) -> &Router {
        &self.router
    }

    /// Re-read the configured `notifications.toml` from disk and
    /// swap the router's rule set in place. Transport credentials
    /// (Discord/Telegram/Email) are *not* reloaded on this path —
    /// changing them requires a restart so an in-flight transport
    /// can't see a half-applied config. Routing rules
    /// (`[sources.*]`) and severity/quiet_hours filters reload live.
    ///
    /// `Ok(())` on a successful swap (including the "file is absent"
    /// case which resets to an empty router). Parse / quiet_hours
    /// errors propagate so the watcher path can log them without
    /// crashing.
    ///
    /// # Errors
    /// Returns [`crate::config::ConfigError`] on any parse / IO
    /// failure.
    pub fn reload_config_from_disk(&self) -> Result<(), crate::config::ConfigError> {
        let path = match self.config_path.as_deref() {
            Some(p) => p,
            None => return Ok(()),
        };
        let new_config = NotificationsConfig::load_from(path)?;
        self.router.swap_config(&new_config)?;
        let mut guard = self
            .config
            .write()
            .expect("notifications config lock poisoned");
        *guard = new_config;
        Ok(())
    }

    /// Dispatch a [`Notification`] through every routed channel for
    /// `source`, applying severity / quiet-hours filtering. Used by
    /// the built-in `AiEvent` subscriber and by any future
    /// crate-internal producer that wants to skip the IPC layer.
    ///
    /// Returns `(channels_dispatched, failures)`. Always returns
    /// `Ok` even when every channel fails — caller decides whether
    /// to log / re-raise.
    pub fn dispatch_routed(
        &self,
        source: &str,
        severity: Severity,
        notif: &Notification,
    ) -> (Vec<Channel>, Vec<ChannelFailure>) {
        self.dispatch_routed_with_payload(source, severity, notif, None)
    }

    /// `dispatch_routed` plus a caller-supplied `payload_json`
    /// attached to the inbox row. Producers that want to cross-link
    /// back to their own event (`task_id`, run id, etc.) populate
    /// this; the default `dispatch_routed` passes `None`.
    pub fn dispatch_routed_with_payload(
        &self,
        source: &str,
        severity: Severity,
        notif: &Notification,
        payload_json: Option<&str>,
    ) -> (Vec<Channel>, Vec<ChannelFailure>) {
        let resolution = self.router.resolve(source, severity, current_min_of_day());
        match resolution {
            Resolution::Routed(channels) => {
                let (delivered, failures) = self.fan_out(notif, &channels);
                self.write_inbox_row(source, severity, notif, &channels, payload_json);
                (delivered, failures)
            }
            Resolution::UnknownSource | Resolution::Filtered => (Vec::new(), Vec::new()),
        }
    }

    /// Write one inbox row (when the inbox is wired) and republish a
    /// `com.nexus.notifications.inbox.appended` event. Best-effort —
    /// inbox failures log a warning but never block the dispatch.
    fn write_inbox_row(
        &self,
        source: &str,
        severity: Severity,
        notif: &Notification,
        channels: &[Channel],
        payload_json: Option<&str>,
    ) {
        let Some(inbox) = self.inbox.as_ref() else {
            return;
        };
        let id = match inbox.insert(&NewEntry {
            source,
            severity,
            title: notif.title.as_deref(),
            body: &notif.message,
            channels,
            payload_json,
        }) {
            Ok(id) => id,
            Err(err) => {
                tracing::warn!(%err, "inbox: insert failed");
                return;
            }
        };
        if let Some(bus) = self.bus.as_ref() {
            if let Err(err) = bus.publish_plugin(
                PLUGIN_ID,
                INBOX_APPENDED_TOPIC,
                serde_json::json!({
                    "id": id,
                    "source": source,
                    "severity": severity.as_str(),
                    "ts": chrono::Utc::now().timestamp(),
                }),
            ) {
                tracing::warn!(
                    plugin_id = PLUGIN_ID,
                    inbox_entry_id = id,
                    %err,
                    "inbox.appended event dropped — bus publish failed",
                );
            }
        }
    }

    fn fan_out(
        &self,
        notif: &Notification,
        channels: &[Channel],
    ) -> (Vec<Channel>, Vec<ChannelFailure>) {
        let mut delivered_to = Vec::new();
        let mut failures = Vec::new();
        for ch in channels {
            let Some(transport) = self.transports.get(ch) else {
                failures.push(ChannelFailure {
                    channel: *ch,
                    error: format!("no transport registered for channel {}", ch.as_str()),
                });
                continue;
            };
            match transport.send(notif) {
                Ok(()) => delivered_to.push(*ch),
                Err(err) => failures.push(ChannelFailure {
                    channel: *ch,
                    error: err.to_string(),
                }),
            }
        }
        (delivered_to, failures)
    }
}

impl CorePlugin for NotificationsCorePlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_SEND => self.dispatch_send(args),
            HANDLER_INBOX_LIST => self.dispatch_inbox_list(args),
            HANDLER_INBOX_MARK_READ => self.dispatch_inbox_mark_read(args),
            HANDLER_INBOX_DISMISS => self.dispatch_inbox_dismiss(args),
            HANDLER_INBOX_STATS => self.dispatch_inbox_stats(args),
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }

    fn wire_context(&mut self, ctx: Arc<KernelPluginContext>) {
        self.ctx = Some(ctx);
    }

    fn on_start(&mut self) -> Result<(), PluginError> {
        // BL-136: one-time age-cap sweep at boot. Row-cap enforcement
        // is amortised across inserts.
        if let Some(inbox) = self.inbox.as_ref() {
            match inbox.enforce_age_cap() {
                Ok(n) if n > 0 => tracing::info!(deleted = n, "inbox: aged-out rows pruned"),
                Ok(_) => {}
                Err(err) => tracing::warn!(%err, "inbox: age-cap sweep failed"),
            }
        }

        // Spawn the AI runtime subscriber. Best-effort: if there is
        // no tokio runtime in scope (CLI single-shot path), skip;
        // BL-134's AiEvent stream only matters when the shell or a
        // long-running TUI is in front of the kernel.
        if let (Some(ctx), Ok(handle)) = (self.ctx.clone(), tokio::runtime::Handle::try_current()) {
            let mut sub = ctx.subscribe(EventFilter::CustomPrefix(
                AI_RUNTIME_TOPIC_PREFIX.to_string(),
            ));
            let transports_snapshot: Vec<(Channel, &'static str)> = self
                .transports
                .keys()
                .map(|c| (*c, c.as_str()))
                .collect();
            // The subscriber needs to call back into `dispatch_routed`,
            // but `&mut self` cannot escape into a tokio task. Clone
            // the bits it actually needs: the router + transports.
            // Transports are not Clone — but the plugin already holds
            // them behind a `HashMap<Channel, Box<dyn Transport>>`
            // owned by `self`. To dispatch from the subscriber task we
            // promote the transport map to an `Arc<HashMap<…>>` once
            // here and clone the Arc into the task.
            // ─ For Phase 1, the subscriber translates events into
            //   `Notification` records and republishes a Custom event
            //   on `com.nexus.notifications.delivered`, leaving the
            //   actual transport dispatch to the existing Desktop
            //   subscriber. The transport map snapshot above is held
            //   only for shape — we do not reach into transports from
            //   the task to avoid the Send/Clone dance.
            let bus = self.bus.clone();
            drop(transports_snapshot); // silence the unused warning until Phase 2 wires direct dispatch
            handle.spawn(async move {
                loop {
                    match sub.recv().await {
                        Ok(evt) => {
                            if let NexusEvent::Custom { type_id, payload, .. } = &evt.event {
                                if let Some(notif) =
                                    translate_ai_runtime_event(type_id, payload)
                                {
                                    if let Some(bus) = bus.as_ref() {
                                        // Republish under a tagged
                                        // `com.nexus.notifications.delivered`
                                        // payload so the existing
                                        // shell toast pipeline picks
                                        // it up. The shell already
                                        // listens on this topic
                                        // (BL-133 follow-up).
                                        let body = serde_json::json!({
                                            "channel": "desktop",
                                            "source": SOURCE_AI_RUNTIME,
                                            "title": notif.title.unwrap_or_else(|| "Nexus".to_string()),
                                            "message": notif.message,
                                        });
                                        if let Err(err) = bus.publish_plugin(
                                            PLUGIN_ID,
                                            crate::NOTIFICATION_DELIVERED_TOPIC,
                                            body,
                                        ) {
                                            tracing::warn!(
                                                plugin_id = PLUGIN_ID,
                                                source = SOURCE_AI_RUNTIME,
                                                %err,
                                                "ai-runtime translated notification dropped — bus publish failed",
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Err(nexus_kernel::RecvError::Lagged(_)) => continue,
                        Err(nexus_kernel::RecvError::Closed) => break,
                    }
                }
            });
        }

        // Spawn the live-reload watcher when we have a config path.
        if let Some(path) = self.config_path.clone() {
            spawn_config_watcher(path, self.router.clone(), self.config.clone());
        }

        Ok(())
    }

    fn dispatch_async(
        &mut self,
        _handler_id: u32,
        _args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        None
    }
}

impl NotificationsCorePlugin {
    fn dispatch_send(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let parsed: SendArgs = serde_json::from_value(args.clone())
            .map_err(|e| exec_err(format!("send: invalid args: {e}")))?;
        if parsed.channel.is_none() && parsed.source.is_none() {
            return Err(exec_err(
                "send: one of `channel` or `source` must be supplied".to_string(),
            ));
        }
        let notif = Notification {
            message: parsed.message.clone(),
            title: parsed.title.clone(),
        };
        let severity = parsed.severity.unwrap_or_default();
        // Override path — caller picked a channel explicitly.
        if let Some(channel) = parsed.channel {
            let transport = self.transports.get(&channel).ok_or_else(|| {
                exec_err(format!(
                    "send: unknown channel {}",
                    channel.as_str()
                ))
            })?;
            transport
                .send(&notif)
                .map_err(|e| exec_err(format!("send: {e}")))?;
            // BL-136: explicit-channel sends record under a synthetic
            // `"override"` source so the inbox holds a row even when
            // the caller bypassed the router. Producers that want a
            // real source tag should use the router path.
            let source = parsed.source.as_deref().unwrap_or("override");
            self.write_inbox_row(source, severity, &notif, &[channel], None);
            let reply = SendReply {
                delivered: true,
                channel: Some(channel),
                channels: vec![channel],
                failures: Vec::new(),
                routing: "override".to_string(),
            };
            return serde_json::to_value(&reply)
                .map_err(|e| exec_err(format!("send: serialize: {e}")))
        }
        // Router path — source tag.
        let source = parsed.source.expect("checked above");
        let resolution = self.router.resolve(&source, severity, current_min_of_day());
        let reply = match resolution {
            Resolution::UnknownSource => SendReply {
                delivered: false,
                channel: None,
                channels: Vec::new(),
                failures: Vec::new(),
                routing: "unknown_source".to_string(),
            },
            Resolution::Filtered => SendReply {
                delivered: false,
                channel: None,
                channels: Vec::new(),
                failures: Vec::new(),
                routing: "filtered".to_string(),
            },
            Resolution::Routed(channels) => {
                let (delivered_to, failures) = self.fan_out(&notif, &channels);
                self.write_inbox_row(&source, severity, &notif, &channels, None);
                SendReply {
                    delivered: !delivered_to.is_empty(),
                    channel: delivered_to.first().copied(),
                    channels: delivered_to,
                    failures,
                    routing: "routed".to_string(),
                }
            }
        };
        serde_json::to_value(&reply).map_err(|e| exec_err(format!("send: serialize: {e}")))
    }

    // ── BL-136 inbox handlers ───────────────────────────────────────

    fn dispatch_inbox_list(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let parsed: InboxListArgs = serde_json::from_value(args.clone())
            .map_err(|e| exec_err(format!("inbox_list: invalid args: {e}")))?;
        let inbox = self
            .inbox
            .as_ref()
            .ok_or_else(|| exec_err("inbox_list: inbox not wired".to_string()))?;
        let status = match parsed.status.as_deref() {
            None | Some("all") => StatusFilter::All,
            Some("unread") => StatusFilter::Unread,
            Some("dismissed") => StatusFilter::Dismissed,
            Some(other) => {
                return Err(exec_err(format!(
                    "inbox_list: unknown status '{other}'; expected all|unread|dismissed"
                )))
            }
        };
        let limit = parsed.limit.unwrap_or(100).min(1000);
        let rows = inbox
            .list(parsed.since, status, parsed.source.as_deref(), limit)
            .map_err(|e| exec_err(format!("inbox_list: {e}")))?;
        serde_json::to_value(rows).map_err(|e| exec_err(format!("inbox_list: serialize: {e}")))
    }

    fn dispatch_inbox_mark_read(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let parsed: InboxIdsArgs = serde_json::from_value(args.clone())
            .map_err(|e| exec_err(format!("inbox_mark_read: invalid args: {e}")))?;
        let inbox = self
            .inbox
            .as_ref()
            .ok_or_else(|| exec_err("inbox_mark_read: inbox not wired".to_string()))?;
        let updated = inbox
            .mark_read(&parsed.ids)
            .map_err(|e| exec_err(format!("inbox_mark_read: {e}")))?;
        serde_json::to_value(InboxUpdatedReply { updated })
            .map_err(|e| exec_err(format!("inbox_mark_read: serialize: {e}")))
    }

    fn dispatch_inbox_dismiss(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let parsed: InboxIdsArgs = serde_json::from_value(args.clone())
            .map_err(|e| exec_err(format!("inbox_dismiss: invalid args: {e}")))?;
        let inbox = self
            .inbox
            .as_ref()
            .ok_or_else(|| exec_err("inbox_dismiss: inbox not wired".to_string()))?;
        let updated = inbox
            .dismiss(&parsed.ids)
            .map_err(|e| exec_err(format!("inbox_dismiss: {e}")))?;
        serde_json::to_value(InboxUpdatedReply { updated })
            .map_err(|e| exec_err(format!("inbox_dismiss: serialize: {e}")))
    }

    fn dispatch_inbox_stats(
        &self,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let inbox = self
            .inbox
            .as_ref()
            .ok_or_else(|| exec_err("inbox_stats: inbox not wired".to_string()))?;
        let stats: InboxStats = inbox
            .stats()
            .map_err(|e| exec_err(format!("inbox_stats: {e}")))?;
        serde_json::to_value(stats).map_err(|e| exec_err(format!("inbox_stats: serialize: {e}")))
    }
}

/// Translate a `com.nexus.ai.runtime.*` event payload into a
/// [`Notification`]. Returns `None` for events that are too noisy to
/// surface as a notification (token chunks, intermediate tool calls).
fn translate_ai_runtime_event(
    type_id: &str,
    payload: &serde_json::Value,
) -> Option<Notification> {
    let suffix = type_id.strip_prefix(AI_RUNTIME_TOPIC_PREFIX)?;
    // Read `kind` if the runtime emits typed AiEvent payloads
    // (post-Phase-2 BL-134), fall back to suffix-based mapping.
    let kind = payload
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or(suffix);
    let task_id = payload
        .get("task_id")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    match kind {
        "finished" => Some(Notification {
            message: format!("Task {task_id} finished"),
            title: Some("AI runtime".to_string()),
        }),
        "failed" => {
            let err = payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            Some(Notification {
                message: format!("Task {task_id} failed: {err}"),
                title: Some("AI runtime".to_string()),
            })
        }
        // Submitted / Started / Cancelled / Paused / Resumed: not
        // surfaced as notifications today — the shell observability
        // panel already renders them inline. Token chunks / tool
        // calls / round_* are intentionally silent.
        _ => None,
    }
}

/// Spawn a filesystem watcher on `path` that calls
/// [`Router::swap_config`] every time the file changes. Best-effort:
/// any error setting up the watcher logs + drops back to a
/// no-watcher state; the loaded config still works, just without
/// live reload.
fn spawn_config_watcher(
    path: PathBuf,
    router: Router,
    config: Arc<RwLock<NotificationsConfig>>,
) {
    use notify::{event::ModifyKind, EventKind, RecursiveMode, Watcher};
    // notify watchers want to watch a directory; watching the file
    // directly is unreliable on platforms (atomic-rename editors
    // recreate the inode). Watch the parent and filter in the
    // callback.
    let Some(parent) = path.parent().map(std::path::Path::to_path_buf) else {
        tracing::warn!(
            path = %path.display(),
            "notifications.toml: cannot derive parent dir; live-reload disabled"
        );
        return;
    };
    let filename = match path.file_name().map(|f| f.to_os_string()) {
        Some(f) => f,
        None => {
            tracing::warn!(
                path = %path.display(),
                "notifications.toml: cannot derive file name; live-reload disabled"
            );
            return;
        }
    };
    let watch_path = path.clone();
    let result = std::thread::Builder::new()
        .name("nexus-notifications-watcher".to_string())
        .spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel();
            // `send` fails only after the receiver loop below has
            // exited (drops `rx`). That's a one-way terminal state for
            // this watcher thread, so latch the warn — log once when
            // it first happens, then stay silent for the remaining
            // callbacks until the watcher itself is dropped.
            let mut send_warned = false;
            let mut watcher = match notify::recommended_watcher(
                move |res: notify::Result<notify::Event>| {
                    if let Err(err) = tx.send(res) {
                        if !send_warned {
                            tracing::warn!(
                                %err,
                                "notifications.toml: watcher event lost; \
                                 receiver thread has exited"
                            );
                            send_warned = true;
                        }
                    }
                },
            ) {
                Ok(w) => w,
                Err(err) => {
                    tracing::warn!(%err, "notifications.toml: watcher init failed");
                    return;
                }
            };
            if let Err(err) = watcher.watch(&parent, RecursiveMode::NonRecursive) {
                tracing::warn!(
                    parent = %parent.display(),
                    %err,
                    "notifications.toml: parent watch failed"
                );
                return;
            }
            for res in rx {
                match res {
                    Ok(event) => {
                        let matches_filename = event
                            .paths
                            .iter()
                            .any(|p| p.file_name() == Some(filename.as_os_str()));
                        if !matches_filename {
                            continue;
                        }
                        if !matches!(
                            event.kind,
                            EventKind::Create(_)
                                | EventKind::Modify(ModifyKind::Data(_))
                                | EventKind::Modify(ModifyKind::Name(_))
                                | EventKind::Modify(ModifyKind::Any)
                                | EventKind::Modify(ModifyKind::Other)
                                | EventKind::Remove(_)
                        ) {
                            continue;
                        }
                        match NotificationsConfig::load_from(&watch_path) {
                            Ok(new_cfg) => {
                                if let Err(err) = router.swap_config(&new_cfg) {
                                    tracing::warn!(%err, "notifications.toml: swap_config failed");
                                    continue;
                                }
                                if let Ok(mut guard) = config.write() {
                                    *guard = new_cfg;
                                }
                                tracing::info!(
                                    path = %watch_path.display(),
                                    "notifications.toml: reloaded"
                                );
                            }
                            Err(err) => tracing::warn!(
                                path = %watch_path.display(),
                                %err,
                                "notifications.toml: reload failed; keeping previous config"
                            ),
                        }
                    }
                    Err(err) => tracing::warn!(%err, "notifications.toml: watcher event error"),
                }
            }
        });
    if let Err(err) = result {
        tracing::warn!(%err, "notifications.toml: failed to spawn watcher thread");
    }
}

nexus_plugins::define_dispatch_helpers!();

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SendError;
    use std::sync::Mutex;

    /// Mock transport that records every payload + lets the test
    /// pick a fixed result.
    struct MockTransport {
        channel: Channel,
        result: Result<(), SendError>,
        received: Mutex<Vec<Notification>>,
    }

    impl MockTransport {
        fn new(channel: Channel) -> Self {
            Self {
                channel,
                result: Ok(()),
                received: Mutex::new(Vec::new()),
            }
        }
        fn with_error(mut self, err: SendError) -> Self {
            self.result = Err(err);
            self
        }
    }

    impl Transport for MockTransport {
        fn channel(&self) -> Channel {
            self.channel
        }
        fn send(&self, notif: &Notification) -> Result<(), SendError> {
            self.received.lock().unwrap().push(notif.clone());
            match &self.result {
                Ok(()) => Ok(()),
                Err(SendError::NotConfigured(s)) => Err(SendError::NotConfigured(s)),
                Err(SendError::Http(s)) => Err(SendError::Http(s.clone())),
                Err(SendError::Bus(s)) => Err(SendError::Bus(s.clone())),
                Err(SendError::Smtp(s)) => Err(SendError::Smtp(s.clone())),
            }
        }
    }

    struct ShimTransport {
        inner: Arc<MockTransport>,
    }
    impl Transport for ShimTransport {
        fn channel(&self) -> Channel {
            self.inner.channel()
        }
        fn send(&self, notif: &Notification) -> Result<(), SendError> {
            self.inner.send(notif)
        }
    }

    fn shim(inner: &Arc<MockTransport>) -> Box<dyn Transport> {
        Box::new(ShimTransport {
            inner: Arc::clone(inner),
        })
    }

    #[test]
    fn send_routes_to_the_configured_transport_via_override_channel() {
        let mock = Arc::new(MockTransport::new(Channel::Desktop));
        let mut transports: HashMap<Channel, Box<dyn Transport>> = HashMap::new();
        transports.insert(Channel::Desktop, shim(&mock));
        let mut plugin = NotificationsCorePlugin::with_transports(transports);
        let resp = plugin
            .dispatch(
                HANDLER_SEND,
                &serde_json::json!({
                    "channel": "desktop",
                    "message": "hello",
                    "title": "test"
                }),
            )
            .expect("dispatch succeeds");
        let reply: SendReply = serde_json::from_value(resp).unwrap();
        assert!(reply.delivered);
        assert_eq!(reply.channel, Some(Channel::Desktop));
        assert_eq!(reply.channels, vec![Channel::Desktop]);
        assert_eq!(reply.routing, "override");
        let received = mock.received.lock().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].message, "hello");
        assert_eq!(received[0].title.as_deref(), Some("test"));
    }

    #[test]
    fn send_unknown_channel_id_errors() {
        let mut plugin = NotificationsCorePlugin::with_transports(HashMap::new());
        let err = plugin
            .dispatch(
                HANDLER_SEND,
                &serde_json::json!({
                    "channel": "discord",
                    "message": "x"
                }),
            )
            .unwrap_err();
        assert!(format!("{err}").contains("unknown channel discord"));
    }

    #[test]
    fn send_transport_failure_surfaces_as_ipc_error_on_override_path() {
        let mut transports: HashMap<Channel, Box<dyn Transport>> = HashMap::new();
        transports.insert(
            Channel::Discord,
            Box::new(
                MockTransport::new(Channel::Discord)
                    .with_error(SendError::Http("503 service unavailable".into())),
            ),
        );
        let mut plugin = NotificationsCorePlugin::with_transports(transports);
        let err = plugin
            .dispatch(
                HANDLER_SEND,
                &serde_json::json!({
                    "channel": "discord",
                    "message": "x"
                }),
            )
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("503"));
        assert!(msg.contains("send:"));
    }

    #[test]
    fn send_rejects_missing_message() {
        let mut plugin =
            NotificationsCorePlugin::with_defaults(None, String::new(), String::new(), String::new(), SmtpConfig::default());
        let err = plugin
            .dispatch(HANDLER_SEND, &serde_json::json!({ "channel": "desktop" }))
            .unwrap_err();
        assert!(format!("{err}").contains("invalid args"));
    }

    #[test]
    fn send_rejects_unknown_field() {
        let mut plugin =
            NotificationsCorePlugin::with_defaults(None, String::new(), String::new(), String::new(), SmtpConfig::default());
        let err = plugin
            .dispatch(
                HANDLER_SEND,
                &serde_json::json!({
                    "channel": "desktop",
                    "message": "x",
                    "rogue_field": 7
                }),
            )
            .unwrap_err();
        assert!(format!("{err}").contains("invalid args"));
    }

    #[test]
    fn send_rejects_neither_channel_nor_source() {
        let mut plugin =
            NotificationsCorePlugin::with_defaults(None, String::new(), String::new(), String::new(), SmtpConfig::default());
        let err = plugin
            .dispatch(HANDLER_SEND, &serde_json::json!({ "message": "x" }))
            .unwrap_err();
        assert!(format!("{err}").contains("must be supplied"));
    }

    #[test]
    fn unknown_handler_id_errors() {
        let mut plugin =
            NotificationsCorePlugin::with_defaults(None, String::new(), String::new(), String::new(), SmtpConfig::default());
        let err = plugin.dispatch(99, &serde_json::json!({})).unwrap_err();
        assert!(format!("{err}").contains("unknown handler id 99"));
    }

    // ── BL-135 router-path tests ────────────────────────────────────

    fn router_plugin(
        toml: &str,
        mocks: &[(Channel, Arc<MockTransport>)],
    ) -> NotificationsCorePlugin {
        let cfg = NotificationsConfig::parse(toml).unwrap();
        let mut transports: HashMap<Channel, Box<dyn Transport>> = HashMap::new();
        for (ch, mock) in mocks {
            transports.insert(*ch, shim(mock));
        }
        NotificationsCorePlugin::with_transports_and_config(transports, cfg)
    }

    #[test]
    fn router_path_fans_out_to_every_configured_channel() {
        let m_desk = Arc::new(MockTransport::new(Channel::Desktop));
        let m_disc = Arc::new(MockTransport::new(Channel::Discord));
        let mut plugin = router_plugin(
            r#"
[sources.workflow]
route = ["desktop", "discord"]
"#,
            &[
                (Channel::Desktop, Arc::clone(&m_desk)),
                (Channel::Discord, Arc::clone(&m_disc)),
            ],
        );
        let resp = plugin
            .dispatch(
                HANDLER_SEND,
                &serde_json::json!({
                    "source": "workflow",
                    "message": "deploy complete",
                    "title": "Workflow nightly"
                }),
            )
            .expect("dispatch succeeds");
        let reply: SendReply = serde_json::from_value(resp).unwrap();
        assert!(reply.delivered);
        assert_eq!(reply.routing, "routed");
        assert_eq!(reply.channels.len(), 2);
        assert_eq!(m_desk.received.lock().unwrap().len(), 1);
        assert_eq!(m_disc.received.lock().unwrap().len(), 1);
    }

    #[test]
    fn router_path_returns_unknown_source_for_unconfigured_tag() {
        let mut plugin = router_plugin("", &[]);
        let resp = plugin
            .dispatch(
                HANDLER_SEND,
                &serde_json::json!({
                    "source": "mystery",
                    "message": "x"
                }),
            )
            .expect("dispatch succeeds");
        let reply: SendReply = serde_json::from_value(resp).unwrap();
        assert!(!reply.delivered);
        assert_eq!(reply.routing, "unknown_source");
        assert!(reply.channels.is_empty());
    }

    #[test]
    fn router_path_filters_below_min_severity() {
        let m_desk = Arc::new(MockTransport::new(Channel::Desktop));
        let mut plugin = router_plugin(
            r#"
[sources.workflow]
route = ["desktop"]
min_severity = "warn"
"#,
            &[(Channel::Desktop, Arc::clone(&m_desk))],
        );
        let resp = plugin
            .dispatch(
                HANDLER_SEND,
                &serde_json::json!({
                    "source": "workflow",
                    "severity": "info",
                    "message": "x"
                }),
            )
            .expect("dispatch succeeds");
        let reply: SendReply = serde_json::from_value(resp).unwrap();
        assert!(!reply.delivered);
        assert_eq!(reply.routing, "filtered");
        assert_eq!(m_desk.received.lock().unwrap().len(), 0);
    }

    #[test]
    fn router_path_reports_per_channel_failures_without_aborting() {
        let m_desk = Arc::new(MockTransport::new(Channel::Desktop));
        let m_disc = Arc::new(
            MockTransport::new(Channel::Discord)
                .with_error(SendError::Http("502".into())),
        );
        let mut plugin = router_plugin(
            r#"
[sources.workflow]
route = ["desktop", "discord"]
"#,
            &[
                (Channel::Desktop, Arc::clone(&m_desk)),
                (Channel::Discord, Arc::clone(&m_disc)),
            ],
        );
        let resp = plugin
            .dispatch(
                HANDLER_SEND,
                &serde_json::json!({
                    "source": "workflow",
                    "message": "x"
                }),
            )
            .expect("dispatch succeeds");
        let reply: SendReply = serde_json::from_value(resp).unwrap();
        assert!(reply.delivered, "at least one channel succeeded");
        assert_eq!(reply.channels, vec![Channel::Desktop]);
        assert_eq!(reply.failures.len(), 1);
        assert_eq!(reply.failures[0].channel, Channel::Discord);
        assert!(reply.failures[0].error.contains("502"));
    }

    #[test]
    fn explicit_channel_wins_over_source_when_both_present() {
        let m_desk = Arc::new(MockTransport::new(Channel::Desktop));
        let m_disc = Arc::new(MockTransport::new(Channel::Discord));
        let mut plugin = router_plugin(
            r#"
[sources.workflow]
route = ["discord"]
"#,
            &[
                (Channel::Desktop, Arc::clone(&m_desk)),
                (Channel::Discord, Arc::clone(&m_disc)),
            ],
        );
        let resp = plugin
            .dispatch(
                HANDLER_SEND,
                &serde_json::json!({
                    "channel": "desktop",
                    "source": "workflow",
                    "message": "override"
                }),
            )
            .expect("dispatch succeeds");
        let reply: SendReply = serde_json::from_value(resp).unwrap();
        assert_eq!(reply.routing, "override");
        assert_eq!(reply.channels, vec![Channel::Desktop]);
        assert_eq!(m_desk.received.lock().unwrap().len(), 1);
        assert_eq!(m_disc.received.lock().unwrap().len(), 0);
    }

    // ── Live-reload unit test ───────────────────────────────────────

    #[test]
    fn reload_config_from_disk_swaps_router_rules() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notifications.toml");
        std::fs::write(
            &path,
            r#"
[sources.workflow]
route = ["desktop"]
"#,
        )
        .unwrap();
        let cfg = NotificationsConfig::load_from(&path).unwrap();
        let plugin = NotificationsCorePlugin::from_config(None, cfg, Some(path.clone())).unwrap();
        // Initial state: workflow → desktop.
        let res = plugin
            .router()
            .resolve("workflow", Severity::Info, 12 * 60);
        assert_eq!(res, Resolution::Routed(vec![Channel::Desktop]));

        // Rewrite + reload.
        std::fs::write(
            &path,
            r#"
[sources.workflow]
route = ["discord"]
"#,
        )
        .unwrap();
        plugin.reload_config_from_disk().unwrap();
        let res = plugin
            .router()
            .resolve("workflow", Severity::Info, 12 * 60);
        assert_eq!(res, Resolution::Routed(vec![Channel::Discord]));
    }

    #[test]
    fn reload_config_from_disk_is_noop_when_path_unset() {
        let plugin = NotificationsCorePlugin::with_transports(HashMap::new());
        // No config_path → reload returns Ok without touching state.
        plugin.reload_config_from_disk().unwrap();
    }

    // ── AiEvent translation ─────────────────────────────────────────

    #[test]
    fn translate_finished_event_renders_completion_notification() {
        let n = translate_ai_runtime_event(
            "com.nexus.ai.runtime.finished",
            &serde_json::json!({
                "kind": "finished",
                "task_id": "abc-123",
                "outcome": {}
            }),
        )
        .expect("finished events translate");
        assert!(n.message.contains("abc-123"));
        assert!(n.message.contains("finished"));
        assert_eq!(n.title.as_deref(), Some("AI runtime"));
    }

    #[test]
    fn translate_failed_event_renders_error_notification() {
        let n = translate_ai_runtime_event(
            "com.nexus.ai.runtime.failed",
            &serde_json::json!({
                "kind": "failed",
                "task_id": "abc-123",
                "error": "boom",
                "retriable": false
            }),
        )
        .expect("failed events translate");
        assert!(n.message.contains("abc-123"));
        assert!(n.message.contains("boom"));
    }

    #[test]
    fn translate_token_chunk_is_silent() {
        let n = translate_ai_runtime_event(
            "com.nexus.ai.runtime.token_chunk",
            &serde_json::json!({
                "kind": "token_chunk",
                "task_id": "abc",
                "text": "hello"
            }),
        );
        assert!(n.is_none(), "token chunks must not become notifications");
    }
}
