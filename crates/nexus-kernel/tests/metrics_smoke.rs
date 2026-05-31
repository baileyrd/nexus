//! BL-093 — metrics smoke test: install the global registry, drive
//! the recording APIs, then assert the snapshot reflects every
//! recorded event.

use std::sync::Arc;

use nexus_kernel::{CallStatus, KernelMetrics};

#[test]
fn install_then_record_then_snapshot_round_trips() {
    // The global is `OnceLock`, so a previous test in the same
    // binary may have installed a registry. We install a fresh
    // registry only when none has been set yet, and then operate
    // through the global accessor regardless.
    let installed = Arc::new(KernelMetrics::new());
    nexus_kernel::metrics::install(Arc::clone(&installed));

    let m = nexus_kernel::metrics::global().expect("metrics installed");

    // Exercise every recording surface.
    m.record_ipc_call("com.test.target", "echo", CallStatus::Ok, 50_000);
    m.record_ipc_call(
        "com.test.target",
        "echo",
        CallStatus::Timeout,
        5_000_000_000,
    );
    m.record_event_publish("com.test.publisher");
    m.record_capability_check("com.test.caller", "fs.read", true);
    m.record_capability_check("com.test.caller", "process.spawn", false);
    m.record_lifecycle_duration("com.test.target", "init", 250_000);

    let snap = m.snapshot();

    assert!(snap.ipc_calls_total["com.test.target::echo::ok"] >= 1);
    assert!(snap.ipc_calls_total["com.test.target::echo::timeout"] >= 1);
    assert!(snap.ipc_call_duration["com.test.target::echo"].count >= 2);

    assert!(snap.event_bus_published_total["com.test.publisher"] >= 1);

    assert!(snap.capability_checks_total["com.test.caller::fs.read::granted"] >= 1);
    assert!(snap.capability_checks_total["com.test.caller::process.spawn::denied"] >= 1);

    assert!(snap.plugin_lifecycle_duration["com.test.target::init"].count >= 1);
}
