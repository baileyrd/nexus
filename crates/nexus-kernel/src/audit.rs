//! Structured audit event helpers.
//!
//! Audit events are `tracing` events with structured fields. Output
//! destination (rolling file, stderr, etc.) is configured by the binary
//! crate via `tracing-subscriber` + `tracing-appender`. This module only
//! emits events.
//!
//! All audit events carry `audit = true` as a structured field so
//! downstream subscribers can filter them from general application logs.

use std::path::Path;

/// Log a capability grant event.
pub fn log_capability_granted(plugin_id: &str, capability: &str) {
    tracing::info!(
        audit = true,
        plugin_id,
        capability,
        result = "granted",
        "capability granted"
    );
    crate::audit_store::append(
        "capability_granted",
        Some(plugin_id),
        &serde_json::json!({ "capability": capability }),
    );
    if let Some(m) = crate::metrics::global() {
        m.record_capability_check(plugin_id, capability, true);
    }
}

/// Log a runtime capability revocation (BL-096). Emitted by the plugin
/// loader when a HIGH-risk grant is rescinded — both for the disk-side
/// `granted_caps.json` update and (when a context is wired) the
/// in-memory cap set on the running plugin's `KernelPluginContext`.
pub fn log_capability_revoked(plugin_id: &str, capability: &str) {
    tracing::warn!(
        audit = true,
        plugin_id,
        capability,
        result = "revoked",
        "capability revoked"
    );
    crate::audit_store::append(
        "capability_revoked",
        Some(plugin_id),
        &serde_json::json!({ "capability": capability }),
    );
}

/// Log a capability denial event.
pub fn log_capability_denied(plugin_id: &str, capability: &str) {
    tracing::warn!(
        audit = true,
        plugin_id,
        capability,
        result = "denied",
        "capability denied"
    );
    crate::audit_store::append(
        "capability_denied",
        Some(plugin_id),
        &serde_json::json!({ "capability": capability }),
    );
    if let Some(m) = crate::metrics::global() {
        m.record_capability_check(plugin_id, capability, false);
    }
}

/// Log a plugin lifecycle transition (e.g. "loaded", "initialized", "started", "stopped", "crashed").
pub fn log_plugin_lifecycle(plugin_id: &str, transition: &str) {
    tracing::info!(
        audit = true,
        plugin_id,
        transition,
        "plugin lifecycle"
    );
    crate::audit_store::append(
        "plugin_lifecycle",
        Some(plugin_id),
        &serde_json::json!({ "transition": transition }),
    );
}

/// Log a credential access event. The credential value is never logged.
pub fn log_credential_access(credential_name: &str, action: &str) {
    tracing::info!(
        audit = true,
        credential_name,
        action,
        "credential access"
    );
    crate::audit_store::append(
        "credential_access",
        None,
        &serde_json::json!({
            "credential_name": credential_name,
            "action": action,
        }),
    );
}

/// Log a path traversal denial.
pub fn log_path_traversal_denied(plugin_id: &str, requested_path: &Path, forge_root: &Path) {
    tracing::warn!(
        audit = true,
        plugin_id,
        requested_path = %requested_path.display(),
        forge_root = %forge_root.display(),
        "path traversal denied"
    );
    crate::audit_store::append(
        "path_traversal_denied",
        Some(plugin_id),
        &serde_json::json!({
            "requested_path": requested_path.display().to_string(),
            "forge_root": forge_root.display().to_string(),
        }),
    );
}

/// Test-only helpers for capturing audit events. Used by audit.rs's own
/// tests and by call-site coverage tests in `context_impl.rs` to prove that
/// gates route through `audit::*` rather than emitting ad-hoc warns.
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::layer::SubscriberExt;

    struct CaptureLayer {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CaptureLayer {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            let mut visitor = StringVisitor(String::new());
            event.record(&mut visitor);
            self.events.lock().unwrap().push(visitor.0);
        }
    }

    struct StringVisitor(String);

    impl tracing::field::Visit for StringVisitor {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            use std::fmt::Write;
            let _ = write!(self.0, "{}={:?} ", field.name(), value);
        }

        fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
            use std::fmt::Write;
            let _ = write!(self.0, "{}={} ", field.name(), value);
        }

        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            use std::fmt::Write;
            let _ = write!(self.0, "{}={} ", field.name(), value);
        }
    }

    /// Run `f` with a tracing subscriber installed, returning every event
    /// emitted during the call as a flat `field=value field=value ...` string.
    pub(crate) fn with_captured_events(f: impl FnOnce()) -> Vec<String> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let layer = CaptureLayer {
            events: Arc::clone(&events),
        };
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, f);
        let guard = events.lock().unwrap();
        guard.clone()
    }

    /// Async variant: takes an async closure and runs it on a fresh
    /// single-threaded runtime so `with_default` (thread-local) covers it.
    pub(crate) fn with_captured_events_async<Fut>(f: impl FnOnce() -> Fut) -> Vec<String>
    where
        Fut: std::future::Future<Output = ()>,
    {
        with_captured_events(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(f());
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::test_support::with_captured_events;

    #[test]
    fn capability_granted_emits_audit_event() {
        let events = with_captured_events(|| {
            log_capability_granted("com.example.test", "fs.read");
        });
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.contains("audit=true"), "missing audit field: {event}");
        assert!(event.contains("plugin_id=com.example.test"), "missing plugin_id: {event}");
        assert!(event.contains("capability=fs.read"), "missing capability: {event}");
        assert!(event.contains("result=granted"), "missing result: {event}");
    }

    #[test]
    fn capability_denied_emits_audit_event() {
        let events = with_captured_events(|| {
            log_capability_denied("com.example.test", "net.http");
        });
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.contains("audit=true"), "missing audit field: {event}");
        assert!(event.contains("result=denied"), "missing result: {event}");
    }

    #[test]
    fn plugin_lifecycle_emits_audit_event() {
        let events = with_captured_events(|| {
            log_plugin_lifecycle("com.example.test", "started");
        });
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.contains("audit=true"), "missing audit field: {event}");
        assert!(event.contains("transition=started"), "missing transition: {event}");
    }

    #[test]
    fn credential_access_emits_audit_event() {
        let events = with_captured_events(|| {
            log_credential_access("ai.anthropic", "retrieve");
        });
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.contains("audit=true"), "missing audit field: {event}");
        assert!(event.contains("credential_name=ai.anthropic"), "missing credential_name: {event}");
        assert!(event.contains("action=retrieve"), "missing action: {event}");
    }

    #[test]
    fn path_traversal_denied_emits_audit_event() {
        let events = with_captured_events(|| {
            log_path_traversal_denied(
                "com.example.test",
                Path::new("/etc/passwd"),
                Path::new("/home/user/forge"),
            );
        });
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.contains("audit=true"), "missing audit field: {event}");
        assert!(event.contains("plugin_id=com.example.test"), "missing plugin_id: {event}");
    }
}
