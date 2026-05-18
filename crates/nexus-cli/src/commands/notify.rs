//! BL-133 / BL-135 — `nexus notify send` command.
//!
//! Thin wrapper over `com.nexus.notifications::send`. Validates the
//! channel string locally before dispatching so a typo doesn't
//! produce a confusing serde error in the response.
//!
//! Two modes:
//! - `--channel <c>`: override path. Routes to that transport
//!   directly, bypassing the BL-135 router.
//! - `--source <s>`: router path. The notifications plugin consults
//!   `<forge>/.forge/notifications.toml` to pick channels.
//!
//! When the caller supplies neither, the CLI defaults to
//! `--source cli` so a bare `nexus notify send "msg"` invocation
//! routes through the canonical `[sources.cli]` block.

use anyhow::{Context, Result};
use nexus_types::constants::IPC_TIMEOUT_SHORT as IPC_TIMEOUT;
use nexus_types::plugin_ids;
use serde_json::Value;

use crate::app::App;

const NOTIFICATIONS_PLUGIN: &str = plugin_ids::NOTIFICATIONS;

/// Default source tag used when the caller supplies neither
/// `--channel` nor `--source`. Pinned so the `[sources.cli]` block
/// in `notifications.toml` is the single configuration surface.
pub const DEFAULT_SOURCE: &str = "cli";

/// Send a notification through `com.nexus.notifications::send`.
///
/// Exactly one of `channel` or `source` should be supplied. When both
/// are `None` the function falls back to [`DEFAULT_SOURCE`].
pub fn send(
    app: &mut App,
    channel: Option<&str>,
    source: Option<&str>,
    severity: Option<&str>,
    message: &str,
    title: Option<&str>,
) -> Result<()> {
    let mut args = serde_json::json!({ "message": message });
    let obj = args.as_object_mut().expect("json object");
    if let Some(ch) = channel {
        let canonical = validate_channel(ch)?;
        obj.insert("channel".into(), Value::String(canonical.to_string()));
    } else {
        let s = source.unwrap_or(DEFAULT_SOURCE);
        if s.is_empty() {
            anyhow::bail!("notify: --source cannot be empty");
        }
        obj.insert("source".into(), Value::String(s.to_string()));
    }
    if let Some(sev) = severity {
        validate_severity(sev)?;
        obj.insert("severity".into(), Value::String(sev.to_string()));
    }
    if let Some(t) = title {
        obj.insert("title".into(), Value::String(t.to_string()));
    }
    let (invoker, rt) = app.invoker()?;
    let response = rt
        .block_on(invoker.ipc_call(NOTIFICATIONS_PLUGIN, "send", args, IPC_TIMEOUT))
        .with_context(|| "notifications ipc call 'send' failed")?;
    print_outcome(&response);
    Ok(())
}

fn print_outcome(response: &Value) {
    let delivered = response
        .get("delivered")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let routing = response
        .get("routing")
        .and_then(Value::as_str)
        .unwrap_or("?");
    let channels: Vec<String> = response
        .get("channels")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if delivered {
        if channels.is_empty() {
            println!("delivered ({routing})");
        } else {
            println!("delivered to {} ({routing})", channels.join(", "));
        }
    } else {
        println!("not delivered ({routing}): {response}");
    }
}

/// Map the user-facing channel string to the wire form expected by
/// the kernel handler. Keeping this local lets the CLI fail fast on
/// typos with a friendlier error than the server-side `serde`
/// rejection.
fn validate_channel(s: &str) -> Result<&'static str> {
    match s.to_ascii_lowercase().as_str() {
        "desktop" => Ok("desktop"),
        "discord" => Ok("discord"),
        "telegram" => Ok("telegram"),
        "email" => Ok("email"),
        other => Err(anyhow::anyhow!(
            "unknown channel '{other}': expected one of desktop / discord / telegram / email"
        )),
    }
}

fn validate_severity(s: &str) -> Result<()> {
    match s {
        "debug" | "info" | "warn" | "error" => Ok(()),
        other => Err(anyhow::anyhow!(
            "unknown severity '{other}': expected debug / info / warn / error"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_channel_accepts_known_names_case_insensitive() {
        assert_eq!(validate_channel("desktop").unwrap(), "desktop");
        assert_eq!(validate_channel("Desktop").unwrap(), "desktop");
        assert_eq!(validate_channel("DESKTOP").unwrap(), "desktop");
        assert_eq!(validate_channel("discord").unwrap(), "discord");
        assert_eq!(validate_channel("Discord").unwrap(), "discord");
        assert_eq!(validate_channel("telegram").unwrap(), "telegram");
        assert_eq!(validate_channel("Telegram").unwrap(), "telegram");
        assert_eq!(validate_channel("TELEGRAM").unwrap(), "telegram");
        assert_eq!(validate_channel("email").unwrap(), "email");
        assert_eq!(validate_channel("Email").unwrap(), "email");
        assert_eq!(validate_channel("EMAIL").unwrap(), "email");
    }

    #[test]
    fn validate_channel_rejects_unknown_names() {
        let err = validate_channel("slack").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("unknown channel"));
        assert!(msg.contains("slack"));
    }

    #[test]
    fn validate_severity_accepts_known_names() {
        assert!(validate_severity("debug").is_ok());
        assert!(validate_severity("info").is_ok());
        assert!(validate_severity("warn").is_ok());
        assert!(validate_severity("error").is_ok());
    }

    #[test]
    fn validate_severity_rejects_unknown_names() {
        let err = validate_severity("critical").unwrap_err();
        assert!(format!("{err}").contains("unknown severity"));
    }
}
