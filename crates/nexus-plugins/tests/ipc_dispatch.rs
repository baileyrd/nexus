//! End-to-end tests for kernel → loader IPC dispatch.
//!
//! These tests wire `KernelPluginContext::ipc_call` through a
//! `SharedPluginLoader` to a registered core plugin's `dispatch` handler and
//! verify every error path (capability denied, unknown plugin, unknown
//! command, missing dispatcher, handler error).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use nexus_kernel::{Ipc as _, 
    Capability, CapabilitySet, EventBus, InMemoryKvStore, IpcDispatcher, IpcError,
    KernelPluginContext, KvStore,
};
use nexus_plugins::{
    parse_manifest, CorePlugin, CorePluginFuture, PluginError, PluginLoader, SharedPluginLoader,
};

// ── A minimal core plugin that records every dispatch and returns a canned value ──

struct EchoPlugin {
    last_call: Arc<Mutex<Option<(u32, serde_json::Value)>>>,
}

impl CorePlugin for EchoPlugin {
    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        *self.last_call.lock().unwrap() = Some((handler_id, args.clone()));
        Ok(serde_json::json!({ "echoed": args.clone(), "handler": handler_id }))
    }
}

// ── A plugin whose dispatch always errors — used for the crash-mapping test ──

struct BrokenPlugin;

impl CorePlugin for BrokenPlugin {
    fn dispatch(
        &mut self,
        _handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        Err(PluginError::ExecutionFailed {
            plugin_id: "dev.test.broken".into(),
            reason: "intentional".into(),
        })
    }
}

// ── Setup helpers ────────────────────────────────────────────────────────────

/// Build a core-plugin manifest with one IPC command registered at handler id `hid`.
fn core_manifest_with_ipc(id: &str, cmd: &str, hid: u32) -> nexus_plugins::PluginManifest {
    let toml = format!(
        r#"
[plugin]
id = "{id}"
name = "Core test plugin"
version = "1.0.0"
trust_level = "core"
api_version = "1"

[[registrations.ipc_command]]
id = "{cmd}"
handler_id = {hid}
"#
    );
    parse_manifest(&toml, "plugin.toml").expect("parse manifest")
}

fn core_manifest_no_ipc(id: &str) -> nexus_plugins::PluginManifest {
    let toml = format!(
        r#"
[plugin]
id = "{id}"
name = "Core test plugin"
version = "1.0.0"
trust_level = "core"
api_version = "1"
"#
    );
    parse_manifest(&toml, "plugin.toml").expect("parse manifest")
}

struct Fixture {
    _forge: tempfile::TempDir,
    forge_root: std::path::PathBuf,
    dispatcher: Arc<dyn IpcDispatcher>,
    echo_calls: Arc<Mutex<Option<(u32, serde_json::Value)>>>,
}

/// Spin up a loader with:
/// - `dev.test.caller`  (no IPC commands — represents the calling plugin)
/// - `dev.test.echo`    (one IPC command `echo` at handler id 7)
/// - `dev.test.broken`  (one IPC command `boom`  at handler id 1, always errors)
fn fixture() -> Fixture {
    let forge = tempfile::tempdir().expect("forge tempdir");
    let plugins_dir = tempfile::tempdir().expect("plugins tempdir");
    let plugin_dir = plugins_dir.path(); // one shared dir is fine for core plugins

    let mut loader = PluginLoader::new(plugins_dir.path());

    let echo_calls: Arc<Mutex<Option<(u32, serde_json::Value)>>> = Arc::new(Mutex::new(None));

    loader
        .register_core(
            core_manifest_no_ipc("dev.test.caller"),
            plugin_dir,
            Box::new(EchoPlugin {
                last_call: Arc::new(Mutex::new(None)),
            }),
        )
        .expect("register caller");
    loader
        .register_core(
            core_manifest_with_ipc("dev.test.echo", "echo", 7),
            plugin_dir,
            Box::new(EchoPlugin {
                last_call: echo_calls.clone(),
            }),
        )
        .expect("register echo");
    loader
        .register_core(
            core_manifest_with_ipc("dev.test.broken", "boom", 1),
            plugin_dir,
            Box::new(BrokenPlugin),
        )
        .expect("register broken");

    let dispatcher: Arc<dyn IpcDispatcher> = Arc::new(SharedPluginLoader::new(loader));

    Fixture {
        forge_root: forge.path().to_path_buf(),
        _forge: forge,
        dispatcher,
        echo_calls,
    }
}

fn make_context(
    fx: &Fixture,
    plugin_id: &str,
    caps: &[Capability],
    with_dispatcher: bool,
) -> KernelPluginContext {
    let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::new());
    let bus = Arc::new(EventBus::new(16));
    KernelPluginContext::new(
        plugin_id,
        "1.0.0",
        CapabilitySet::from_iter(caps.iter().copied()),
        kv,
        bus,
        fx.forge_root.as_path(),
        if with_dispatcher {
            Some(fx.dispatcher.clone())
        } else {
            None
        },
    )
    .expect("construct context")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn ipc_call_routes_to_target_plugin() {
    let fx = fixture();
    let ctx = make_context(&fx, "dev.test.caller", &[Capability::IpcCall], true);

    let reply = ctx
        .ipc_call(
            "dev.test.echo",
            "echo",
            serde_json::json!({ "hello": 1 }),
            Duration::from_secs(5),
        )
        .await
        .expect("ipc_call succeeds");

    assert_eq!(reply["handler"], 7);
    assert_eq!(reply["echoed"], serde_json::json!({ "hello": 1 }));

    let captured = fx.echo_calls.lock().unwrap().clone();
    assert_eq!(
        captured,
        Some((7u32, serde_json::json!({ "hello": 1 }))),
        "handler should see exact args"
    );
}

#[tokio::test]
async fn ipc_call_without_capability_is_denied() {
    let fx = fixture();
    let ctx = make_context(&fx, "dev.test.caller", &[], true);

    let err = ctx
        .ipc_call(
            "dev.test.echo",
            "echo",
            serde_json::json!({}),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(err, IpcError::CapabilityDenied { ref plugin_id } if plugin_id == "dev.test.caller"),
        "got {err:?}"
    );
}

#[tokio::test]
async fn ipc_call_unknown_plugin_returns_plugin_not_found() {
    let fx = fixture();
    let ctx = make_context(&fx, "dev.test.caller", &[Capability::IpcCall], true);

    let err = ctx
        .ipc_call(
            "dev.test.missing",
            "echo",
            serde_json::json!({}),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(err, IpcError::PluginNotFound { ref plugin_id } if plugin_id == "dev.test.missing"),
        "got {err:?}"
    );
}

#[tokio::test]
async fn ipc_call_unknown_command_returns_command_not_found() {
    let fx = fixture();
    let ctx = make_context(&fx, "dev.test.caller", &[Capability::IpcCall], true);

    let err = ctx
        .ipc_call(
            "dev.test.echo",
            "not-a-command",
            serde_json::json!({}),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            IpcError::CommandNotFound { ref plugin_id, ref command }
                if plugin_id == "dev.test.echo" && command == "not-a-command"
        ),
        "got {err:?}"
    );
}

#[tokio::test]
async fn ipc_call_handler_error_becomes_crashed_during_call() {
    let fx = fixture();
    let ctx = make_context(&fx, "dev.test.caller", &[Capability::IpcCall], true);

    let err = ctx
        .ipc_call(
            "dev.test.broken",
            "boom",
            serde_json::json!({}),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            IpcError::PluginCrashedDuringCall { ref plugin_id, ref command, .. }
                if plugin_id == "dev.test.broken" && command == "boom"
        ),
        "got {err:?}"
    );
}

// ── Async-path tests ─────────────────────────────────────────────────────────

/// A core plugin that exposes both sync and async handlers.
struct DualPlugin;

impl CorePlugin for DualPlugin {
    fn dispatch(
        &mut self,
        _handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        Err(PluginError::ExecutionFailed {
            plugin_id: "dev.test.dual".into(),
            reason: "sync path should not be hit when async handler is present".into(),
        })
    }

    fn dispatch_async(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        let args = args.clone();
        Some(Box::pin(async move {
            // A trivial `.await` point proves the future runs through the
            // async executor rather than synchronously.
            tokio::task::yield_now().await;
            Ok(serde_json::json!({ "handler": handler_id, "echoed": args }))
        }))
    }
}

#[tokio::test]
async fn ipc_call_prefers_async_handler() {
    let forge = tempfile::tempdir().expect("forge tempdir");
    let plugins_dir = tempfile::tempdir().expect("plugins tempdir");
    let plugin_dir = plugins_dir.path();

    let mut loader = PluginLoader::new(plugins_dir.path());
    loader
        .register_core(
            core_manifest_no_ipc("dev.test.caller"),
            plugin_dir,
            Box::new(EchoPlugin {
                last_call: Arc::new(Mutex::new(None)),
            }),
        )
        .expect("register caller");
    loader
        .register_core(
            core_manifest_with_ipc("dev.test.dual", "dual", 42),
            plugin_dir,
            Box::new(DualPlugin),
        )
        .expect("register dual");

    let dispatcher: Arc<dyn IpcDispatcher> = Arc::new(SharedPluginLoader::new(loader));

    let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::new());
    let bus = Arc::new(EventBus::new(16));
    let ctx = KernelPluginContext::new(
        "dev.test.caller",
        "1.0.0",
        CapabilitySet::from_iter([Capability::IpcCall].iter().copied()),
        kv,
        bus,
        forge.path(),
        Some(dispatcher),
    )
    .expect("construct context");

    let reply = ctx
        .ipc_call(
            "dev.test.dual",
            "dual",
            serde_json::json!({ "n": 3 }),
            Duration::from_secs(5),
        )
        .await
        .expect("async ipc_call succeeds");

    assert_eq!(reply["handler"], 42);
    assert_eq!(reply["echoed"], serde_json::json!({ "n": 3 }));
}

#[tokio::test]
async fn ipc_call_without_dispatcher_returns_unavailable() {
    let fx = fixture();
    let ctx = make_context(&fx, "dev.test.caller", &[Capability::IpcCall], false);

    let err = ctx
        .ipc_call(
            "dev.test.echo",
            "echo",
            serde_json::json!({}),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(err, IpcError::DispatcherUnavailable),
        "got {err:?}"
    );
}

