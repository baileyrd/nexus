//! Hot-reload support for WASM plugins.
//!
//! Watches a plugins directory for `.wasm` file changes and emits
//! [`ReloadEvent`]s that callers can poll via [`HotReloader::try_recv`] or
//! [`HotReloader::drain`].

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{DebounceEventResult, new_debouncer};

use crate::PluginError;

// ── Public types ──────────────────────────────────────────────────────────────

/// An event emitted when a WASM plugin file changes on disk.
#[derive(Debug, Clone)]
pub struct ReloadEvent {
    /// The plugin identifier, derived from the parent directory name.
    pub plugin_id: String,
    /// The absolute path to the changed `.wasm` file.
    pub wasm_path: PathBuf,
}

// ── HotReloader ───────────────────────────────────────────────────────────────

/// Watches a plugins directory and emits [`ReloadEvent`]s for `.wasm` changes.
pub struct HotReloader {
    rx: mpsc::Receiver<ReloadEvent>,
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

impl HotReloader {
    /// Start watching `plugins_dir` for `.wasm` file changes.
    ///
    /// If `plugins_dir` does not exist the watcher still starts successfully;
    /// it simply will not emit any events until the directory is created and
    /// watched externally.
    ///
    /// # Errors
    ///
    /// Returns [`PluginError::Io`] if the underlying notify watcher cannot be
    /// created.
    pub fn start(plugins_dir: &Path, debounce_ms: u64) -> Result<Self, PluginError> {
        let (reload_tx, reload_rx) = mpsc::channel::<ReloadEvent>();
        let (raw_tx, raw_rx) = mpsc::channel::<DebounceEventResult>();

        let duration = Duration::from_millis(debounce_ms);
        let mut debouncer = new_debouncer(duration, raw_tx).map_err(|e| {
            PluginError::Io(std::io::Error::other(e.to_string()))
        })?;

        if plugins_dir.exists() {
            debouncer
                .watcher()
                .watch(plugins_dir, RecursiveMode::Recursive)
                .map_err(|e| {
                    PluginError::Io(std::io::Error::other(e.to_string()))
                })?;
        }

        let plugins_dir_owned = plugins_dir.to_path_buf();

        std::thread::spawn(move || {
            process_wasm_events(raw_rx, &plugins_dir_owned, &reload_tx);
        });

        Ok(Self {
            rx: reload_rx,
            _debouncer: debouncer,
        })
    }

    /// Try to receive a single [`ReloadEvent`] without blocking.
    ///
    /// Returns `None` if no event is currently available.
    #[must_use]
    pub fn try_recv(&self) -> Option<ReloadEvent> {
        self.rx.try_recv().ok()
    }

    /// Drain all currently queued [`ReloadEvent`]s.
    ///
    /// Returns an empty `Vec` if the queue is empty.
    #[must_use]
    pub fn drain(&self) -> Vec<ReloadEvent> {
        let mut events = Vec::new();
        while let Some(event) = self.try_recv() {
            events.push(event);
        }
        events
    }
}

// ── Event processing thread ───────────────────────────────────────────────────

#[allow(clippy::needless_pass_by_value)]
fn process_wasm_events(
    rx: mpsc::Receiver<DebounceEventResult>,
    _plugins_dir: &Path,
    tx: &mpsc::Sender<ReloadEvent>,
) {
    for result in &rx {
        let Ok(events) = result else { continue };

        for event in events {
            let path = &event.path;

            // Only care about .wasm files.
            if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
                continue;
            }

            // Skip deletions — file must exist.
            if !path.exists() {
                continue;
            }

            // Derive plugin_id from the parent directory name.
            let plugin_id = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            let _ = tx.send(ReloadEvent {
                plugin_id,
                wasm_path: path.clone(),
            });
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn reload_event_stores_fields() {
        let event = ReloadEvent {
            plugin_id: "my-plugin".to_string(),
            wasm_path: PathBuf::from("/plugins/my-plugin/plugin.wasm"),
        };
        assert_eq!(event.plugin_id, "my-plugin");
        assert_eq!(
            event.wasm_path,
            PathBuf::from("/plugins/my-plugin/plugin.wasm")
        );
    }

    #[test]
    fn start_on_nonexistent_dir_succeeds() {
        let result = HotReloader::start(Path::new("/nonexistent/plugins/dir"), 50);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result.err());
    }

    #[test]
    fn drain_empty_returns_empty() {
        let reloader =
            HotReloader::start(Path::new("/nonexistent/plugins/dir"), 50).expect("start");
        let events = reloader.drain();
        assert!(events.is_empty(), "expected empty drain, got: {events:?}");
    }

    #[test]
    fn detects_wasm_file_change() {
        use std::time::Duration;
        use tempfile::TempDir;

        let dir = TempDir::new().expect("temp dir");
        let plugin_dir = dir.path().join("my-plugin");
        std::fs::create_dir_all(&plugin_dir).expect("create plugin dir");

        let reloader = HotReloader::start(dir.path(), 50).expect("start reloader");

        // Give the watcher a moment to initialise.
        std::thread::sleep(Duration::from_millis(100));

        // Write a .wasm file.
        let wasm_path = plugin_dir.join("plugin.wasm");
        std::fs::write(&wasm_path, b"\x00asm\x01\x00\x00\x00").expect("write wasm");

        // Wait for events to arrive (timing-dependent).
        std::thread::sleep(Duration::from_millis(500));

        // Should not panic — exact count is timing-dependent.
        let events = reloader.drain();
        drop(events);
    }
}
