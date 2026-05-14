//! BL-133 — multi-channel notification dispatcher.
//!
//! Nexus agent and workflow outputs are surfaced only in the active
//! frontend session. A background workflow that completes at 02:00
//! (the Dream Cycle, a scheduled agent run, a file-event workflow)
//! has no delivery channel if the Tauri shell is closed. This crate
//! ships the dispatch surface so plugins, the CLI, and (eventually)
//! the workflow + agent emitters can route a single `send` call to
//! one or more configured channels.
//!
//! ## v1 transports
//!
//! - **`Channel::Desktop`** — publishes a `com.nexus.notifications.delivered`
//!   event on the kernel bus. The shell subscribes and routes the
//!   payload into its existing toast surface. A future shell-side
//!   refinement can swap to the Tauri `notification` plugin so
//!   delivery survives a closed window; the bus contract stays the
//!   same.
//! - **`Channel::Discord`** — HTTP POST to a webhook URL (config or
//!   keyring). Blocking `reqwest`, same shape as `nexus-linkpreview`.
//!
//! Telegram, SMTP, in-app push, shell-settings UI, workflow `notify`
//! step, and the agent run-completion auto-notify subscriber are all
//! filed as follow-ups (BL-133 closure note in `BACKLOG_COMPLETED.md`).
//!
//! ## Wire shape
//!
//! `com.nexus.notifications::send` accepts:
//!
//! ```jsonc
//! {
//!   "channel": "desktop" | "discord",
//!   "message": "string",
//!   "title": "string?"
//! }
//! ```
//!
//! and returns:
//!
//! ```jsonc
//! { "delivered": true, "channel": "discord" }
//! ```
//!
//! Per-channel transport errors are surfaced as IPC errors so the
//! caller can decide whether to retry, fall back, or surface to the
//! user.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod core_plugin;

/// Bus topic published when a notification has been delivered (or
/// attempted) on a frontend-rendered channel. Shell subscribers can
/// hook this and surface the payload through their toast surface.
pub const NOTIFICATION_DELIVERED_TOPIC: &str = "com.nexus.notifications.delivered";

/// Supported notification channels for v1. Add variants in append-
/// only order; the wire form (`serde rename_all = "snake_case"`)
/// pins the JSON tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Desktop OS notification, bridged through the shell via a bus
    /// event ([`NOTIFICATION_DELIVERED_TOPIC`]).
    Desktop,
    /// Discord webhook — HTTP POST to a configured URL.
    Discord,
}

impl Channel {
    /// Human-readable channel name, used in audit-log entries +
    /// error messages.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Channel::Desktop => "desktop",
            Channel::Discord => "discord",
        }
    }
}

/// Payload threaded through every transport. `title` is optional —
/// some channels render it as a header, some prepend it inline, some
/// ignore it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Body of the notification. UTF-8, no length cap at this layer
    /// — transports clip if they have to (Telegram's 4096-char limit,
    /// for example).
    pub message: String,
    /// Optional title. Defaults to `"Nexus"` when the transport
    /// needs one and the caller didn't supply.
    pub title: Option<String>,
}

/// Errors a transport can surface.
#[derive(Debug, Error)]
pub enum SendError {
    /// Channel not configured in `.forge/config.toml`.
    #[error("channel {0} is not configured")]
    NotConfigured(&'static str),
    /// HTTP error from the underlying transport (Discord today;
    /// Telegram / SMTP follow-ups will reuse this variant).
    #[error("HTTP transport error: {0}")]
    Http(String),
    /// Local bus publish failed (Desktop transport).
    #[error("bus publish failed: {0}")]
    Bus(String),
}

/// Transport trait — one impl per [`Channel`]. Lives behind a
/// trait so transports can be mocked for unit testing without
/// hitting the network.
pub trait Transport: Send + Sync {
    /// Which channel this transport serves.
    fn channel(&self) -> Channel;
    /// Deliver a notification synchronously. Returns `Ok(())` on
    /// successful delivery; on failure returns the most specific
    /// [`SendError`] variant.
    fn send(&self, notif: &Notification) -> Result<(), SendError>;
}

/// Desktop transport — publishes a `com.nexus.notifications.delivered`
/// event on the kernel bus. The shell subscribes and renders the
/// payload through its existing toast surface (or, in a future
/// follow-up, hands off to the Tauri `notification` plugin so
/// delivery survives a closed window).
pub struct DesktopTransport {
    bus: Option<Arc<nexus_kernel::EventBus>>,
}

impl DesktopTransport {
    /// Construct a desktop transport bound to an event bus. Without
    /// a bus the [`Transport::send`] call returns
    /// [`SendError::NotConfigured`] — used by unit tests that
    /// exercise the wiring without spinning up a kernel.
    #[must_use]
    pub fn new(bus: Option<Arc<nexus_kernel::EventBus>>) -> Self {
        Self { bus }
    }
}

impl Transport for DesktopTransport {
    fn channel(&self) -> Channel {
        Channel::Desktop
    }
    fn send(&self, notif: &Notification) -> Result<(), SendError> {
        let bus = self.bus.as_ref().ok_or(SendError::NotConfigured("desktop"))?;
        let payload = serde_json::json!({
            "channel": "desktop",
            "title": notif.title.as_deref().unwrap_or("Nexus"),
            "message": notif.message,
        });
        bus.publish_plugin(
            core_plugin::PLUGIN_ID,
            NOTIFICATION_DELIVERED_TOPIC,
            payload,
        )
        .map_err(|e| SendError::Bus(e.to_string()))
    }
}

/// Discord webhook transport. Posts `{ "username": "Nexus", "content":
/// "<title or default>\n<message>" }` to the configured URL with
/// `Content-Type: application/json`. The 2000-char content limit is
/// not enforced at this layer — callers that ship long agent
/// transcripts should pre-split.
pub struct DiscordWebhook {
    webhook_url: String,
    client: reqwest::blocking::Client,
}

impl DiscordWebhook {
    /// Build a webhook transport bound to the URL in
    /// `.forge/config.toml::[notifications.discord]`. Empty / missing
    /// URLs surface at [`Transport::send`] time as
    /// [`SendError::NotConfigured`].
    #[must_use]
    pub fn new(webhook_url: String) -> Self {
        Self {
            webhook_url,
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl Transport for DiscordWebhook {
    fn channel(&self) -> Channel {
        Channel::Discord
    }
    fn send(&self, notif: &Notification) -> Result<(), SendError> {
        if self.webhook_url.is_empty() {
            return Err(SendError::NotConfigured("discord"));
        }
        let body = format!(
            "{}{}",
            notif
                .title
                .as_deref()
                .map(|t| format!("**{t}**\n"))
                .unwrap_or_default(),
            notif.message,
        );
        let resp = self
            .client
            .post(&self.webhook_url)
            .json(&serde_json::json!({ "username": "Nexus", "content": body }))
            .send()
            .map_err(|e| SendError::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(SendError::Http(format!(
                "discord webhook returned {}",
                resp.status()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_serializes_snake_case() {
        let d = serde_json::to_value(Channel::Desktop).unwrap();
        assert_eq!(d, serde_json::Value::String("desktop".into()));
        let g = serde_json::to_value(Channel::Discord).unwrap();
        assert_eq!(g, serde_json::Value::String("discord".into()));
    }

    #[test]
    fn channel_deserializes_snake_case() {
        let d: Channel = serde_json::from_str("\"desktop\"").unwrap();
        assert_eq!(d, Channel::Desktop);
        let g: Channel = serde_json::from_str("\"discord\"").unwrap();
        assert_eq!(g, Channel::Discord);
    }

    #[test]
    fn desktop_transport_without_bus_reports_not_configured() {
        let t = DesktopTransport::new(None);
        let err = t
            .send(&Notification {
                message: "hi".into(),
                title: None,
            })
            .unwrap_err();
        assert!(matches!(err, SendError::NotConfigured("desktop")));
    }

    #[test]
    fn desktop_transport_publishes_payload_when_bus_present() {
        // EventBus uses a tokio broadcast channel — give it a tokio
        // runtime so it can publish. Subscribe before send to capture
        // the event.
        use nexus_kernel::{EventBus, EventFilter, NexusEvent};
        let bus = Arc::new(EventBus::new(8));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix(
            "com.nexus.notifications.".to_owned(),
        ));
        let t = DesktopTransport::new(Some(Arc::clone(&bus)));
        t.send(&Notification {
            message: "hello world".into(),
            title: Some("greeting".into()),
        })
        .unwrap();
        let evt = sub.try_recv().expect("event channel ready").expect("event present");
        match &evt.event {
            NexusEvent::Custom { type_id, payload, .. } => {
                assert_eq!(type_id, NOTIFICATION_DELIVERED_TOPIC);
                assert_eq!(payload["channel"], "desktop");
                assert_eq!(payload["title"], "greeting");
                assert_eq!(payload["message"], "hello world");
            }
            other => panic!("expected Custom, got {other:?}"),
        }
    }

    #[test]
    fn desktop_transport_defaults_title_to_nexus() {
        use nexus_kernel::{EventBus, EventFilter, NexusEvent};
        let bus = Arc::new(EventBus::new(8));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix(
            "com.nexus.notifications.".to_owned(),
        ));
        let t = DesktopTransport::new(Some(Arc::clone(&bus)));
        t.send(&Notification {
            message: "no title".into(),
            title: None,
        })
        .unwrap();
        let evt = sub.try_recv().expect("event channel ready").expect("event present");
        if let NexusEvent::Custom { payload, .. } = &evt.event {
            assert_eq!(payload["title"], "Nexus");
        } else {
            panic!("expected Custom event");
        }
    }

    #[test]
    fn discord_transport_empty_url_reports_not_configured() {
        let t = DiscordWebhook::new(String::new());
        let err = t
            .send(&Notification {
                message: "hi".into(),
                title: None,
            })
            .unwrap_err();
        assert!(matches!(err, SendError::NotConfigured("discord")));
    }
}
