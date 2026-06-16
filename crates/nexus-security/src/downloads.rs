//! Permissioned download broker — the approved-egress companion to the
//! network-off OS sandbox.
//!
//! A process confined by a network-off [`SandboxPolicy`](nexus_types::SandboxPolicy)
//! cannot open raw sockets (seccomp denies `socket(AF_INET…)`). Rather than
//! poke a hole in that, Nexus can perform *specific, allowlisted* downloads on
//! the process's behalf and drop the result into a writable root. This module
//! is the gate: [`validate`] enforces the permission rules (pure, fully
//! testable) and [`fetch`] performs the validated download.
//!
//! Rules enforced by [`validate`]:
//! 1. downloads must be **enabled** in the policy;
//! 2. the URL scheme must be **https**;
//! 3. the host must be on the **allowlist** (exact match);
//! 4. the destination must lie inside one of the sandbox's **writable roots**
//!    (so the confined process can actually read what was fetched).

use std::path::{Path, PathBuf};

use nexus_types::WritableRoot;
use reqwest::Url;
use serde::{Deserialize, Serialize};

/// What downloads the broker will perform. Off by default (mirrors
/// network-off-by-default); an operator opts in and names allowed hosts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadPolicy {
    /// Whether brokered downloads are permitted at all.
    pub enabled: bool,
    /// Hosts that may be fetched from (exact host match, e.g. `"static.crates.io"`).
    pub allowed_hosts: Vec<String>,
    /// Hard cap on a single download's size, in bytes.
    pub max_bytes: u64,
}

impl Default for DownloadPolicy {
    fn default() -> Self {
        Self { enabled: false, allowed_hosts: Vec::new(), max_bytes: 100 * 1024 * 1024 }
    }
}

/// A request to fetch `url` into `dest`.
#[derive(Debug, Clone, Copy)]
pub struct DownloadRequest<'a> {
    /// The source URL (must be https + allowlisted).
    pub url: &'a str,
    /// The destination path (must be inside a writable root).
    pub dest: &'a Path,
}

/// Why a download was refused or failed.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    /// Brokered downloads are disabled by policy.
    #[error("brokered downloads are disabled by policy")]
    Disabled,
    /// The URL could not be parsed.
    #[error("invalid url: {0}")]
    Url(String),
    /// The URL scheme is not https.
    #[error("only https downloads are permitted (got scheme {0:?})")]
    Scheme(String),
    /// The host is not on the allowlist.
    #[error("host {0:?} is not on the download allowlist")]
    HostNotAllowed(String),
    /// The destination is not inside a sandbox writable root.
    #[error("destination {0} is not inside a writable root")]
    DestNotWritable(PathBuf),
    /// The download exceeded the size cap.
    #[error("download exceeded the {max}-byte cap (got at least {got})")]
    TooLarge {
        /// The configured cap.
        max: u64,
        /// Bytes seen before aborting.
        got: u64,
    },
    /// The HTTP request failed.
    #[error("http error: {0}")]
    Http(String),
    /// Writing the destination failed.
    #[error("io error: {0}")]
    Io(String),
}

/// Validate `req` against `policy` and the sandbox's `writable_roots`, returning
/// the parsed URL on success. Pure — performs no I/O.
///
/// # Errors
/// Returns the specific [`DownloadError`] for the first rule that fails.
pub fn validate(
    req: &DownloadRequest<'_>,
    policy: &DownloadPolicy,
    writable_roots: &[WritableRoot],
) -> Result<Url, DownloadError> {
    if !policy.enabled {
        return Err(DownloadError::Disabled);
    }
    let url = Url::parse(req.url).map_err(|e| DownloadError::Url(e.to_string()))?;
    if url.scheme() != "https" {
        return Err(DownloadError::Scheme(url.scheme().to_string()));
    }
    let host = url
        .host_str()
        .ok_or_else(|| DownloadError::Url("missing host".to_string()))?;
    if !policy.allowed_hosts.iter().any(|h| h == host) {
        return Err(DownloadError::HostNotAllowed(host.to_string()));
    }
    if !writable_roots.iter().any(|r| r.is_path_writable(req.dest)) {
        return Err(DownloadError::DestNotWritable(req.dest.to_path_buf()));
    }
    Ok(url)
}

/// Validate, then perform the download, streaming to `req.dest` and aborting if
/// the size cap is exceeded. Returns the number of bytes written.
///
/// # Errors
/// Returns a [`DownloadError`] if validation fails, the request errors, the
/// response is unsuccessful, the cap is exceeded, or the write fails.
pub async fn fetch(
    req: &DownloadRequest<'_>,
    policy: &DownloadPolicy,
    writable_roots: &[WritableRoot],
) -> Result<u64, DownloadError> {
    let url = validate(req, policy, writable_roots)?;

    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| DownloadError::Http(e.to_string()))?;
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| DownloadError::Http(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(DownloadError::Http(format!("status {}", resp.status())));
    }
    // Reject early if the declared length already blows the cap.
    if let Some(len) = resp.content_length() {
        if len > policy.max_bytes {
            return Err(DownloadError::TooLarge { max: policy.max_bytes, got: len });
        }
    }

    let mut file =
        std::fs::File::create(req.dest).map_err(|e| DownloadError::Io(e.to_string()))?;
    let mut written: u64 = 0;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| DownloadError::Http(e.to_string()))?
    {
        written += u64::try_from(chunk.len()).unwrap_or(u64::MAX);
        if written > policy.max_bytes {
            drop(file);
            let _ = std::fs::remove_file(req.dest);
            return Err(DownloadError::TooLarge { max: policy.max_bytes, got: written });
        }
        std::io::Write::write_all(&mut file, &chunk).map_err(|e| DownloadError::Io(e.to_string()))?;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Vec<WritableRoot> {
        vec![WritableRoot::new(PathBuf::from("/work"))]
    }

    fn policy() -> DownloadPolicy {
        DownloadPolicy {
            enabled: true,
            allowed_hosts: vec!["static.crates.io".to_string()],
            max_bytes: 1024,
        }
    }

    #[test]
    fn default_policy_is_off_with_sane_cap() {
        let p = DownloadPolicy::default();
        assert!(!p.enabled);
        assert!(p.allowed_hosts.is_empty());
        assert_eq!(p.max_bytes, 100 * 1024 * 1024);
    }

    #[test]
    fn validate_accepts_allowlisted_https_into_writable_root() {
        let req = DownloadRequest {
            url: "https://static.crates.io/crates/x.crate",
            dest: Path::new("/work/x.crate"),
        };
        let url = validate(&req, &policy(), &roots()).unwrap();
        assert_eq!(url.host_str(), Some("static.crates.io"));
    }

    #[test]
    fn validate_rejects_when_disabled() {
        let mut p = policy();
        p.enabled = false;
        let req = DownloadRequest {
            url: "https://static.crates.io/x",
            dest: Path::new("/work/x"),
        };
        assert!(matches!(validate(&req, &p, &roots()), Err(DownloadError::Disabled)));
    }

    #[test]
    fn validate_rejects_non_https() {
        let req = DownloadRequest {
            url: "http://static.crates.io/x",
            dest: Path::new("/work/x"),
        };
        assert!(matches!(validate(&req, &policy(), &roots()), Err(DownloadError::Scheme(_))));
    }

    #[test]
    fn validate_rejects_host_off_allowlist() {
        let req = DownloadRequest {
            url: "https://evil.example.com/x",
            dest: Path::new("/work/x"),
        };
        assert!(matches!(
            validate(&req, &policy(), &roots()),
            Err(DownloadError::HostNotAllowed(h)) if h == "evil.example.com"
        ));
    }

    #[test]
    fn validate_rejects_dest_outside_writable_roots() {
        let req = DownloadRequest {
            url: "https://static.crates.io/x",
            dest: Path::new("/etc/passwd"),
        };
        assert!(matches!(
            validate(&req, &policy(), &roots()),
            Err(DownloadError::DestNotWritable(_))
        ));
    }

    #[test]
    fn policy_serde_round_trips() {
        let json = serde_json::to_string(&policy()).unwrap();
        assert_eq!(serde_json::from_str::<DownloadPolicy>(&json).unwrap(), policy());
        // Missing fields fall back to defaults.
        let partial: DownloadPolicy = serde_json::from_str("{\"enabled\":true}").unwrap();
        assert!(partial.enabled);
        assert_eq!(partial.max_bytes, DownloadPolicy::default().max_bytes);
    }
}
