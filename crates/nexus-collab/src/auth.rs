//! Shared-secret token verification for the BL-143 relay.
//!
//! Phase 1 shipped a single static token configured at `nexus collab
//! serve` time and passed by clients in the
//! [`crate::protocol::ClientMessage::Hello`] frame. [`TokenSet`]
//! (gap-analysis 2026-07-01 §1.4) generalises that to **named
//! per-user tokens**: the relay accepts any member of the set and
//! learns *which* credential authenticated, so joins are attributable
//! and a single user's token can be rotated or revoked without
//! re-keying every peer. A one-entry set is wire- and
//! behaviour-identical to Phase 1. Tokens are compared in constant
//! time so a network attacker cannot infer a secret from
//! response-timing differences. TLS remains deferred (front the relay
//! with a TLS-terminating proxy until then).

/// Newtype around the configured shared-secret token. Constructed at
/// server-setup time; `verify` runs on every successful handshake.
#[derive(Clone, Debug)]
pub struct Token(String);

/// Reasons [`Token::new`] may reject a candidate token.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    /// Empty tokens disable the entire auth check; the relay refuses
    /// to start with one.
    #[error("token must not be empty")]
    Empty,
}

impl Token {
    /// Wrap a non-empty shared secret.
    ///
    /// # Errors
    /// Returns [`TokenError::Empty`] if `secret` is empty.
    pub fn new(secret: impl Into<String>) -> Result<Self, TokenError> {
        let s = secret.into();
        if s.is_empty() {
            return Err(TokenError::Empty);
        }
        Ok(Self(s))
    }

    /// Constant-time compare a candidate against the stored secret.
    /// Returns `true` on match.
    #[must_use]
    pub fn verify(&self, candidate: &str) -> bool {
        constant_time_eq(self.0.as_bytes(), candidate.as_bytes())
    }
}

/// Constant-time equality. Returns `false` on length mismatch
/// immediately (length is not secret in our threat model) and otherwise
/// XOR-accumulates every byte so the loop's branch behaviour does not
/// depend on early matches.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// A named collection of accepted relay tokens (per-user credentials).
///
/// `verify` scans every entry with the same constant-time comparison as
/// [`Token::verify`] and returns the **name** of the entry that
/// matched, so callers can attribute the connection and revoke or
/// rotate one user without re-keying the rest. All entries are always
/// scanned — match position is not observable from timing.
#[derive(Clone, Debug)]
pub struct TokenSet {
    entries: Vec<(String, Token)>,
}

/// Reasons [`TokenSet::new`] may reject a candidate set.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TokenSetError {
    /// An empty set would disable auth entirely; the relay refuses it.
    #[error("token set must not be empty")]
    Empty,
    /// Entry names must be unique so revocation and attribution are
    /// unambiguous.
    #[error("duplicate token name {0:?}")]
    DuplicateName(String),
    /// One of the entries carried an invalid token.
    #[error("token {0:?}: {1}")]
    Token(String, TokenError),
}

impl TokenSet {
    /// Build a set from `(name, secret)` pairs.
    ///
    /// # Errors
    /// Returns [`TokenSetError`] on an empty set, a duplicate name, or
    /// an empty secret.
    pub fn new<N, S>(pairs: impl IntoIterator<Item = (N, S)>) -> Result<Self, TokenSetError>
    where
        N: Into<String>,
        S: Into<String>,
    {
        let mut entries: Vec<(String, Token)> = Vec::new();
        for (name, secret) in pairs {
            let name = name.into();
            if entries.iter().any(|(n, _)| *n == name) {
                return Err(TokenSetError::DuplicateName(name));
            }
            let token = Token::new(secret).map_err(|e| TokenSetError::Token(name.clone(), e))?;
            entries.push((name, token));
        }
        if entries.is_empty() {
            return Err(TokenSetError::Empty);
        }
        Ok(Self { entries })
    }

    /// Wrap a single Phase-1 token as a one-entry set named `default`.
    #[must_use]
    pub fn single(token: Token) -> Self {
        Self {
            entries: vec![("default".to_string(), token)],
        }
    }

    /// Constant-time verify `candidate` against every entry. Returns
    /// the matching entry's name, or `None` when nothing matched.
    /// Every entry is always compared (no early exit), so timing does
    /// not reveal which position matched.
    #[must_use]
    pub fn verify(&self, candidate: &str) -> Option<&str> {
        let mut matched: Option<&str> = None;
        for (name, token) in &self.entries {
            if token.verify(candidate) && matched.is_none() {
                matched = Some(name.as_str());
            }
        }
        matched
    }

    /// Names in the set, in insertion order (for `collab token list`
    /// style surfaces; secrets are never exposed).
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(n, _)| n.as_str())
    }
}

impl From<Token> for TokenSet {
    fn from(token: Token) -> Self {
        Self::single(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_token_rejected() {
        assert_eq!(Token::new("").unwrap_err(), TokenError::Empty);
    }

    #[test]
    fn matching_token_verifies() {
        let t = Token::new("hunter2").unwrap();
        assert!(t.verify("hunter2"));
    }

    #[test]
    fn wrong_token_rejected() {
        let t = Token::new("hunter2").unwrap();
        assert!(!t.verify("hunter3"));
    }

    #[test]
    fn length_mismatch_rejected() {
        let t = Token::new("hunter2").unwrap();
        assert!(!t.verify("hunter22"));
        assert!(!t.verify("hunter"));
    }

    #[test]
    fn constant_time_eq_matches_basic_cases() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn token_set_verifies_and_attributes_by_name() {
        let set = TokenSet::new([("alice", "secret-a"), ("bob", "secret-b")]).unwrap();
        assert_eq!(set.verify("secret-a"), Some("alice"));
        assert_eq!(set.verify("secret-b"), Some("bob"));
        assert_eq!(set.verify("wrong"), None);
        assert_eq!(set.names().collect::<Vec<_>>(), vec!["alice", "bob"]);
    }

    #[test]
    fn token_set_rejects_empty_duplicate_and_blank() {
        assert_eq!(
            TokenSet::new(Vec::<(String, String)>::new()).unwrap_err(),
            TokenSetError::Empty
        );
        assert_eq!(
            TokenSet::new([("a", "x"), ("a", "y")]).unwrap_err(),
            TokenSetError::DuplicateName("a".to_string())
        );
        assert!(matches!(
            TokenSet::new([("a", "")]).unwrap_err(),
            TokenSetError::Token(_, TokenError::Empty)
        ));
    }

    #[test]
    fn single_token_set_matches_phase1_behaviour() {
        let set = TokenSet::single(Token::new("hunter2").unwrap());
        assert_eq!(set.verify("hunter2"), Some("default"));
        assert_eq!(set.verify("nope"), None);
    }
}
