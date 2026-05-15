//! BL-138 — named args-aware capability policies.
//!
//! The per-handler cap matrix in [`crate::cap_matrix`] is declarative
//! (TOML), but a handful of handlers (`com.nexus.ai::stream_chat`,
//! `com.nexus.ai::propose_tool_calls` per ADR 0022 Phase 2) need to
//! inspect call arguments to decide which caps are required. Those
//! closures cannot live in TOML, so each one is registered here under
//! a stable name and referenced from the matrix via
//! `policy = "<name>"`.
//!
//! Adding a new policy:
//!   1. Write the closure as a free function in this module.
//!   2. Add a row to [`POLICIES`].
//!   3. Reference it from `cap_matrix.toml`'s `policy = "<name>"`
//!      field on the handler row.

use std::sync::Arc;

use nexus_plugins::CapRequirementFn;

/// Static registry of named policies referenceable from the cap
/// matrix file. Each entry is `(policy_name, closure_constructor)`.
/// The matrix loader fails at bootstrap if a `policy = "…"` row
/// references a name absent from this table.
fn policies() -> Vec<(&'static str, CapRequirementFn)> {
    vec![("ai_tools_policy", ai_tools_policy())]
}

/// Resolve a policy name to its closure. Returns `None` if no policy
/// is registered under `name`; the matrix loader treats that as a
/// fatal bootstrap error.
#[must_use]
pub fn resolve(name: &str) -> Option<CapRequirementFn> {
    policies()
        .into_iter()
        .find(|(n, _)| *n == name)
        .map(|(_, f)| f)
}

/// True iff `name` is a registered policy. Used by the matrix
/// loader's validation pass before any apply happens.
#[must_use]
pub fn is_registered(name: &str) -> bool {
    policies().iter().any(|(n, _)| *n == name)
}

/// ADR 0022 Phase 2 — args-aware tool-policy enforcement for
/// `com.nexus.ai::{stream_chat, propose_tool_calls}`. A caller that
/// requests `tools=auto` (default) needs `ai.tools.write` because
/// the registry includes `write_file`; `auto_with_mcp` additionally
/// needs `ai.tools.mcp`. Read-only and no-tools paths add nothing
/// on top of `ai.chat`. Both handlers carry the field in the same
/// shape, so they share the closure.
fn ai_tools_policy() -> CapRequirementFn {
    Arc::new(|args: &serde_json::Value| {
        let policy = args
            .get("tools")
            .and_then(|v| serde_json::from_value::<nexus_ai::ipc::AiToolPolicy>(v.clone()).ok())
            .unwrap_or_default();
        nexus_ai::ipc::extra_caps_for_policy(policy)
    })
}
