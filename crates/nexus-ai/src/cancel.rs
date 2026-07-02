//! C26 (#379) — session-keyed cooperative cancellation for streaming
//! chat.
//!
//! `handle_stream_chat` registers a flag under its `session_id` and
//! installs it on the per-request provider; the `cancel_stream` IPC
//! verb sets it. Streaming providers check the flag between SSE
//! chunks and bail with [`crate::error::AiError::Cancelled`], dropping
//! the HTTP response — which closes the connection, so the provider
//! stops generating (and billing) mid-stream instead of running to
//! completion as it did pre-C26.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

type Registry = Mutex<HashMap<String, Arc<AtomicBool>>>;

fn registry() -> &'static Registry {
    static REG: OnceLock<Registry> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a fresh cancel flag for `session_id`, replacing any stale
/// one. Returns the flag to install on the provider. Pair with
/// [`CancelGuard`] so the entry is swept on every handler exit path.
pub(crate) fn register(session_id: &str) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut reg) = registry().lock() {
        reg.insert(session_id.to_string(), Arc::clone(&flag));
    }
    flag
}

/// Fire the cancel flag for `session_id`. Returns `true` when a live
/// stream was found (and flagged), `false` when nothing matched —
/// either id unknown or the stream already finished.
pub(crate) fn cancel(session_id: &str) -> bool {
    registry()
        .lock()
        .ok()
        .and_then(|reg| {
            reg.get(session_id).map(|flag| {
                flag.store(true, Ordering::Relaxed);
                true
            })
        })
        .unwrap_or(false)
}

/// RAII sweep of a registered session flag.
pub(crate) struct CancelGuard(pub(crate) String);

impl Drop for CancelGuard {
    fn drop(&mut self) {
        if let Ok(mut reg) = registry().lock() {
            reg.remove(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_cancel_and_guard_sweep() {
        let flag = register("s1");
        assert!(!flag.load(Ordering::Relaxed));
        assert!(cancel("s1"), "live session cancels");
        assert!(flag.load(Ordering::Relaxed), "flag fired");
        {
            let _guard = CancelGuard("s1".to_string());
        }
        assert!(!cancel("s1"), "swept session no longer cancellable");
        assert!(!cancel("never-registered"));
    }
}
