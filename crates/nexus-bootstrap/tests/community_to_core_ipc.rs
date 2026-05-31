//! Cross-registry IPC: community → core plugin routing.
//!
//! Proves that a community plugin's dispatcher (wrapped in
//! [`CompositeIpcDispatcher`]) can reach core plugins registered in the
//! bootstrap [`SharedPluginLoader`] via the fallback path.
//!
//! Without this, a community WASM plugin calling
//! `host::invoke_command("com.nexus.storage", "read_file", …)` would
//! silently fail with `PluginNotFound` because the community
//! `PluginManager` and the bootstrap loader are separate registries.

use std::sync::Arc;

use nexus_bootstrap::build_cli_runtime;
use nexus_kernel::{IpcDispatcher, IpcError, IpcFuture};
use nexus_plugins::{CompositeIpcDispatcher, FallbackCell};

/// A stand-in for the community `TauriIpcDispatcher` that knows about
/// zero plugins — every dispatch returns `PluginNotFound`. Lets us
/// exercise the fall-through path in isolation without having to load a
/// real WASM sandbox just for the test.
struct EmptyCommunityDispatcher;

impl IpcDispatcher for EmptyCommunityDispatcher {
    fn dispatch(
        &self,
        target_plugin_id: &str,
        _command_id: &str,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, IpcError> {
        Err(IpcError::PluginNotFound {
            plugin_id: target_plugin_id.to_string(),
        })
    }

    fn dispatch_async(
        &self,
        _target_plugin_id: &str,
        _command_id: &str,
        _args: serde_json::Value,
    ) -> Option<IpcFuture> {
        None
    }
}

/// Like `EmptyCommunityDispatcher` but pretends `com.community.fake`
/// exists and has no matching command. Used to verify that
/// `CommandNotFound` does *not* trigger fall-through (so a community
/// plugin can't shadow a core command by registering its own).
struct StubCommunityDispatcher;

impl IpcDispatcher for StubCommunityDispatcher {
    fn dispatch(
        &self,
        target_plugin_id: &str,
        command_id: &str,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, IpcError> {
        if target_plugin_id == "com.community.fake" {
            Err(IpcError::CommandNotFound {
                plugin_id: target_plugin_id.to_string(),
                command: command_id.to_string(),
            })
        } else {
            Err(IpcError::PluginNotFound {
                plugin_id: target_plugin_id.to_string(),
            })
        }
    }
}

fn scratch_forge() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    dir
}

fn build_composite(runtime: &nexus_bootstrap::Runtime) -> CompositeIpcDispatcher {
    let fallback = FallbackCell::new();
    let core: Arc<dyn IpcDispatcher> = Arc::clone(&runtime.loader) as Arc<dyn IpcDispatcher>;
    fallback.set(core);
    CompositeIpcDispatcher::new(Arc::new(EmptyCommunityDispatcher), fallback)
}

#[test]
fn community_dispatcher_falls_through_to_core_theme_plugin() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let composite = build_composite(&runtime);

    let cfg = composite
        .dispatch(
            "com.nexus.theme",
            "get_theme_config",
            &serde_json::json!({}),
        )
        .expect("get_theme_config routes through fallback");

    assert!(
        cfg.get("theme_id").is_some(),
        "expected theme config object, got {cfg}"
    );
}

#[test]
fn community_dispatcher_falls_through_to_core_storage_plugin() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let composite = build_composite(&runtime);

    let value = composite
        .dispatch("com.nexus.storage", "query_files", &serde_json::json!({}))
        .expect("query_files routes through fallback");

    assert!(
        value.is_array(),
        "query_files should return an array, got {value}"
    );
}

#[test]
fn fallback_surfaces_core_command_not_found_errors() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let composite = build_composite(&runtime);

    let err = composite
        .dispatch(
            "com.nexus.theme",
            "not-a-real-command",
            &serde_json::json!({}),
        )
        .unwrap_err();

    assert!(
        matches!(
            &err,
            IpcError::CommandNotFound { plugin_id, command }
                if plugin_id == "com.nexus.theme" && command == "not-a-real-command"
        ),
        "expected CommandNotFound from core plugin, got {err:?}"
    );
}

#[test]
fn community_command_not_found_does_not_fall_through() {
    let forge = scratch_forge();
    let runtime = build_cli_runtime(forge.path().to_path_buf()).expect("build runtime");

    let fallback = FallbackCell::new();
    let core: Arc<dyn IpcDispatcher> = Arc::clone(&runtime.loader) as Arc<dyn IpcDispatcher>;
    fallback.set(core);
    let composite = CompositeIpcDispatcher::new(Arc::new(StubCommunityDispatcher), fallback);

    // The primary claims ownership of "com.community.fake" but has no
    // matching command. The composite must return that as-is — NOT
    // retry through the core loader (which would return PluginNotFound
    // and change the error shape the caller sees).
    let err = composite
        .dispatch("com.community.fake", "noop", &serde_json::json!({}))
        .unwrap_err();

    assert!(
        matches!(
            &err,
            IpcError::CommandNotFound { plugin_id, .. } if plugin_id == "com.community.fake"
        ),
        "expected CommandNotFound to propagate, got {err:?}"
    );
}

#[test]
fn empty_fallback_returns_primary_plugin_not_found() {
    let fallback = FallbackCell::new();
    let composite = CompositeIpcDispatcher::new(Arc::new(EmptyCommunityDispatcher), fallback);

    let err = composite
        .dispatch(
            "com.nexus.theme",
            "get_theme_config",
            &serde_json::json!({}),
        )
        .unwrap_err();

    assert!(
        matches!(&err, IpcError::PluginNotFound { plugin_id } if plugin_id == "com.nexus.theme"),
        "expected primary's PluginNotFound when fallback is unset, got {err:?}"
    );
}
