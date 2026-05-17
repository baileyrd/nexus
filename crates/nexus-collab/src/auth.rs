//! Shared-secret token verification for the BL-143 relay.
//!
//! Phase 1 uses a single static token configured at `nexus collab serve`
//! time and passed by clients in the [`crate::protocol::ClientMessage::Hello`]
//! frame. Tokens are compared in constant time so a network attacker
//! cannot infer the secret from response-timing differences. Hosted /
//! per-user credentials are deferred to a later phase along with TLS.

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
}
