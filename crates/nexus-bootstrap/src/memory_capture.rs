//! Bus-event capture pump — the AI-first-class "everything on the bus" feed.
//!
//! Subscribes to every kernel event and writes each into the native memory
//! store via [`nexus_memory::event_to_memory`], which drops the memory plugin's
//! own events (feedback-loop guard) and redacts secret-looking payload values.
//!
//! Runs as a detached tokio task for the process lifetime (mirrors
//! [`crate::collab::start_if_enabled`]): lagging is logged, a closed bus ends
//! the task. Best-effort — if the store can't be opened, capture is disabled
//! without affecting boot. Concurrent writes are safe because [`MemoryDb`]
//! opens in WAL mode with a busy timeout, so this pump and the plugin's IPC
//! handlers don't contend on the same file.

use std::path::Path;
use std::sync::Arc;

use nexus_kernel::{EventBus, EventFilter, RecvError};
use nexus_memory::{event_to_memory, MemoryDb};
use tokio::task::JoinHandle;

/// Start the capture pump against `bus`, persisting into the forge's memory
/// store (`<forge_root>/.forge/memory/memory.db`).
///
/// Returns the detached task handle, or `None` when capture can't start
/// (best-effort; never fatal to boot). Must be called from within a tokio
/// runtime (the bootstrap `build` path is, same as collab's relay).
#[must_use]
pub fn start_capture(forge_root: &Path, bus: Arc<EventBus>) -> Option<JoinHandle<()>> {
    // Best-effort: capture only runs inside a tokio runtime (the CLI/TUI/shell
    // boot path). Synchronous callers without a runtime (some tests, `forge init`)
    // would otherwise panic inside `tokio::spawn` — skip cleanly instead.
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(rt) => rt,
        Err(_) => {
            tracing::debug!("memory capture: no tokio runtime; capture disabled");
            return None;
        }
    };
    let dir = forge_root.join(".forge").join("memory");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!(error = %e, "memory capture: cannot create memory dir; capture disabled");
        return None;
    }
    let db = match MemoryDb::open(&dir.join("memory.db")) {
        Ok(db) => db,
        Err(e) => {
            tracing::warn!(error = %e, "memory capture: cannot open store; capture disabled");
            return None;
        }
    };
    let mut sub = bus.subscribe(EventFilter::All);
    Some(rt.spawn(async move {
        loop {
            match sub.recv().await {
                Ok(ev) => {
                    if let Some(mem) = event_to_memory(&ev) {
                        if let Err(e) = db.insert(&mem) {
                            tracing::debug!(error = %e, "memory capture: insert failed");
                        }
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(
                        dropped = n,
                        "memory capture: subscriber lagged; events dropped"
                    );
                }
                Err(RecvError::Closed) => break,
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_kernel::NexusEvent;

    // Multi-threaded runtime so the spawned capture task runs concurrently with
    // the poll loop — matches the CLI/shell runtime where `build()` is called.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn captures_foreign_events_and_drops_own() {
        let dir = tempfile::tempdir().unwrap();
        let forge = dir.path();
        let bus = Arc::new(EventBus::new(64));
        let handle = start_capture(forge, Arc::clone(&bus)).expect("capture should start");

        // Published FIFO: the memory-namespace event must be dropped (loop guard),
        // the terminal event must be captured. Publishing the dropped one first
        // means it is processed before the captured one, so a final count of
        // exactly 1 proves both behaviours at once.
        bus.publish_core(
            "com.nexus.memory",
            NexusEvent::Custom {
                type_id: "com.nexus.memory.added".to_string(),
                emitting_plugin: "com.nexus.memory".to_string(),
                payload: serde_json::json!({ "id": "x" }),
            },
        )
        .expect("publish");
        bus.publish_core(
            "com.nexus.terminal",
            NexusEvent::Custom {
                type_id: "com.nexus.terminal.command_run".to_string(),
                emitting_plugin: "com.nexus.terminal".to_string(),
                payload: serde_json::json!({ "cmd": "ls" }),
            },
        )
        .expect("publish");

        // Poll a separate store handle until the captured row lands.
        let store = MemoryDb::open(&forge.join(".forge").join("memory").join("memory.db")).unwrap();
        let mut count = 0u64;
        for _ in 0..200 {
            count = store.count().unwrap();
            if count >= 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        handle.abort();
        assert_eq!(
            count, 1,
            "exactly the foreign event is captured; the memory-namespace event is dropped"
        );
        // The captured content is the event topic; the payload (cmd=ls) lives in
        // metadata (FTS indexes content/category/tags, not metadata).
        let hits = store.search("terminal", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source, "event");
        assert_eq!(hits[0].metadata["payload"]["cmd"], "ls");
    }
}
