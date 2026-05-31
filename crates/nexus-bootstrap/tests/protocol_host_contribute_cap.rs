//! BL-113 follow-up — regression test for the `protocol.host.contribute`
//! cap gate on the four protocol-host contribution-lifecycle verbs.
//!
//! ADR 0027 routes contribution wiring through the bootstrap's
//! `*_contribution_wiring` modules, which run as the invoker plugin and
//! therefore hold every capability (TrustLevel::Core => Capability::ALL).
//! A community plugin holding only `ipc.call` must be denied by the kernel
//! at the `required_caller_caps_for_args` check in `ipc_call_inner`, before
//! the call reaches the host handler — otherwise `contributed_by`
//! provenance and marketplace install records desynchronise with the
//! contribution pipeline.
//!
//! This test pins that gate for every register/unregister verb in a single
//! place; the cap_matrix is the actual mechanism, the test just proves the
//! matrix rows do what the comment says.

#![cfg(not(target_arch = "wasm32"))]

#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;
use std::time::Duration;

use common::MinimalForge;
use nexus_kernel::{
    Capability, CapabilitySet, EventBus, InMemoryKvStore, Ipc as _, IpcDispatcher, IpcError,
    KernelPluginContext, KvStore,
};

const CALL_TIMEOUT: Duration = Duration::from_secs(5);

fn community_caller_context(forge: &MinimalForge) -> KernelPluginContext {
    let kv: Arc<dyn KvStore> = Arc::new(InMemoryKvStore::new());
    let bus = Arc::new(EventBus::new(16));
    let dispatcher: Arc<dyn IpcDispatcher> = forge.runtime.loader.clone();
    KernelPluginContext::new(
        "community.evil",
        "1.0.0",
        CapabilitySet::from_iter([Capability::IpcCall]),
        kv,
        bus,
        forge.root(),
        Some(dispatcher),
    )
    .expect("construct community caller context")
}

async fn assert_denied(forge: &MinimalForge, target: &str, command: &str) {
    let ctx = community_caller_context(forge);
    let err = ctx
        .ipc_call(target, command, serde_json::json!({}), CALL_TIMEOUT)
        .await
        .expect_err(&format!(
            "{target}::{command} must reject ipc.call-only callers"
        ));
    assert!(
        matches!(err, IpcError::CapabilityDenied { ref plugin_id } if plugin_id == "community.evil"),
        "{target}::{command} returned {err:?} (expected CapabilityDenied)"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn protocol_host_register_verbs_reject_community_caller() {
    let forge = MinimalForge::new();
    for (target, command) in [
        ("com.nexus.dap", "register_adapter"),
        ("com.nexus.dap", "unregister_adapter"),
        ("com.nexus.lsp", "register_server"),
        ("com.nexus.lsp", "unregister_server"),
        ("com.nexus.mcp.host", "register_server"),
        ("com.nexus.mcp.host", "unregister_server"),
        ("com.nexus.acp", "register_server"),
        ("com.nexus.acp", "unregister_server"),
    ] {
        assert_denied(&forge, target, command).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn protocol_host_register_verbs_admit_invoker_caller() {
    // The invoker context (CLI) holds Capability::ALL via the
    // `TrustLevel::Core` grant, so `protocol.host.contribute` is held
    // automatically. The handlers may still reject the synthetic args
    // we pass (no `id`, no `command`, etc.) but the failure mode must
    // be a handler-level error, not a kernel-level `CapabilityDenied`
    // — that's what proves the gate is at the right layer.
    let forge = MinimalForge::new();
    for (target, command) in [
        ("com.nexus.dap", "register_adapter"),
        ("com.nexus.lsp", "register_server"),
        ("com.nexus.mcp.host", "register_server"),
        ("com.nexus.acp", "register_server"),
    ] {
        let result = forge.ipc_call(target, command, serde_json::json!({})).await;
        match result {
            Ok(_) => {}
            Err(IpcError::CapabilityDenied { .. }) => {
                panic!(
                    "{target}::{command} rejected the invoker on the cap gate — \
                     `protocol.host.contribute` must be in `Capability::ALL`"
                );
            }
            Err(_) => {} // handler-level rejection is fine — the gate let us through.
        }
    }
}
