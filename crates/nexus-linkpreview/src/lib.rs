//! Nexus link-preview subsystem.
//!
//! Fetches a URL, parses Open Graph / Twitter-card / HTML metadata,
//! and returns a [`LinkPreview`] the shell can render into a link
//! card on the canvas. Deliberately small + regex-based: production-
//! perfect HTML parsing is overkill for OG metadata, and avoiding an
//! HTML-parser dep keeps this crate cheap to compile.
//!
//! The public surface is intentionally narrow: [`fetch_blocking`]
//! is what the IPC handler calls; [`parse_html`] is exposed mainly
//! so tests (and future tooling) can exercise the parser without
//! making network calls.

#![deny(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

use std::io::Read;
use std::net::{IpAddr, ToSocketAddrs};
use std::time::Duration;

use regex_lite::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(feature = "ts-export")]
use schemars::JsonSchema;
#[cfg(feature = "ts-export")]
use ts_rs::TS;

pub mod core_plugin;

/// Request timeout — covers DNS + connect + read. Kept short because
/// the caller is a user action (hovering/opening a canvas) and we
/// prefer a fast fallback card over a laggy UI waiting on a slow
/// host.
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
/// Hard cap on the HTML body we parse. Anything larger is almost
/// certainly not a plain web page (big images, PDFs, zip files) and
/// we don't want to read megabytes into memory before giving up.
const MAX_BODY_BYTES: usize = 512 * 1024;
/// Conservative browser-ish UA so servers serve the real HTML instead
/// of a bot-challenge page.
const USER_AGENT: &str =
    "Mozilla/5.0 (Nexus Canvas) AppleWebKit/537.36 (KHTML, like Gecko) Nexus/0.1";

/// Structured metadata extracted from a web page. Every field is
/// optional — the shell renders whatever it gets and falls back to
/// the raw URL when everything is missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]
#[cfg_attr(
    feature = "ts-export",
    ts(
        export,
        export_to = "../../../packages/nexus-extension-api/src/generated/ipc/"
    )
)]
#[serde(deny_unknown_fields)]
pub struct LinkPreview {
    /// Canonical URL the preview describes. Echoes the request URL
    /// so callers can detect redirects or canonicalisations without
    /// round-tripping.
    pub url: String,
    /// `og:title` or `<title>` contents.
    pub title: Option<String>,
    /// `og:description` or `<meta name="description">`.
    pub description: Option<String>,
    /// `og:image` absolute URL.
    pub image_url: Option<String>,
    /// `og:site_name` or the URL's hostname as a fallback.
    pub site_name: Option<String>,
    /// Best-effort favicon URL (absolute where possible).
    pub favicon_url: Option<String>,
}

/// Errors bubbled out of the fetch pipeline. Deliberately coarse —
/// the shell's only useful response to any of these is "show the
/// fallback card", so fine-grained classification isn't load-bearing.
#[derive(Debug, Error)]
pub enum FetchError {
    /// Caller passed something that isn't a valid `http`/`https` URL.
    #[error("invalid or unsupported URL: {0}")]
    InvalidUrl(String),
    /// Network / transport / DNS failure from reqwest.
    #[error("request failed: {0}")]
    Request(String),
    /// Non-2xx HTTP status.
    #[error("http status {0}")]
    Status(u16),
}

/// Return `true` if `ip` is a non-public address that an outbound
/// HTTP client must refuse to connect to: loopback (`127.0.0.0/8`,
/// `::1`), link-local (`169.254.0.0/16`, `fe80::/10` — also covers
/// the AWS EC2 metadata IP `169.254.169.254`), RFC1918 private
/// (`10/8`, `172.16/12`, `192.168/16`), shared address space
/// (`100.64/10`, RFC6598), IPv4 broadcast (`255.255.255.255`), IPv6
/// ULA (`fc00::/7`), unspecified (`0.0.0.0`, `::`), multicast, or
/// IPv4-mapped IPv6 of any of the above. See issue #78.
///
/// Pure helper so the SSRF guard can be exhaustively unit-tested
/// without standing up an HTTP server.
#[must_use]
pub fn is_blocked_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_loopback() || v4.is_unspecified() || v4.is_multicast() || v4.is_broadcast() {
                return true;
            }
            // RFC1918 private + link-local + shared (CGNAT).
            let octs = v4.octets();
            v4.is_private()
                || v4.is_link_local()
                // 100.64.0.0/10 — RFC6598 carrier-grade NAT.
                || (octs[0] == 100 && (64..128).contains(&octs[1]))
                // 0.0.0.0/8 — "this network" reserved.
                || octs[0] == 0
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() {
                return true;
            }
            // IPv4-mapped IPv6 (`::ffff:a.b.c.d`) — recurse into the
            // v4 check so attackers can't bypass the guard by smuggling
            // 127.0.0.1 as `::ffff:127.0.0.1`.
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_address(IpAddr::V4(mapped));
            }
            // fc00::/7 — Unique Local Addresses (RFC4193).
            let segs = v6.segments();
            if (segs[0] & 0xfe00) == 0xfc00 {
                return true;
            }
            // fe80::/10 — link-local.
            if (segs[0] & 0xffc0) == 0xfe80 {
                return true;
            }
            false
        }
    }
}

/// Resolve `host` and `port` to socket addresses, returning an error
/// if any resolved address is non-public per [`is_blocked_address`].
/// Returns the first allowed address so the caller can record it.
fn resolve_public_address(host: &str, port: u16) -> Result<IpAddr, FetchError> {
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|e| FetchError::Request(format!("DNS resolution failed for {host}: {e}")))?;
    let mut found_any = false;
    for addr in addrs {
        found_any = true;
        let ip = addr.ip();
        if is_blocked_address(ip) {
            return Err(FetchError::InvalidUrl(format!(
                "host {host} resolves to non-public address {ip} — refused"
            )));
        }
    }
    if !found_any {
        return Err(FetchError::Request(format!(
            "host {host} resolved to no addresses"
        )));
    }
    // We re-resolve at the redirect-policy callback for each redirect
    // hop, so returning the first IP here is informational only — the
    // actual outbound connection re-resolves through reqwest. There's
    // a residual TOCTOU between this check and the connection
    // (DNS rebinding); locking reqwest to a specific IP is a deeper
    // change and is documented as residual risk on issue #78.
    (host, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut iter| iter.next())
        .map(|s| s.ip())
        .ok_or_else(|| FetchError::Request(format!("host {host} resolved to no addresses")))
}

/// Validate that `url` (already known to be http/https) doesn't
/// resolve to a non-public address. Returns the resolved IP for
/// audit/logging purposes.
fn validate_url_target(url: &reqwest::Url) -> Result<IpAddr, FetchError> {
    let host = url
        .host_str()
        .ok_or_else(|| FetchError::InvalidUrl(format!("URL has no host: {url}")))?;
    // If the URL itself contains a literal IP, we can short-circuit
    // the resolver entirely.
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_blocked_address(ip) {
            return Err(FetchError::InvalidUrl(format!(
                "URL targets non-public address {ip} — refused"
            )));
        }
        return Ok(ip);
    }
    let port = url
        .port_or_known_default()
        .unwrap_or(if url.scheme() == "https" { 443 } else { 80 });
    resolve_public_address(host, port)
}

/// Fetch `url`, parse metadata, return a [`LinkPreview`]. Blocks
/// the calling thread — run from a kernel handler thread rather
/// than from async contexts.
///
/// # Errors
/// Returns [`FetchError`] for invalid URLs, transport failures, or
/// non-2xx responses. Response bodies that are valid but contain no
/// recognisable metadata produce an `Ok` with mostly-empty fields.
pub fn fetch_blocking(url: &str) -> Result<LinkPreview, FetchError> {
    // Parse + validate scheme up front. `reqwest::Url` does the same
    // parsing reqwest itself does; doing it here means we can run
    // the SSRF guard before reqwest opens any socket.
    let parsed = reqwest::Url::parse(url).map_err(|_| FetchError::InvalidUrl(url.to_string()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(FetchError::InvalidUrl(url.to_string()));
    }

    // SSRF guard #1 — initial URL. Resolve the hostname and refuse
    // anything that lands on a loopback / link-local / private /
    // metadata IP. See `is_blocked_address`.
    let _ = validate_url_target(&parsed)?;

    // SSRF guard #2 — every redirect hop. Reqwest follows up to 10
    // redirects by default; without this check, the initial URL
    // could be public but a redirect could be `http://169.254.169.254/...`
    // (AWS metadata) and reqwest would happily follow.
    let redirect_policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.error("too many redirects");
        }
        match validate_url_target(attempt.url()) {
            Ok(_) => attempt.follow(),
            Err(e) => attempt.error(e.to_string()),
        }
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(USER_AGENT)
        .redirect(redirect_policy)
        .build()
        .map_err(|e| FetchError::Request(e.to_string()))?;
    let resp = client
        .get(parsed)
        .send()
        .map_err(|e| FetchError::Request(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(FetchError::Status(status.as_u16()));
    }
    let final_url = resp.url().to_string();

    // Cap at the transport layer via `Read::take` so a server
    // streaming gigabytes (or a gzip-bomb body, if decompression were
    // ever enabled) can't be read into memory before the cap kicks
    // in. `Response` impls `Read` for blocking, so this is just a
    // bounded `read_to_end`.
    let mut buf = Vec::with_capacity(MAX_BODY_BYTES.min(64 * 1024));
    resp.take(MAX_BODY_BYTES as u64)
        .read_to_end(&mut buf)
        .map_err(|e| FetchError::Request(e.to_string()))?;
    // `parse_html` only needs the HTML head; treat the bytes as
    // UTF-8 lossily so a server returning a non-UTF-8 charset
    // doesn't fail outright.
    let body = String::from_utf8_lossy(&buf);

    let mut preview = parse_html(&final_url, &body);
    if preview.site_name.is_none() {
        preview.site_name = hostname(&final_url);
    }
    Ok(preview)
}

/// Parse `html` into a [`LinkPreview`]. `base_url` is used to
/// resolve relative image / favicon URLs. Returns a preview with
/// `url = base_url` and whatever metadata the regexes could
/// extract. Pure + synchronous so tests don't need a mock server.
#[must_use]
pub fn parse_html(base_url: &str, html: &str) -> LinkPreview {
    LinkPreview {
        url: base_url.to_string(),
        title: find_meta(html, "og:title")
            .or_else(|| find_meta_name(html, "twitter:title"))
            .or_else(|| find_title(html)),
        description: find_meta(html, "og:description")
            .or_else(|| find_meta_name(html, "twitter:description"))
            .or_else(|| find_meta_name(html, "description")),
        image_url: find_meta(html, "og:image")
            .or_else(|| find_meta_name(html, "twitter:image"))
            .map(|v| absolutise(base_url, &v)),
        site_name: find_meta(html, "og:site_name"),
        favicon_url: find_favicon(html).map(|v| absolutise(base_url, &v)),
    }
}

/// Find `<meta property="…" content="…">` by property name. OG tags
/// use `property=`; older style uses `name=`.
fn find_meta(html: &str, property: &str) -> Option<String> {
    // Allow the attributes in either order, single or double quotes,
    // and any whitespace around the equals signs.
    let pat = format!(
        r#"(?is)<meta\b[^>]*?\bproperty\s*=\s*["']{p}["'][^>]*?\bcontent\s*=\s*["']([^"']*)["']"#,
        p = regex_escape(property),
    );
    let a = Regex::new(&pat)
        .ok()?
        .captures(html)
        .map(|c| c[1].to_string());
    if a.is_some() {
        return normalize(a);
    }
    let pat2 = format!(
        r#"(?is)<meta\b[^>]*?\bcontent\s*=\s*["']([^"']*)["'][^>]*?\bproperty\s*=\s*["']{p}["']"#,
        p = regex_escape(property),
    );
    normalize(
        Regex::new(&pat2)
            .ok()?
            .captures(html)
            .map(|c| c[1].to_string()),
    )
}

/// Find `<meta name="…" content="…">` — for `twitter:*` + plain
/// `description`.
fn find_meta_name(html: &str, name: &str) -> Option<String> {
    let pat = format!(
        r#"(?is)<meta\b[^>]*?\bname\s*=\s*["']{n}["'][^>]*?\bcontent\s*=\s*["']([^"']*)["']"#,
        n = regex_escape(name),
    );
    let a = Regex::new(&pat)
        .ok()?
        .captures(html)
        .map(|c| c[1].to_string());
    if a.is_some() {
        return normalize(a);
    }
    let pat2 = format!(
        r#"(?is)<meta\b[^>]*?\bcontent\s*=\s*["']([^"']*)["'][^>]*?\bname\s*=\s*["']{n}["']"#,
        n = regex_escape(name),
    );
    normalize(
        Regex::new(&pat2)
            .ok()?
            .captures(html)
            .map(|c| c[1].to_string()),
    )
}

/// Find the first `<title>...</title>` in `<head>`. Falls back to
/// the first `<title>` anywhere if the head is unparseable.
fn find_title(html: &str) -> Option<String> {
    let re = Regex::new(r"(?is)<title\b[^>]*>(.*?)</title>").ok()?;
    normalize(re.captures(html).map(|c| c[1].to_string())).map(|t| decode_entities(&t))
}

/// Find a link rel="icon" variant. Returns the first href we spot —
/// size / format selection is out of scope for the first cut.
fn find_favicon(html: &str) -> Option<String> {
    // Accept: icon, shortcut icon, apple-touch-icon.
    let re = Regex::new(
        r#"(?is)<link\b[^>]*?\brel\s*=\s*["'](?:shortcut\s+icon|icon|apple-touch-icon)["'][^>]*?\bhref\s*=\s*["']([^"']+)["']"#,
    )
    .ok()?;
    let a = re.captures(html).map(|c| c[1].to_string());
    if a.is_some() {
        return normalize(a);
    }
    // href-before-rel variant.
    let re2 = Regex::new(
        r#"(?is)<link\b[^>]*?\bhref\s*=\s*["']([^"']+)["'][^>]*?\brel\s*=\s*["'](?:shortcut\s+icon|icon|apple-touch-icon)["']"#,
    )
    .ok()?;
    normalize(re2.captures(html).map(|c| c[1].to_string()))
}

/// Resolve a possibly-relative URL against a base.
fn absolutise(base: &str, v: &str) -> String {
    let trimmed = v.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("//") {
        // protocol-relative — use the base's scheme
        let scheme = if base.starts_with("https://") {
            "https:"
        } else {
            "http:"
        };
        return format!("{scheme}//{rest}");
    }
    let origin = origin_of(base).unwrap_or_default();
    if trimmed.starts_with('/') {
        return format!("{origin}{trimmed}");
    }
    // Path-relative — drop the base's last segment.
    let stripped = base.trim_end_matches(|c: char| c != '/');
    format!("{stripped}{trimmed}")
}

/// `https://a.com/x/y?z=1` → `https://a.com`.
fn origin_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let scheme = if url.starts_with("https://") {
        "https://"
    } else {
        "http://"
    };
    let host_end = rest.find('/').unwrap_or(rest.len());
    Some(format!("{scheme}{}", &rest[..host_end]))
}

/// Extract the hostname portion of a URL for `site_name` fallback.
fn hostname(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let host_end = rest.find('/').unwrap_or(rest.len());
    Some(rest[..host_end].to_string())
}

/// Minimal HTML-entity decode for the handful of entities that show
/// up in `<title>`s. We intentionally don't pull in a full decoder.
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

fn normalize(s: Option<String>) -> Option<String> {
    let t = s?.trim().to_string();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

/// Escape a literal string for embedding inside a regex.
fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_og_suite() {
        let html = r#"
            <html><head>
              <meta property="og:title" content="Example Page">
              <meta property="og:description" content="An example page.">
              <meta property="og:image" content="/hero.png">
              <meta property="og:site_name" content="Example">
              <link rel="icon" href="/favicon.ico">
              <title>Fallback</title>
            </head></html>
        "#;
        let p = parse_html("https://example.com/post", html);
        assert_eq!(p.title.as_deref(), Some("Example Page"));
        assert_eq!(p.description.as_deref(), Some("An example page."));
        assert_eq!(p.image_url.as_deref(), Some("https://example.com/hero.png"));
        assert_eq!(p.site_name.as_deref(), Some("Example"));
        assert_eq!(
            p.favicon_url.as_deref(),
            Some("https://example.com/favicon.ico")
        );
    }

    #[test]
    fn falls_back_to_title_and_twitter_tags() {
        let html = r#"
            <head>
              <meta name="twitter:title" content="Twitter Title">
              <meta name="twitter:description" content="TX desc">
              <meta name="twitter:image" content="https://cdn.example.net/img.jpg">
              <title>HTML Title &amp; More</title>
            </head>
        "#;
        let p = parse_html("https://example.net/", html);
        assert_eq!(p.title.as_deref(), Some("Twitter Title"));
        assert_eq!(p.description.as_deref(), Some("TX desc"));
        assert_eq!(
            p.image_url.as_deref(),
            Some("https://cdn.example.net/img.jpg")
        );
    }

    #[test]
    fn title_fallback_decodes_entities() {
        let html = "<head><title>A &amp; B</title></head>";
        let p = parse_html("https://x.io/", html);
        assert_eq!(p.title.as_deref(), Some("A & B"));
    }

    #[test]
    fn description_fallback_from_meta_name() {
        let html = r#"<meta name="description" content="The site.">"#;
        let p = parse_html("https://x.io/", html);
        assert_eq!(p.description.as_deref(), Some("The site."));
    }

    #[test]
    fn attribute_order_is_flexible() {
        // content attr before property attr — still matches.
        let html = r#"<meta content="ordered" property="og:title">"#;
        let p = parse_html("https://x.io/", html);
        assert_eq!(p.title.as_deref(), Some("ordered"));
    }

    #[test]
    fn absolutises_protocol_relative_and_root_paths() {
        assert_eq!(
            absolutise("https://a.com/x", "//cdn.b.com/i.png"),
            "https://cdn.b.com/i.png"
        );
        assert_eq!(
            absolutise("https://a.com/x/y", "/z.png"),
            "https://a.com/z.png"
        );
        assert_eq!(
            absolutise("https://a.com/x/y", "z.png"),
            "https://a.com/x/z.png"
        );
        assert_eq!(
            absolutise("https://a.com/x/y", "https://other/x.png"),
            "https://other/x.png"
        );
    }

    #[test]
    fn empty_content_produces_none() {
        let html = r#"<meta property="og:title" content="">"#;
        let p = parse_html("https://x.io/", html);
        assert!(p.title.is_none());
    }

    #[test]
    fn apple_touch_icon_is_accepted() {
        let html = r#"<link rel="apple-touch-icon" href="/ati.png">"#;
        let p = parse_html("https://x.io/", html);
        assert_eq!(p.favicon_url.as_deref(), Some("https://x.io/ati.png"));
    }

    #[test]
    fn rejects_non_http_urls() {
        let err = fetch_blocking("ftp://example.com/").unwrap_err();
        assert!(matches!(err, FetchError::InvalidUrl(_)));
        let err = fetch_blocking("javascript:alert(1)").unwrap_err();
        assert!(matches!(err, FetchError::InvalidUrl(_)));
    }
}
