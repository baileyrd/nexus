//! BL-133 — `nexus notify send` command.
//!
//! Thin wrapper over `com.nexus.notifications::send`. Validates the
//! channel string locally before dispatching so a typo doesn't
//! produce a confusing serde error in the response.

use std::time::Duration;

use anyhow::{Context, Result};
use nexus_kernel::PluginContext;
use serde_json::Value;

use crate::app::App;

const NOTIFICATIONS_PLUGIN: &str = "com.nexus.notifications";
const IPC_TIMEOUT: Duration = Duration::from_secs(15);

/// Send a notification through `com.nexus.notifications::send`.
pub fn send(app: &mut App, channel: &str, message: &str, title: Option<&str>) -> Result<()> {
    let channel = validate_channel(channel)?;
    let mut args = serde_json::json!({
        "channel": channel,
        "message": message,
    });
    if let Some(t) = title {
        if let Some(map) = args.as_object_mut() {
            map.insert("title".into(), Value::String(t.to_string()));
        }
    }
    let (runtime, rt) = app.runtime()?;
    let response = rt
        .block_on(
            runtime
                .context
                .ipc_call(NOTIFICATIONS_PLUGIN, "send", args, IPC_TIMEOUT),
        )
        .with_context(|| "notifications ipc call 'send' failed")?;
    let delivered = response
        .get("delivered")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if delivered {
        println!("delivered to {channel}");
    } else {
        println!("not delivered (channel {channel}): {response}");
    }
    Ok(())
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
}
