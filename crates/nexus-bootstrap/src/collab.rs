//! BL-143 Phase 1.2 — optional collab-client wiring.
//!
//! Reads a `[collab]` block from `<forge>/.forge/config.toml`. When
//! present and `enabled = true`, spawns a [`nexus_collab::CollabClient`]
//! bridging the local kernel [`EventBus`] to a remote relay over
//! WebSocket. The bridge ships every `com.nexus.editor.ops.*` event
//! to the relay and republishes inbound envelopes back onto the bus.
//!
//! Wiring policy
//! -------------
//!
//! * Spawn requires an ambient tokio runtime (the CLI/TUI/shell
//!   invokers all run under `#[tokio::main]`). When called from a
//!   non-async context (e.g. a tool that exits before any IPC happens),
//!   we log at debug and skip — same fall-back the BL-136 notifications
//!   subscriber uses.
//! * Failures during the initial connection are logged at warn and
//!   non-fatal. The runtime keeps booting; reconnect resilience is the
//!   BL-143 Phase 1.5 follow-up.
//! * Site-based self-echo dedup is left at `None` for now: the relay's
//!   own peer-id echo suppression covers the obvious loop, and threading
//!   the [`nexus_bootstrap::crdt_publisher::CrdtPublisher`]'s `site()`
//!   through the editor-plugin registration belongs to the same
//!   follow-up as exposing it for tests.
//!
//! Config shape
//! ------------
//!
//! ```toml
//! [collab]
//! enabled = true
//! relay_url = "ws://127.0.0.1:7700/"
//! token = "shared-secret"
//! peer_id = "alice@laptop"
//! display_name = "Alice"
//! ```
//!
//! All fields except `enabled` are required when `enabled = true`;
//! missing fields surface a warn log and the spawn is skipped.

use std::path::Path;
use std::sync::Arc;

use nexus_collab::{CollabClient, CollabClientConfig, ConnectParams};
use nexus_kernel::EventBus;
use serde::Deserialize;
use tokio::task::JoinHandle;

/// On-disk shape for `[collab]` in `.forge/config.toml`. Mirrored
/// from [`CollabClient::connect`]'s argument list plus an `enabled`
/// toggle. All connection fields are optional in the struct so a
/// partially-filled block doesn't error during parse; the `enabled`
/// check + per-field empty-string guard surface the actionable warning
/// instead.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct CollabConfig {
    /// Master switch. Defaults to `false`.
    pub enabled: bool,
    /// `ws://host:port/` URL of the relay.
    pub relay_url: String,
    /// Shared secret matching the relay's token.
    pub token: String,
    /// Caller-chosen peer identifier (must be unique on the relay).
    pub peer_id: String,
    /// Human-readable name for the peers panel.
    pub display_name: String,
}

impl CollabConfig {
    fn fields_complete(&self) -> bool {
        !self.relay_url.is_empty()
            && !self.token.is_empty()
            && !self.peer_id.is_empty()
            && !self.display_name.is_empty()
    }
}

/// Parse a `host`, `port`, and `url` from `ws://host:port[/...]`. We
/// don't pull a full URL parser dep for this one job; the relay-side
/// requirement is just `Host` header presence, which any string works
/// for. Returns `None` if the URL doesn't start with `ws://` or the
/// authority chunk doesn't contain a port — anything else (paths,
/// queries) is forwarded verbatim as the URL the WS handshake announces.
fn parse_ws_url(url: &str) -> Option<(String, u16)> {
    let rest = url.strip_prefix("ws://")?;
    // Split off path/query — anything after the first `/` or `?`.
    let authority = rest.split_once(['/', '?']).map_or(rest, |(a, _)| a);
    let (host, port_str) = authority.rsplit_once(':')?;
    let port: u16 = port_str.parse().ok()?;
    if host.is_empty() {
        return None;
    }
    Some((host.to_string(), port))
}

/// Read `<forge>/.forge/config.toml` and return the `[collab]` block.
/// Missing file / missing block / parse errors all collapse to
/// `CollabConfig::default()` (disabled).
fn load_config(forge_root: &Path) -> CollabConfig {
    #[derive(Deserialize)]
    struct Wrapper {
        #[serde(default)]
        collab: Option<CollabConfig>,
    }
    let path = forge_root.join(".forge").join("config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return CollabConfig::default();
        }
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "config.toml: read failed; [collab] disabled");
            return CollabConfig::default();
        }
    };
    match toml::from_str::<Wrapper>(&text) {
        Ok(w) => w.collab.unwrap_or_default(),
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "config.toml: [collab] failed to parse; disabled");
            CollabConfig::default()
        }
    }
}

/// Spawn the collab bridge if `[collab].enabled = true` in
/// `.forge/config.toml`. Returns the task handle so the caller can hold
/// it to keep the bridge alive (drop = best-effort abort). Returns
/// `None` when:
///
/// * `[collab]` is absent or `enabled = false`
/// * one or more required fields are missing
/// * `relay_url` doesn't parse as `ws://host:port[/...]`
/// * no ambient tokio runtime is reachable via
///   [`tokio::runtime::Handle::try_current`]
/// * the initial `CollabClient::connect` fails (logged at warn)
///
/// In every skip case the runtime continues booting normally —
/// collaboration is opt-in and never fatal.
pub fn start_if_enabled(forge_root: &Path, bus: Arc<EventBus>) -> Option<JoinHandle<()>> {
    let cfg = load_config(forge_root);
    if !cfg.enabled {
        return None;
    }
    if !cfg.fields_complete() {
        tracing::warn!("config.toml: [collab].enabled but required fields missing; skipping");
        return None;
    }
    let Some((host, port)) = parse_ws_url(&cfg.relay_url) else {
        tracing::warn!(
            relay_url = %cfg.relay_url,
            "config.toml: [collab].relay_url must be ws://host:port[/...]; skipping"
        );
        return None;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        tracing::debug!(
            "no ambient tokio runtime; skipping BL-143 collab spawn (CLI single-shot)"
        );
        return None;
    };
    let task = handle.spawn(async move {
        let params = ConnectParams {
            host,
            port,
            url: cfg.relay_url.clone(),
            token: cfg.token,
            peer_id: cfg.peer_id.clone(),
            display_name: cfg.display_name,
        };
        let client = match CollabClient::connect(params, bus, CollabClientConfig::default()).await
        {
            Ok(c) => c,
            Err(err) => {
                tracing::warn!(
                    %err,
                    relay_url = %cfg.relay_url,
                    "BL-143 collab connect failed; relay-bridge disabled this session"
                );
                return;
            }
        };
        tracing::info!(
            peer_id = %client.peer_id(),
            relay_url = %cfg.relay_url,
            initial_peers = client.initial_peers().len(),
            "BL-143 collab bridge online"
        );
        // The connect future returns immediately; the bridge tasks run
        // independently inside the client. We keep this future alive
        // (without busy-spinning) so the client is dropped only when
        // the spawned task itself is cancelled — that ties the
        // bridge's lifetime to the JoinHandle the caller holds.
        let () = std::future::pending().await;
        drop(client);
    });
    Some(task)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ws_url_accepts_host_port() {
        assert_eq!(parse_ws_url("ws://127.0.0.1:7700/"), Some(("127.0.0.1".into(), 7700)));
        assert_eq!(parse_ws_url("ws://relay.example.com:9000"), Some(("relay.example.com".into(), 9000)));
        assert_eq!(parse_ws_url("ws://h:1/path?q=1"), Some(("h".into(), 1)));
    }

    #[test]
    fn parse_ws_url_rejects_missing_port() {
        assert!(parse_ws_url("ws://example.com/").is_none());
    }

    #[test]
    fn parse_ws_url_rejects_wrong_scheme() {
        assert!(parse_ws_url("http://example.com:80/").is_none());
        assert!(parse_ws_url("wss://example.com:443/").is_none());
    }

    #[test]
    fn parse_ws_url_rejects_empty_host() {
        assert!(parse_ws_url("ws://:7700/").is_none());
    }

    #[test]
    fn load_config_returns_default_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = load_config(dir.path());
        assert!(!cfg.enabled);
    }

    #[test]
    fn load_config_parses_a_complete_block() {
        let dir = tempfile::tempdir().unwrap();
        let forge = dir.path().join(".forge");
        std::fs::create_dir_all(&forge).unwrap();
        std::fs::write(
            forge.join("config.toml"),
            r#"
[collab]
enabled = true
relay_url = "ws://relay:7700/"
token = "secret"
peer_id = "alice"
display_name = "Alice"
            "#,
        )
        .unwrap();
        let cfg = load_config(dir.path());
        assert!(cfg.enabled);
        assert_eq!(cfg.relay_url, "ws://relay:7700/");
        assert_eq!(cfg.peer_id, "alice");
        assert!(cfg.fields_complete());
    }

    #[test]
    fn fields_complete_rejects_blanks() {
        let cfg = CollabConfig {
            enabled: true,
            relay_url: "ws://h:1/".into(),
            token: "t".into(),
            peer_id: String::new(),
            display_name: "x".into(),
        };
        assert!(!cfg.fields_complete());
    }

    #[test]
    fn start_if_enabled_skips_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let bus = Arc::new(EventBus::new(8));
        let h = start_if_enabled(dir.path(), bus);
        assert!(h.is_none(), "no config means no spawn");
    }

    #[tokio::test]
    async fn start_if_enabled_skips_when_fields_blank() {
        let dir = tempfile::tempdir().unwrap();
        let forge = dir.path().join(".forge");
        std::fs::create_dir_all(&forge).unwrap();
        std::fs::write(
            forge.join("config.toml"),
            r#"
[collab]
enabled = true
relay_url = "ws://h:1/"
            "#,
        )
        .unwrap();
        let bus = Arc::new(EventBus::new(8));
        let h = start_if_enabled(dir.path(), bus);
        assert!(h.is_none(), "missing fields means no spawn");
    }
}
