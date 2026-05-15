//! Core plugin wrapping the [`Transport`] registry.
//!
//! Exposes one IPC handler — `send` — that routes a typed
//! [`SendArgs`] payload to the configured [`Transport`] for the
//! requested [`Channel`]. The plugin holds an immutable map of
//! `Channel → Box<dyn Transport>` initialised at construction; today
//! that's a fixed pair (`Desktop` + `Discord`), with Telegram / SMTP
//! filed as follow-ups.

use std::collections::HashMap;
use std::sync::Arc;

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, PluginError};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::{
    Channel, DesktopTransport, DiscordWebhook, Notification, SmtpConfig, SmtpTransport,
    TelegramBot, Transport,
};

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = "com.nexus.notifications";

/// `send` handler id.
pub const HANDLER_SEND: u32 = 1;

/// Args for `com.nexus.notifications::send` (handler id `1`).
///
/// Lifted to a file-scope public type so the schema generator can
/// emit a JSON Schema + TypeScript binding for the IPC contract.
/// Matches the `nexus-linkpreview::FetchArgs` pattern.
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
    /// Target channel — one of [`Channel`]'s snake_case variants.
    pub channel: Channel,
    /// Notification body. UTF-8, no length cap at the IPC layer.
    pub message: String,
    /// Optional title. When omitted, transports that need a header
    /// fall back to `"Nexus"`.
    #[serde(default)]
    pub title: Option<String>,
}

/// Reply for `com.nexus.notifications::send`. Lightweight — the
/// caller can branch on the `delivered` flag without parsing per-
/// transport status fields.
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
    /// `true` when the transport accepted the notification.
    pub delivered: bool,
    /// Echoes the channel that was used so consumers don't have to
    /// thread the request shape back through.
    pub channel: Channel,
}

/// Core plugin that owns the channel → transport map.
pub struct NotificationsCorePlugin {
    transports: HashMap<Channel, Box<dyn Transport>>,
}

impl NotificationsCorePlugin {
    /// Build a fresh plugin with the BL-133 default transports
    /// wired in:
    ///
    /// - `Channel::Desktop` → [`DesktopTransport`] bound to the
    ///   supplied event bus (no bus → `SendError::NotConfigured`).
    /// - `Channel::Discord` → [`DiscordWebhook`] bound to
    ///   `discord_webhook_url`. Empty string surfaces
    ///   `SendError::NotConfigured` at send time so missing config
    ///   doesn't crash at boot.
    /// - `Channel::Telegram` → [`TelegramBot`] bound to
    ///   `telegram_bot_token` + `telegram_chat_id`. Either empty
    ///   surfaces `SendError::NotConfigured` at send time.
    /// - `Channel::Email` → [`SmtpTransport`] bound to `smtp_config`.
    ///   Empty / partial fields surface `SendError::NotConfigured`
    ///   at send time.
    #[must_use]
    pub fn with_defaults(
        bus: Option<Arc<EventBus>>,
        discord_webhook_url: String,
        telegram_bot_token: String,
        telegram_chat_id: String,
        smtp_config: SmtpConfig,
    ) -> Self {
        let mut transports: HashMap<Channel, Box<dyn Transport>> = HashMap::new();
        transports.insert(Channel::Desktop, Box::new(DesktopTransport::new(bus)));
        transports.insert(
            Channel::Discord,
            Box::new(DiscordWebhook::new(discord_webhook_url)),
        );
        transports.insert(
            Channel::Telegram,
            Box::new(TelegramBot::new(telegram_bot_token, telegram_chat_id)),
        );
        transports.insert(Channel::Email, Box::new(SmtpTransport::new(smtp_config)));
        Self { transports }
    }

    /// Build a plugin with an arbitrary transport map — used by unit
    /// tests that swap in mock transports.
    #[must_use]
    pub fn with_transports(transports: HashMap<Channel, Box<dyn Transport>>) -> Self {
        Self { transports }
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
            other => Err(exec_err(format!("unknown handler id {other}"))),
        }
    }
}

impl NotificationsCorePlugin {
    fn dispatch_send(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        let parsed: SendArgs = serde_json::from_value(args.clone())
            .map_err(|e| exec_err(format!("send: invalid args: {e}")))?;
        let transport = self
            .transports
            .get(&parsed.channel)
            .ok_or_else(|| exec_err(format!("send: unknown channel {}", parsed.channel.as_str())))?;
        let notif = Notification {
            message: parsed.message,
            title: parsed.title,
        };
        transport
            .send(&notif)
            .map_err(|e| exec_err(format!("send: {e}")))?;
        let reply = SendReply {
            delivered: true,
            channel: parsed.channel,
        };
        serde_json::to_value(&reply).map_err(|e| exec_err(format!("send: serialize: {e}")))
    }
}

fn exec_err(msg: impl Into<String>) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: msg.into(),
    }
}

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

    #[test]
    fn send_routes_to_the_configured_transport() {
        let mock = Arc::new(MockTransport::new(Channel::Desktop));
        // We need the MockTransport to be the same instance the
        // plugin uses + we need to inspect its `received` after.
        // Box::new clones the data via the trait object — so wrap
        // the receiver list in an Arc to share across the two
        // references.
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
        let mut transports: HashMap<Channel, Box<dyn Transport>> = HashMap::new();
        transports.insert(
            Channel::Desktop,
            Box::new(ShimTransport {
                inner: Arc::clone(&mock),
            }),
        );
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
        assert_eq!(reply.channel, Channel::Desktop);
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
    fn send_transport_failure_surfaces_as_ipc_error() {
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
    fn unknown_handler_id_errors() {
        let mut plugin =
            NotificationsCorePlugin::with_defaults(None, String::new(), String::new(), String::new(), SmtpConfig::default());
        let err = plugin.dispatch(99, &serde_json::json!({})).unwrap_err();
        assert!(format!("{err}").contains("unknown handler id 99"));
    }
}
