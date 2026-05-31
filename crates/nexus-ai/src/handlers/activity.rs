//! BL-037 — activity timeline handlers (`activity_list`, `activity_clear`).

use nexus_plugins::PluginError;

use crate::activity_log::ActivityRecorder;
use crate::handlers::shared::exec_err;
use crate::ipc::{AiActivityListArgs, AiActivityListResult};

pub(crate) async fn handle_activity_list(
    activity: Option<ActivityRecorder>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let parsed: AiActivityListArgs = serde_json::from_value(args.clone()).unwrap_or_default();
    let Some(rec) = activity else {
        // Pre-`wire_context`: no recorder yet. Return an empty list
        // rather than erroring so the shell can poll on activate
        // without a race.
        return serde_json::to_value(&AiActivityListResult {
            entries: Vec::new(),
        })
        .map_err(|e| exec_err(format!("activity_list: encode: {e}")));
    };
    // The on-disk log is oldest-first; the IPC contract returns
    // newest-first. Reverse + cap.
    let mut entries = rec.read_all().await?;
    entries.reverse();
    if let Some(limit) = parsed.limit {
        let limit_usize = limit as usize;
        entries.truncate(limit_usize);
    }
    serde_json::to_value(&AiActivityListResult { entries })
        .map_err(|e| exec_err(format!("activity_list: encode: {e}")))
}

pub(crate) async fn handle_activity_clear(
    activity: Option<ActivityRecorder>,
) -> Result<serde_json::Value, PluginError> {
    let Some(rec) = activity else {
        return Ok(serde_json::json!({ "cleared": false }));
    };
    rec.clear().await?;
    Ok(serde_json::json!({ "cleared": true }))
}
