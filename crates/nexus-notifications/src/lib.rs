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
//! - **`Channel::Telegram`** — HTTP POST to the Telegram Bot API
//!   `sendMessage` endpoint. Splits long bodies at UTF-8 boundaries
//!   under the 4096-byte sendMessage cap.
//! - **`Channel::Email`** — SMTP submission via `lettre` over rustls
//!   TLS. `[notifications.email]` config block supplies host / port /
//!   credentials / from / to / subject template. Plain-text only; HTML
//!   bodies are out of scope for v1.
//! - **`Channel::Webhook`** — generic HTTP POST to a configured URL
//!   (C90). `[notifications.webhook]` supplies the URL, optional
//!   custom headers (e.g. an auth token for ntfy/Gotify), and an
//!   optional `body_template` with `{title}` / `{message}`
//!   placeholders (JSON-escaped on substitution) for services whose
//!   payload shape isn't the default `{"title", "message"}` — Slack's
//!   Incoming Webhooks (`{"text": "..."}`), ntfy, Gotify, Matrix (via
//!   a bridge), or any arbitrary JSON-POST endpoint.
//!
//! Shell-settings UI, workflow `notify` step, and the agent
//! run-completion auto-notify subscriber are filed as follow-ups
//! (BL-133 closure note in `backlog/`).
//!
//! ## Wire shape
//!
//! `com.nexus.notifications::send` accepts:
//!
//! ```jsonc
//! {
//!   "channel": "desktop" | "discord" | "telegram" | "email" | "webhook",
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

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

pub mod config;
pub mod core_plugin;
pub mod inbox;
pub mod router;

pub use config::{
    ChannelsConfig, ConfigError, DiscordChannel, EmailChannel, InboxConfig, NotificationsConfig,
    QuietHours, ResolvedSource, Severity, SourceConfig, TelegramChannel, WebhookChannel,
};
pub use inbox::{Inbox, InboxEntry, InboxError, InboxStats, NewEntry, StatusFilter};
pub use router::{Resolution, Router};

/// Default location of the BL-136 inbox database, relative to the
/// forge root. Bootstrap threads `<forge_root>/.forge/notifications/inbox.db`
/// into the plugin constructor; tests use [`Inbox::in_memory`].
pub const INBOX_DB_RELPATH: &str = ".forge/notifications/inbox.db";

/// Default cap on the byte size of a single Telegram `sendMessage`
/// request, used when `notifications.toml::[channels.telegram].max_bytes`
/// is unset. Telegram's documented limit is 4096 UTF-16 code units;
/// the safer 4096-byte UTF-8 cap is what we enforce — at worst we send
/// fewer characters than the API permits, which is fine. The
/// [`TelegramBot`] splitter chunks at character boundaries under this
/// cap.
pub const DEFAULT_TELEGRAM_MAX_BYTES: usize = 4096;

/// Bus topic published when a new row lands in the inbox. Payload is
/// `{ id, source, severity, ts }` — used by the shell to bump its
/// unread badge without polling [`Inbox::stats`].
pub const INBOX_APPENDED_TOPIC: &str = "com.nexus.notifications.inbox.appended";

/// Default location of the BL-135 router config, relative to the
/// forge root. Bootstrap and the file-watcher live-reload path both
/// reference this so the path stays single-sourced.
pub const NOTIFICATIONS_CONFIG_RELPATH: &str = ".forge/notifications.toml";

/// Bus topic published when a notification has been delivered (or
/// attempted) on a frontend-rendered channel. Shell subscribers can
/// hook this and surface the payload through their toast surface.
pub const NOTIFICATION_DELIVERED_TOPIC: &str = "com.nexus.notifications.delivered";

/// Supported notification channels for v1. Add variants in append-
/// only order; the wire form (`serde rename_all = "snake_case"`)
/// pins the JSON tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Desktop OS notification, bridged through the shell via a bus
    /// event ([`NOTIFICATION_DELIVERED_TOPIC`]).
    Desktop,
    /// Discord webhook — HTTP POST to a configured URL.
    Discord,
    /// Telegram bot — HTTP POST to
    /// `https://api.telegram.org/bot<TOKEN>/sendMessage`.
    Telegram,
    /// Email — SMTP submission via [`SmtpTransport`].
    Email,
    /// Generic webhook — HTTP POST to a configured URL. See
    /// [`GenericWebhook`] (C90 / #443).
    Webhook,
}

impl Channel {
    /// Human-readable channel name, used in audit-log entries +
    /// error messages.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Channel::Desktop => "desktop",
            Channel::Discord => "discord",
            Channel::Telegram => "telegram",
            Channel::Email => "email",
            Channel::Webhook => "webhook",
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
    /// HTTP error from the underlying transport (Discord / Telegram).
    #[error("HTTP transport error: {0}")]
    Http(String),
    /// Local bus publish failed (Desktop transport).
    #[error("bus publish failed: {0}")]
    Bus(String),
    /// SMTP-layer error (connect / auth / submission). Distinct from
    /// `Http` so callers can differentiate "the SMTP server rejected
    /// us" from "Discord's webhook returned 5xx".
    #[error("SMTP error: {0}")]
    Smtp(String),
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
        let bus = self
            .bus
            .as_ref()
            .ok_or(SendError::NotConfigured("desktop"))?;
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

/// TCP connect deadline for webhook transports (V4,
/// `repo-review-2026-06-10.md`).
const WEBHOOK_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Overall request deadline for webhook transports. Webhook posts are
/// small fire-and-forget JSON bodies, so unlike the streaming AI
/// clients a total timeout is appropriate here.
const WEBHOOK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Build the lazy blocking client for webhook transports with connect
/// and overall timeouts. Falls back to a stock client if the builder
/// fails (it has no fallible inputs in practice) so `send` never
/// loses its transport.
fn blocking_webhook_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .connect_timeout(WEBHOOK_CONNECT_TIMEOUT)
        .timeout(WEBHOOK_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

/// Discord webhook transport. Posts `{ "username": "Nexus", "content":
/// "<title or default>\n<message>" }` to the configured URL with
/// `Content-Type: application/json`. The 2000-char content limit is
/// not enforced at this layer — callers that ship long agent
/// transcripts should pre-split.
pub struct DiscordWebhook {
    webhook_url: String,
    // Lazy so the inner reqwest "internal sync runtime" thread isn't
    // spawned at boot. `reqwest::blocking::Client::new()` panics under
    // debug-assertions when called from a thread already inside a tokio
    // async context (it builds + drops a current-thread runtime for a
    // sanity check), which the shell's `boot_kernel` async-Tauri-command
    // is. Building on first `send` runs through `spawn_blocking`, where
    // tokio's blocking-region check passes.
    client: std::sync::OnceLock<reqwest::blocking::Client>,
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
            client: std::sync::OnceLock::new(),
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
            .get_or_init(blocking_webhook_client)
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

/// Generic webhook transport (C90 / #443). Unlike [`DiscordWebhook`]
/// / [`TelegramBot`], the request shape isn't hardcoded to one
/// service's API — a configurable `body_template` lets it target
/// Slack Incoming Webhooks, ntfy, Gotify, Matrix (via a bridge), or
/// any endpoint that accepts a JSON POST.
pub struct GenericWebhook {
    url: String,
    headers: std::collections::BTreeMap<String, String>,
    body_template: Option<String>,
    // Lazy — see the same note on `DiscordWebhook::client`.
    client: std::sync::OnceLock<reqwest::blocking::Client>,
}

impl GenericWebhook {
    /// Build a webhook transport bound to `.forge/notifications.toml::[channels.webhook]`.
    /// An empty `url` surfaces at [`Transport::send`] time as
    /// [`SendError::NotConfigured`].
    #[must_use]
    pub fn new(
        url: String,
        headers: std::collections::BTreeMap<String, String>,
        body_template: Option<String>,
    ) -> Self {
        Self {
            url,
            headers,
            body_template,
            client: std::sync::OnceLock::new(),
        }
    }

    /// JSON-encode `s` and strip the surrounding quotes, so a
    /// `body_template` author can write `"{message}"` and get a
    /// properly-escaped JSON string fragment rather than raw
    /// (potentially quote- or newline-breaking) user text spliced
    /// into their template.
    fn json_escape_fragment(s: &str) -> String {
        let quoted = serde_json::to_string(s).unwrap_or_default();
        // `serde_json::to_string` on a `&str` always emits a quoted
        // JSON string (no fallible input), so stripping one leading +
        // one trailing byte is safe; the `unwrap_or_default` above is
        // defensive only, matching `blocking_webhook_client`'s stance.
        quoted
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(&quoted)
            .to_string()
    }

    /// Render the outgoing JSON body: the configured template with
    /// `{title}` / `{message}` substituted, or the default
    /// `{"title", "message"}` shape when no template is set.
    fn render_body(&self, notif: &Notification) -> Result<serde_json::Value, SendError> {
        let title = notif.title.as_deref().unwrap_or("Nexus");
        let Some(template) = &self.body_template else {
            return Ok(serde_json::json!({ "title": title, "message": notif.message }));
        };
        let rendered = template
            .replace("{title}", &Self::json_escape_fragment(title))
            .replace("{message}", &Self::json_escape_fragment(&notif.message));
        serde_json::from_str(&rendered).map_err(|e| {
            SendError::Http(format!(
                "webhook body_template did not render valid JSON: {e}"
            ))
        })
    }
}

impl Transport for GenericWebhook {
    fn channel(&self) -> Channel {
        Channel::Webhook
    }
    fn send(&self, notif: &Notification) -> Result<(), SendError> {
        if self.url.is_empty() {
            return Err(SendError::NotConfigured("webhook"));
        }
        let body = self.render_body(notif)?;
        let mut req = self
            .client
            .get_or_init(blocking_webhook_client)
            .post(&self.url)
            .json(&body);
        for (name, value) in &self.headers {
            req = req.header(name, value);
        }
        let resp = req.send().map_err(|e| SendError::Http(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(SendError::Http(format!(
                "webhook returned {}",
                resp.status()
            )));
        }
        Ok(())
    }
}

/// Telegram bot transport. Posts to
/// `https://api.telegram.org/bot<BOT_TOKEN>/sendMessage` with
/// `{ chat_id, text }`. The 4096-char message limit is enforced by
/// splitting at character boundaries and posting each chunk
/// sequentially so the original message arrives in order.
pub struct TelegramBot {
    bot_token: String,
    chat_id: String,
    max_bytes: usize,
    // Lazy — see the same note on `DiscordWebhook::client`.
    client: std::sync::OnceLock<reqwest::blocking::Client>,
}

impl TelegramBot {
    /// Build a Telegram transport with the bot token + authorised
    /// chat id from `.forge/notifications.toml::[channels.telegram]`.
    /// Empty `bot_token` OR empty `chat_id` surfaces as
    /// [`SendError::NotConfigured`] at [`Transport::send`] time —
    /// matches the Discord transport's "fail at dispatch, not at
    /// boot" stance so missing config doesn't crash the runtime.
    ///
    /// `max_bytes` caps the per-`sendMessage` UTF-8 byte length; pass
    /// [`crate::DEFAULT_TELEGRAM_MAX_BYTES`] when no override is set.
    /// A value of `0` is replaced with the default so a misconfigured
    /// override can't disable splitting entirely.
    #[must_use]
    pub fn new(bot_token: String, chat_id: String, max_bytes: usize) -> Self {
        let max_bytes = if max_bytes == 0 {
            crate::DEFAULT_TELEGRAM_MAX_BYTES
        } else {
            max_bytes
        };
        Self {
            bot_token,
            chat_id,
            max_bytes,
            client: std::sync::OnceLock::new(),
        }
    }

    /// Split a UTF-8 string into chunks of at most `max_chars` bytes
    /// at character boundaries. Public for unit-testing the
    /// 4096-char limit-enforcement path. Empty input returns a
    /// single empty chunk so a "test ping" with no message body
    /// still hits the API.
    #[must_use]
    pub fn split_at_byte_limit(text: &str, max_bytes: usize) -> Vec<&str> {
        if text.len() <= max_bytes {
            return vec![text];
        }
        let mut chunks = Vec::new();
        let mut start = 0;
        while start < text.len() {
            let end_target = (start + max_bytes).min(text.len());
            // Snap to the previous char boundary if `end_target`
            // lands mid-character.
            let mut end = end_target;
            while end > start && !text.is_char_boundary(end) {
                end -= 1;
            }
            if end == start {
                // Pathological: a single character is wider than
                // max_bytes. Force forward by snapping to the next
                // boundary so we don't infinite-loop. Telegram's
                // 4096-byte limit is well above any single-char
                // width so this is defensive only.
                end = end_target;
                while end < text.len() && !text.is_char_boundary(end) {
                    end += 1;
                }
            }
            chunks.push(&text[start..end]);
            start = end;
        }
        chunks
    }
}

impl Transport for TelegramBot {
    fn channel(&self) -> Channel {
        Channel::Telegram
    }
    fn send(&self, notif: &Notification) -> Result<(), SendError> {
        if self.bot_token.is_empty() || self.chat_id.is_empty() {
            return Err(SendError::NotConfigured("telegram"));
        }
        let body = format!(
            "{}{}",
            notif
                .title
                .as_deref()
                .map(|t| format!("*{t}*\n"))
                .unwrap_or_default(),
            notif.message,
        );
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token);
        for chunk in Self::split_at_byte_limit(&body, self.max_bytes) {
            let resp = self
                .client
                .get_or_init(blocking_webhook_client)
                .post(&url)
                .json(&serde_json::json!({
                    "chat_id": self.chat_id,
                    "text": chunk,
                }))
                .send()
                .map_err(|e| SendError::Http(e.to_string()))?;
            if !resp.status().is_success() {
                return Err(SendError::Http(format!(
                    "telegram sendMessage returned {}",
                    resp.status()
                )));
            }
        }
        Ok(())
    }
}

/// SMTP transport configuration.
///
/// Mirrors the shape of `[notifications.email]` in
/// `<forge>/.forge/config.toml`. Defaulting every field to an empty
/// string keeps the boot path tolerant of partial configuration —
/// missing fields surface at [`Transport::send`] time as
/// [`SendError::NotConfigured`] rather than panicking at
/// construction.
#[derive(Debug, Clone, Default)]
pub struct SmtpConfig {
    /// SMTP hostname, e.g. `smtp.gmail.com`.
    pub host: String,
    /// SMTP submission port. Common values: 465 (implicit TLS),
    /// 587 (STARTTLS).
    pub port: u16,
    /// Username for SMTP AUTH (PLAIN/LOGIN). Empty disables auth.
    pub username: String,
    /// Password for SMTP AUTH. Plain text in `config.toml` for v1 —
    /// the BL-133 follow-up tail tracks routing this through the
    /// `nexus-security` keyring once the IPC surface lands.
    pub password: String,
    /// `From:` envelope address — must be a valid `addr@host`.
    pub from: String,
    /// `To:` envelope address. Multiple recipients can be supplied
    /// comma-separated; each is parsed with `addr.parse::<Mailbox>()`
    /// at send-time and the message is delivered to all of them.
    pub to: String,
    /// Subject template used when the notification carries no title.
    /// `{title}` is interpolated when the notification *does* carry a
    /// title; default falls back to `"Nexus notification"`.
    pub subject_template: String,
}

/// SMTP transport. Uses [`lettre`] with implicit TLS (port 465) or
/// STARTTLS (port 587), keyed off the configured `port`. The TLS
/// stack is rustls + ring, matching the rest of the Nexus dep graph.
///
/// Connection pooling is on (lettre's default `pool` feature) so
/// repeat-fire notifications (e.g. workflow runs that emit on each
/// step) don't re-handshake per send.
pub struct SmtpTransport {
    config: SmtpConfig,
}

impl SmtpTransport {
    /// Build a fresh SMTP transport bound to `config`. No I/O happens
    /// here; the actual connection is lazy on the first [`Transport::send`]
    /// call.
    #[must_use]
    pub fn new(config: SmtpConfig) -> Self {
        Self { config }
    }

    /// Validate that every config field needed to actually deliver a
    /// message is populated. Public for the unit-test surface — the
    /// production path runs the same gate at the top of
    /// [`Transport::send`].
    #[must_use]
    pub fn is_configured(&self) -> bool {
        !self.config.host.is_empty()
            && self.config.port != 0
            && !self.config.from.is_empty()
            && !self.config.to.is_empty()
    }

    /// Build the message subject. Falls back to the configured
    /// `subject_template` (with `{title}` substituted) when the
    /// notification carries a title, then to `"Nexus notification"`,
    /// then to a final last-resort literal so the SMTP envelope never
    /// ships a blank Subject header.
    #[must_use]
    pub fn compose_subject(template: &str, title: Option<&str>) -> String {
        let trimmed = template.trim();
        match (title, trimmed.is_empty()) {
            (Some(t), false) => trimmed.replace("{title}", t),
            (Some(t), true) => t.to_string(),
            (None, false) => trimmed.replace("{title}", "Nexus notification"),
            (None, true) => "Nexus notification".to_string(),
        }
    }
}

impl Transport for SmtpTransport {
    fn channel(&self) -> Channel {
        Channel::Email
    }
    fn send(&self, notif: &Notification) -> Result<(), SendError> {
        use lettre::Transport as _;
        if !self.is_configured() {
            return Err(SendError::NotConfigured("email"));
        }
        let subject = Self::compose_subject(&self.config.subject_template, notif.title.as_deref());
        let from: lettre::message::Mailbox =
            self.config
                .from
                .parse()
                .map_err(|e: lettre::address::AddressError| {
                    SendError::Smtp(format!("invalid from address '{}': {e}", self.config.from))
                })?;
        let mut builder = lettre::Message::builder().from(from).subject(subject);
        for raw in self
            .config
            .to
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let mailbox: lettre::message::Mailbox =
                raw.parse().map_err(|e: lettre::address::AddressError| {
                    SendError::Smtp(format!("invalid to address '{raw}': {e}"))
                })?;
            builder = builder.to(mailbox);
        }
        let email = builder
            .body(notif.message.clone())
            .map_err(|e| SendError::Smtp(format!("build message: {e}")))?;

        // Port-based TLS mode. 465 = implicit TLS (SMTPS), anything
        // else = STARTTLS on submission. Bare port 25 isn't offered
        // — submission paths should always be authenticated + TLS.
        let mut t = if self.config.port == 465 {
            lettre::SmtpTransport::relay(&self.config.host)
                .map_err(|e| SendError::Smtp(format!("smtps relay: {e}")))?
                .port(self.config.port)
        } else {
            lettre::SmtpTransport::starttls_relay(&self.config.host)
                .map_err(|e| SendError::Smtp(format!("starttls relay: {e}")))?
                .port(self.config.port)
        };
        if !self.config.username.is_empty() {
            t = t.credentials(lettre::transport::smtp::authentication::Credentials::new(
                self.config.username.clone(),
                self.config.password.clone(),
            ));
        }
        let mailer = t.build();
        mailer
            .send(&email)
            .map(|_| ())
            .map_err(|e| SendError::Smtp(format!("send: {e}")))
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
        let evt = sub
            .try_recv()
            .expect("event channel ready")
            .expect("event present");
        match &evt.event {
            NexusEvent::Custom {
                type_id, payload, ..
            } => {
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
        let evt = sub
            .try_recv()
            .expect("event channel ready")
            .expect("event present");
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

    // ── Generic webhook transport (C90 / #443) ──────────────────

    #[test]
    fn channel_webhook_serde_round_trip() {
        let s = serde_json::to_value(Channel::Webhook).unwrap();
        assert_eq!(s, serde_json::Value::String("webhook".into()));
        let c: Channel = serde_json::from_str("\"webhook\"").unwrap();
        assert_eq!(c, Channel::Webhook);
    }

    #[test]
    fn webhook_transport_empty_url_reports_not_configured() {
        let t = GenericWebhook::new(String::new(), std::collections::BTreeMap::new(), None);
        let err = t
            .send(&Notification {
                message: "hi".into(),
                title: None,
            })
            .unwrap_err();
        assert!(matches!(err, SendError::NotConfigured("webhook")));
    }

    #[test]
    fn webhook_default_body_has_title_and_message() {
        let t = GenericWebhook::new("https://example.test/hook".into(), std::collections::BTreeMap::new(), None);
        let body = t
            .render_body(&Notification {
                message: "hello".into(),
                title: Some("Greeting".into()),
            })
            .unwrap();
        assert_eq!(body, serde_json::json!({ "title": "Greeting", "message": "hello" }));
    }

    #[test]
    fn webhook_default_body_falls_back_to_nexus_title() {
        let t = GenericWebhook::new("https://example.test/hook".into(), std::collections::BTreeMap::new(), None);
        let body = t
            .render_body(&Notification {
                message: "hello".into(),
                title: None,
            })
            .unwrap();
        assert_eq!(body["title"], "Nexus");
    }

    #[test]
    fn webhook_custom_template_substitutes_and_escapes() {
        // Slack-shaped template. The message contains a double quote
        // and a newline — both must come out JSON-escaped so the
        // rendered template still parses.
        let t = GenericWebhook::new(
            "https://hooks.slack.example/x".into(),
            std::collections::BTreeMap::new(),
            Some(r#"{"text": "{title}: {message}"}"#.into()),
        );
        let body = t
            .render_body(&Notification {
                message: "she said \"hi\"\nline two".into(),
                title: Some("Alert".into()),
            })
            .unwrap();
        assert_eq!(body["text"], "Alert: she said \"hi\"\nline two");
    }

    #[test]
    fn webhook_template_rendering_invalid_json_errors() {
        let t = GenericWebhook::new(
            "https://example.test/hook".into(),
            std::collections::BTreeMap::new(),
            Some("not json at all: {message}".into()),
        );
        let err = t
            .render_body(&Notification {
                message: "hi".into(),
                title: None,
            })
            .unwrap_err();
        assert!(matches!(err, SendError::Http(_)));
    }

    // ── Telegram transport ──────────────────────────────────────

    #[test]
    fn channel_telegram_serde_round_trip() {
        let t: Channel = serde_json::from_str("\"telegram\"").unwrap();
        assert_eq!(t, Channel::Telegram);
        let s = serde_json::to_value(Channel::Telegram).unwrap();
        assert_eq!(s, serde_json::Value::String("telegram".into()));
        assert_eq!(Channel::Telegram.as_str(), "telegram");
    }

    #[test]
    fn telegram_transport_empty_bot_token_reports_not_configured() {
        let t = TelegramBot::new(
            String::new(),
            "12345".into(),
            crate::DEFAULT_TELEGRAM_MAX_BYTES,
        );
        let err = t
            .send(&Notification {
                message: "hi".into(),
                title: None,
            })
            .unwrap_err();
        assert!(matches!(err, SendError::NotConfigured("telegram")));
    }

    #[test]
    fn telegram_transport_empty_chat_id_reports_not_configured() {
        let t = TelegramBot::new(
            "bot:token".into(),
            String::new(),
            crate::DEFAULT_TELEGRAM_MAX_BYTES,
        );
        let err = t
            .send(&Notification {
                message: "hi".into(),
                title: None,
            })
            .unwrap_err();
        assert!(matches!(err, SendError::NotConfigured("telegram")));
    }

    #[test]
    fn telegram_split_at_byte_limit_short_text_single_chunk() {
        let chunks = TelegramBot::split_at_byte_limit("hello", 100);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn telegram_split_at_byte_limit_long_text_chunks() {
        let text = "x".repeat(10_000);
        let chunks = TelegramBot::split_at_byte_limit(&text, 4096);
        assert_eq!(chunks.len(), 3); // 4096 + 4096 + 1808
        assert_eq!(chunks[0].len(), 4096);
        assert_eq!(chunks[1].len(), 4096);
        assert_eq!(chunks[2].len(), 10_000 - 4096 - 4096);
        // Round-trip: chunks concatenated equal the original.
        let joined: String = chunks.into_iter().collect();
        assert_eq!(joined, text);
    }

    // ── SMTP transport ──────────────────────────────────────────

    #[test]
    fn channel_email_serde_round_trip() {
        let t: Channel = serde_json::from_str("\"email\"").unwrap();
        assert_eq!(t, Channel::Email);
        let s = serde_json::to_value(Channel::Email).unwrap();
        assert_eq!(s, serde_json::Value::String("email".into()));
        assert_eq!(Channel::Email.as_str(), "email");
    }

    #[test]
    fn smtp_transport_empty_config_reports_not_configured() {
        let t = SmtpTransport::new(SmtpConfig::default());
        assert!(!t.is_configured());
        let err = t
            .send(&Notification {
                message: "hi".into(),
                title: None,
            })
            .unwrap_err();
        assert!(matches!(err, SendError::NotConfigured("email")));
    }

    #[test]
    fn smtp_transport_partial_config_reports_not_configured() {
        // Host present, port zero, from + to missing — still not
        // ready to deliver. Should fail at the gate, not the send.
        let t = SmtpTransport::new(SmtpConfig {
            host: "smtp.example.com".into(),
            ..SmtpConfig::default()
        });
        assert!(!t.is_configured());
        let err = t
            .send(&Notification {
                message: "hi".into(),
                title: None,
            })
            .unwrap_err();
        assert!(matches!(err, SendError::NotConfigured("email")));
    }

    #[test]
    fn smtp_compose_subject_uses_title_when_template_has_placeholder() {
        let s = SmtpTransport::compose_subject("Nexus: {title}", Some("Backup done"));
        assert_eq!(s, "Nexus: Backup done");
    }

    #[test]
    fn smtp_compose_subject_uses_title_directly_when_no_template() {
        let s = SmtpTransport::compose_subject("", Some("Inline"));
        assert_eq!(s, "Inline");
        let s = SmtpTransport::compose_subject("   ", Some("Inline"));
        assert_eq!(s, "Inline");
    }

    #[test]
    fn smtp_compose_subject_falls_back_to_template_default_when_no_title() {
        // Title missing + non-empty template → template renders with
        // a literal placeholder substitute so the subject is not
        // blank but also not the empty default.
        let s = SmtpTransport::compose_subject("Nexus: {title}", None);
        assert_eq!(s, "Nexus: Nexus notification");
    }

    #[test]
    fn smtp_compose_subject_falls_back_to_literal_when_blank_template_and_no_title() {
        let s = SmtpTransport::compose_subject("", None);
        assert_eq!(s, "Nexus notification");
    }

    #[test]
    fn smtp_transport_rejects_invalid_from_address() {
        // Configured (so the gate passes) but the from header is not
        // a valid mailbox — surfaces as `Smtp` (build-time), not
        // `NotConfigured`.
        let t = SmtpTransport::new(SmtpConfig {
            host: "smtp.example.com".into(),
            port: 587,
            from: "not-an-email".into(),
            to: "ok@example.com".into(),
            ..SmtpConfig::default()
        });
        assert!(t.is_configured());
        let err = t
            .send(&Notification {
                message: "hi".into(),
                title: None,
            })
            .unwrap_err();
        match err {
            SendError::Smtp(msg) => assert!(msg.contains("invalid from address")),
            other => panic!("expected Smtp, got {other:?}"),
        }
    }

    #[test]
    fn telegram_split_at_byte_limit_respects_utf8_boundaries() {
        // 4-byte emoji 🦀 (4 bytes) placed at a position where a
        // naive cut would split mid-codepoint. With max=5, the
        // limit lands at byte 5 → snap back to byte 4 (after 🦀)
        // → first chunk = "🦀".
        let text = "🦀🦀";
        let chunks = TelegramBot::split_at_byte_limit(text, 5);
        for c in &chunks {
            // Every chunk must be valid UTF-8 (slicing wouldn't
            // even produce a Vec<&str> otherwise — but pin the
            // invariant explicitly).
            assert!(std::str::from_utf8(c.as_bytes()).is_ok());
        }
        let joined: String = chunks.into_iter().collect();
        assert_eq!(joined, text);
    }
}
