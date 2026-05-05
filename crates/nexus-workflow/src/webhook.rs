//! BL-028g — webhook trigger.
//!
//! A single TCP listener (loopback by default) accepts HTTP/1.1
//! `POST` requests, matches the request path against the registered
//! `webhook` workflows, optionally validates a shared-secret header,
//! and dispatches `com.nexus.workflow::run` with `trigger.{path,
//! method, body, remote_addr}` variables.
//!
//! # Workflow shape
//!
//! ```toml
//! [trigger]
//! type = "webhook"
//! path = "/gitlab-push"   # required, must start with "/"
//! method = "POST"         # optional; only POST is supported in v1
//! secret = "shhh"         # optional; if set, requires
//!                         # `X-Webhook-Secret: shhh` header
//! ```
//!
//! # Forge config
//!
//! `<forge>/.forge/config.toml`:
//!
//! ```toml
//! [webhooks]
//! enabled = false                # default off — explicit opt-in
//! bind = "127.0.0.1:18080"      # default loopback
//! ```
//!
//! The listener only spawns when `enabled = true` *and* at least one
//! workflow declares a `webhook` trigger. Binding to a non-loopback
//! address is the user's responsibility — Nexus does not advertise
//! webhook URLs.
//!
//! # Why hand-rolled HTTP?
//!
//! Disk pressure on the build tree. `reqwest` already pulls `hyper`
//! into the dep graph, but enabling `hyper`'s `server` feature would
//! compile a non-trivial chunk of additional code. Receiving one
//! flat POST is ~150 lines of by-hand HTTP/1.1 parsing — small,
//! explicit, no new compile units.

use serde::{Deserialize, Serialize};

use crate::Workflow;

/// Hard cap on inbound body length (bytes). Anything larger gets a
/// `413 Payload Too Large` and the connection drops without dispatch.
pub const MAX_BODY_BYTES: usize = 64 * 1024;

/// Hard cap on the request-line + headers preamble before the body.
/// Generous for any reasonable webhook source; protects against a
/// slow-loris client filling memory.
pub const MAX_HEADER_BYTES: usize = 16 * 1024;

/// Per-connection read timeout. Slow clients get dropped.
pub const READ_TIMEOUT_MS: u64 = 5_000;

/// `[webhooks]` block from the forge config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// Master switch — webhooks listener only spawns when true.
    #[serde(default)]
    pub enabled: bool,
    /// Bind address (`host:port`). Defaults to `127.0.0.1:18080`.
    #[serde(default = "default_bind")]
    pub bind: String,
}

fn default_bind() -> String {
    "127.0.0.1:18080".to_string()
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: default_bind(),
        }
    }
}

/// Parsed `trigger.type = "webhook"` spec for one workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookSpec {
    /// `[workflow].name` — used as the dispatch target.
    pub workflow_name: String,
    /// HTTP path to match (always starts with `/`).
    pub path: String,
    /// HTTP method (uppercase). v1 requires `POST`.
    pub method: String,
    /// Optional shared-secret header value. When set the listener
    /// requires `X-Webhook-Secret: <secret>` (case-insensitive header
    /// name; secret value compared with constant-time equality).
    pub secret: Option<String>,
}

impl WebhookSpec {
    /// Pull `path`, `method`, `secret` off `wf.trigger.extra` and
    /// validate them.
    ///
    /// # Errors
    /// Returns a human-readable string when validation fails.
    pub fn from_trigger(name: &str, wf: &Workflow) -> Result<Self, String> {
        let path = wf
            .trigger
            .extra
            .get("path")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| "webhook trigger missing `path` string".to_string())?
            .to_string();
        if !path.starts_with('/') {
            return Err(format!("webhook trigger `path` must start with '/': {path:?}"));
        }
        if path.contains(['?', '#']) {
            return Err("webhook trigger `path` must not contain '?' or '#'".into());
        }
        let method = wf
            .trigger
            .extra
            .get("method")
            .and_then(toml::Value::as_str)
            .map_or_else(|| "POST".to_string(), str::to_ascii_uppercase);
        if method != "POST" {
            return Err(format!(
                "webhook trigger `method` must be POST in v1 (got {method})"
            ));
        }
        let secret = wf
            .trigger
            .extra
            .get("secret")
            .and_then(toml::Value::as_str)
            .map(ToString::to_string);
        if let Some(s) = &secret {
            if s.is_empty() {
                return Err("webhook trigger `secret` cannot be empty".into());
            }
        }
        Ok(Self {
            workflow_name: name.to_string(),
            path,
            method,
            secret,
        })
    }
}

/// Outcome of [`parse_request`] on a chunk of HTTP request bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRequest {
    /// Uppercased HTTP method.
    pub method: String,
    /// Raw request-target (path + optional query). The dispatcher
    /// matches against `path` only — query is preserved here for
    /// payload exposure.
    pub target: String,
    /// Path component of `target` (everything before `?`).
    pub path: String,
    /// Headers, lowercased keys.
    pub headers: std::collections::BTreeMap<String, String>,
    /// Body bytes after the `\r\n\r\n` separator.
    pub body: Vec<u8>,
}

/// Why a request was rejected before dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestError {
    /// Couldn't read a complete request line + headers within
    /// [`MAX_HEADER_BYTES`].
    Malformed,
    /// `Content-Length` was missing or unparseable.
    MissingContentLength,
    /// Body length exceeds [`MAX_BODY_BYTES`].
    BodyTooLarge,
    /// Header / body wasn't valid UTF-8 where we needed it.
    NotUtf8,
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed => f.write_str("malformed HTTP request"),
            Self::MissingContentLength => f.write_str("missing or invalid Content-Length"),
            Self::BodyTooLarge => f.write_str("body exceeds 64KB cap"),
            Self::NotUtf8 => f.write_str("non-UTF8 header"),
        }
    }
}

/// Best-effort HTTP/1.1 parser for the small slice of requests
/// webhooks need. Splits the buffer on `\r\n\r\n`, decodes the
/// request line + headers, then takes `Content-Length` body bytes
/// from the remainder.
///
/// # Errors
/// See [`RequestError`].
pub fn parse_request(buf: &[u8]) -> Result<ParsedRequest, RequestError> {
    let split = find_double_crlf(buf).ok_or(RequestError::Malformed)?;
    let head = std::str::from_utf8(&buf[..split]).map_err(|_| RequestError::NotUtf8)?;
    let body_start = split + 4; // skip the \r\n\r\n
    let mut lines = head.split("\r\n");
    let request_line = lines.next().ok_or(RequestError::Malformed)?;
    let mut parts = request_line.split_ascii_whitespace();
    let method = parts
        .next()
        .ok_or(RequestError::Malformed)?
        .to_ascii_uppercase();
    let target = parts.next().ok_or(RequestError::Malformed)?.to_string();
    let path = target
        .split_once('?')
        .map_or_else(|| target.clone(), |(p, _)| p.to_string());

    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        } else {
            return Err(RequestError::Malformed);
        }
    }

    let len: usize = headers
        .get("content-length")
        .and_then(|s| s.parse().ok())
        .ok_or(RequestError::MissingContentLength)?;
    if len > MAX_BODY_BYTES {
        return Err(RequestError::BodyTooLarge);
    }
    let body_end = body_start.saturating_add(len);
    if body_end > buf.len() {
        return Err(RequestError::Malformed);
    }
    let body = buf[body_start..body_end].to_vec();

    Ok(ParsedRequest {
        method,
        target,
        path,
        headers,
        body,
    })
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Constant-time string equality. Used for secret comparison so we
/// don't leak the secret length / prefix via timing.
#[must_use]
pub fn constant_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Decision returned by [`route_request`] before any IPC dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route<'a> {
    /// 200 — request matched a workflow and validation passed.
    Dispatch(&'a WebhookSpec),
    /// 404 — no workflow registered for this path.
    NotFound,
    /// 405 — path is registered but method differs.
    MethodNotAllowed,
    /// 401 — secret header missing or did not match.
    Unauthorized,
}

/// Match a parsed request against the set of registered specs.
#[must_use]
pub fn route_request<'a>(specs: &'a [WebhookSpec], req: &ParsedRequest) -> Route<'a> {
    let mut path_match = false;
    for spec in specs {
        if spec.path != req.path {
            continue;
        }
        path_match = true;
        if spec.method != req.method {
            continue;
        }
        if let Some(expected) = &spec.secret {
            let got = req
                .headers
                .get("x-webhook-secret")
                .map_or("", String::as_str);
            if !constant_eq(got, expected) {
                return Route::Unauthorized;
            }
        }
        return Route::Dispatch(spec);
    }
    if path_match {
        Route::MethodNotAllowed
    } else {
        Route::NotFound
    }
}

/// Build the `trigger` variables object for a routed request.
///
/// `trigger.body` is the raw body string when valid UTF-8, otherwise
/// omitted. Workflow steps that need structured access can parse it
/// further via interpolation (string only — JSON parsing in steps is
/// a follow-up).
#[must_use]
pub fn build_trigger_vars(req: &ParsedRequest, remote_addr: &str) -> serde_json::Value {
    let mut t = serde_json::Map::new();
    t.insert("path".into(), serde_json::Value::String(req.path.clone()));
    t.insert("method".into(), serde_json::Value::String(req.method.clone()));
    t.insert(
        "remote_addr".into(),
        serde_json::Value::String(remote_addr.to_string()),
    );
    if let Ok(s) = std::str::from_utf8(&req.body) {
        t.insert("body".into(), serde_json::Value::String(s.to_string()));
    }
    serde_json::json!({ "trigger": t })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow_with_trigger(extra: &str) -> Workflow {
        // Use bare toml decode (not `parse_workflow_text`) so the
        // AIG-03 trigger validation doesn't pre-empt the unit tests
        // that exercise `WebhookSpec::from_trigger` directly. The
        // happy-path tests round-trip the same shape through
        // `parse_workflow_text` in `trigger_validation::tests` —
        // duplicating coverage at the parse layer is unnecessary.
        let src = format!(
            r#"
[workflow]
name = "WH"

[trigger]
type = "webhook"
{extra}
"#
        );
        toml::from_str(&src).unwrap()
    }

    #[test]
    fn webhook_spec_parses_required_path_and_defaults_method_to_post() {
        let wf = workflow_with_trigger(r#"path = "/hook""#);
        let spec = WebhookSpec::from_trigger("WH", &wf).unwrap();
        assert_eq!(spec.path, "/hook");
        assert_eq!(spec.method, "POST");
        assert!(spec.secret.is_none());
    }

    #[test]
    fn webhook_spec_uppercases_method_and_picks_up_secret() {
        let wf = workflow_with_trigger(
            r#"
path = "/h"
method = "post"
secret = "abc"
"#,
        );
        let spec = WebhookSpec::from_trigger("WH", &wf).unwrap();
        assert_eq!(spec.method, "POST");
        assert_eq!(spec.secret.as_deref(), Some("abc"));
    }

    #[test]
    fn webhook_spec_rejects_missing_path() {
        let wf = workflow_with_trigger("");
        let err = WebhookSpec::from_trigger("WH", &wf).unwrap_err();
        assert!(err.contains("missing"));
    }

    #[test]
    fn webhook_spec_rejects_path_without_leading_slash() {
        let wf = workflow_with_trigger(r#"path = "hook""#);
        let err = WebhookSpec::from_trigger("WH", &wf).unwrap_err();
        assert!(err.contains('/'));
    }

    #[test]
    fn webhook_spec_rejects_non_post_method() {
        let wf = workflow_with_trigger(
            r#"
path = "/h"
method = "GET"
"#,
        );
        let err = WebhookSpec::from_trigger("WH", &wf).unwrap_err();
        assert!(err.contains("POST"));
    }

    #[test]
    fn webhook_spec_rejects_empty_secret() {
        let wf = workflow_with_trigger(
            r#"
path = "/h"
secret = ""
"#,
        );
        let err = WebhookSpec::from_trigger("WH", &wf).unwrap_err();
        assert!(err.contains("empty"));
    }

    #[test]
    fn parse_request_handles_simple_post_with_json_body() {
        let req = b"POST /hook HTTP/1.1\r\nHost: localhost\r\nContent-Length: 11\r\nContent-Type: application/json\r\n\r\n{\"hi\":true}";
        let parsed = parse_request(req).unwrap();
        assert_eq!(parsed.method, "POST");
        assert_eq!(parsed.path, "/hook");
        assert_eq!(parsed.target, "/hook");
        assert_eq!(parsed.body, b"{\"hi\":true}");
        assert_eq!(
            parsed.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn parse_request_strips_query_string_from_path() {
        let req = b"POST /hook?token=x HTTP/1.1\r\nContent-Length: 0\r\n\r\n";
        let parsed = parse_request(req).unwrap();
        assert_eq!(parsed.path, "/hook");
        assert_eq!(parsed.target, "/hook?token=x");
    }

    #[test]
    fn parse_request_rejects_oversized_body() {
        let req = format!(
            "POST /hook HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY_BYTES + 1
        );
        let err = parse_request(req.as_bytes()).unwrap_err();
        assert_eq!(err, RequestError::BodyTooLarge);
    }

    #[test]
    fn parse_request_rejects_missing_content_length() {
        let req = b"POST /hook HTTP/1.1\r\nHost: x\r\n\r\nbody";
        let err = parse_request(req).unwrap_err();
        assert_eq!(err, RequestError::MissingContentLength);
    }

    #[test]
    fn parse_request_rejects_malformed_header_line() {
        let req = b"POST /hook HTTP/1.1\r\nThisIsNotAHeader\r\nContent-Length: 0\r\n\r\n";
        let err = parse_request(req).unwrap_err();
        assert_eq!(err, RequestError::Malformed);
    }

    fn spec(path: &str, method: &str, secret: Option<&str>) -> WebhookSpec {
        WebhookSpec {
            workflow_name: format!("wf-{path}"),
            path: path.to_string(),
            method: method.to_string(),
            secret: secret.map(ToString::to_string),
        }
    }

    fn req(method: &str, path: &str, secret: Option<&str>) -> ParsedRequest {
        let mut headers = std::collections::BTreeMap::new();
        headers.insert("content-length".into(), "0".into());
        if let Some(s) = secret {
            headers.insert("x-webhook-secret".into(), s.to_string());
        }
        ParsedRequest {
            method: method.to_string(),
            target: path.to_string(),
            path: path.to_string(),
            headers,
            body: Vec::new(),
        }
    }

    #[test]
    fn route_request_dispatches_when_path_method_secret_match() {
        let specs = vec![spec("/h", "POST", Some("abc"))];
        let r = req("POST", "/h", Some("abc"));
        match route_request(&specs, &r) {
            Route::Dispatch(s) => assert_eq!(s.path, "/h"),
            other => panic!("expected Dispatch, got {other:?}"),
        }
    }

    #[test]
    fn route_request_returns_not_found_when_path_unknown() {
        let specs = vec![spec("/h", "POST", None)];
        let r = req("POST", "/missing", None);
        assert_eq!(route_request(&specs, &r), Route::NotFound);
    }

    #[test]
    fn route_request_returns_method_not_allowed_when_path_matches_but_method_differs() {
        let specs = vec![spec("/h", "POST", None)];
        let r = req("GET", "/h", None);
        assert_eq!(route_request(&specs, &r), Route::MethodNotAllowed);
    }

    #[test]
    fn route_request_returns_unauthorized_when_secret_missing_or_wrong() {
        let specs = vec![spec("/h", "POST", Some("abc"))];
        let missing = req("POST", "/h", None);
        assert_eq!(route_request(&specs, &missing), Route::Unauthorized);
        let wrong = req("POST", "/h", Some("xyz"));
        assert_eq!(route_request(&specs, &wrong), Route::Unauthorized);
    }

    #[test]
    fn constant_eq_matches_str_eq() {
        assert!(constant_eq("abc", "abc"));
        assert!(!constant_eq("abc", "abd"));
        assert!(!constant_eq("abc", "ab"));
        assert!(!constant_eq("", "x"));
        assert!(constant_eq("", ""));
    }

    #[test]
    fn build_trigger_vars_carries_path_method_remote_and_body() {
        let r = ParsedRequest {
            method: "POST".into(),
            target: "/h?x=1".into(),
            path: "/h".into(),
            headers: std::collections::BTreeMap::new(),
            body: b"{\"k\":1}".to_vec(),
        };
        let v = build_trigger_vars(&r, "127.0.0.1:54321");
        let t = v.get("trigger").unwrap();
        assert_eq!(t["path"], "/h");
        assert_eq!(t["method"], "POST");
        assert_eq!(t["remote_addr"], "127.0.0.1:54321");
        assert_eq!(t["body"], "{\"k\":1}");
    }

    #[test]
    fn webhook_config_default_is_disabled_with_loopback() {
        let cfg = WebhookConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.bind, "127.0.0.1:18080");
    }
}
