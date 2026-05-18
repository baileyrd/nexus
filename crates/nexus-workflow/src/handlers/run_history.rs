//! `com.nexus.workflow::run_history` handler — list persisted run
//! history rows. Lifted out of `core_plugin.rs` by the BL-137
//! oversized-file decomposition.

use std::sync::Arc;

use nexus_plugins::PluginError;

use crate::core_plugin::RunHistoryArgs;
use crate::run_history::RunHistoryStore;

use super::shared::{parse_args, to_value};

pub(crate) fn handle(
    store: &Arc<RunHistoryStore>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let a: RunHistoryArgs = parse_args(args, "run_history")?;
    let limit = a.limit.map(|n| usize::try_from(n).unwrap_or(usize::MAX));
    let rows = store.list(a.name.as_deref(), limit);
    to_value(&rows, "run_history")
}
