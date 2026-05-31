//! BL-143 Phase 1.4 — `nexus collab serve|join|token` CLI verbs.
//!
//! - `serve --port <p> [--token <t>]` boots a [`nexus_collab::RelayServer`]
//!   on `127.0.0.1:<p>` and blocks until SIGINT.
//! - `join <ws-url> [--token <t>] [--peer-id <id>] [--display-name <n>]
//!   [--save-token]` connects a [`nexus_collab::CollabClient`] to the
//!   given relay, bridges the local kernel `EventBus`, and blocks
//!   until SIGINT.
//! - `token set <value>` / `token clear` manage the keyring-stored
//!   shared secret for the current forge.
//!
//! Both `serve` and `join` use the `tokio::signal::ctrl_c` future for
//! shutdown so they're well-behaved under interactive terminal use.
//! Tokens flow through [`nexus_security::CredentialVault`] when
//! `--save-token` is passed (store) or when neither `--token` nor a
//! `?token=` URL query is provided (retrieve).

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use nexus_bootstrap::{build_cli_runtime, Runtime};
use nexus_collab::{
    parse_ws_url, ConnectParams, ReconnectConfig, ReconnectingClient, RelayServer, Token,
    COLLAB_TOPIC_PREFIX, OPS_TOPIC_PREFIX,
};
use nexus_kernel::EventFilter;
use nexus_security::CredentialVault;
use tokio::net::TcpListener;
use tokio::signal;

use crate::app::App;

/// Keyring entry name under which `nexus collab` stores the shared
/// secret. Single entry per host — Phase 1.4 assumes one relay per
/// user; multi-relay credential storage is a deferred polish.
const KEYRING_TOKEN_NAME: &str = "nexus.collab.token";

/// Default bind port for `nexus collab serve`. Matches the example URL
/// the BL-143 spec uses and keeps the verb invokable with no args.
pub const DEFAULT_SERVE_PORT: u16 = 7700;

/// P2-05 — default interface for `nexus collab serve`. `0.0.0.0`
/// listens on every IPv4 interface, matching the original BL-143
/// behaviour. Override with `--bind 127.0.0.1` (loopback only) or
/// `--bind <ip>` to constrain access.
pub const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0";

/// Default `peer_id` falls back to `$USER` or `nexus-cli` if the
/// environment variable is missing.
fn default_peer_id() -> String {
    std::env::var("USER").unwrap_or_else(|_| "nexus-cli".into())
}

/// Default display name reuses [`default_peer_id`] capitalised.
fn default_display_name() -> String {
    let id = default_peer_id();
    let mut chars = id.chars();
    chars
        .next()
        .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
        .unwrap_or_else(|| "Nexus".into())
}

/// Resolve the shared secret. Precedence: explicit `--token` > URL
/// `?token=` > keyring entry. Returns an actionable error when none
/// produces a non-empty value.
fn resolve_token(
    explicit: Option<&str>,
    url_token: Option<&str>,
    vault: &CredentialVault,
) -> Result<String> {
    if let Some(t) = explicit {
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    if let Some(t) = url_token {
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    match vault.retrieve(KEYRING_TOKEN_NAME) {
        Ok(t) if !t.is_empty() => Ok(t),
        _ => Err(anyhow!(
            "no token provided. Pass --token, embed ?token=… in the URL, or run `nexus collab token set <value>` first."
        )),
    }
}

// ---------------------------------------------------------------------------
// serve
// ---------------------------------------------------------------------------

/// `nexus collab serve --port <p> [--token <t>] [--save-token]`.
///
/// # Errors
/// Returns an error if the listener cannot bind, the token cannot be
/// resolved, or the relay accept loop fails.
pub fn serve(port: u16, bind_address: &str, token: Option<String>, save_token: bool) -> Result<()> {
    let vault = CredentialVault::new();
    let secret = resolve_token(token.as_deref(), None, &vault)?;
    if save_token {
        store_token(&vault, &secret)?;
    }
    let token = Token::new(secret).context("token must not be empty")?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .max_blocking_threads(nexus_types::constants::KERNEL_BLOCKING_POOL_SIZE)
        .enable_all()
        .build()
        .context("build tokio runtime")?;
    rt.block_on(async move {
        let server = Arc::new(RelayServer::new(token));
        let bind = format!("{bind_address}:{port}");
        let listener = TcpListener::bind(&bind)
            .await
            .with_context(|| format!("bind {bind}"))?;
        let addr = listener.local_addr().context("query listener local_addr")?;
        eprintln!("nexus collab serve: listening on ws://{addr}/");
        eprintln!("nexus collab serve: peers join with `nexus collab join ws://{addr}/`");
        eprintln!("nexus collab serve: Ctrl+C to stop");

        tokio::select! {
            biased;
            _ = signal::ctrl_c() => {
                eprintln!("nexus collab serve: SIGINT received, shutting down");
                Ok::<(), anyhow::Error>(())
            }
            res = server.serve_listener(listener) => {
                res.map_err(|e| anyhow!("relay accept loop failed: {e}"))
            }
        }
    })
}

// ---------------------------------------------------------------------------
// join
// ---------------------------------------------------------------------------

/// `nexus collab join <ws-url> [--token <t>] [--peer-id <id>]
///  [--display-name <name>] [--save-token]`.
///
/// Builds a CLI runtime (so the bus has the editor/CrdtPublisher/etc.
/// wired) then connects a `CollabClient` and blocks until SIGINT.
///
/// # Errors
/// Returns an error if the URL is malformed, the runtime fails to
/// build, the token cannot be resolved, or the WebSocket handshake
/// fails.
pub fn join(
    app: &App,
    url: &str,
    token: Option<String>,
    peer_id: Option<String>,
    display_name: Option<String>,
    save_token: bool,
) -> Result<()> {
    let endpoint =
        parse_ws_url(url).ok_or_else(|| anyhow!("expected ws://host:port[?token=…]; got {url}"))?;
    let vault = CredentialVault::new();
    let secret = resolve_token(token.as_deref(), endpoint.token.as_deref(), &vault)?;
    if save_token {
        store_token(&vault, &secret)?;
    }
    let peer_id = peer_id.unwrap_or_else(default_peer_id);
    let display_name = display_name.unwrap_or_else(default_display_name);

    let forge_root = app.forge_root().to_path_buf();
    let runtime = build_cli_runtime(forge_root.clone())
        .with_context(|| format!("failed to build runtime at {}", forge_root.display()))?;
    let Runtime {
        kernel,
        context: _context,
        loader: _loader,
    } = runtime;
    let bus = kernel.event_bus();

    let params = ConnectParams {
        host: endpoint.host.clone(),
        port: endpoint.port,
        url: endpoint.url.clone(),
        token: secret,
        peer_id: peer_id.clone(),
        display_name,
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .max_blocking_threads(nexus_types::constants::KERNEL_BLOCKING_POOL_SIZE)
        .enable_all()
        .build()
        .context("build tokio runtime")?;
    rt.block_on(async move {
        let client = ReconnectingClient::start(
            params,
            bus,
            vec![
                EventFilter::CustomPrefix(OPS_TOPIC_PREFIX.to_string()),
                EventFilter::CustomPrefix(COLLAB_TOPIC_PREFIX.to_string()),
            ],
            None,
            ReconnectConfig::default(),
        );
        eprintln!("nexus collab join: bridge online (auto-reconnect enabled). Ctrl+C to leave.");
        signal::ctrl_c().await.context("wait for ctrl_c")?;
        eprintln!("nexus collab join: SIGINT received, leaving relay");
        client.shutdown().await;
        Ok::<(), anyhow::Error>(())
    })?;
    // Hold the kernel alive for the duration of the bridge so plugins
    // shut down cleanly on Drop.
    drop(kernel);
    Ok(())
}

// ---------------------------------------------------------------------------
// token
// ---------------------------------------------------------------------------

/// `nexus collab token set <value>`.
///
/// # Errors
/// Returns an error if the OS keyring is unavailable or rejects the
/// write.
pub fn token_set(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(anyhow!("token must not be empty"));
    }
    store_token(&CredentialVault::new(), value)?;
    eprintln!("nexus collab token: stored under '{KEYRING_TOKEN_NAME}'");
    Ok(())
}

/// `nexus collab token clear`.
///
/// # Errors
/// Returns an error if the OS keyring is unavailable. A missing entry
/// is *not* an error — the post-condition (no stored token) holds.
pub fn token_clear() -> Result<()> {
    let vault = CredentialVault::new();
    match vault.delete(KEYRING_TOKEN_NAME) {
        Ok(()) => eprintln!("nexus collab token: cleared"),
        Err(err) => {
            // The keyring crate returns NoEntry when the entry is
            // already absent — treat that as success.
            let s = err.to_string();
            if s.contains("NoEntry") || s.contains("not found") {
                eprintln!("nexus collab token: no stored token");
            } else {
                return Err(anyhow!("keyring delete failed: {err}"));
            }
        }
    }
    Ok(())
}

fn store_token(vault: &CredentialVault, value: &str) -> Result<()> {
    vault
        .store(KEYRING_TOKEN_NAME, value)
        .map_err(|e| anyhow!("keyring store failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_token_prefers_explicit() {
        let vault = CredentialVault::disabled();
        let t = resolve_token(Some("explicit"), Some("from_url"), &vault).unwrap();
        assert_eq!(t, "explicit");
    }

    #[test]
    fn resolve_token_falls_back_to_url_when_explicit_missing() {
        let vault = CredentialVault::disabled();
        let t = resolve_token(None, Some("from_url"), &vault).unwrap();
        assert_eq!(t, "from_url");
    }

    #[test]
    fn resolve_token_ignores_blank_explicit() {
        let vault = CredentialVault::disabled();
        let t = resolve_token(Some(""), Some("from_url"), &vault).unwrap();
        assert_eq!(t, "from_url");
    }

    #[test]
    fn resolve_token_errors_when_no_source_provides_value() {
        let vault = CredentialVault::disabled();
        let err = resolve_token(None, None, &vault).unwrap_err();
        assert!(err.to_string().contains("token"));
    }

    #[test]
    fn default_peer_id_is_non_empty() {
        assert!(!default_peer_id().is_empty());
        assert!(!default_display_name().is_empty());
    }
}
