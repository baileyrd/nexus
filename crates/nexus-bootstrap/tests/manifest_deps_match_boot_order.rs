//! Phase 3.3 — verifies that every core plugin's declared
//! `MANIFEST_DEPS` is satisfied by the registration order in
//! `crates/nexus-bootstrap/src/plugins/mod.rs::register_all`.
//!
//! The loader's runtime `check_dependencies` enforces this at boot,
//! but a build-time test catches drift earlier and produces a clearer
//! error message. If you add an IPC call from `nexus-foo` into
//! `com.nexus.bar`, and `bar` registers later than `foo`, this test
//! fails with the exact ordering violation.
//!
//! Source of truth for the boot order is the BOOT_ORDER constant
//! below — keep it synced with `register_all`. The dep_invariants
//! test that gates direct Cargo dependencies covers a complementary
//! concern; this one covers declared IPC fan-out.

/// Reverse-DNS plugin ids in the exact order `register_all` loads
/// them. Must mirror `crates/nexus-bootstrap/src/plugins/mod.rs`.
const BOOT_ORDER: &[&str] = &[
    "com.nexus.security",
    "com.nexus.storage",
    "com.nexus.database",
    "com.nexus.editor",
    "com.nexus.theme",
    "com.nexus.ai.runtime",
    "com.nexus.ai",
    "com.nexus.skills",
    "com.nexus.templates",
    "com.nexus.formats",
    "com.nexus.workflow",
    "com.nexus.linkpreview",
    "com.nexus.notifications",
    "com.nexus.audio",
    "com.nexus.comments",
    "com.nexus.agent",
    "com.nexus.mcp.host",
    "com.nexus.lsp",
    "com.nexus.dap",
    "com.nexus.acp",
    "com.nexus.git",
    "com.nexus.terminal",
    "com.nexus.collab",
];

/// Every (plugin_id, MANIFEST_DEPS_slice) pair the test inspects.
/// Add a new row when a core plugin starts declaring deps.
fn registered_deps() -> Vec<(&'static str, &'static [&'static str])> {
    vec![
        ("com.nexus.editor", nexus_editor::core_plugin::MANIFEST_DEPS),
        (
            "com.nexus.terminal",
            nexus_terminal::core_plugin::MANIFEST_DEPS,
        ),
        ("com.nexus.git", nexus_git::core_plugin::MANIFEST_DEPS),
        ("com.nexus.ai", nexus_ai::core_plugin::MANIFEST_DEPS),
        ("com.nexus.agent", nexus_agent::core_plugin::MANIFEST_DEPS),
        (
            "com.nexus.workflow",
            nexus_workflow::core_plugin::MANIFEST_DEPS,
        ),
        ("com.nexus.audio", nexus_audio::core_plugin::MANIFEST_DEPS),
    ]
}

#[test]
fn manifest_deps_load_before_consumer() {
    let order: std::collections::HashMap<&str, usize> = BOOT_ORDER
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i))
        .collect();

    let mut violations = Vec::new();
    for (consumer_id, deps) in registered_deps() {
        let consumer_idx = order.get(consumer_id).copied().unwrap_or_else(|| {
            panic!(
                "consumer plugin id {} is not present in BOOT_ORDER; \
                 update the constant if register_all changed",
                consumer_id
            )
        });
        for dep_id in deps {
            let dep_idx = match order.get(dep_id) {
                Some(idx) => *idx,
                None => {
                    violations.push(format!(
                        "{} declares dep '{}' which is not a known core plugin id",
                        consumer_id, dep_id
                    ));
                    continue;
                }
            };
            if dep_idx >= consumer_idx {
                violations.push(format!(
                    "{} (boot order #{}) declares dep '{}' (boot order #{}), \
                     which loads AFTER it — runtime check_dependencies would reject this",
                    consumer_id, consumer_idx, dep_id, dep_idx
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "MANIFEST_DEPS / boot-order violations:\n  - {}",
        violations.join("\n  - ")
    );
}

#[test]
fn boot_order_constant_length_matches_register_all() {
    // Sanity check — BOOT_ORDER should list 23 plugins. If the
    // microkernel grows or shrinks, update both this constant and
    // `register_all` together.
    assert_eq!(
        BOOT_ORDER.len(),
        23,
        "BOOT_ORDER must match the count in register_all"
    );
}
