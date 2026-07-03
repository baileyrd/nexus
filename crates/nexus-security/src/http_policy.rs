//! Policy + execution for the brokered `http_request` IPC handler (C81).
//!
//! Distinct from [`crate::DownloadPolicy`]: `download` is GET-only, streams
//! straight to a file inside a sandbox writable root, and never returns
//! response bytes to the caller. `http_request` allows an arbitrary method,
//! caller-supplied headers and body, and returns the response body to the
//! caller — a materially different exfiltration risk profile (flagged by the
//! 2026-05-18 sandbox-security audit), so it gets its own allowlist rather
//! than silently reusing the download one.

use std::collections::BTreeMap;
use std::time::Duration;

use reqwest::Url;
use serde::{Deserialize, Serialize};

/// Methods the broker will forward. Deliberately excludes `CONNECT`/`TRACE`
/// and anything non-standard — the integration-plugin category this exists
/// for (RSS, Zotero, GitHub/Jira sync, Readwise) needs nothing exotic.
const ALLOWED_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD"];

/// What outbound HTTP requests the broker will perform. Off by default
/// (mirrors [`crate::DownloadPolicy`]'s closed-by-default posture); an
/// operator opts in and names allowed hosts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HttpPolicy {
    /// Whether brokered HTTP requests are permitted at all.
    pub enabled: bool,
    /// Hosts that may be requested (exact host match, e.g. `"api.github.com"`).
    pub allowed_hosts: Vec<String>,
    /// Hard cap on a single response body, in bytes. Exceeding it aborts the
    /// request with [`HttpPolicyError::TooLarge`].
    pub max_response_bytes: u64,
    /// Per-request timeout, in milliseconds.
    pub timeout_ms: u64,
}

impl Default for HttpPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_hosts: Vec::new(),
            max_response_bytes: 10 * 1024 * 1024,
            timeout_ms: 30_000,
        }
    }
}

/// Why a brokered HTTP request was refused or failed.
#[derive(Debug, thiserror::Error)]
pub enum HttpPolicyError {
    /// Brokered HTTP requests are disabled by policy.
    #[error("brokered http requests are disabled by policy")]
    Disabled,
    /// The requested method is not on [`ALLOWED_METHODS`].
    #[error("unsupported method {0:?}")]
    Method(String),
    /// The URL could not be parsed.
    #[error("invalid url: {0}")]
    Url(String),
    /// The URL scheme is not https.
    #[error("only https requests are permitted (got scheme {0:?})")]
    Scheme(String),
    /// The host is not on the allowlist.
    #[error("host {0:?} is not on the http allowlist")]
    HostNotAllowed(String),
    /// The request header map contained an invalid name or value.
    #[error("invalid header: {0}")]
    Header(String),
    /// The HTTP request failed at the transport layer.
    #[error("http error: {0}")]
    Http(String),
    /// The response exceeded the configured size cap.
    #[error("response exceeded the {max}-byte cap (got at least {got})")]
    TooLarge {
        /// The configured cap.
        max: u64,
        /// Bytes seen before aborting.
        got: u64,
    },
}

/// A validated, ready-to-send request.
#[derive(Debug, Clone)]
pub struct ValidatedRequest {
    /// Uppercased HTTP method.
    pub method: String,
    /// Parsed, allowlisted URL.
    pub url: Url,
}

/// Validate `method` + `url` against `policy`. Pure — performs no I/O.
///
/// # Errors
/// Returns the specific [`HttpPolicyError`] for the first rule that fails.
pub fn validate(method: &str, url: &str, policy: &HttpPolicy) -> Result<ValidatedRequest, HttpPolicyError> {
    if !policy.enabled {
        return Err(HttpPolicyError::Disabled);
    }
    let method = method.to_ascii_uppercase();
    if !ALLOWED_METHODS.contains(&method.as_str()) {
        return Err(HttpPolicyError::Method(method));
    }
    let parsed = Url::parse(url).map_err(|e| HttpPolicyError::Url(e.to_string()))?;
    if parsed.scheme() != "https" {
        return Err(HttpPolicyError::Scheme(parsed.scheme().to_string()));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| HttpPolicyError::Url("missing host".to_string()))?;
    if !policy.allowed_hosts.iter().any(|h| h == host) {
        return Err(HttpPolicyError::HostNotAllowed(host.to_string()));
    }
    Ok(ValidatedRequest {
        method,
        url: parsed,
    })
}

/// The outcome of a successfully executed brokered request.
#[derive(Debug, Clone)]
pub struct ExecutedResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers. Repeated header names are joined with `", "`.
    pub headers: BTreeMap<String, String>,
    /// Raw response body bytes, capped at `policy.max_response_bytes`.
    pub body: Vec<u8>,
}

/// Execute an already-validated request, streaming the response body with a
/// `max_bytes` cap. Callers that have run [`validate`] themselves use this
/// directly.
///
/// # Errors
/// Returns an [`HttpPolicyError`] if headers are invalid, the request
/// errors, or the response exceeds `max_bytes`.
pub async fn execute(
    req: &ValidatedRequest,
    headers: &BTreeMap<String, String>,
    body: Option<&str>,
    max_bytes: u64,
    timeout: Duration,
) -> Result<ExecutedResponse, HttpPolicyError> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| HttpPolicyError::Http(e.to_string()))?;

    let method = reqwest::Method::from_bytes(req.method.as_bytes())
        .map_err(|e| HttpPolicyError::Method(e.to_string()))?;
    let mut builder = client.request(method, req.url.clone());
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    if let Some(b) = body {
        builder = builder.body(b.to_string());
    }

    let mut resp = builder
        .send()
        .await
        .map_err(|e| HttpPolicyError::Http(e.to_string()))?;
    let status = resp.status().as_u16();

    // Reject early if the declared length already blows the cap.
    if let Some(len) = resp.content_length() {
        if len > max_bytes {
            return Err(HttpPolicyError::TooLarge { max: max_bytes, got: len });
        }
    }

    let mut resp_headers = BTreeMap::new();
    for (name, value) in resp.headers() {
        let Ok(value_str) = value.to_str() else {
            continue;
        };
        resp_headers
            .entry(name.as_str().to_string())
            .and_modify(|existing: &mut String| {
                existing.push_str(", ");
                existing.push_str(value_str);
            })
            .or_insert_with(|| value_str.to_string());
    }

    let mut body_bytes = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| HttpPolicyError::Http(e.to_string()))?
    {
        body_bytes.extend_from_slice(&chunk);
        if u64::try_from(body_bytes.len()).unwrap_or(u64::MAX) > max_bytes {
            return Err(HttpPolicyError::TooLarge {
                max: max_bytes,
                got: u64::try_from(body_bytes.len()).unwrap_or(u64::MAX),
            });
        }
    }

    Ok(ExecutedResponse {
        status,
        headers: resp_headers,
        body: body_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> HttpPolicy {
        HttpPolicy {
            enabled: true,
            allowed_hosts: vec!["api.example.com".to_string()],
            max_response_bytes: 1024,
            timeout_ms: 5_000,
        }
    }

    #[test]
    fn default_policy_is_off_with_sane_defaults() {
        let p = HttpPolicy::default();
        assert!(!p.enabled);
        assert!(p.allowed_hosts.is_empty());
        assert_eq!(p.max_response_bytes, 10 * 1024 * 1024);
        assert_eq!(p.timeout_ms, 30_000);
    }

    #[test]
    fn validate_accepts_allowlisted_https_get() {
        let req = validate("get", "https://api.example.com/x", &policy()).unwrap();
        assert_eq!(req.method, "GET");
        assert_eq!(req.url.host_str(), Some("api.example.com"));
    }

    #[test]
    fn validate_rejects_when_disabled() {
        let mut p = policy();
        p.enabled = false;
        assert!(matches!(
            validate("GET", "https://api.example.com/x", &p),
            Err(HttpPolicyError::Disabled)
        ));
    }

    #[test]
    fn validate_rejects_unsupported_method() {
        assert!(matches!(
            validate("CONNECT", "https://api.example.com/x", &policy()),
            Err(HttpPolicyError::Method(m)) if m == "CONNECT"
        ));
    }

    #[test]
    fn validate_rejects_non_https() {
        assert!(matches!(
            validate("GET", "http://api.example.com/x", &policy()),
            Err(HttpPolicyError::Scheme(_))
        ));
    }

    #[test]
    fn validate_rejects_host_off_allowlist() {
        assert!(matches!(
            validate("GET", "https://evil.example.com/x", &policy()),
            Err(HttpPolicyError::HostNotAllowed(h)) if h == "evil.example.com"
        ));
    }

    #[test]
    fn validate_rejects_invalid_url() {
        assert!(matches!(
            validate("GET", "not a url", &policy()),
            Err(HttpPolicyError::Url(_))
        ));
    }

    #[test]
    fn policy_serde_round_trips() {
        let json = serde_json::to_string(&policy()).unwrap();
        assert_eq!(serde_json::from_str::<HttpPolicy>(&json).unwrap(), policy());
        // Missing fields fall back to defaults.
        let partial: HttpPolicy = serde_json::from_str("{\"enabled\":true}").unwrap();
        assert!(partial.enabled);
        assert_eq!(partial.max_response_bytes, HttpPolicy::default().max_response_bytes);
    }

    /// Spawn a one-shot HTTP/1.1 server on a loopback port: reads (and
    /// discards) the incoming request, hands the raw request bytes to
    /// `on_request` for inspection, then writes back `response_body` with
    /// `status`. Returns the bound base URL (`http://127.0.0.1:<port>`).
    /// Mirrors the pattern already proven in `nexus-ai/src/ollama.rs`'s
    /// tests — no new test-only HTTP-mocking dependency needed.
    fn spawn_one_shot_server(
        status: &'static str,
        response_body: Vec<u8>,
        extra_headers: &'static str,
        on_request: impl FnOnce(String) + Send + 'static,
    ) -> String {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = vec![0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                on_request(String::from_utf8_lossy(&buf[..n]).to_string());
                let response = format!(
                    "HTTP/1.1 {status}\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    response_body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&response_body);
                let _ = stream.flush();
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn execute_returns_status_headers_and_body() {
        let base = spawn_one_shot_server(
            "200 OK",
            b"hi there".to_vec(),
            "X-Test: yes\r\n",
            |_req| {},
        );
        let req = ValidatedRequest {
            method: "GET".to_string(),
            url: Url::parse(&format!("{base}/hello")).unwrap(),
        };
        let resp = execute(&req, &BTreeMap::new(), None, 1024, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.headers.get("x-test").map(String::as_str), Some("yes"));
        assert_eq!(resp.body, b"hi there");
    }

    #[tokio::test]
    async fn execute_sends_headers_and_body_for_post() {
        let (tx, rx) = std::sync::mpsc::channel();
        let base = spawn_one_shot_server("201 Created", Vec::new(), "", move |req| {
            let _ = tx.send(req);
        });
        let req = ValidatedRequest {
            method: "POST".to_string(),
            url: Url::parse(&format!("{base}/submit")).unwrap(),
        };
        let mut headers = BTreeMap::new();
        headers.insert("x-api-key".to_string(), "secret".to_string());
        let resp = execute(&req, &headers, Some("payload"), 1024, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(resp.status, 201);
        let received = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(received.contains("x-api-key: secret"), "got: {received}");
        assert!(received.contains("payload"), "got: {received}");
        assert!(received.starts_with("POST "), "got: {received}");
    }

    #[tokio::test]
    async fn execute_aborts_when_response_exceeds_cap() {
        let base = spawn_one_shot_server("200 OK", vec![b'x'; 200], "", |_req| {});
        let req = ValidatedRequest {
            method: "GET".to_string(),
            url: Url::parse(&format!("{base}/big")).unwrap(),
        };
        let err = execute(&req, &BTreeMap::new(), None, 32, Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(matches!(err, HttpPolicyError::TooLarge { max: 32, .. }));
    }
}
