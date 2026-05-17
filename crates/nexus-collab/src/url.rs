//! BL-143 Phase 1.4 — minimal `ws://` URL parser shared between the
//! bootstrap config loader and the `nexus collab` CLI.
//!
//! We deliberately do not pull a full URL parser dep for this one job.
//! Relays speak `ws://host:port[/path][?token=…]`; the only fields the
//! callers care about are `host`, `port`, and the optional `token`
//! query parameter. Anything past the authority is preserved verbatim
//! in the returned `url` (the WebSocket client forwards it as the
//! `Host` / `Origin` header for routing).

/// Parsed components of a `ws://host:port[/path][?token=…]` URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WsEndpoint {
    /// Host portion of the authority. Non-empty.
    pub host: String,
    /// TCP port from the authority. Required; URLs lacking a port are
    /// rejected at parse time.
    pub port: u16,
    /// Token extracted from a `?token=…` query parameter, when
    /// present. Multi-value or multi-key query strings: only the
    /// first `token=` pair wins; the rest are ignored.
    pub token: Option<String>,
    /// The full URL as supplied, suitable for passing to
    /// `tokio_tungstenite::client_async_with_config` for the
    /// `Host`/`Origin` header.
    pub url: String,
}

/// Parse a `ws://host:port[/path][?token=…]` URL.
///
/// Returns `None` when the scheme is wrong, the authority has no port,
/// or the host portion is empty. Unknown / extra query parameters are
/// preserved in `url` but otherwise ignored.
#[must_use]
pub fn parse(url: &str) -> Option<WsEndpoint> {
    let rest = url.strip_prefix("ws://")?;
    // Authority ends at the first `/` or `?`; the remainder is the
    // path+query.
    let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let (host, port_str) = authority.rsplit_once(':')?;
    if host.is_empty() {
        return None;
    }
    let port: u16 = port_str.parse().ok()?;
    // Query string starts at the first `?` in the *whole* URL after
    // the scheme. Looking only past the authority keeps a literal `?`
    // inside (already-invalid) host/port portions from being misread.
    let token = rest[authority_end..]
        .find('?')
        .map(|i| &rest[authority_end + i + 1..])
        .and_then(extract_token);
    Some(WsEndpoint {
        host: host.to_string(),
        port,
        token,
        url: url.to_string(),
    })
}

/// Pick out the first `token=…` value from a URL-encoded query string.
/// `&` separates key=value pairs; `=` separates key and value. Pairs
/// without `=` are skipped (rather than aborting the whole scan, which
/// would have happened if we propagated the `?` through `split_once`).
fn extract_token(query: &str) -> Option<String> {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == "token" {
                return Some(v.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_url() {
        let p = parse("ws://127.0.0.1:7700/").unwrap();
        assert_eq!(p.host, "127.0.0.1");
        assert_eq!(p.port, 7700);
        assert_eq!(p.token, None);
    }

    #[test]
    fn parses_token_query() {
        let p = parse("ws://relay:7700/?token=hunter2").unwrap();
        assert_eq!(p.host, "relay");
        assert_eq!(p.port, 7700);
        assert_eq!(p.token.as_deref(), Some("hunter2"));
    }

    #[test]
    fn parses_token_without_path() {
        let p = parse("ws://relay:7700?token=hunter2").unwrap();
        assert_eq!(p.token.as_deref(), Some("hunter2"));
    }

    #[test]
    fn rejects_missing_port() {
        assert!(parse("ws://example.com/").is_none());
    }

    #[test]
    fn rejects_wrong_scheme() {
        assert!(parse("http://example.com:80/").is_none());
        assert!(parse("wss://example.com:443/").is_none());
    }

    #[test]
    fn rejects_empty_host() {
        assert!(parse("ws://:7700/").is_none());
    }

    #[test]
    fn ignores_unknown_query_keys() {
        let p = parse("ws://h:1/?other=x&token=t&extra=y").unwrap();
        assert_eq!(p.token.as_deref(), Some("t"));
    }

    #[test]
    fn token_absent_when_only_other_keys() {
        let p = parse("ws://h:1/?other=x").unwrap();
        assert!(p.token.is_none());
    }

    #[test]
    fn url_field_round_trips_input() {
        let raw = "ws://h:1/path?token=t";
        let p = parse(raw).unwrap();
        assert_eq!(p.url, raw);
    }
}
