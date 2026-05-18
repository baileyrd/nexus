//! BL-025 — Authentication for the MCP **Host** client.
//!
//! `mcp.toml` entries that target a remote transport (`transport = "http"`)
//! can declare an `[servers.<name>.auth]` table picking one of three
//! supported flows. Each variant resolves to a single HTTP `Authorization`
//! header value at connect time; the header is then handed off to rmcp's
//! [`StreamableHttpClientTransport`] via its `auth_header(...)` config.
//!
//! ```toml
//! # API key (custom header — not necessarily named `Authorization`):
//! [servers.alpha.auth]
//! type = "api_key"
//! header = "X-API-Key"
//! value = "sk-…"             # or: env = "ALPHA_API_KEY"
//!
//! # Static bearer token:
//! [servers.beta.auth]
//! type = "bearer"
//! token = "ey…"              # or: env = "BETA_TOKEN"
//!
//! # OAuth 2.0 client_credentials (RFC 6749 §4.4) — token fetched at
//! # connect time and used as a bearer:
//! [servers.gamma.auth]
//! type = "oauth_client_credentials"
//! token_url = "https://auth.example.com/oauth2/token"
//! client_id = "nexus"        # or: client_id_env = "GAMMA_CLIENT_ID"
//! client_secret = "secret"   # or: client_secret_env = "GAMMA_CLIENT_SECRET"
//! scope = "mcp:tools"        # optional
//! ```
//!
//! # Why a hand-rolled flow rather than `rmcp/auth`?
//!
//! rmcp 1.5 ships a full OAuth manager (`rmcp::transport::auth`) that
//! covers PKCE, refresh tokens, and DCR — but enabling it pulls in
//! `dep:reqwest` at rmcp's pinned version (0.13.x) plus extra TLS-feature
//! scaffolding the workspace doesn't otherwise need, and exposes a
//! `OAuthState`-style stateful surface that's overkill for the
//! single-shot client-credentials flow PRD-14 §8 calls for. The handful
//! of lines below is faster to audit and avoids a second `reqwest`
//! version shipping through the rmcp dep — `auth.rs` uses the
//! workspace's `reqwest 0.12` which is already in the closure for the
//! AI provider crates.
//!
//! # ADR-0009 (keychain hard-fail) interaction
//!
//! Today the resolver supports inline values + env-var indirection.
//! When ADR-0009 lands a real keyring service, [`McpAuthSecret::Env`]'s
//! sibling `Keyring { service, account }` variant slots in alongside it
//! without a wire-format break — every consumer of [`ResolvedAuth`]
//! sees the same final string regardless of where it came from.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One of the three auth flows supported per BL-025.
///
/// Tagged via serde's `#[serde(tag = "type", rename_all = "snake_case")]`
/// so a `mcp.toml` entry reads naturally:
///
/// ```toml
/// [servers.x.auth]
/// type = "bearer"
/// token = "…"
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpAuth {
    /// Static API key — sent on a caller-named header (defaults to
    /// `Authorization`). Use this for servers that take a non-OAuth
    /// "drop the key on the X-Foo header" scheme.
    ApiKey {
        /// Header name to set. Defaults to `Authorization` when omitted.
        #[serde(default = "default_api_key_header")]
        header: String,
        /// The API key — either inline or via env-var indirection.
        #[serde(flatten)]
        value: McpAuthSecret,
    },
    /// Static bearer token — equivalent to `ApiKey { header:
    /// "Authorization", value: "Bearer <token>" }` but the more familiar
    /// shape per RFC 6750.
    Bearer {
        /// The bearer token (without the `Bearer ` prefix).
        #[serde(flatten)]
        token: McpAuthSecret,
    },
    /// OAuth 2.0 `client_credentials` flow (RFC 6749 §4.4). The resolver
    /// fetches an access token at connect time and uses it as a bearer.
    /// Refresh-on-401 is not implemented — token is refetched on the
    /// next `connect()`.
    OauthClientCredentials {
        /// Token endpoint URL.
        token_url: String,
        /// Client id — inline or env.
        #[serde(flatten)]
        client_id: ClientIdSecret,
        /// Client secret — inline or env.
        #[serde(flatten)]
        client_secret: ClientSecretSecret,
        /// Optional space-separated scope string.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<String>,
    },
}

fn default_api_key_header() -> String {
    "Authorization".to_string()
}

/// Either an inline secret value or a pointer to an environment variable.
///
/// Always exactly one form on the wire — serde's `#[serde(untagged)]`
/// resolves which based on which key is present (`value` vs `env`). A
/// future ADR-0009 keyring variant slots in here without breaking the
/// existing TOML.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum McpAuthSecret {
    /// Inline literal value — convenient for development, surfaces in
    /// the `mcp.toml` file verbatim.
    Inline {
        /// The literal secret.
        value: String,
    },
    /// Pull the secret from an environment variable. Missing/empty env
    /// vars surface as [`AuthError::MissingEnv`] at resolve time.
    Env {
        /// Env-var name.
        env: String,
    },
}

/// Specialisation of [`McpAuthSecret`] for OAuth `client_id`. Same
/// shape, different field names so `client_id` / `client_id_env` read
/// naturally in the TOML.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ClientIdSecret {
    /// Inline `client_id`.
    Inline {
        /// The client id literal.
        client_id: String,
    },
    /// `client_id_env = "VAR"`.
    Env {
        /// Env-var holding the client id.
        client_id_env: String,
    },
}

/// Specialisation of [`McpAuthSecret`] for OAuth `client_secret`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ClientSecretSecret {
    /// Inline `client_secret`.
    Inline {
        /// The client secret literal.
        client_secret: String,
    },
    /// `client_secret_env = "VAR"`.
    Env {
        /// Env-var holding the client secret.
        client_secret_env: String,
    },
}

/// What the resolver hands back to [`crate::client::McpClient`]. The
/// wire shape after resolve is "extra HTTP headers" — every flow
/// reduces to one or more headers attached to the rmcp transport.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedAuth {
    /// Goes into rmcp's `auth_header(...)` (the `Authorization` value,
    /// without the leading `Authorization:` name).
    pub authorization: Option<String>,
    /// Other custom headers (e.g. an API-key on `X-API-Key`).
    pub extra_headers: BTreeMap<String, String>,
}

/// Errors from the auth resolver. All recoverable from an operator's
/// perspective — fix the config / set the env var / get the OAuth
/// server up — so we surface enough context to act on.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// An env-var-indirected secret was unset or empty at resolve time.
    #[error("env var '{name}' is unset or empty")]
    MissingEnv {
        /// The env-var name from the spec.
        name: String,
    },
    /// The OAuth token endpoint refused or could not be reached.
    #[error("oauth token endpoint {url}: {reason}")]
    Oauth {
        /// Endpoint URL we tried.
        url: String,
        /// Human-readable reason.
        reason: String,
    },
    /// The OAuth response was missing `access_token` or had a non-bearer
    /// `token_type`.
    #[error("oauth token response invalid: {reason}")]
    OauthResponse {
        /// What was wrong with the response body.
        reason: String,
    },
}

impl McpAuthSecret {
    fn resolve(&self, label: &str) -> Result<String, AuthError> {
        match self {
            Self::Inline { value } => Ok(value.clone()),
            Self::Env { env } => read_env(env, label),
        }
    }
}

impl ClientIdSecret {
    fn resolve(&self) -> Result<String, AuthError> {
        match self {
            Self::Inline { client_id } => Ok(client_id.clone()),
            Self::Env { client_id_env } => read_env(client_id_env, "client_id"),
        }
    }
}

impl ClientSecretSecret {
    fn resolve(&self) -> Result<String, AuthError> {
        match self {
            Self::Inline { client_secret } => Ok(client_secret.clone()),
            Self::Env { client_secret_env } => read_env(client_secret_env, "client_secret"),
        }
    }
}

fn read_env(name: &str, _label: &str) -> Result<String, AuthError> {
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => Ok(v),
        _ => Err(AuthError::MissingEnv {
            name: name.to_string(),
        }),
    }
}

/// Token response shape for OAuth 2.0 `client_credentials`. We accept
/// any extra fields the provider returns — `token_type` is required to
/// be `bearer` (case-insensitive) per RFC 6749 §5.1.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
}

/// Resolve an [`McpAuth`] declaration to wire-ready headers.
///
/// Pure for [`McpAuth::ApiKey`] / [`McpAuth::Bearer`] (no I/O). The
/// OAuth flow makes one HTTP POST to the token endpoint using the
/// workspace's `reqwest` 0.12 client. A 30-second timeout caps the
/// fetch so a hung token endpoint can't stall MCP connect.
///
/// # Errors
/// - [`AuthError::MissingEnv`] for an env-indirected secret with no
///   value.
/// - [`AuthError::Oauth`] / [`AuthError::OauthResponse`] for token
///   endpoint problems.
pub async fn resolve(auth: &McpAuth) -> Result<ResolvedAuth, AuthError> {
    match auth {
        McpAuth::ApiKey { header, value } => {
            let v = value.resolve("api_key")?;
            let mut out = ResolvedAuth::default();
            // Authorization is the canonical header — surface it on the
            // dedicated rmcp slot so the transport applies the same
            // 401-handling it would for any other auth header.
            if header.eq_ignore_ascii_case("authorization") {
                out.authorization = Some(v);
            } else {
                out.extra_headers.insert(header.clone(), v);
            }
            Ok(out)
        }
        McpAuth::Bearer { token } => {
            let raw = token.resolve("bearer")?;
            // Allow callers to write either `"ey…"` or `"Bearer ey…"`
            // — the latter shows up when migrating from a hand-rolled
            // `auth_header = "Bearer …"` setup.
            let header_value = if raw.to_lowercase().starts_with("bearer ") {
                raw
            } else {
                format!("Bearer {raw}")
            };
            Ok(ResolvedAuth {
                authorization: Some(header_value),
                extra_headers: BTreeMap::new(),
            })
        }
        McpAuth::OauthClientCredentials {
            token_url,
            client_id,
            client_secret,
            scope,
        } => {
            let client_id = client_id.resolve()?;
            let client_secret = client_secret.resolve()?;
            let token = fetch_client_credentials_token(
                token_url,
                &client_id,
                &client_secret,
                scope.as_deref(),
            )
            .await?;
            Ok(ResolvedAuth {
                authorization: Some(format!("Bearer {token}")),
                extra_headers: BTreeMap::new(),
            })
        }
    }
}

/// P2-06 — wall-clock budget for the OAuth token POST. Picked at the
/// same order of magnitude as `client::CONNECT_TIMEOUT` (15 s) so a
/// stalled token endpoint doesn't outlive the rmcp handshake budget
/// by much. Override via a future `[mcp.timeouts] oauth_secs = N`
/// block (deferred from P2-06).
pub const DEFAULT_OAUTH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const OAUTH_TIMEOUT: std::time::Duration = DEFAULT_OAUTH_TIMEOUT;

async fn fetch_client_credentials_token(
    token_url: &str,
    client_id: &str,
    client_secret: &str,
    scope: Option<&str>,
) -> Result<String, AuthError> {
    // RFC 6749 §4.4.2 + §2.3.1: POST grant_type=client_credentials with
    // client_id/secret in the body OR HTTP Basic auth. We use HTTP
    // Basic for the credentials (the more interoperable path; some
    // providers only accept that) and put `grant_type` + `scope` in
    // the form body.
    let mut form: Vec<(&str, &str)> = vec![("grant_type", "client_credentials")];
    if let Some(s) = scope {
        if !s.is_empty() {
            form.push(("scope", s));
        }
    }
    let client = reqwest::Client::builder()
        .timeout(OAUTH_TIMEOUT)
        .build()
        .map_err(|e| AuthError::Oauth {
            url: token_url.to_string(),
            reason: format!("client build failed: {e}"),
        })?;
    let resp = client
        .post(token_url)
        .basic_auth(client_id, Some(client_secret))
        .form(&form)
        .send()
        .await
        .map_err(|e| AuthError::Oauth {
            url: token_url.to_string(),
            reason: e.to_string(),
        })?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AuthError::Oauth {
            url: token_url.to_string(),
            reason: format!("HTTP {status}: {body}"),
        });
    }
    let parsed: TokenResponse = resp.json().await.map_err(|e| AuthError::OauthResponse {
        reason: format!("decode token response: {e}"),
    })?;
    if let Some(tt) = parsed.token_type.as_deref() {
        if !tt.eq_ignore_ascii_case("bearer") {
            return Err(AuthError::OauthResponse {
                reason: format!("unsupported token_type '{tt}'"),
            });
        }
    }
    if parsed.access_token.is_empty() {
        return Err(AuthError::OauthResponse {
            reason: "missing access_token".to_string(),
        });
    }
    Ok(parsed.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn api_key_inline_authorization_lands_on_authorization_slot() {
        let auth = McpAuth::ApiKey {
            header: "Authorization".to_string(),
            value: McpAuthSecret::Inline {
                value: "Bearer xyz".to_string(),
            },
        };
        let resolved = resolve(&auth).await.unwrap();
        assert_eq!(resolved.authorization.as_deref(), Some("Bearer xyz"));
        assert!(resolved.extra_headers.is_empty());
    }

    #[tokio::test]
    async fn api_key_custom_header_lands_on_extras() {
        let auth = McpAuth::ApiKey {
            header: "X-API-Key".to_string(),
            value: McpAuthSecret::Inline {
                value: "abc".to_string(),
            },
        };
        let resolved = resolve(&auth).await.unwrap();
        assert!(resolved.authorization.is_none());
        assert_eq!(resolved.extra_headers.get("X-API-Key").unwrap(), "abc");
    }

    #[tokio::test]
    async fn bearer_inline_prepends_scheme_when_missing() {
        let auth = McpAuth::Bearer {
            token: McpAuthSecret::Inline {
                value: "xyz".to_string(),
            },
        };
        let resolved = resolve(&auth).await.unwrap();
        assert_eq!(resolved.authorization.as_deref(), Some("Bearer xyz"));
    }

    #[tokio::test]
    async fn bearer_inline_preserves_caller_supplied_scheme() {
        let auth = McpAuth::Bearer {
            token: McpAuthSecret::Inline {
                value: "Bearer xyz".to_string(),
            },
        };
        let resolved = resolve(&auth).await.unwrap();
        assert_eq!(resolved.authorization.as_deref(), Some("Bearer xyz"));
    }

    #[tokio::test]
    async fn env_indirection_resolves_when_set() {
        let var = "NEXUS_TEST_BL025_TOKEN";
        // SAFETY: tests in the same crate share a process; this var is
        // unique to this test, and we tear it down on the way out.
        // SAFETY: process-global env mutation; bracketed for the test body.
        unsafe { std::env::set_var(var, "from-env") };
        let auth = McpAuth::Bearer {
            token: McpAuthSecret::Env {
                env: var.to_string(),
            },
        };
        let resolved = resolve(&auth).await.unwrap();
        assert_eq!(resolved.authorization.as_deref(), Some("Bearer from-env"));
        unsafe { std::env::remove_var(var) };
    }

    #[tokio::test]
    async fn env_indirection_errors_when_unset() {
        let var = "NEXUS_TEST_BL025_DEFINITELY_UNSET";
        unsafe { std::env::remove_var(var) };
        let auth = McpAuth::Bearer {
            token: McpAuthSecret::Env {
                env: var.to_string(),
            },
        };
        let err = resolve(&auth).await.unwrap_err();
        assert!(matches!(err, AuthError::MissingEnv { .. }), "got {err:?}");
    }

    #[test]
    fn toml_parses_api_key_with_env_indirection() {
        // Round-trip the McpAuth shape standalone (no full `mcp.toml`)
        // to keep this test focused on the auth-side parser.
        #[derive(Deserialize)]
        struct Wrap {
            auth: McpAuth,
        }
        let toml_text = r#"
            [auth]
            type = "api_key"
            header = "X-API-Key"
            env = "MY_KEY"
        "#;
        let wrap: Wrap = toml::from_str(toml_text).unwrap();
        match wrap.auth {
            McpAuth::ApiKey { header, value } => {
                assert_eq!(header, "X-API-Key");
                match value {
                    McpAuthSecret::Env { env } => assert_eq!(env, "MY_KEY"),
                    McpAuthSecret::Inline { .. } => panic!("expected Env, got Inline"),
                }
            }
            other => panic!("expected ApiKey, got {other:?}"),
        }
    }

    #[test]
    fn toml_parses_oauth_client_credentials() {
        #[derive(Deserialize)]
        struct Wrap {
            auth: McpAuth,
        }
        let toml_text = r#"
            [auth]
            type = "oauth_client_credentials"
            token_url = "https://auth.example.com/token"
            client_id = "id"
            client_secret_env = "SECRET"
            scope = "mcp:tools"
        "#;
        let wrap: Wrap = toml::from_str(toml_text).unwrap();
        match wrap.auth {
            McpAuth::OauthClientCredentials {
                token_url,
                client_id,
                client_secret,
                scope,
            } => {
                assert_eq!(token_url, "https://auth.example.com/token");
                assert!(matches!(
                    client_id,
                    ClientIdSecret::Inline { client_id: ref s } if s == "id"
                ));
                assert!(matches!(
                    client_secret,
                    ClientSecretSecret::Env { ref client_secret_env } if client_secret_env == "SECRET"
                ));
                assert_eq!(scope.as_deref(), Some("mcp:tools"));
            }
            other => panic!("expected OauthClientCredentials, got {other:?}"),
        }
    }
}
