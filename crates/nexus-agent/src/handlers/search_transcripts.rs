//! BL-121 — `search_transcripts` handler.

use nexus_plugins::PluginError;

use super::shared::{exec_err, parse};

/// Synchronous handler — the FTS5 index is in-process so no kernel
/// IPC is needed.
pub(crate) fn handle_search_transcripts(
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: crate::transcript_search::SearchArgs =
        parse(args, "search_transcripts")?;
    let Some(store) = crate::transcript_search::global() else {
        return Ok(serde_json::json!({
            "hits": [],
            "available": false,
            "reason": "transcript-search index not initialised; boot the agent plugin against a forge",
        }));
    };
    let hits = store
        .search(&parsed)
        .map_err(|e| exec_err(format!("search_transcripts: {e}")))?;
    Ok(serde_json::json!({
        "hits": hits,
        "available": true,
    }))
}
