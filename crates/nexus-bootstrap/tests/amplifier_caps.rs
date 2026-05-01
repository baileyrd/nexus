//! Regression test for issue #73 — the workflow and agent core
//! plugins were wired with `KernelPluginContext`s holding
//! `Capability::ALL`, even though their actual direct-cap usage is
//! much narrower. That over-grant meant an LLM-generated agent plan
//! or a user-authored `.workflows/*.toml` step could exercise
//! arbitrary kernel-side capabilities (FsWrite outside the forge,
//! NetHttp, ProcessSpawn, …) without going through the gates that
//! capability model is supposed to enforce.
//!
//! The fix narrows the wired contexts to exactly the caps each
//! plugin actually uses directly:
//!
//! - `com.nexus.workflow`: `{ IpcCall }` — every step routes
//!   through `ctx.ipc_call(...)`.
//! - `com.nexus.agent`: `{ IpcCall, FsRead }` — IpcCall for tool
//!   dispatch, FsRead for plan-history file reads at
//!   `crates/nexus-agent/src/core_plugin.rs:517`.
//!
//! Bootstrap exposes the cap-set helpers
//! (`nexus_bootstrap::agent_capabilities` /
//! `nexus_bootstrap::workflow_capabilities`) so this test can pin
//! their contents from outside the crate. The wiring at
//! `lib.rs:185-205` / `lib.rs:218-238` calls these helpers by name,
//! which makes any drift between helper and wiring loud at code
//! review time. The wired-context capabilities — what
//! `KernelPluginContext` uses for runtime capability checks — live
//! inside the `KernelPluginContext` instance handed to
//! `wire_context` and are not externally readable; the helper
//! functions are the externally-visible source of truth.
//!
//! We assert both directions:
//!
//! 1. Each cap that is supposed to be granted is in fact present
//!    (positive control — without this, dropping IpcCall by mistake
//!    would silently break agent/workflow IPC).
//! 2. Each high-impact cap that should NOT be granted is absent
//!    (the regression-protection direction — this is the property
//!    that pre-fix was violated).

use nexus_bootstrap::{agent_capabilities, workflow_capabilities};
use nexus_kernel::{Capability, CapabilitySet};

/// HIGH-impact caps the audit was specifically worried about that no
/// amplifier should hold directly. `FsRead`/`FsWrite` aren't here
/// because the kernel confines those to the forge root via
/// `confine_path`; the dangerous escape-the-forge variants are the
/// `*External` ones, plus NetHttp / ProcessSpawn.
const FORBIDDEN_AMPLIFIER_CAPS: &[Capability] = &[
    Capability::FsReadExternal,
    Capability::FsWriteExternal,
    Capability::NetHttp,
    Capability::ProcessSpawn,
];

#[test]
fn agent_caps_match_documented_direct_usage() {
    let caps = agent_capabilities();

    // Positive control: the caps the agent legitimately uses.
    assert!(
        caps.contains(Capability::IpcCall),
        "agent must keep IpcCall — every ToolCall dispatches via ipc_call"
    );
    assert!(
        caps.contains(Capability::FsRead),
        "agent must keep FsRead — core_plugin.rs:517 reads plan history via ctx.read_file"
    );
    assert!(
        caps.contains(Capability::FsWrite),
        "agent must keep FsWrite — core_plugin.rs:580 deletes plan history via ctx.delete_file"
    );

    // Regression direction: high-impact escape-the-forge caps must NOT
    // leak through. FsRead/FsWrite are confined to the forge root by
    // the kernel; FsReadExternal/FsWriteExternal/NetHttp/ProcessSpawn
    // are the ones that grant unbounded reach.
    for cap in FORBIDDEN_AMPLIFIER_CAPS {
        assert!(
            !caps.contains(*cap),
            "agent context must not directly hold {cap:?} — that's the #73 amplifier-plugin pattern"
        );
    }
}

#[test]
fn workflow_caps_grant_only_ipccall() {
    let caps = workflow_capabilities();

    // Positive control: the cap the workflow plugin legitimately uses.
    assert!(
        caps.contains(Capability::IpcCall),
        "workflow must keep IpcCall — every step routes through ctx.ipc_call(...)"
    );

    // Regression direction.
    for cap in FORBIDDEN_AMPLIFIER_CAPS {
        assert!(
            !caps.contains(*cap),
            "workflow context must not directly hold {cap:?} — that's the #73 amplifier-plugin pattern"
        );
    }
    assert!(
        !caps.contains(Capability::FsRead),
        "workflow does not directly read files — all reads route through ipc_call to storage"
    );
    assert!(
        !caps.contains(Capability::FsWrite),
        "workflow does not directly write files — all writes route through ipc_call to storage"
    );
}

#[test]
fn neither_amplifier_holds_capability_all() {
    // The audit's headline shape: workflow + agent had `Capability::ALL`.
    // Both must now be a strict subset.
    let agent = agent_capabilities();
    let workflow = workflow_capabilities();
    let all: CapabilitySet = Capability::ALL.iter().copied().collect();

    let agent_count = agent.iter().count();
    let workflow_count = workflow.iter().count();
    let all_count = all.iter().count();

    assert!(
        agent_count < all_count,
        "agent must not equal Capability::ALL: agent has {agent_count} of {all_count}"
    );
    assert!(
        workflow_count < all_count,
        "workflow must not equal Capability::ALL: workflow has {workflow_count} of {all_count}"
    );
}

#[test]
fn cli_runtime_wires_amplifier_plugins_without_panicking() {
    // Smoke: the wiring path that constructs the contexts using the
    // cap-set helpers must still build a runtime end-to-end. Catches
    // the case where a future refactor accidentally drops a cap the
    // plugins' on_init / lifecycle code needs.
    let dir = tempfile::tempdir().expect("tempdir");
    nexus_storage::StorageEngine::init(dir.path()).expect("init scratch forge");
    let _runtime = nexus_bootstrap::build_cli_runtime(dir.path().to_path_buf())
        .expect("runtime wiring with restricted amplifier caps must succeed");
}
