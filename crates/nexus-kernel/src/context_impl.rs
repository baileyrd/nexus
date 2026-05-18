//! `KernelPluginContext` — the concrete implementation of [`PluginContext`]
//! that the kernel provides to each plugin.
//!
//! Every capability-gated method checks the plugin's [`CapabilitySet`] before
//! performing any work and returns [`Error::Capability`] on denial. Path
//! operations are confined to `forge_root` via a canonicalize-then-prefix
//! check to prevent traversal attacks.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use async_trait::async_trait;
use nexus_types::{ForgePathValidator, PathValidationError};

use crate::audit;
use crate::capability::{Capability, CapabilitySet};
use crate::context::PluginContext;
use crate::error::{BusError, CapabilityError, Error, IpcError, Result};
use crate::event::EventFilter;
use crate::event_bus::{EventBus, EventSubscription};
use crate::ipc::IpcDispatcher;
use crate::kv_store::KvStore;
use crate::log::LogLevel;

use nexus_plugin_api::plugin::TrustLevel;

/// Concrete kernel implementation of [`PluginContext`].
///
/// Constructed by the plugin loader when a plugin is instantiated. Holds
/// shared handles to kernel services; cheap to clone as Arc-backed.
#[derive(Clone)]
pub struct KernelPluginContext {
    plugin_id: String,
    plugin_version: String,
    /// Live, mutable capability set (BL-096). Revoking a capability via
    /// [`PluginLoader::revoke_capability`] mutates this in place — every
    /// subsequent `has_capability` / `require_capability` / IPC caller
    /// check observes the new state without a plugin restart.
    capabilities: Arc<RwLock<CapabilitySet>>,
    kv: Arc<dyn KvStore>,
    event_bus: Arc<EventBus>,
    /// Canonical form of the forge root, used for path confinement.
    forge_root_canonical: PathBuf,
    /// Path validator scoped to the forge root. Used by the write path to
    /// close the canonicalize-parent-then-open TOCTOU race (MK audit
    /// findings F-5.3.1 / F-5.3.2). Constructed once at context creation.
    path_validator: ForgePathValidator,
    /// Optional dispatcher for plugin-to-plugin IPC. `None` means this context
    /// was built without a plugin loader (e.g. in unit tests) and `ipc_call`
    /// will return [`IpcError::DispatcherUnavailable`].
    ipc_dispatcher: Option<Arc<dyn IpcDispatcher>>,
    /// P1-02 — the *caller's* trust level. Used to gate handlers
    /// marked `internal = true` in the cap matrix (which require a
    /// Core-trust caller regardless of caps held). Defaults to
    /// [`TrustLevel::Community`] — the more restrictive value — so a
    /// context constructed without explicit elevation cannot reach
    /// in-tree-only handlers.
    caller_trust_level: TrustLevel,
}

impl KernelPluginContext {
    /// Create a new context for the given plugin.
    ///
    /// `forge_root` is canonicalized once at construction time so all
    /// subsequent path checks are fast prefix comparisons.
    ///
    /// # Errors
    /// Returns `Error::Io` if `forge_root` cannot be canonicalized (e.g. it
    /// doesn't exist yet).
    pub fn new(
        plugin_id: impl Into<String>,
        plugin_version: impl Into<String>,
        capabilities: CapabilitySet,
        kv: Arc<dyn KvStore>,
        event_bus: Arc<EventBus>,
        forge_root: &Path,
        ipc_dispatcher: Option<Arc<dyn IpcDispatcher>>,
    ) -> Result<Self> {
        // Canonicalize once via the validator (which we need to
        // construct anyway), then reuse its canonical root for
        // `forge_root_canonical`. Pre-#81 this called
        // `forge_root.canonicalize()` twice — once here and once
        // inside `ForgePathValidator::new` — for the same path.
        let path_validator = ForgePathValidator::new(forge_root).map_err(|e| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("forge root '{}': {e}", forge_root.display()),
            ))
        })?;
        let forge_root_canonical = path_validator.forge_root().to_path_buf();
        Ok(Self {
            plugin_id: plugin_id.into(),
            plugin_version: plugin_version.into(),
            capabilities: Arc::new(RwLock::new(capabilities)),
            kv,
            event_bus,
            forge_root_canonical,
            path_validator,
            ipc_dispatcher,
            caller_trust_level: TrustLevel::Community,
        })
    }

    /// P1-02 — builder hook the loader / bootstrap uses to mark this
    /// context as carrying core trust for the purposes of
    /// `internal = true` handler gates. Default is
    /// [`TrustLevel::Community`]; bootstrap upgrades core-plugin
    /// contexts to [`TrustLevel::Core`] before wiring them.
    #[must_use]
    pub fn with_trust_level(mut self, level: TrustLevel) -> Self {
        self.caller_trust_level = level;
        self
    }

    /// Return a clone of the live capability handle (BL-096).
    ///
    /// The plugin loader stashes this so that
    /// [`PluginLoader::revoke_capability`] can mutate the set in place
    /// — every subsequent `has_capability` / IPC gate sees the new
    /// state without a plugin restart.
    #[must_use]
    pub fn caps_handle(&self) -> Arc<RwLock<CapabilitySet>> {
        Arc::clone(&self.capabilities)
    }

    /// Snapshot the current capability set. Used by the loader's
    /// `PluginInfo` reporter and tests; live gates should call
    /// [`has_capability`] instead so the read lock is held for the
    /// shortest possible window.
    #[must_use]
    pub fn capabilities_snapshot(&self) -> CapabilitySet {
        self.capabilities.read().expect("caps lock").clone()
    }

    fn caps_contains(&self, cap: Capability) -> bool {
        self.capabilities.read().expect("caps lock").contains(cap)
    }

    /// Body of [`ipc_call`](Self::ipc_call) extracted so the public
    /// method can wrap the whole flow in a metrics timer (BL-093)
    /// and classify the result on every exit path.
    async fn ipc_call_inner(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: serde_json::Value,
        timeout: Duration,
    ) -> std::result::Result<serde_json::Value, IpcError> {
        if !self.caps_contains(Capability::IpcCall) {
            audit::log_capability_denied(&self.plugin_id, Capability::IpcCall.as_str());
            return Err(IpcError::CapabilityDenied {
                plugin_id: self.plugin_id.clone(),
            });
        }

        let dispatcher = self
            .ipc_dispatcher
            .clone()
            .ok_or(IpcError::DispatcherUnavailable)?;

        for required in
            dispatcher.required_caller_caps_for_args(target_plugin_id, command_id, &args)
        {
            if !self.caps_contains(required) {
                audit::log_capability_denied(&self.plugin_id, required.as_str());
                return Err(IpcError::CapabilityDenied {
                    plugin_id: self.plugin_id.clone(),
                });
            }
        }

        // P1-02 — handlers marked `internal = true` in the cap matrix
        // require a core-trust caller no matter what caps it holds.
        if dispatcher.is_handler_internal_only(target_plugin_id, command_id)
            && self.caller_trust_level != TrustLevel::Core
        {
            audit::log_capability_denied(
                &self.plugin_id,
                &format!("internal-only:{target_plugin_id}::{command_id}"),
            );
            return Err(IpcError::CapabilityDenied {
                plugin_id: self.plugin_id.clone(),
            });
        }

        let target = target_plugin_id.to_string();
        let command = command_id.to_string();
        let timeout_ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);

        if let Some(fut) = dispatcher.dispatch_async(&target, &command, args.clone()) {
            return match tokio::time::timeout(timeout, fut).await {
                Ok(result) => result,
                Err(_elapsed) => Err(IpcError::Timeout {
                    plugin_id: target,
                    command,
                    timeout_ms,
                }),
            };
        }

        let join = tokio::task::spawn_blocking({
            let target = target.clone();
            let command = command.clone();
            move || dispatcher.dispatch(&target, &command, &args)
        });

        match tokio::time::timeout(timeout, join).await {
            Ok(Ok(result)) => result,
            Ok(Err(_panic)) => Err(IpcError::PluginCrashedDuringCall {
                plugin_id: target,
                command,
                reason: String::new(),
            }),
            Err(_elapsed) => Err(IpcError::Timeout {
                plugin_id: target,
                command,
                timeout_ms,
            }),
        }
    }

    /// Check that the plugin holds `cap`, logging a denial and returning an
    /// error if not.
    fn require_capability(&self, cap: Capability) -> Result<()> {
        if self.caps_contains(cap) {
            return Ok(());
        }
        audit::log_capability_denied(&self.plugin_id, cap.as_str());
        Err(CapabilityError::Denied {
            plugin_id: self.plugin_id.clone(),
            cap,
        }
        .into())
    }

    /// Canonicalize `path` and verify it falls within `forge_root`.
    ///
    /// Relative paths are resolved against `forge_root_canonical`; absolute
    /// paths are taken as-is and then run through the same canonicalize +
    /// prefix-check. There is **no auto-promotion** from `FsRead` to
    /// `FsReadExternal` based on absoluteness — historically the kernel
    /// silently escalated absolute paths to the `*External` capability,
    /// which made the contract on `PlatformFsAPI.read/write` ambiguous
    /// (OI-12, MICROKERNEL-AUDIT F-6.3.1). The current rule is simpler:
    /// any path that canonicalizes outside `forge_root` is rejected with
    /// an `Error::Io(PermissionDenied)` and an `audit::*` traversal
    /// event, regardless of whether the caller passed it as a relative
    /// or absolute path.
    ///
    /// Returns the canonical path on success, or an `Error::Io` wrapping
    /// a permission-denied error if the path escapes the forge root.
    fn confine_path(&self, path: &Path) -> Result<PathBuf> {
        // Resolve relative to forge_root if the path is relative.
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.forge_root_canonical.join(path)
        };

        let canonical = absolute.canonicalize().map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!("path '{}': {e}", absolute.display()),
            ))
        })?;

        if !canonical.starts_with(&self.forge_root_canonical) {
            audit::log_path_traversal_denied(
                &self.plugin_id,
                &canonical,
                &self.forge_root_canonical,
            );
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "path traversal denied: '{}' is outside forge root '{}'",
                    canonical.display(),
                    self.forge_root_canonical.display()
                ),
            )));
        }

        Ok(canonical)
    }
}

#[async_trait]
impl PluginContext for KernelPluginContext {
    // ---- Identity --------------------------------------------------------

    fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    fn plugin_version(&self) -> &str {
        &self.plugin_version
    }

    fn has_capability(&self, cap: Capability) -> bool {
        self.caps_contains(cap)
    }

    // ---- File system -----------------------------------------------------

    /// Read a file inside `forge_root`.
    ///
    /// Requires the `FsRead` capability. The path is canonicalized and
    /// prefix-checked against the canonical forge root by
    /// [`confine_path`](Self::confine_path) — absolute paths outside the
    /// forge root return `Error::Io(PermissionDenied)` and emit an
    /// `audit::log_path_traversal_denied` event.
    ///
    /// **No auto-promotion** to `FsReadExternal`: a plugin that holds
    /// only `FsRead` and passes an absolute path outside the forge gets
    /// a loud, audit-logged failure rather than a silent capability
    /// escalation. Reaching outside the forge requires a dedicated
    /// external-read IPC surface that does not yet exist (OI-12 /
    /// MICROKERNEL-AUDIT F-6.3.1).
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        self.require_capability(Capability::FsRead)?;
        let confined = self.confine_path(path)?;
        tokio::fs::read(&confined).await.map_err(Error::Io)
    }

    async fn write_file(&self, path: &Path, contents: &[u8]) -> Result<()> {
        self.require_capability(Capability::FsWrite)?;

        // TOCTOU-safe write via `ForgePathValidator::validate_for_write`:
        // walks up to the deepest existing ancestor, canonicalizes *that*
        // (resolving symlinks), prefix-checks against the canonical forge
        // root, and rebuilds the target as `canonical_ancestor + tail`.
        // This closes the symlink-swap race between canonicalize and
        // open that the prior inline pattern was vulnerable to (MK audit
        // finding F-5.3.2).
        //
        // The validator treats absolute inputs as relative to the forge
        // root (strips the leading `/`). Callers that pass an absolute
        // path inside `forge_root_canonical` (e.g. tests joining on
        // `dir.path()`) therefore need their input rewritten to the
        // forge-root-relative form before validation.
        let relative_view = path
            .strip_prefix(&self.forge_root_canonical)
            .unwrap_or(path);
        let target = self.path_validator.validate_for_write(relative_view).map_err(|e| {
            match e {
                PathValidationError::PathTraversal(ref bad) => {
                    audit::log_path_traversal_denied(
                        &self.plugin_id,
                        bad,
                        &self.forge_root_canonical,
                    );
                    Error::Io(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!(
                            "path traversal denied: '{}' is outside forge root",
                            path.display()
                        ),
                    ))
                }
                PathValidationError::InvalidPath(msg) => Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    msg,
                )),
            }
        })?;
        tokio::fs::write(&target, contents).await.map_err(Error::Io)
    }

    async fn delete_file(&self, path: &Path) -> Result<()> {
        self.require_capability(Capability::FsWrite)?;
        let confined = self.confine_path(path)?;
        tokio::fs::remove_file(&confined).await.map_err(Error::Io)
    }

    async fn list_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        self.require_capability(Capability::FsRead)?;
        let confined = self.confine_path(dir)?;
        let mut entries = tokio::fs::read_dir(&confined).await.map_err(Error::Io)?;
        let mut paths = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(Error::Io)? {
            paths.push(entry.path());
        }
        Ok(paths)
    }

    // ---- KV store --------------------------------------------------------

    async fn kv_get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.require_capability(Capability::KvRead)?;
        self.kv
            .get(&self.plugin_id, key)
            .map_err(Error::Kv)
    }

    async fn kv_set(&self, key: &str, value: &[u8]) -> Result<()> {
        self.require_capability(Capability::KvWrite)?;
        self.kv
            .set(&self.plugin_id, key, value)
            .map_err(Error::Kv)
    }

    async fn kv_delete(&self, key: &str) -> Result<()> {
        self.require_capability(Capability::KvWrite)?;
        self.kv
            .delete(&self.plugin_id, key)
            .map_err(Error::Kv)
    }

    // ---- Events ----------------------------------------------------------

    fn publish(&self, type_id: &str, payload: serde_json::Value) -> Result<()> {
        // Fast-fail at the context boundary so the caller gets the same
        // error class regardless of whether the bus call eventually runs.
        // See `event_bus::type_id_in_namespace` for why a plain
        // `starts_with` is unsafe.
        if !crate::event_bus::type_id_in_namespace(type_id, &self.plugin_id)
            && !crate::event_bus::is_kernel_owned_shared_topic(type_id)
        {
            return Err(BusError::TypeIdNamespaceMismatch {
                plugin_id: self.plugin_id.clone(),
                type_id: type_id.to_string(),
            }
            .into());
        }
        self.event_bus
            .publish_plugin(&self.plugin_id, type_id, payload)
    }

    fn subscribe(&self, filter: EventFilter) -> EventSubscription {
        self.event_bus.subscribe(filter)
    }

    // ---- IPC -------------------------------------------------------------

    async fn ipc_call(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        args: serde_json::Value,
        timeout: Duration,
    ) -> std::result::Result<serde_json::Value, IpcError> {
        // BL-093: bracket the entire dispatch with a timer so every
        // exit path records `ipc_calls_total` + `ipc_call_duration`.
        let started = std::time::Instant::now();
        let result =
            self.ipc_call_inner(target_plugin_id, command_id, args, timeout).await;
        if let Some(m) = crate::metrics::global() {
            let status = match &result {
                Ok(_) => crate::metrics::CallStatus::Ok,
                Err(IpcError::CapabilityDenied { .. }) => {
                    crate::metrics::CallStatus::CapabilityDenied
                }
                Err(IpcError::CommandNotFound { .. } | IpcError::PluginNotFound { .. }) => {
                    crate::metrics::CallStatus::NotFound
                }
                Err(IpcError::Timeout { .. }) => crate::metrics::CallStatus::Timeout,
                _ => crate::metrics::CallStatus::Error,
            };
            m.record_ipc_call(
                target_plugin_id,
                command_id,
                status,
                u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
            );
        }
        result
    }

    // ---- Logging ---------------------------------------------------------

    fn log(&self, level: LogLevel, message: &str) {
        match level {
            LogLevel::Trace => tracing::trace!(plugin_id = %self.plugin_id, "{message}"),
            LogLevel::Debug => tracing::debug!(plugin_id = %self.plugin_id, "{message}"),
            LogLevel::Info  => tracing::info!(plugin_id = %self.plugin_id, "{message}"),
            LogLevel::Warn  => tracing::warn!(plugin_id = %self.plugin_id, "{message}"),
            LogLevel::Error => tracing::error!(plugin_id = %self.plugin_id, "{message}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::NexusEvent;
    use crate::kv_store::InMemoryKvStore;

    fn make_context(dir: &Path, caps: &[Capability]) -> KernelPluginContext {
        let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::new());
        let bus = Arc::new(EventBus::new(16));
        KernelPluginContext::new(
            "com.test.plugin",
            "1.0.0",
            caps.iter().copied().collect::<CapabilitySet>(),
            kv,
            bus,
            dir,
            None,
        )
        .unwrap()
    }

    #[test]
    fn identity_methods_return_correct_values() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[]);
        assert_eq!(ctx.plugin_id(), "com.test.plugin");
        assert_eq!(ctx.plugin_version(), "1.0.0");
    }

    #[test]
    fn has_capability_reflects_granted_caps() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[Capability::KvRead]);
        assert!(ctx.has_capability(Capability::KvRead));
        assert!(!ctx.has_capability(Capability::KvWrite));
    }

    #[tokio::test]
    async fn kv_requires_capability() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[]);
        assert!(ctx.kv_get("key").await.is_err());
        assert!(ctx.kv_set("key", b"val").await.is_err());
    }

    #[tokio::test]
    async fn kv_get_set_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[Capability::KvRead, Capability::KvWrite]);
        ctx.kv_set("key", b"hello").await.unwrap();
        let val = ctx.kv_get("key").await.unwrap().unwrap();
        assert_eq!(val, b"hello");
        ctx.kv_delete("key").await.unwrap();
        assert!(ctx.kv_get("key").await.unwrap().is_none());
    }

    #[test]
    fn publish_rejects_namespace_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[]);
        let result = ctx.publish("com.other.event", serde_json::json!({}));
        assert!(result.is_err());
    }

    /// Regression for issue #79. The pre-fix check at the context
    /// boundary was `type_id.starts_with(plugin_id)`, allowing a plugin
    /// with id `com.test` to publish topics namespaced under
    /// `com.testimony` etc. `make_context` uses `com.test.plugin`, so
    /// the substring-prefix attack here is `com.test.plugin*` → the
    /// hostile `com.test.plugin-evil.event` would have passed pre-fix.
    #[test]
    fn publish_rejects_substring_prefix_spoof() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[]);
        // Same-prefix-different-namespace: shares the `com.test.plugin`
        // characters but `-evil` breaks the dotted boundary, so the
        // strict check rejects it.
        let result =
            ctx.publish("com.test.plugin-evil.event", serde_json::json!({}));
        assert!(
            result.is_err(),
            "com.test.plugin must NOT be allowed to publish com.test.plugin-evil.event",
        );
    }

    #[test]
    fn publish_allows_dotted_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[]);
        ctx.publish("com.test.plugin.event", serde_json::json!({}))
            .expect("dotted suffix is the legitimate namespace shape");
    }

    #[tokio::test]
    async fn publish_emits_to_subscriber() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[]);
        let mut sub = ctx.subscribe(EventFilter::All);

        ctx.publish("com.test.plugin.ping", serde_json::json!({"x": 1})).unwrap();

        let evt = sub.recv().await.unwrap();
        match &evt.event {
            NexusEvent::Custom { type_id, .. } => assert_eq!(type_id, "com.test.plugin.ping"),
            _ => panic!("wrong event"),
        }
    }

    #[tokio::test]
    async fn read_file_denied_without_capability() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[]);
        let result = ctx.read_file(&dir.path().join("test.txt")).await;
        assert!(result.is_err());
    }

    /// Coverage for OI-07: a denied capability gate routes through
    /// `audit::log_capability_denied`, not an ad-hoc `tracing::warn!`. Asserts
    /// the structured `audit=true` field reaches the tracing channel so a
    /// security-stream filter can pick it up.
    #[test]
    fn capability_denial_emits_audit_event_through_gate() {
        let events = audit::test_support::with_captured_events_async(|| async {
            let dir = tempfile::tempdir().unwrap();
            let ctx = make_context(dir.path(), &[]);
            let _ = ctx.kv_get("anything").await;
        });
        let denial = events
            .iter()
            .find(|e| e.contains("audit=true") && e.contains("result=denied"))
            .unwrap_or_else(|| panic!("no audit denial event emitted; got: {events:?}"));
        assert!(denial.contains("plugin_id=com.test.plugin"), "{denial}");
        assert!(denial.contains("capability=kv.read"), "{denial}");
    }

    /// Coverage for OI-07: a path-traversal rejection routes through
    /// `audit::log_path_traversal_denied` and reaches the structured channel.
    #[test]
    fn path_traversal_emits_audit_event_through_gate() {
        let events = audit::test_support::with_captured_events_async(|| async {
            let dir = tempfile::tempdir().unwrap();
            let ctx = make_context(dir.path(), &[Capability::FsRead]);
            let _ = ctx.read_file(Path::new("/etc/passwd")).await;
        });
        let traversal = events
            .iter()
            .find(|e| e.contains("audit=true") && e.contains("path traversal denied"))
            .unwrap_or_else(|| panic!("no audit traversal event emitted; got: {events:?}"));
        assert!(traversal.contains("plugin_id=com.test.plugin"), "{traversal}");
    }

    #[tokio::test]
    async fn read_write_file_with_capability() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[Capability::FsRead, Capability::FsWrite]);
        let file_path = dir.path().join("test.txt");
        ctx.write_file(&file_path, b"hello forge").await.unwrap();
        let contents = ctx.read_file(&file_path).await.unwrap();
        assert_eq!(contents, b"hello forge");
    }

    #[tokio::test]
    async fn confine_path_blocks_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[Capability::FsRead]);
        // Try to read /etc/passwd via traversal
        let result = ctx.read_file(Path::new("/etc/passwd")).await;
        assert!(result.is_err());
    }

    /// OI-12 acceptance: an absolute path outside the forge must produce
    /// a *loud, typed* failure, not a silent denial — no auto-promotion
    /// from `FsRead` to `FsReadExternal`. We assert the error is the
    /// `PermissionDenied` traversal variant (not, say, a generic
    /// `CapabilityDenied`) so callers can distinguish "you asked for a
    /// file outside the forge" from "you don't hold `FsRead` at all".
    #[tokio::test]
    async fn read_file_absolute_outside_forge_returns_typed_traversal_error() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[Capability::FsRead]);
        let err = ctx
            .read_file(Path::new("/etc/passwd"))
            .await
            .expect_err("absolute outside-forge read must fail");
        match err {
            Error::Io(io_err) => {
                assert_eq!(
                    io_err.kind(),
                    std::io::ErrorKind::PermissionDenied,
                    "expected PermissionDenied, got {:?}",
                    io_err.kind(),
                );
                assert!(
                    io_err.to_string().contains("path traversal denied"),
                    "expected traversal message, got: {io_err}",
                );
            }
            other => panic!("expected Error::Io, got {other:?}"),
        }
    }

    /// OI-12 mirror for the write side. `validate_for_write` strips the
    /// leading `/` and treats absolute inputs as forge-root-relative; an
    /// absolute path that resolves outside the forge (here via a `..`
    /// payload) hits the same `PermissionDenied` traversal path.
    #[tokio::test]
    async fn write_file_absolute_outside_forge_returns_typed_traversal_error() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = make_context(dir.path(), &[Capability::FsWrite]);
        let err = ctx
            .write_file(Path::new("/../escape.txt"), b"x")
            .await
            .expect_err("absolute traversal write must fail");
        match err {
            Error::Io(io_err) => {
                assert_eq!(
                    io_err.kind(),
                    std::io::ErrorKind::PermissionDenied,
                    "expected PermissionDenied, got {:?}",
                    io_err.kind(),
                );
                assert!(
                    io_err.to_string().contains("path traversal denied"),
                    "expected traversal message, got: {io_err}",
                );
            }
            other => panic!("expected Error::Io, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_file_rejects_symlinked_parent() {
        // Regression for MK F-5.3.2: a symlinked parent directory must not
        // let a plugin write outside the forge root. `validate_for_write`
        // canonicalizes the deepest existing ancestor (the symlink target)
        // and the prefix check rejects it.
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();

        let ctx = make_context(dir.path(), &[Capability::FsWrite]);
        let result = ctx
            .write_file(&dir.path().join("escape/victim.txt"), b"pwned")
            .await;
        assert!(result.is_err(), "write through symlinked parent must fail");
        // The file must not have been created outside the sandbox.
        assert!(!outside.path().join("victim.txt").exists());
    }
}
