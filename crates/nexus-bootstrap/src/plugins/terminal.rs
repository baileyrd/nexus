//! Terminal & process manager plugin registration.
//!
//! PRD-09. Pure-library crate wrapped behind `com.nexus.terminal` so
//! UI / script plugins reach it over dispatch rather than linking it
//! directly (ARCHITECTURE §7 invariant #3). Saved-commands (§14.1) and
//! ad-hoc history (§10) share the same SQLite file at
//! `<forge>/.forge/procmgr.sqlite` — separate tables, separate
//! `Connection`s. Failure to open either store is logged and the
//! plugin loads without that handler family (session IPC stays usable
//! even when SQLite misbehaves).

use std::sync::Arc;

use anyhow::Result;
use nexus_kernel::EventBus;
use nexus_plugins::PluginLoader;
use nexus_terminal::TerminalCorePlugin;

use super::{core_manifest_with_ipc_and_deps, with_v1_aliases, LifecycleFlags, RegisterCoreResultExt};

pub(super) fn register(
    loader: &mut PluginLoader,
    forge_root: &std::path::Path,
    event_bus: &Arc<EventBus>,
) -> Result<()> {
    // Env-hygiene default (authoritative) from `<forge>/.forge/terminal.toml`.
    // Missing file → permissive (legacy behaviour). A malformed file is
    // logged and skipped rather than failing terminal startup — the
    // service must stay usable.
    let terminal_toml = forge_root.join(".forge").join("terminal.toml");
    let spawn_policy_default = match nexus_terminal::TerminalConfig::read_from(&terminal_toml) {
        Ok(cfg) => cfg.spawn,
        Err(err) => {
            tracing::warn!(
                path = %terminal_toml.display(),
                err = %err,
                "com.nexus.terminal: terminal.toml invalid; spawning with a permissive env policy"
            );
            nexus_types::SpawnPolicy::permissive()
        }
    };

    let saved_db = forge_root.join(".forge").join("procmgr.sqlite");
    let terminal_plugin = match nexus_terminal::SqliteSavedCommandStore::open(&saved_db) {
        Ok(store) => TerminalCorePlugin::new().with_saved_store(store),
        Err(err) => {
            tracing::warn!(
                path = %saved_db.display(),
                err = %err,
                "com.nexus.terminal: saved-commands store unavailable; handlers will return errors"
            );
            TerminalCorePlugin::new()
        }
    };
    let terminal_plugin = terminal_plugin.with_spawn_policy_default(spawn_policy_default);
    let terminal_plugin = match nexus_terminal::SqliteAdHocStore::open(&saved_db) {
        Ok(store) => terminal_plugin.with_adhoc_store(store),
        Err(err) => {
            tracing::warn!(
                path = %saved_db.display(),
                err = %err,
                "com.nexus.terminal: ad-hoc history store unavailable; adhoc_* handlers will return errors"
            );
            terminal_plugin
        }
    };
    // BL-061 — memory monitor: enabled by default with PRD-09 §7.3
    // recommended limits (250 MB soft / 500 MB hard). The poller is
    // spawned alongside the byte-stream drainer in `with_event_bus`;
    // the order here matters so `with_event_bus` sees the configured
    // monitor and starts the poller. BL-061 follow-up (2026-05-08)
    // wired per-saved-command overrides via `SavedCommand.memory_limit_mb`:
    // `dispatch_run_saved` stages the override so the next poller
    // round applies it instead of the bootstrap-wide default.
    // Operators that want measure-only semantics for ad-hoc sessions
    // (RSS chip but no auto-kill) still ride this default — saved
    // commands with an explicit limit are the per-session path.
    let terminal_plugin =
        terminal_plugin.with_memory_monitor(nexus_terminal::MemoryLimits::default_recommended());
    // BL-062 — install an eviction persister that durably stashes
    // the scrollback of any LRU-evicted session. BL-063 — share the
    // same `SqliteSessionStore` handle with the plugin so the
    // `cross_session_search` handler can read the FTS5 index that
    // the persister populates on every save. Without this hook the
    // snapshot is dropped silently and search returns "store not
    // configured" (matching pre-BL-062 behaviour). The session store
    // sits alongside the saved / adhoc stores at
    // `<forge>/.forge/sessions.sqlite`; scrollback blobs land at
    // `<forge>/.forge/sessions/<session_id>/scrollback.bin`.
    let session_db = forge_root.join(".forge").join("sessions.sqlite");
    let scrollback_dir = forge_root.join(".forge").join("sessions");
    let terminal_plugin = match nexus_terminal::SqliteSessionStore::open(
        &session_db,
        &scrollback_dir,
    ) {
        Ok(store) => {
            let store = Arc::new(std::sync::Mutex::new(store));
            let persister_store = Arc::clone(&store);
            let persister: nexus_terminal::EvictionPersister = Box::new(move |id, bytes| {
                let g = persister_store
                    .lock()
                    .map_err(|_| nexus_terminal::TerminalError::Persist(
                        "eviction persister: store mutex poisoned".into(),
                    ))?;
                g.save_scrollback(id, bytes)
            });
            terminal_plugin
                .with_eviction_persister(persister)
                .with_session_store(store)
        }
        Err(err) => {
            tracing::warn!(
                path = %session_db.display(),
                err = %err,
                "com.nexus.terminal: session store unavailable; LRU-evicted scrollback will be dropped",
            );
            terminal_plugin
        }
    };
    // Phase 2 WI-12: stream PTY output as kernel events so the shell
    // can switch off its 100ms poll. The legacy `pump` handler still
    // returns its byte count; this is purely additive.
    let terminal_plugin = terminal_plugin.with_event_bus(Arc::clone(event_bus));
    loader
        .register_core(
            core_manifest_with_ipc_and_deps(
                "com.nexus.terminal",
                "Terminal",
                LifecycleFlags::NONE,
                &with_v1_aliases(nexus_terminal::IPC_HANDLERS),
                nexus_terminal::MANIFEST_DEPS,
            ),
            forge_root,
            Box::new(terminal_plugin),
        )
        .or_lifecycle_skip(event_bus, "com.nexus.terminal")?;
    Ok(())
}
