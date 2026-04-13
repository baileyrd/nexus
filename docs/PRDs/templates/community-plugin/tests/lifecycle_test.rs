//! Plugin lifecycle integration tests.
//!
//! These tests verify the full lifecycle state machine and
//! state persistence across simulated hot-reloads.

use nexus_core::test_utils::MockPluginContext;

// Import the plugin entry point.
use {{crate_name}}::create_plugin;

#[tokio::test]
async fn test_plugin_lifecycle_happy_path() {
    let mut plugin = create_plugin();
    let ctx = MockPluginContext::new();

    // Full lifecycle: load → init → start → stop → shutdown.
    plugin.on_load().await.unwrap();
    plugin.on_init(&ctx).await.unwrap();
    plugin.on_start().await.unwrap();
    plugin.on_stop().await.unwrap();
    plugin.on_shutdown().await.unwrap();
}

#[tokio::test]
async fn test_plugin_state_persists_across_reload() {
    let ctx = MockPluginContext::new();

    // First run: init and stop (persists state).
    let mut plugin = create_plugin();
    plugin.on_load().await.unwrap();
    plugin.on_init(&ctx).await.unwrap();
    plugin.on_start().await.unwrap();
    plugin.on_stop().await.unwrap();
    plugin.on_shutdown().await.unwrap();

    // Simulate hot-reload: new instance, same context (same KV store).
    let mut plugin2 = create_plugin();
    plugin2.on_load().await.unwrap();
    plugin2.on_init(&ctx).await.unwrap();

    // TODO: Assert that state was restored from the KV store.
    // Add plugin-specific assertions here once state fields are defined.

    plugin2.on_stop().await.unwrap();
    plugin2.on_shutdown().await.unwrap();
}

#[tokio::test]
async fn test_plugin_handles_missing_state_gracefully() {
    // Fresh context with no prior KV data — plugin should start with defaults.
    let ctx = MockPluginContext::new();
    let mut plugin = create_plugin();

    plugin.on_load().await.unwrap();
    plugin.on_init(&ctx).await.unwrap();
    // Should not panic or error — fresh state is used.
    plugin.on_shutdown().await.unwrap();
}
