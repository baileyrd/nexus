//! Capability types — re-exported from `nexus-plugin-api` for stable ABI (F-2.1.1).
//! All capability types are now defined in `nexus-plugin-api`; this module is
//! a backwards-compatible shim so internal `crate::capability::*` imports compile.
pub use nexus_plugin_api::capability::{Capability, CapabilityParseError, CapabilitySet};
