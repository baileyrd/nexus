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
//!
//! # Governance (C37, #390)
//!
//! Reads an optional `[memory]` block from `<forge>/.forge/config.toml`,
//! mirroring [`crate::collab::CollabConfig`]'s loader shape. Unlike collab
//! (opt-in, default off), passive capture has run unconditionally since it
//! shipped, so [`MemoryConfig::capture_enabled`] defaults to `true` —
//! existing forges see no behavior change until a user opts out.
//!
//! ```toml
//! [memory]
//! capture_enabled = true
//! capture_exclude_plugins = ["com.nexus.terminal"]
//! event_retention_max_rows = 20000
//! ```

use std::path::Path;
use std::sync::Arc;

use nexus_kernel::{EventBus, EventFilter, RecvError};
use nexus_memory::{event_to_memory, MemoryDb};
use serde::Deserialize;
use tokio::task::JoinHandle;

/// On-disk shape for `[memory]` in `.forge/config.toml` — governs the
/// passive bus-capture pump only (C37); the `com.nexus.memory` plugin's own
/// deliberate `add`/`auto_capture`/`sync` verbs are unaffected.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    /// Master switch for the passive capture pump. Defaults to `true` —
    /// capture has always run unconditionally; this makes it possible to
    /// turn off, not off by default.
    pub capture_enabled: bool,
    /// Event source-plugin id prefixes to skip capturing (e.g.
    /// `"com.nexus.terminal"` to stop recording every shell command).
    /// Checked against [`nexus_plugin_api::event::EventMetadata::source_plugin_id`].
    /// Empty (default) captures every plugin's events, same as before C37.
    pub capture_exclude_plugins: Vec<String>,
    /// When set, passively-captured rows (`source = "event"`) beyond this
    /// count are pruned oldest-first after each insert (see
    /// [`nexus_memory::db::MemoryDb::prune_event_rows`]). `None` (default)
    /// is unbounded, matching pre-C37 behavior. Deliberate memories
    /// (`add`/`auto_capture`/`import`) are never pruned by this setting.
    #[serde(default)]
    pub event_retention_max_rows: Option<u64>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            capture_enabled: true,
            capture_exclude_plugins: Vec::new(),
            event_retention_max_rows: None,
        }
    }
}

/// Read `<forge>/.forge/config.toml` and return the `[memory]` block.
/// Missing file / missing block / parse errors all collapse to
/// `MemoryConfig::default()` (capture on, unfiltered, unbounded) — matching
/// [`crate::collab::load_config`]'s fail-safe shape.
fn load_config(forge_root: &Path) -> MemoryConfig {
    #[derive(Deserialize)]
    struct Wrapper {
        #[serde(default)]
        memory: Option<MemoryConfig>,
    }
    let path = forge_root.join(".forge").join("config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return MemoryConfig::default();
        }
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "config.toml: read failed; [memory] defaults");
            return MemoryConfig::default();
        }
    };
    match toml::from_str::<Wrapper>(&text) {
        Ok(w) => w.memory.unwrap_or_default(),
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "config.toml: [memory] failed to parse; defaults");
            MemoryConfig::default()
        }
    }
}

/// Start the capture pump against `bus`, persisting into the forge's memory
/// store (`<forge_root>/.forge/memory/memory.db`), governed by the
/// `[memory]` block in `.forge/config.toml` (C37).
///
/// Returns the detached task handle, or `None` when capture can't start or
/// is disabled (best-effort; never fatal to boot). Must be called from
/// within a tokio runtime (the bootstrap `build` path is, same as collab's
/// relay).
#[must_use]
pub fn start_capture(forge_root: &Path, bus: Arc<EventBus>) -> Option<JoinHandle<()>> {
    let cfg = load_config(forge_root);
    if !cfg.capture_enabled {
        tracing::debug!("memory capture: disabled via [memory].capture_enabled = false");
        return None;
    }
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
                    let excluded = cfg
                        .capture_exclude_plugins
                        .iter()
                        .any(|p| ev.metadata.source_plugin_id.starts_with(p.as_str()));
                    if excluded {
                        continue;
                    }
                    if let Some(mem) = event_to_memory(&ev) {
                        if let Err(e) = db.insert(&mem) {
                            tracing::debug!(error = %e, "memory capture: insert failed");
                        } else if let Some(max_rows) = cfg.event_retention_max_rows {
                            if let Err(e) = db.prune_event_rows(max_rows) {
                                tracing::debug!(error = %e, "memory capture: retention prune failed");
                            }
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

    // ── C37 (#390) — [memory] config governance ─────────────────────────

    #[test]
    fn load_config_defaults_to_capture_enabled_for_a_missing_file() {
        // Unlike [collab] (opt-in, default off), capture has always run
        // unconditionally — the default must preserve that, not flip it.
        let dir = tempfile::tempdir().unwrap();
        let cfg = load_config(dir.path());
        assert!(cfg.capture_enabled);
        assert!(cfg.capture_exclude_plugins.is_empty());
        assert_eq!(cfg.event_retention_max_rows, None);
    }

    #[test]
    fn load_config_parses_a_complete_block() {
        let dir = tempfile::tempdir().unwrap();
        let forge = dir.path().join(".forge");
        std::fs::create_dir_all(&forge).unwrap();
        std::fs::write(
            forge.join("config.toml"),
            r#"
[memory]
capture_enabled = false
capture_exclude_plugins = ["com.nexus.terminal", "com.nexus.audio"]
event_retention_max_rows = 500
            "#,
        )
        .unwrap();
        let cfg = load_config(dir.path());
        assert!(!cfg.capture_enabled);
        assert_eq!(
            cfg.capture_exclude_plugins,
            vec!["com.nexus.terminal".to_string(), "com.nexus.audio".to_string()]
        );
        assert_eq!(cfg.event_retention_max_rows, Some(500));
    }

    #[test]
    fn load_config_falls_back_to_defaults_on_unparsable_toml() {
        let dir = tempfile::tempdir().unwrap();
        let forge = dir.path().join(".forge");
        std::fs::create_dir_all(&forge).unwrap();
        std::fs::write(forge.join("config.toml"), "not valid [ toml").unwrap();
        let cfg = load_config(dir.path());
        assert!(cfg.capture_enabled, "parse failure must fail open to the safe default");
    }

    #[test]
    fn start_capture_skips_when_disabled_via_config() {
        // No tokio runtime is entered here on purpose: the disabled check
        // must short-circuit before start_capture ever touches
        // Handle::try_current, matching collab's start_if_enabled shape.
        let dir = tempfile::tempdir().unwrap();
        let forge = dir.path().join(".forge");
        std::fs::create_dir_all(&forge).unwrap();
        std::fs::write(
            forge.join("config.toml"),
            "[memory]\ncapture_enabled = false\n",
        )
        .unwrap();
        let bus = Arc::new(EventBus::new(8));
        assert!(start_capture(dir.path(), bus).is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capture_exclude_plugins_skips_matching_source_events() {
        let dir = tempfile::tempdir().unwrap();
        let forge = dir.path();
        std::fs::create_dir_all(forge.join(".forge")).unwrap();
        std::fs::write(
            forge.join(".forge").join("config.toml"),
            "[memory]\ncapture_exclude_plugins = [\"com.nexus.terminal\"]\n",
        )
        .unwrap();
        let bus = Arc::new(EventBus::new(64));
        let handle = start_capture(forge, Arc::clone(&bus)).expect("capture should start");

        // Excluded plugin's event must never land; a non-excluded plugin's
        // event published right after it proves the pump kept running (a
        // stuck/panicked task would leave count at 0 for both).
        bus.publish_core(
            "com.nexus.terminal",
            NexusEvent::Custom {
                type_id: "com.nexus.terminal.command_run".to_string(),
                emitting_plugin: "com.nexus.terminal".to_string(),
                payload: serde_json::json!({ "cmd": "rm -rf /" }),
            },
        )
        .expect("publish");
        bus.publish_core(
            "com.nexus.editor",
            NexusEvent::Custom {
                type_id: "com.nexus.editor.saved".to_string(),
                emitting_plugin: "com.nexus.editor".to_string(),
                payload: serde_json::json!({ "path": "notes.md" }),
            },
        )
        .expect("publish");

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
            "only the non-excluded plugin's event should be captured"
        );
        assert_eq!(store.search("rm", 10).unwrap().len(), 0, "excluded plugin's event must not be captured");
        assert_eq!(store.search("editor", 10).unwrap().len(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn event_retention_max_rows_prunes_captured_rows_after_each_insert() {
        let dir = tempfile::tempdir().unwrap();
        let forge = dir.path();
        std::fs::create_dir_all(forge.join(".forge")).unwrap();
        std::fs::write(
            forge.join(".forge").join("config.toml"),
            "[memory]\nevent_retention_max_rows = 2\n",
        )
        .unwrap();
        let bus = Arc::new(EventBus::new(64));
        let handle = start_capture(forge, Arc::clone(&bus)).expect("capture should start");

        for i in 0..5 {
            bus.publish_core(
                "com.nexus.editor",
                NexusEvent::Custom {
                    type_id: "com.nexus.editor.saved".to_string(),
                    emitting_plugin: "com.nexus.editor".to_string(),
                    payload: serde_json::json!({ "path": format!("note-{i}.md") }),
                },
            )
            .expect("publish");
        }

        let store = MemoryDb::open(&forge.join(".forge").join("memory").join("memory.db")).unwrap();
        // Poll until all 5 inserts have landed and pruning has caught the
        // store back down to the configured cap.
        let mut count = 0u64;
        for _ in 0..200 {
            count = store.count().unwrap();
            assert!(count <= 2, "retention cap must never be exceeded, got {count}");
            if count == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        handle.abort();
        assert_eq!(count, 2, "pruned down to the configured cap");
    }
}
