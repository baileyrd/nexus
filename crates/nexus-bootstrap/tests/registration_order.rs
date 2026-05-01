//! Issue #83 — assert that the bootstrap registers core plugins in
//! the documented order.
//!
//! `register_core_plugins` ships ~14 `register_core` calls in a
//! specific sequence, with the comment `// Security first so audit
//! events are available before other plugins emit.` Pre-#83 there
//! was no test that pinned this ordering — a refactor that
//! shuffled the calls (e.g. as part of an alphabetisation cleanup)
//! could silently push security past plugins that legitimately
//! emit audit events at registration time.
//!
//! The loader exposes [`PluginLoader::registration_order()`] which
//! returns the ids in the order they were registered. We boot a CLI
//! runtime, ask the loader for the order, and assert the
//! security-first invariant + the relative position of a few other
//! anchor plugins so a future re-shuffle is loud at CI time.

use nexus_bootstrap::build_cli_runtime;

/// Index of `id` in `order`, panicking with a descriptive message
/// if absent. Wrapped so the test failure message names the missing
/// plugin without a generic `unwrap` panic.
fn position_of(order: &[String], id: &str) -> usize {
    order
        .iter()
        .position(|p| p == id)
        .unwrap_or_else(|| panic!("plugin '{id}' not in registration order: {order:?}"))
}

#[test]
fn registers_security_before_every_other_core_plugin() {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init forge");
    let runtime = build_cli_runtime(dir.path().to_path_buf()).expect("build runtime");

    let order = runtime.loader.lock().registration_order();
    let security_idx = position_of(&order, "com.nexus.security");
    assert_eq!(
        security_idx, 0,
        "security plugin must be registered first so its audit-event \
         subscribers are armed before any other plugin emits — \
         registration order was: {order:?}"
    );
}

#[test]
fn registers_storage_before_consumers_that_ipc_into_it() {
    // Storage owns the SQLite index + watcher; plugins that ipc_call
    // into it (ai for indexing, agent for history reads, editor for
    // open/save) need it registered earlier so on_start can find the
    // engine via the loader.
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init forge");
    let runtime = build_cli_runtime(dir.path().to_path_buf()).expect("build runtime");

    let order = runtime.loader.lock().registration_order();
    let storage_idx = position_of(&order, "com.nexus.storage");

    for consumer in ["com.nexus.ai", "com.nexus.agent", "com.nexus.editor"] {
        let consumer_idx = position_of(&order, consumer);
        assert!(
            storage_idx < consumer_idx,
            "{consumer} (idx {consumer_idx}) must register AFTER \
             com.nexus.storage (idx {storage_idx}); registration \
             order was: {order:?}"
        );
    }
}

#[test]
fn invoker_registers_after_every_core_plugin() {
    // The invoker (CLI/TUI) is the last `register_core` call so its
    // dependency graph is fully populated before its own lifecycle
    // hooks fire. The shell's bridge.rs depends on this ordering for
    // the kernel boot path.
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init forge");
    let runtime = build_cli_runtime(dir.path().to_path_buf()).expect("build runtime");

    let order = runtime.loader.lock().registration_order();
    let invoker_idx = position_of(&order, nexus_bootstrap::CLI_PLUGIN_ID);
    assert_eq!(
        invoker_idx,
        order.len() - 1,
        "invoker plugin must be registered last; registration order \
         was: {order:?}"
    );
}
