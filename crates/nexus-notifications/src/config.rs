//! BL-135 — `<forge>/.forge/notifications.toml` schema, parser, and
//! the in-process `NotificationsConfig` it deserialises into.
//!
//! Today every notification producer hardcodes which channel it
//! targets. This config file centralises the producer→transport
//! mapping under named `[sources.<name>]` blocks. The router
//! ([`crate::router`]) reads the loaded config and resolves a
//! source-tagged [`crate::Notification`] into the list of channels it
//! should be dispatched on.
//!
//! Per-channel transport credentials live under `[channels.<name>]`
//! blocks — they replace the legacy `[notifications.<channel>]`
//! blocks in `<forge>/.forge/config.toml`. For backward compatibility
//! the bootstrap loader still falls back to the legacy blocks when
//! `notifications.toml` is absent (see
//! `nexus-bootstrap::load_notifications_config`).
//!
//! Missing file → [`NotificationsConfig::default`] (no sources, no
//! channels). Producers that ship a `source` tag in that state get
//! routed nowhere and the send is a no-op. Explicit-channel callers
//! keep working because they don't consult the router.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::{Channel, SmtpConfig};

/// Severity associated with a [`crate::Notification`] dispatch.
///
/// Wire form is snake_case (`debug` / `info` / `warn` / `error`). The
/// ordering matters — the router filters out events below a source's
/// `min_severity`, so the `PartialOrd` derive must reflect the
/// declaration order (`Debug < Info < Warn < Error`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Verbose lifecycle traces. Filtered by every source by default.
    Debug,
    /// Default level. Routine completion events.
    #[default]
    Info,
    /// Worth surfacing — degraded behaviour, retries, soft failures.
    Warn,
    /// Hard failure. Pages-grade signal.
    Error,
}

impl Severity {
    /// Wire/text form for log messages and error rendering.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Debug => "debug",
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
        }
    }
}

/// Compact 24-hour-clock pair parsed from a `"HH:MM-HH:MM"` string.
///
/// `start > end` is allowed (e.g. `"22:00-08:00"` straddles midnight)
/// and is interpreted as "outside `end..start`".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuietHours {
    /// Start minute-of-day (inclusive), 0..=1439.
    pub start_min: u16,
    /// End minute-of-day (exclusive), 0..=1439.
    pub end_min: u16,
}

impl QuietHours {
    /// Parse a `"HH:MM-HH:MM"` string. Whitespace around the dash is
    /// tolerated; lower/upper hour ordering is preserved (overnight
    /// ranges are valid).
    ///
    /// # Errors
    /// Returns [`ConfigError::QuietHours`] on any malformed input.
    pub fn parse(raw: &str) -> Result<Self, ConfigError> {
        let trimmed = raw.trim();
        let (a, b) = trimmed
            .split_once('-')
            .ok_or_else(|| ConfigError::QuietHours(raw.to_string()))?;
        let start_min = parse_hhmm(a.trim()).ok_or_else(|| ConfigError::QuietHours(raw.to_string()))?;
        let end_min = parse_hhmm(b.trim()).ok_or_else(|| ConfigError::QuietHours(raw.to_string()))?;
        Ok(Self { start_min, end_min })
    }

    /// `true` when `min_of_day` falls inside the configured quiet
    /// window. Overnight ranges (`start_min > end_min`) wrap around
    /// midnight.
    #[must_use]
    pub fn contains(&self, min_of_day: u16) -> bool {
        if self.start_min <= self.end_min {
            min_of_day >= self.start_min && min_of_day < self.end_min
        } else {
            // overnight: e.g. 22:00–08:00 → quiet when >=22:00 OR <08:00
            min_of_day >= self.start_min || min_of_day < self.end_min
        }
    }
}

fn parse_hhmm(s: &str) -> Option<u16> {
    let (h, m) = s.split_once(':')?;
    let hh: u16 = h.parse().ok()?;
    let mm: u16 = m.parse().ok()?;
    if hh > 23 || mm > 59 {
        return None;
    }
    Some(hh * 60 + mm)
}

/// One `[sources.<name>]` block.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {
    /// Bus topic prefixes/exact names this source listens to. Today
    /// the router doesn't auto-subscribe per source — only the
    /// built-in `ai_runtime` subscriber does. Field is parsed so the
    /// schema stays stable when later phases add per-source bus
    /// listeners.
    #[serde(default)]
    pub on: Vec<String>,
    /// Channel names this source dispatches to. Unknown channel
    /// names are dropped at router-build time with a warning so a
    /// typo doesn't crash the boot path.
    #[serde(default)]
    pub route: Vec<String>,
    /// Filter below this severity. Defaults to `info`.
    #[serde(default)]
    pub min_severity: Severity,
    /// Optional `"HH:MM-HH:MM"` window during which notifications
    /// from this source are dropped.
    #[serde(default)]
    pub quiet_hours: Option<String>,
}

/// Resolved view of a source's routing rules. Built once at config
/// load time so the per-dispatch hot path doesn't reparse the
/// channel names / quiet_hours string.
#[derive(Debug, Clone, Default)]
pub struct ResolvedSource {
    /// Channels resolved from `route` — unknown names dropped.
    pub channels: Vec<Channel>,
    /// Severity floor.
    pub min_severity: Severity,
    /// Parsed quiet-hours window, if any.
    pub quiet_hours: Option<QuietHours>,
}

/// `[channels.discord]` block.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiscordChannel {
    /// Webhook URL. Empty/missing → transport surfaces
    /// `NotConfigured` at send time.
    #[serde(default)]
    pub webhook_url: String,
}

/// `[channels.telegram]` block.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TelegramChannel {
    /// Telegram bot token.
    #[serde(default)]
    pub bot_token: String,
    /// Authorised chat id.
    #[serde(default)]
    pub chat_id: String,
}

/// `[channels.email]` block. Identical fields to [`SmtpConfig`] but
/// re-deserialised here so the schema is self-contained in
/// `notifications.toml`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EmailChannel {
    /// SMTP hostname.
    #[serde(default)]
    pub host: String,
    /// SMTP submission port.
    #[serde(default)]
    pub port: u16,
    /// Username for SMTP AUTH.
    #[serde(default)]
    pub username: String,
    /// Password for SMTP AUTH.
    #[serde(default)]
    pub password: String,
    /// `From:` envelope address.
    #[serde(default)]
    pub from: String,
    /// `To:` envelope address.
    #[serde(default)]
    pub to: String,
    /// Subject template — see [`SmtpConfig::subject_template`].
    #[serde(default)]
    pub subject_template: String,
}

impl EmailChannel {
    /// Convert to the lower-level [`SmtpConfig`] used by the SMTP
    /// transport. The two shapes are identical today; the
    /// indirection lets the transport stay agnostic of the config
    /// file format.
    #[must_use]
    pub fn to_smtp_config(&self) -> SmtpConfig {
        SmtpConfig {
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            password: self.password.clone(),
            from: self.from.clone(),
            to: self.to.clone(),
            subject_template: self.subject_template.clone(),
        }
    }
}

/// All `[channels.*]` blocks rolled up.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelsConfig {
    /// Discord webhook config.
    #[serde(default)]
    pub discord: DiscordChannel,
    /// Telegram bot config.
    #[serde(default)]
    pub telegram: TelegramChannel,
    /// SMTP email config.
    #[serde(default)]
    pub email: EmailChannel,
}

/// `[inbox]` block. Reserved for BL-136 (Notification Center). The
/// router doesn't read these fields today; they're parsed so the
/// schema is forward-compatible.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InboxConfig {
    /// Maximum row cap before rotation.
    #[serde(default)]
    pub max_rows: Option<u32>,
    /// Maximum age in days before rotation.
    #[serde(default)]
    pub max_age_days: Option<u32>,
}

/// Top-level `notifications.toml` shape.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationsConfig {
    /// One entry per `[sources.<name>]` block.
    #[serde(default)]
    pub sources: BTreeMap<String, SourceConfig>,
    /// Per-channel transport credentials.
    #[serde(default)]
    pub channels: ChannelsConfig,
    /// Inbox knobs (BL-136 forward-compat).
    #[serde(default)]
    pub inbox: InboxConfig,
}

/// Parser/loader errors.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Filesystem error reading the file.
    #[error("read notifications.toml: {0}")]
    Read(#[from] std::io::Error),
    /// TOML parse error.
    #[error("parse notifications.toml: {0}")]
    Parse(#[from] toml::de::Error),
    /// Malformed `quiet_hours` field.
    #[error("invalid quiet_hours '{0}': expected 'HH:MM-HH:MM'")]
    QuietHours(String),
}

impl NotificationsConfig {
    /// Load + parse `<forge>/.forge/notifications.toml`. Returns the
    /// default (empty sources/channels) shape when the file is
    /// absent. Parse failures surface so the bootstrap can choose
    /// whether to fall back to the legacy `config.toml` blocks.
    ///
    /// # Errors
    /// - [`ConfigError::Read`] on filesystem errors other than `NotFound`.
    /// - [`ConfigError::Parse`] on TOML decode failure.
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(ConfigError::Read(err)),
        };
        Self::parse(&text)
    }

    /// Parse a literal `notifications.toml` body. Public for unit
    /// tests; production callers route through [`Self::load_from`].
    ///
    /// # Errors
    /// - [`ConfigError::Parse`] on TOML decode failure.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let cfg: Self = toml::from_str(text)?;
        Ok(cfg)
    }

    /// Build a name→[`ResolvedSource`] map ready for the router. Any
    /// channel name that doesn't map to a [`Channel`] variant is
    /// logged + dropped; an empty `route` after that filter is kept
    /// so the source stays in the map (the router treats it as "drop
    /// every event from this source").
    ///
    /// # Errors
    /// - [`ConfigError::QuietHours`] on malformed `quiet_hours`.
    pub fn resolve_sources(&self) -> Result<BTreeMap<String, ResolvedSource>, ConfigError> {
        let mut out = BTreeMap::new();
        for (name, src) in &self.sources {
            let mut channels = Vec::with_capacity(src.route.len());
            for raw in &src.route {
                match channel_from_str(raw) {
                    Some(c) => channels.push(c),
                    None => tracing::warn!(
                        source = %name,
                        channel = %raw,
                        "notifications.toml: unknown channel name in [sources.{name}].route — dropped"
                    ),
                }
            }
            let quiet_hours = match src.quiet_hours.as_deref() {
                Some(s) if !s.trim().is_empty() => Some(QuietHours::parse(s)?),
                _ => None,
            };
            out.insert(
                name.clone(),
                ResolvedSource {
                    channels,
                    min_severity: src.min_severity,
                    quiet_hours,
                },
            );
        }
        Ok(out)
    }
}

/// Map a wire channel name (the snake_case form of [`Channel`]) to
/// the enum. Returns `None` on unknown names.
#[must_use]
pub fn channel_from_str(s: &str) -> Option<Channel> {
    match s {
        "desktop" => Some(Channel::Desktop),
        "discord" => Some(Channel::Discord),
        "telegram" => Some(Channel::Telegram),
        "email" => Some(Channel::Email),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_empty_toml() {
        let cfg = NotificationsConfig::parse("").unwrap();
        assert!(cfg.sources.is_empty());
        assert!(cfg.channels.discord.webhook_url.is_empty());
    }

    #[test]
    fn parses_full_schema() {
        let text = r#"
[sources.workflow]
on = ["com.nexus.workflow.run_completed"]
route = ["desktop", "discord"]
min_severity = "warn"
quiet_hours = "22:00-08:00"

[sources.ai_runtime]
route = ["desktop"]

[channels.discord]
webhook_url = "https://discord.example/webhook"

[channels.telegram]
bot_token = "bot:token"
chat_id = "12345"

[channels.email]
host = "smtp.example.com"
port = 587
from = "nexus@example.com"
to = "ops@example.com"
subject_template = "Nexus: {title}"

[inbox]
max_rows = 1000
max_age_days = 30
"#;
        let cfg = NotificationsConfig::parse(text).unwrap();
        assert_eq!(cfg.sources.len(), 2);
        let wf = &cfg.sources["workflow"];
        assert_eq!(wf.route, vec!["desktop", "discord"]);
        assert_eq!(wf.min_severity, Severity::Warn);
        assert_eq!(wf.quiet_hours.as_deref(), Some("22:00-08:00"));
        assert_eq!(cfg.channels.discord.webhook_url, "https://discord.example/webhook");
        assert_eq!(cfg.channels.email.port, 587);
        assert_eq!(cfg.inbox.max_rows, Some(1000));
    }

    #[test]
    fn rejects_unknown_top_level_field() {
        let text = r#"
[unknown_section]
foo = 1
"#;
        let err = NotificationsConfig::parse(text).unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn resolve_sources_drops_unknown_channels() {
        let text = r#"
[sources.workflow]
route = ["desktop", "carrier_pigeon"]
"#;
        let cfg = NotificationsConfig::parse(text).unwrap();
        let resolved = cfg.resolve_sources().unwrap();
        let wf = &resolved["workflow"];
        assert_eq!(wf.channels, vec![Channel::Desktop]);
    }

    #[test]
    fn resolve_sources_parses_quiet_hours() {
        let text = r#"
[sources.workflow]
route = ["desktop"]
quiet_hours = "22:00-08:00"
"#;
        let cfg = NotificationsConfig::parse(text).unwrap();
        let resolved = cfg.resolve_sources().unwrap();
        let qh = resolved["workflow"].quiet_hours.unwrap();
        assert_eq!(qh.start_min, 22 * 60);
        assert_eq!(qh.end_min, 8 * 60);
    }

    #[test]
    fn resolve_sources_rejects_malformed_quiet_hours() {
        let text = r#"
[sources.workflow]
route = ["desktop"]
quiet_hours = "nope"
"#;
        let cfg = NotificationsConfig::parse(text).unwrap();
        let err = cfg.resolve_sources().unwrap_err();
        assert!(matches!(err, ConfigError::QuietHours(_)));
    }

    #[test]
    fn quiet_hours_overnight_window() {
        let qh = QuietHours::parse("22:00-08:00").unwrap();
        assert!(qh.contains(22 * 60));       // 22:00 — inside
        assert!(qh.contains(23 * 60 + 30));  // 23:30 — inside
        assert!(qh.contains(0));             // 00:00 — inside
        assert!(qh.contains(7 * 60 + 59));   // 07:59 — inside
        assert!(!qh.contains(8 * 60));       // 08:00 — outside
        assert!(!qh.contains(12 * 60));      // 12:00 — outside
        assert!(!qh.contains(21 * 60));      // 21:00 — outside
    }

    #[test]
    fn quiet_hours_daytime_window() {
        let qh = QuietHours::parse("09:00-17:00").unwrap();
        assert!(!qh.contains(8 * 60));
        assert!(qh.contains(9 * 60));
        assert!(qh.contains(12 * 60));
        assert!(!qh.contains(17 * 60));
        assert!(!qh.contains(20 * 60));
    }

    #[test]
    fn severity_ordering_matches_filter_intent() {
        assert!(Severity::Debug < Severity::Info);
        assert!(Severity::Info < Severity::Warn);
        assert!(Severity::Warn < Severity::Error);
    }

    #[test]
    fn load_from_missing_file_returns_default() {
        let cfg = NotificationsConfig::load_from(std::path::Path::new(
            "/this/path/definitely/does/not/exist/notifications.toml",
        ))
        .unwrap();
        assert!(cfg.sources.is_empty());
    }

    #[test]
    fn channel_from_str_known_names() {
        assert_eq!(channel_from_str("desktop"), Some(Channel::Desktop));
        assert_eq!(channel_from_str("discord"), Some(Channel::Discord));
        assert_eq!(channel_from_str("telegram"), Some(Channel::Telegram));
        assert_eq!(channel_from_str("email"), Some(Channel::Email));
        assert_eq!(channel_from_str("slack"), None);
    }

    #[test]
    fn email_channel_translates_to_smtp_config() {
        let e = EmailChannel {
            host: "smtp.example.com".into(),
            port: 587,
            from: "a@b".into(),
            to: "c@d".into(),
            subject_template: "Nexus: {title}".into(),
            username: "u".into(),
            password: "p".into(),
        };
        let smtp = e.to_smtp_config();
        assert_eq!(smtp.host, "smtp.example.com");
        assert_eq!(smtp.port, 587);
        assert_eq!(smtp.subject_template, "Nexus: {title}");
    }
}
