//! Regression test for issue #77 — capability laundering via the
//! terminal and MCP plugins.
//!
//! Pre-#77, any caller holding `IpcCall` could invoke
//! `com.nexus.terminal::create_session` (which takes guest-supplied
//! `shell` + `working_dir` + `env` and spawns the process) or
//! `com.nexus.mcp.host::connect` (which takes a `McpServerSpec` and
//! spawns the MCP server's `command` over stdio). The capability
//! check on the caller's side was just `IpcCall`, so the effect of
//! `ProcessSpawn` was reachable to anyone the user had granted IPC
//! consent to — including LLM-influenced agent tool dispatches and
//! user-authored `.workflows/*.toml` steps.
//!
//! The fix introduces a per-(target, command) capability gate at the
//! kernel context's `ipc_call`. Bootstrap registers
//! `(com.nexus.terminal, create_session) -> [ProcessSpawn]` and
//! `(com.nexus.mcp.host, connect) -> [ProcessSpawn]`, and the kernel
//! denies the dispatch unless the caller's `CapabilitySet` covers the
//! requirement.
//!
//! These tests build a real runtime (so the loader's bootstrap
//! requirements are populated) and then construct fresh per-call
//! contexts with controlled cap sets pointing at the same dispatcher,
//! so we can assert both the deny shape and the positive controls
//! without actually spawning a shell or an MCP server (the deny
//! happens before the handler runs; the positive controls use args
//! shaped to fail at arg-parse time so the test never invokes
//! `Command::new`).

use std::sync::Arc;
use std::time::Duration;

use nexus_kernel::{
    Capability, CapabilitySet, EventBus, Ipc as _, IpcDispatcher, IpcError, KernelPluginContext,
    KvStore,
};
use nexus_plugins::SharedPluginLoader;

fn make_ctx(
    loader: Arc<SharedPluginLoader>,
    forge_root: &std::path::Path,
    caps: &[Capability],
) -> KernelPluginContext {
    let kv: Arc<dyn KvStore> = Arc::new(nexus_kernel::InMemoryKvStore::new());
    let bus = Arc::new(EventBus::new(16));
    let cap_set: CapabilitySet = caps.iter().copied().collect();
    let dispatcher: Arc<dyn IpcDispatcher> = loader as Arc<dyn IpcDispatcher>;
    KernelPluginContext::new(
        "com.test.caller",
        "0.0.1",
        cap_set,
        kv,
        bus,
        forge_root,
        Some(dispatcher),
    )
    .expect("build context")
}

/// Tuple of (target_plugin_id, command_id) the audit identified as
/// requiring `ProcessSpawn` because the handler ends up in a
/// `Command::new` somewhere.
const GATED_COMMANDS: &[(&str, &str)] = &[
    ("com.nexus.terminal", "create_session"),
    ("com.nexus.mcp.host", "connect"),
];

#[tokio::test]
async fn ipc_call_denied_without_process_spawn_capability() {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init forge");
    let runtime =
        nexus_bootstrap::build_cli_runtime(dir.path().to_path_buf()).expect("build runtime");
    let loader = Arc::clone(&runtime.loader);

    // Caller holds only `IpcCall` — same shape as workflow's post-#73 caps.
    let ctx = make_ctx(loader, dir.path(), &[Capability::IpcCall]);

    for (target, command) in GATED_COMMANDS {
        let err = ctx
            .ipc_call(
                target,
                command,
                serde_json::json!({}),
                Duration::from_secs(5),
            )
            .await
            .expect_err(&format!(
                "{target}::{command} must be denied without ProcessSpawn"
            ));
        match err {
            IpcError::CapabilityDenied { plugin_id } => {
                assert_eq!(
                    plugin_id, "com.test.caller",
                    "denied envelope must name the caller, not the target"
                );
            }
            other => panic!("{target}::{command}: expected CapabilityDenied, got: {other:?}"),
        }
    }
}

#[tokio::test]
async fn ipc_call_passes_gate_with_process_spawn_capability() {
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init forge");
    let runtime =
        nexus_bootstrap::build_cli_runtime(dir.path().to_path_buf()).expect("build runtime");
    let loader = Arc::clone(&runtime.loader);

    // Caller holds `IpcCall + ProcessSpawn` — same shape the shell's
    // invoker context has via `Capability::ALL`. Should pass the gate.
    // We send empty args so the handler fails at arg-parse before
    // actually spawning — the test signal is "error class != CapabilityDenied".
    let ctx = make_ctx(
        loader,
        dir.path(),
        &[Capability::IpcCall, Capability::ProcessSpawn],
    );

    for (target, command) in GATED_COMMANDS {
        let result = ctx
            .ipc_call(
                target,
                command,
                serde_json::json!({}),
                Duration::from_secs(5),
            )
            .await;
        match result {
            Ok(_) => {
                // Some commands accept `{}` — that's fine, the gate passed.
            }
            Err(IpcError::CapabilityDenied { .. }) => {
                panic!("{target}::{command}: gate must pass with ProcessSpawn granted");
            }
            Err(_other) => {
                // Any other error class (arg parse, plugin not started,
                // etc.) is fine — proves the gate let the call through.
            }
        }
    }
}

#[tokio::test]
async fn unrelated_targets_pass_with_only_ipc_call() {
    // Positive control: the gate is per-(target, command), not a
    // global "IpcCall is restricted" change. A caller with only
    // `IpcCall` must still reach handlers without registered
    // requirements (e.g., `com.nexus.storage::read_file`).
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init forge");
    let runtime =
        nexus_bootstrap::build_cli_runtime(dir.path().to_path_buf()).expect("build runtime");
    let loader = Arc::clone(&runtime.loader);

    let ctx = make_ctx(loader, dir.path(), &[Capability::IpcCall]);

    let result = ctx
        .ipc_call(
            "com.nexus.storage",
            "read_file",
            serde_json::json!({"path": "does/not/exist.md"}),
            Duration::from_secs(5),
        )
        .await;
    match result {
        Ok(_) => { /* missing file returns null bytes — fine */ }
        Err(IpcError::CapabilityDenied { plugin_id }) => panic!(
            "com.nexus.storage::read_file must not be cap-gated; got CapabilityDenied for {plugin_id}",
        ),
        Err(_) => { /* any other class = gate let it through */ }
    }
}

#[test]
fn loader_required_caller_caps_returns_process_spawn_for_gated_commands() {
    // White-box test of the SharedPluginLoader policy after bootstrap
    // has populated it. Catches regressions where bootstrap stops
    // calling `add_cap_requirement` (the integration tests would
    // still pass if the gate happened to be configured elsewhere;
    // this test pins the loader-side wiring specifically).
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init forge");
    let runtime =
        nexus_bootstrap::build_cli_runtime(dir.path().to_path_buf()).expect("build runtime");
    let loader: Arc<dyn IpcDispatcher> = Arc::clone(&runtime.loader) as Arc<dyn IpcDispatcher>;

    for (target, command) in GATED_COMMANDS {
        let caps = loader.required_caller_caps(target, command);
        assert!(
            caps.contains(&Capability::ProcessSpawn),
            "{target}::{command} must require ProcessSpawn; got: {caps:?}"
        );
    }

    // Spot-check an unrelated command returns no requirements.
    let unrelated = loader.required_caller_caps("com.nexus.storage", "read_file");
    assert!(
        unrelated.is_empty(),
        "com.nexus.storage::read_file must not have a cap requirement; got: {unrelated:?}"
    );
}
