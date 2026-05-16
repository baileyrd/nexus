//! Per-plugin registration helpers for the bootstrap path.
//!
//! Decomposed from the former monolithic `register_core_plugins` in
//! `lib.rs` so each in-tree core plugin has its own registration site.
//! [`register_all`] is the orchestrator the bootstrap path calls; it
//! preserves the original registration order (security first so audit
//! events route through it, then storage, then everything else).

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_plugins::{parse_manifest, PluginError, PluginLoader, PluginManifest};

mod acp;
mod agent;
mod ai;
mod ai_runtime;
mod audio;
mod comments;
mod dap;
mod database;
mod editor;
mod formats;
mod git;
mod linkpreview;
mod lsp;
mod mcp;
mod notifications;
mod security;
mod skills;
mod storage;
mod templates;
mod terminal;
mod theme;
mod workflow;

/// Register every in-tree core plugin in the order the bootstrap path
/// requires: security first so audit events are available before other
/// plugins emit, storage next so AI/editor/etc. can call into it during
/// their own lifecycle hooks, then everything else in the historical
/// order.
pub(crate) fn register_all(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    security::register(loader, forge_root, event_bus)?;
    storage::register(loader, forge_root, event_bus)?;
    database::register(loader, forge_root, event_bus)?;
    editor::register(loader, forge_root, event_bus)?;
    theme::register(loader, forge_root, event_bus)?;
    // BL-134 Phase 4 — ai-runtime registers BEFORE ai so the runtime's
    // pool handle is already published (via `WorkerPool::publish_shared_handle`
    // in the runtime's `wire_context`) by the time ai's `wire_context` fires
    // and starts the BL-041 indexing daemon. The daemon picks up the shared
    // handle in lieu of building its own tokio runtime; a `None` fallback
    // preserves boot in environments where the runtime plugin isn't registered.
    ai_runtime::register(loader, forge_root, event_bus)?;
    ai::register(loader, forge_root, event_bus)?;
    skills::register(loader, forge_root, event_bus)?;
    templates::register(loader, forge_root, event_bus)?;
    formats::register(loader, forge_root, event_bus)?;
    workflow::register(loader, forge_root, event_bus)?;
    linkpreview::register(loader, forge_root, event_bus)?;
    notifications::register(loader, forge_root, event_bus)?;
    audio::register(loader, forge_root, event_bus)?;
    comments::register(loader, forge_root, event_bus)?;
    agent::register(loader, forge_root, event_bus)?;
    mcp::register(loader, forge_root, event_bus)?;
    lsp::register(loader, forge_root, event_bus)?;
    dap::register(loader, forge_root, event_bus)?;
    acp::register(loader, forge_root, event_bus)?;
    git::register(loader, forge_root, event_bus)?;
    terminal::register(loader, forge_root, event_bus)?;
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) struct LifecycleFlags {
    pub(crate) on_init: bool,
    pub(crate) on_start: bool,
    pub(crate) on_stop: bool,
}

impl LifecycleFlags {
    pub(crate) const NONE: Self = Self {
        on_init: false,
        on_start: false,
        on_stop: false,
    };
}

/// Generate a core-plugin manifest inline with no IPC commands declared.
pub(crate) fn core_manifest(id: &str, name: &str, lc: LifecycleFlags) -> PluginManifest {
    let no_commands: &[(&str, u32)] = &[];
    core_manifest_with_ipc(id, name, lc, no_commands)
}

/// Generate a core-plugin manifest with IPC command registrations.
///
/// Generic over the command-name string type so the same builder accepts
/// both the static `&str` slices used by most subsystems and the owned
/// `String`s produced by [`with_v1_aliases`] (ADR 0021).
pub(crate) fn core_manifest_with_ipc<S: AsRef<str>>(
    id: &str,
    name: &str,
    lc: LifecycleFlags,
    ipc_commands: &[(S, u32)],
) -> PluginManifest {
    let mut toml = format!(
        r#"
[plugin]
id = "{id}"
name = "{name}"
version = "0.1.0"
trust_level = "core"
api_version = "1"

[lifecycle]
on_init = {init}
on_start = {start}
on_stop = {stop}
"#,
        init = lc.on_init,
        start = lc.on_start,
        stop = lc.on_stop,
    );
    for (cmd_id, handler_id) in ipc_commands {
        use std::fmt::Write as _;
        let cmd_id = cmd_id.as_ref();
        let _ = write!(
            toml,
            "\n[[registrations.ipc_command]]\nid = \"{cmd_id}\"\nhandler_id = {handler_id}\n"
        );
    }
    parse_manifest(&toml, "bootstrap.toml")
        .unwrap_or_else(|e| panic!("bootstrap manifest for {id} failed to parse: {e}"))
}

/// Expand a list of `(command, handler_id)` pairs to include `.v1`
/// aliases per [ADR 0021](../../../../docs/adr/0021-ipc-handler-versioning.md).
///
/// For `[("search", 7)]` returns `[("search", 7), ("search.v1", 7)]`.
/// Both names resolve to the same handler — the bare form is the
/// "current version" alias and `.v1` is the explicit version pin. When
/// `search.v2` ships, the subsystem switches to a hand-written list that
/// carries all three names (bare → v2's handler, `.v1` → legacy
/// handler, `.v2` → new handler) so the deprecation timeline is visible
/// at the registration site.
pub(crate) fn with_v1_aliases(ipc_commands: &[(&str, u32)]) -> Vec<(String, u32)> {
    let mut out = Vec::with_capacity(ipc_commands.len() * 2);
    for &(name, handler_id) in ipc_commands {
        out.push((name.to_string(), handler_id));
        out.push((format!("{name}.v1"), handler_id));
    }
    out
}

/// BL-095 follow-up — extension trait that converts a single
/// plugin's `LifecycleTimeout` from a fatal boot error into a
/// "skip and continue" signal. Other `register_core` errors
/// (manifest invalid, duplicate id, downstream lifecycle hook
/// returning a real error) still abort boot — the watchdog only
/// catches the case where a hook *hangs*, which is the recoverable
/// failure mode where the rest of the plugin set is still useful.
///
/// Each skip publishes `com.nexus.kernel.plugin_lifecycle_timeout`
/// onto the event bus so the shell (or any subscriber) can render
/// a "<plugin> failed to start" notice. The synthetic `com.nexus.kernel`
/// source-plugin-id is anchor-only — bus's namespace anti-spoof
/// check passes since the topic lies inside that string namespace.
pub(crate) trait RegisterCoreResultExt {
    fn or_lifecycle_skip(
        self,
        event_bus: &EventBus,
        label: &str,
    ) -> Result<()>;
}

impl RegisterCoreResultExt
    for std::result::Result<nexus_plugins::PluginInfo, PluginError>
{
    fn or_lifecycle_skip(
        self,
        event_bus: &EventBus,
        label: &str,
    ) -> Result<()> {
        match self {
            Ok(_) => Ok(()),
            Err(PluginError::LifecycleTimeout {
                plugin_id,
                hook,
                timeout_secs,
            }) => {
                tracing::warn!(
                    plugin_id = %plugin_id,
                    ?hook,
                    timeout_secs,
                    "BL-095: plugin lifecycle hook timed out — continuing with degraded plugin set",
                );
                let _ = event_bus.publish_plugin(
                    "com.nexus.kernel",
                    "com.nexus.kernel.plugin_lifecycle_timeout",
                    serde_json::json!({
                        "plugin_id": plugin_id,
                        "hook": format!("{:?}", hook),
                        "timeout_secs": timeout_secs,
                    }),
                );
                Ok(())
            }
            Err(e) => {
                Err(anyhow::Error::new(e).context(format!("failed to register {label}")))
            }
        }
    }
}

#[cfg(test)]
mod with_v1_aliases_tests {
    use super::with_v1_aliases;

    #[test]
    fn doubles_each_entry_with_v1_suffix() {
        let expanded = with_v1_aliases(&[("search", 7), ("read_file", 2)]);
        assert_eq!(
            expanded,
            vec![
                ("search".to_string(), 7),
                ("search.v1".to_string(), 7),
                ("read_file".to_string(), 2),
                ("read_file.v1".to_string(), 2),
            ],
            "every input pair must produce a bare alias and a .v1 alias"
        );
    }

    #[test]
    fn empty_input_yields_empty_output() {
        assert!(with_v1_aliases(&[]).is_empty());
    }

    #[test]
    fn handler_id_is_shared_between_bare_and_v1() {
        let expanded = with_v1_aliases(&[("delete_file", 12)]);
        let bare = expanded.iter().find(|(n, _)| n == "delete_file");
        let v1 = expanded.iter().find(|(n, _)| n == "delete_file.v1");
        assert_eq!(
            bare.map(|(_, h)| *h),
            v1.map(|(_, h)| *h),
            "bare and .v1 must point at the same handler id (alias semantics)"
        );
    }
}

#[cfg(test)]
mod or_lifecycle_skip_tests {
    use std::sync::Arc;

    use nexus_kernel::{EventBus, EventFilter, NexusEvent};
    use nexus_plugins::PluginError;

    use super::RegisterCoreResultExt;

    /// BL-095 follow-up — a `LifecycleTimeout` error is converted to
    /// `Ok(())` and a `com.nexus.kernel.plugin_lifecycle_timeout`
    /// event lands on the bus carrying the plugin id, the hook name,
    /// and the timeout value. The shell can subscribe to this and
    /// surface a "<plugin> failed to start" notice.
    #[test]
    fn lifecycle_timeout_skips_and_publishes_bus_event() {
        let bus = Arc::new(EventBus::new(16));
        let mut sub = bus.subscribe(EventFilter::CustomPrefix(
            "com.nexus.kernel.".to_string(),
        ));
        let result: Result<nexus_plugins::PluginInfo, PluginError> =
            Err(PluginError::LifecycleTimeout {
                plugin_id: "com.nexus.test".to_string(),
                hook: "init".to_string(),
                timeout_secs: 30,
            });
        let outcome = result.or_lifecycle_skip(&bus, "com.nexus.test");
        assert!(outcome.is_ok(), "lifecycle timeout should be swallowed, got {outcome:?}");
        let ev = sub
            .try_recv()
            .expect("bus alive")
            .expect("expected one published event");
        match &ev.event {
            NexusEvent::Custom { type_id, payload, .. } => {
                assert_eq!(type_id, "com.nexus.kernel.plugin_lifecycle_timeout");
                assert_eq!(payload["plugin_id"], "com.nexus.test");
                assert_eq!(payload["hook"], "\"init\"");
                assert_eq!(payload["timeout_secs"], 30);
            }
            other => panic!("expected Custom event, got {other:?}"),
        }
    }

    /// Non-timeout errors still abort with the original anyhow
    /// context attached. The skip path is narrow on purpose: a
    /// manifest-invalid or duplicate-id error is a programming bug,
    /// not a "slow plugin" we should silently skip past.
    #[test]
    fn non_timeout_errors_still_propagate() {
        let bus = Arc::new(EventBus::new(16));
        let result: Result<nexus_plugins::PluginInfo, PluginError> =
            Err(PluginError::DuplicatePlugin("com.nexus.test".to_string()));
        let outcome = result.or_lifecycle_skip(&bus, "com.nexus.test");
        let err = outcome.expect_err("duplicate-id should propagate");
        let msg = err.to_string();
        assert!(
            msg.contains("failed to register com.nexus.test"),
            "context label missing in {msg}",
        );
    }

    // The non-error path is exercised live by every other test in
    // the bootstrap suite (every successful boot routes through
    // `or_lifecycle_skip`); a synthetic `Ok(PluginInfo)` test would
    // have to fabricate a real `PluginInfo` whose constructor isn't
    // public, so we skip it here.
}
