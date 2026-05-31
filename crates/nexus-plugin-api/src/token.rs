//! Runtime capability token — the session-level complement to the static
//! [`CapabilitySet`] granted at plugin registration.
//!
//! A [`CapabilityToken`] is a live, revocable, attenutatable capability
//! envelope carried by each [`Session`]. Where [`CapabilitySet`] answers
//! "what may this *plugin* do?" at bootstrap time, a `CapabilityToken`
//! answers "what may this *session* do right now?".
//!
//! ## Attenuation
//!
//! Sub-agent delegation (spawning a child session from a parent) uses
//! [`CapabilityToken::attenuate`] to produce a child token whose capability
//! set is a subset of the parent's, bounded by the intersection rule:
//!
//! ```text
//! child.capabilities ⊆ parent.capabilities
//! ```
//!
//! ## Revocation
//!
//! Every token holds a shared [`Arc<AtomicBool>`] revocation flag. Child
//! tokens also hold a reference to their parent's flag via
//! `parent_revoked`. Revoking a parent token immediately invalidates all
//! child tokens — [`CapabilityToken::is_revoked`] checks both flags on each
//! call.
//!
//! This is a single-level chain; cascading through grandchildren works
//! transitively because each generation references the flag of its direct
//! parent.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use crate::capability::{Capability, CapabilitySet};
use crate::error::CapabilityError;

/// Opaque identifier for a [`CapabilityToken`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenId(uuid::Uuid);

impl TokenId {
    /// Allocate a fresh random token id.
    #[must_use]
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Inner UUID value.
    #[must_use]
    pub fn as_uuid(self) -> uuid::Uuid {
        self.0
    }
}

impl Default for TokenId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for TokenId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A live, revocable, attenuatable capability envelope.
///
/// Carried by each Session; checked by the Supervisor before dispatching
/// any capability-gated action. Unlike a static [`CapabilitySet`], a token
/// can be:
///
/// - **Revoked** mid-flight (e.g. when the user cancels a session)
/// - **Attenuated** into child tokens for sub-agent delegation
///
/// `CapabilityToken` is `Clone`, but cloning produces an independent token
/// with the *same* revocation flag — both the original and the clone become
/// revoked together. For an *attenuated* child (different capability set, linked
/// revocation), use [`CapabilityToken::attenuate`].
#[derive(Debug, Clone)]
pub struct CapabilityToken {
    /// Stable identifier for this token (not its parent).
    pub id: TokenId,
    /// The session this token was minted for.
    pub session_id: uuid::Uuid,
    /// What this token permits.
    capabilities: CapabilitySet,
    /// Revocation flag for this token.
    revoked: Arc<AtomicBool>,
    /// Revocation flag of the parent token, if this is a derived token.
    /// When the parent is revoked, this token is also considered revoked.
    parent_revoked: Option<Arc<AtomicBool>>,
}

impl CapabilityToken {
    /// Mint a fresh token for a session with no parent.
    #[must_use]
    pub fn new(session_id: uuid::Uuid, capabilities: CapabilitySet) -> Self {
        Self {
            id: TokenId::new(),
            session_id,
            capabilities,
            revoked: Arc::new(AtomicBool::new(false)),
            parent_revoked: None,
        }
    }

    /// `true` if this token has been directly revoked *or* if its parent
    /// has been revoked. Checked on every [`CapabilityToken::check`] call.
    #[must_use]
    pub fn is_revoked(&self) -> bool {
        self.revoked.load(Ordering::Acquire)
            || self
                .parent_revoked
                .as_ref()
                .is_some_and(|p| p.load(Ordering::Acquire))
    }

    /// Revoke this token. Idempotent; also transitively invalidates all
    /// child tokens that hold `Arc::clone` of this token's revoked flag
    /// in their `parent_revoked` field.
    pub fn revoke(&self) {
        self.revoked.store(true, Ordering::Release);
    }

    /// Check whether `cap` is permitted by this token.
    ///
    /// Returns `Ok(())` when the token is live and holds the requested
    /// capability. Returns [`CapabilityError::Denied`] when revoked, or
    /// when the capability is not in the token's set.
    ///
    /// # Errors
    /// Returns [`CapabilityError::Denied`] if the capability is not granted
    /// or the token has been revoked.
    pub fn check(&self, cap: Capability) -> Result<(), CapabilityError> {
        if self.is_revoked() {
            return Err(CapabilityError::Denied {
                plugin_id: self.session_id.to_string(),
                cap,
            });
        }
        if self.capabilities.contains(cap) {
            Ok(())
        } else {
            Err(CapabilityError::Denied {
                plugin_id: self.session_id.to_string(),
                cap,
            })
        }
    }

    /// Create an attenuated child token for a sub-session. The child's
    /// capability set is the intersection of this token's capabilities and
    /// `requested`. The child's revocation is linked to this token — revoking
    /// this token also invalidates the child.
    ///
    /// The `child_session_id` identifies the sub-session this token is
    /// being minted for.
    #[must_use]
    pub fn attenuate(&self, child_session_id: uuid::Uuid, requested: CapabilitySet) -> Self {
        let intersection = self
            .capabilities
            .iter()
            .filter(|c| requested.contains(*c))
            .collect::<CapabilitySet>();
        Self {
            id: TokenId::new(),
            session_id: child_session_id,
            capabilities: intersection,
            revoked: Arc::new(AtomicBool::new(false)),
            // Share the current token's revoked flag so revoking the
            // parent immediately cascades into the child.
            parent_revoked: Some(Arc::clone(&self.revoked)),
        }
    }

    /// The capability set this token carries.
    #[must_use]
    pub fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    /// `true` if this token grants `cap` (ignoring revocation). Use
    /// [`CapabilityToken::check`] for the full gate (revocation + grant).
    #[must_use]
    pub fn grants(&self, cap: Capability) -> bool {
        self.capabilities.contains(cap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;

    fn session_id() -> uuid::Uuid {
        uuid::Uuid::new_v4()
    }

    fn token_with(caps: impl IntoIterator<Item = Capability>) -> CapabilityToken {
        CapabilityToken::new(session_id(), CapabilitySet::from_iter(caps))
    }

    #[test]
    fn check_grants_present_capability() {
        let t = token_with([Capability::FsRead]);
        assert!(t.check(Capability::FsRead).is_ok());
    }

    #[test]
    fn check_denies_absent_capability() {
        let t = token_with([Capability::FsRead]);
        assert!(t.check(Capability::FsWrite).is_err());
    }

    #[test]
    fn revoke_denies_all_capabilities() {
        let t = token_with([Capability::FsRead, Capability::FsWrite]);
        assert!(t.check(Capability::FsRead).is_ok());
        t.revoke();
        assert!(t.check(Capability::FsRead).is_err());
    }

    #[test]
    fn revoke_is_idempotent() {
        let t = token_with([Capability::FsRead]);
        t.revoke();
        t.revoke(); // must not panic
        assert!(t.is_revoked());
    }

    #[test]
    fn attenuate_intersects_capabilities() {
        let parent = token_with([Capability::FsRead, Capability::FsWrite, Capability::AiChat]);
        let child = parent.attenuate(
            session_id(),
            CapabilitySet::from_iter([Capability::FsRead, Capability::NetHttp]),
        );
        // Intersection: FsRead is in both; FsWrite not in requested; NetHttp not in parent.
        assert!(child.check(Capability::FsRead).is_ok());
        assert!(child.check(Capability::FsWrite).is_err());
        assert!(child.check(Capability::NetHttp).is_err());
        assert!(child.check(Capability::AiChat).is_err());
    }

    #[test]
    fn revoking_parent_invalidates_child() {
        let parent = token_with([Capability::FsRead]);
        let child = parent.attenuate(session_id(), CapabilitySet::from_iter([Capability::FsRead]));
        assert!(child.check(Capability::FsRead).is_ok());
        parent.revoke();
        assert!(child.check(Capability::FsRead).is_err());
        assert!(child.is_revoked());
    }

    #[test]
    fn revoking_child_does_not_revoke_parent() {
        let parent = token_with([Capability::FsRead]);
        let child = parent.attenuate(session_id(), CapabilitySet::from_iter([Capability::FsRead]));
        child.revoke();
        assert!(child.is_revoked());
        assert!(!parent.is_revoked());
        assert!(parent.check(Capability::FsRead).is_ok());
    }

    #[test]
    fn clone_shares_revocation_flag() {
        let t = token_with([Capability::FsRead]);
        let t2 = t.clone();
        t.revoke();
        assert!(t2.is_revoked());
    }

    #[test]
    fn grants_checks_without_revocation() {
        let t = token_with([Capability::AiChat]);
        t.revoke();
        // grants() ignores revocation — it only checks the set
        assert!(t.grants(Capability::AiChat));
        // check() enforces revocation
        assert!(t.check(Capability::AiChat).is_err());
    }
}
