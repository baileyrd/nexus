//! Parser for the `ssh://user@host[:port]/path` forge URI introduced
//! by BL-140 Phase 2.
//!
//! The frontend (CLI / Tauri shell) accepts a forge URI in lieu of a
//! local path. When the URI scheme is `ssh`, the bootstrap layer (see
//! a follow-up PR — Phase 2b) spawns an `ssh user@host:port nexus
//! serve --forge-path /path --stdio` subprocess and routes IPC through
//! the BL-140 Phase 1 wire protocol.
//!
//! Today this module is consumed only by the Phase 2a client tests;
//! the runtime factory that turns a [`ForgeUri::Ssh`] into an actual
//! SSH child process lives elsewhere and isn't gated on this PR.

use std::fmt;

/// A forge location supplied by the user. The `ssh://` form drives a
/// headless `nexus serve --stdio` running on the remote host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeUri {
    /// `ssh://[user@]host[:port]/abs/path` — remote forge over SSH.
    Ssh(SshForgeUri),
}

/// Parsed components of an `ssh://` forge URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshForgeUri {
    /// Optional username component (everything before the `@` in the
    /// authority). When absent, the caller falls back to the local
    /// user's default (the same posture as plain `ssh host`).
    pub user: Option<String>,
    /// Hostname (or IP literal). IPv6 literals must be bracketed —
    /// `ssh://user@[::1]:22/path` — to disambiguate the port colon
    /// from the address colons.
    pub host: String,
    /// Optional explicit port (1..=65535). When absent, the caller
    /// falls back to the SSH config / library default (22).
    pub port: Option<u16>,
    /// Absolute path to the forge root on the remote host. The leading
    /// `/` is preserved so `/home/alice/forge` round-trips correctly.
    pub path: String,
}

/// Errors raised by [`ForgeUri::parse`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ParseError {
    /// The URI doesn't carry a scheme followed by `://`.
    #[error("missing or malformed scheme (expected '<scheme>://...'); got: {0}")]
    NoScheme(String),
    /// The scheme is recognised but not supported by this build (only
    /// `ssh` is recognised today; local filesystem paths bypass this
    /// module).
    #[error("unsupported scheme '{0}' — only 'ssh' is supported")]
    UnsupportedScheme(String),
    /// The authority (`[user@]host[:port]`) is empty.
    #[error("empty authority — expected '[user@]host[:port]' after '{0}://'")]
    EmptyAuthority(String),
    /// The user component (between scheme and `@`) is empty.
    #[error("empty user component — '{0}://@host/path' is not valid")]
    EmptyUser(String),
    /// The host component is empty after stripping user/port.
    #[error("empty host component")]
    EmptyHost,
    /// The port component is empty (e.g. trailing colon) or doesn't
    /// parse as a u16.
    #[error("invalid port '{0}' (expected 1..=65535)")]
    InvalidPort(String),
    /// The path component is missing (`ssh://host` with no `/...`
    /// tail). A remote forge path is required.
    #[error("missing path component — expected '...:port/abs/path'")]
    MissingPath,
    /// The path component is present but not absolute. We require the
    /// leading `/` because relative paths against the remote user's
    /// home introduce ambiguity that's better surfaced explicitly.
    #[error("path must be absolute, got: {0}")]
    RelativePath(String),
    /// Bracketed IPv6 authority is malformed (missing `]` or contains
    /// invalid bytes between the brackets).
    #[error("malformed bracketed host: {0}")]
    MalformedBracketedHost(String),
}

impl fmt::Display for ForgeUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ForgeUri::Ssh(s) => {
                f.write_str("ssh://")?;
                if let Some(u) = &s.user {
                    write!(f, "{u}@")?;
                }
                // Re-bracket the host on output if it looks like an
                // IPv6 literal (contains a colon and no brackets).
                let needs_brackets = s.host.contains(':') && !s.host.starts_with('[');
                if needs_brackets {
                    write!(f, "[{}]", s.host)?;
                } else {
                    f.write_str(&s.host)?;
                }
                if let Some(p) = s.port {
                    write!(f, ":{p}")?;
                }
                f.write_str(&s.path)
            }
        }
    }
}

impl ForgeUri {
    /// Parse a forge URI string.
    ///
    /// Recognises `ssh://[user@]host[:port]/abs/path`. Local filesystem
    /// paths (no `://` separator) intentionally return
    /// [`ParseError::NoScheme`] — the calling layer is expected to
    /// detect those before reaching this parser.
    ///
    /// # Errors
    /// Returns a [`ParseError`] variant naming the first malformed
    /// component.
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        let Some((scheme, rest)) = s.split_once("://") else {
            return Err(ParseError::NoScheme(s.to_string()));
        };
        match scheme {
            "ssh" => parse_ssh(rest).map(ForgeUri::Ssh),
            other => Err(ParseError::UnsupportedScheme(other.to_string())),
        }
    }
}

fn parse_ssh(after_scheme: &str) -> Result<SshForgeUri, ParseError> {
    // Split into authority + path on the first `/` that isn't inside a
    // bracketed IPv6 host. The path must start with `/` per
    // RelativePath.
    let (authority, path) = split_authority_and_path(after_scheme)?;
    if authority.is_empty() {
        return Err(ParseError::EmptyAuthority("ssh".to_string()));
    }
    let (user_part, host_port) = match authority.split_once('@') {
        Some((u, hp)) => (Some(u), hp),
        None => (None, authority),
    };
    let user = match user_part {
        Some("") => return Err(ParseError::EmptyUser("ssh".to_string())),
        Some(u) => Some(u.to_string()),
        None => None,
    };
    let (host, port) = parse_host_port(host_port)?;
    Ok(SshForgeUri {
        user,
        host,
        port,
        path,
    })
}

/// Find the first `/` that lies *outside* a bracketed IPv6 host. The
/// returned `(authority, path)` includes the leading `/` on the path
/// side. Empty path → [`ParseError::MissingPath`]; non-empty path that
/// doesn't start with `/` is impossible by construction here (we
/// always split on `/`), but we still enforce `RelativePath` for paths
/// like `ssh://host//` (which would split into authority `host`, path
/// `//` — accepted as a normalised `/`).
fn split_authority_and_path(s: &str) -> Result<(&str, String), ParseError> {
    let bytes = s.as_bytes();
    let mut in_brackets = false;
    let mut saw_open_bracket = false;
    let mut split_idx: Option<usize> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'[' => {
                in_brackets = true;
                saw_open_bracket = true;
            }
            b']' => in_brackets = false,
            b'/' if !in_brackets => {
                split_idx = Some(i);
                break;
            }
            _ => {}
        }
    }
    if in_brackets {
        // We opened a `[` and never saw the matching `]` before
        // reaching end-of-string (or a `/` that's still nested
        // inside). That's a malformed bracketed host — surface it
        // explicitly rather than as MissingPath.
        return Err(ParseError::MalformedBracketedHost(s.to_string()));
    }
    if !saw_open_bracket && split_idx.is_none() {
        return Err(ParseError::MissingPath);
    }
    match split_idx {
        Some(i) => {
            let authority = &s[..i];
            let path = &s[i..];
            if !path.starts_with('/') {
                return Err(ParseError::RelativePath(path.to_string()));
            }
            Ok((authority, path.to_string()))
        }
        None => Err(ParseError::MissingPath),
    }
}

fn parse_host_port(s: &str) -> Result<(String, Option<u16>), ParseError> {
    // Bracketed IPv6: `[::1]` or `[::1]:22`.
    if let Some(stripped) = s.strip_prefix('[') {
        let Some(close) = stripped.find(']') else {
            return Err(ParseError::MalformedBracketedHost(s.to_string()));
        };
        let host = &stripped[..close];
        if host.is_empty() {
            return Err(ParseError::MalformedBracketedHost(s.to_string()));
        }
        let after = &stripped[close + 1..];
        let port = if after.is_empty() {
            None
        } else if let Some(rest) = after.strip_prefix(':') {
            Some(parse_port(rest)?)
        } else {
            return Err(ParseError::MalformedBracketedHost(s.to_string()));
        };
        return Ok((host.to_string(), port));
    }
    // Plain host[:port]. We use rsplit_once so a host that happens to
    // contain colons would have been bracketed — anything reaching
    // here with a `:` is host:port.
    match s.rsplit_once(':') {
        Some((host, port_str)) => {
            if host.is_empty() {
                return Err(ParseError::EmptyHost);
            }
            Ok((host.to_string(), Some(parse_port(port_str)?)))
        }
        None => {
            if s.is_empty() {
                Err(ParseError::EmptyHost)
            } else {
                Ok((s.to_string(), None))
            }
        }
    }
}

fn parse_port(s: &str) -> Result<u16, ParseError> {
    if s.is_empty() {
        return Err(ParseError::InvalidPort(s.to_string()));
    }
    s.parse::<u16>()
        .map_err(|_| ParseError::InvalidPort(s.to_string()))
        .and_then(|p| {
            if p == 0 {
                Err(ParseError::InvalidPort(s.to_string()))
            } else {
                Ok(p)
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssh(user: Option<&str>, host: &str, port: Option<u16>, path: &str) -> ForgeUri {
        ForgeUri::Ssh(SshForgeUri {
            user: user.map(str::to_string),
            host: host.to_string(),
            port,
            path: path.to_string(),
        })
    }

    #[test]
    fn user_host_port_path() {
        assert_eq!(
            ForgeUri::parse("ssh://alice@host.example.com:2222/srv/forge").unwrap(),
            ssh(Some("alice"), "host.example.com", Some(2222), "/srv/forge"),
        );
    }

    #[test]
    fn host_path_only_defaults_user_and_port() {
        assert_eq!(
            ForgeUri::parse("ssh://host/srv/forge").unwrap(),
            ssh(None, "host", None, "/srv/forge"),
        );
    }

    #[test]
    fn user_host_path_no_port() {
        assert_eq!(
            ForgeUri::parse("ssh://alice@host/srv/forge").unwrap(),
            ssh(Some("alice"), "host", None, "/srv/forge"),
        );
    }

    #[test]
    fn host_port_path_no_user() {
        assert_eq!(
            ForgeUri::parse("ssh://host:2200/srv/forge").unwrap(),
            ssh(None, "host", Some(2200), "/srv/forge"),
        );
    }

    #[test]
    fn ipv6_bracketed_without_port() {
        assert_eq!(
            ForgeUri::parse("ssh://[::1]/srv/forge").unwrap(),
            ssh(None, "::1", None, "/srv/forge"),
        );
    }

    #[test]
    fn ipv6_bracketed_with_port_and_user() {
        assert_eq!(
            ForgeUri::parse("ssh://root@[2001:db8::1]:22/srv/forge").unwrap(),
            ssh(Some("root"), "2001:db8::1", Some(22), "/srv/forge"),
        );
    }

    #[test]
    fn ipv4_literal_host() {
        assert_eq!(
            ForgeUri::parse("ssh://10.0.0.5:22/srv/forge").unwrap(),
            ssh(None, "10.0.0.5", Some(22), "/srv/forge"),
        );
    }

    #[test]
    fn path_with_spaces_and_dots() {
        assert_eq!(
            ForgeUri::parse("ssh://host/srv/my forge/.git/../forge").unwrap(),
            ssh(None, "host", None, "/srv/my forge/.git/../forge"),
        );
    }

    #[test]
    fn rejects_no_scheme() {
        assert!(matches!(
            ForgeUri::parse("/local/path"),
            Err(ParseError::NoScheme(_))
        ));
        assert!(matches!(
            ForgeUri::parse("host:/path"),
            Err(ParseError::NoScheme(_))
        ));
    }

    #[test]
    fn rejects_unsupported_scheme() {
        assert!(matches!(
            ForgeUri::parse("http://host/path"),
            Err(ParseError::UnsupportedScheme(s)) if s == "http"
        ));
    }

    #[test]
    fn rejects_empty_authority() {
        assert!(matches!(
            ForgeUri::parse("ssh:///srv/forge"),
            Err(ParseError::EmptyAuthority(_))
        ));
    }

    #[test]
    fn rejects_empty_user() {
        assert!(matches!(
            ForgeUri::parse("ssh://@host/path"),
            Err(ParseError::EmptyUser(_))
        ));
    }

    #[test]
    fn rejects_empty_host() {
        assert!(matches!(
            ForgeUri::parse("ssh://alice@:22/path"),
            Err(ParseError::EmptyHost)
        ));
    }

    #[test]
    fn rejects_invalid_port_non_numeric() {
        assert!(matches!(
            ForgeUri::parse("ssh://host:abc/path"),
            Err(ParseError::InvalidPort(_))
        ));
    }

    #[test]
    fn rejects_invalid_port_zero() {
        assert!(matches!(
            ForgeUri::parse("ssh://host:0/path"),
            Err(ParseError::InvalidPort(_))
        ));
    }

    #[test]
    fn rejects_invalid_port_overflow() {
        assert!(matches!(
            ForgeUri::parse("ssh://host:65536/path"),
            Err(ParseError::InvalidPort(_))
        ));
    }

    #[test]
    fn rejects_missing_path() {
        assert!(matches!(
            ForgeUri::parse("ssh://host"),
            Err(ParseError::MissingPath)
        ));
        assert!(matches!(
            ForgeUri::parse("ssh://alice@host:22"),
            Err(ParseError::MissingPath)
        ));
    }

    #[test]
    fn rejects_malformed_bracketed_host_missing_close() {
        assert!(matches!(
            ForgeUri::parse("ssh://[::1/path"),
            Err(ParseError::MalformedBracketedHost(_))
        ));
    }

    #[test]
    fn rejects_malformed_bracketed_host_empty() {
        assert!(matches!(
            ForgeUri::parse("ssh://[]/path"),
            Err(ParseError::MalformedBracketedHost(_))
        ));
    }

    #[test]
    fn display_round_trips_user_host_port_path() {
        let original = "ssh://alice@host.example.com:2222/srv/forge";
        let parsed = ForgeUri::parse(original).unwrap();
        assert_eq!(parsed.to_string(), original);
    }

    #[test]
    fn display_round_trips_ipv6_bracketed() {
        let original = "ssh://root@[2001:db8::1]:22/srv/forge";
        let parsed = ForgeUri::parse(original).unwrap();
        assert_eq!(parsed.to_string(), original);
    }

    #[test]
    fn display_round_trips_minimal() {
        let original = "ssh://host/srv/forge";
        let parsed = ForgeUri::parse(original).unwrap();
        assert_eq!(parsed.to_string(), original);
    }
}
