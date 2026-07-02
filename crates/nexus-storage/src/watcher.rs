//! File watcher for the forge storage directory.
//!
//! Wraps `notify-debouncer-mini` to emit [`StorageEvent`]s for changes inside
//! `notes/` and `attachments/` subdirectories of a forge root.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, DebouncedEventKind};

use crate::StorageError;

/// Upper bound on the number of `StorageEvent`s the watcher buffers
/// for its consumer. A stop-the-world consumer (or one slower than
/// the filesystem burst rate of a `git checkout` / mass rename)
/// would otherwise grow the queue without limit; with the bound,
/// per-file events that don't fit are dropped and a single
/// `ReconcileRequested` is enqueued the moment the consumer drains
/// enough to take it. The consumer then re-walks the forge to
/// recover the missed state.
///
/// 1024 comfortably covers a single git operation in a medium
/// vault (~tens-of-thousands of files would still overflow, but the
/// reconcile recovery path is designed for exactly that case). If a
/// settings audit promotes this, route it through `StorageConfig`.
const WATCHER_CHANNEL_BOUND: usize = 1024;

// ── Public types ──────────────────────────────────────────────────────────────

/// An event emitted by the file watcher.
///
/// `FileCreated` and `FileRenamed` are not currently emitted —
/// the underlying notify-debouncer-mini collapses them into
/// `FileModified` / `FileDeleted` shapes, and we re-derive
/// finer-grained events from path-existence checks. The variants
/// remain in the enum so a future watcher upgrade that distinguishes
/// create / rename can land without a public-API break for
/// downstream pattern matchers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageEvent {
    /// A new file was created.
    FileCreated {
        /// Relative path from the forge root.
        path: String,
        /// SHA-256 hex digest of the file content.
        content_hash: String,
    },
    /// An existing file was modified.
    FileModified {
        /// Relative path from the forge root.
        path: String,
        /// SHA-256 hex digest of the file content.
        content_hash: String,
    },
    /// A file was deleted.
    FileDeleted {
        /// Relative path from the forge root.
        path: String,
    },
    /// A file was renamed.
    FileRenamed {
        /// Previous relative path.
        from: String,
        /// New relative path.
        to: String,
        /// SHA-256 hex digest of the new file content.
        content_hash: String,
    },
    /// The watcher is recommending consumers run a reconcile pass
    /// (re-walk the forge root, rebuild the index from the file
    /// system). Emitted after a git "batch mode" — when
    /// `.git/index.lock` exists and then disappears, signaling that
    /// a checkout / rebase / merge probably moved many files at
    /// once and per-file events would be unreliable — OR after the
    /// downstream channel overflowed and per-file events had to be
    /// dropped (see `WATCHER_CHANNEL_BOUND`).
    ///
    /// `dropped_events` is the number of debounced filesystem
    /// notifications the watcher discarded since the last reconcile
    /// signal. Zero means the reconcile is informational (e.g. an
    /// initial-load recommendation); non-zero quantifies what the
    /// consumer needs to recover. Surfaced in
    /// `com.nexus.storage.indexing.completed` so operators can see
    /// when git batch-mode or channel overflow is masking changes.
    ///
    /// Pre-#84 this was signaled in-band by a `FileModified`
    /// event with empty `path` and `content_hash` strings, which
    /// downstream consumers had to special-case. The dedicated
    /// variant makes the contract explicit.
    ReconcileRequested {
        /// Number of debounced filesystem notifications discarded
        /// since the last reconcile signal. Drives operator-visible
        /// observability for git batch-mode and channel overflow.
        dropped_events: usize,
    },
}

// ── Public helpers ────────────────────────────────────────────────────────────

/// Check if a path should be ignored by the watcher.
///
/// Ignores any path whose components include `.git`, `.forge`,
/// `node_modules`, or `target` (intermediate directories the user
/// wouldn't author notes inside), and filenames ending with `~`,
/// `.swp`, or `.DS_Store`.
///
/// Component-based matching: pre-#84 this used substring matching
/// (`path.contains(".git")`), which falsely ignored legitimate
/// note filenames like `my-.git-history-notes.md`. The fix
/// inspects each `Component::Normal` segment for an exact match
/// against the ignored directory names.
#[must_use]
pub fn should_ignore(path: &Path) -> bool {
    use std::path::Component;
    // `.trash` — C3 (#356): forge-level trash; trashed content must not
    // re-enter the index via watcher events or reconcile scans.
    const IGNORED_DIR_COMPONENTS: &[&str] =
        &[".git", ".forge", ".trash", "node_modules", "target"];

    for component in path.components() {
        if let Component::Normal(seg) = component {
            if let Some(seg_str) = seg.to_str() {
                if IGNORED_DIR_COMPONENTS.contains(&seg_str) {
                    return true;
                }
            }
        }
    }

    // Check filename suffixes.
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name.ends_with('~')
            || Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("swp"))
            || name == ".DS_Store"
        {
            return true;
        }
    }

    false
}

/// Convert an absolute path to a relative path string from the forge root.
///
/// Returns `None` if `absolute` is not under `forge_root`.
#[must_use]
pub fn relative_path(forge_root: &Path, absolute: &Path) -> Option<String> {
    absolute
        .strip_prefix(forge_root)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

// ── Watcher ───────────────────────────────────────────────────────────────────

/// A live file watcher that emits [`StorageEvent`]s for changes in the forge.
pub struct Watcher {
    rx: mpsc::Receiver<StorageEvent>,
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
}

impl Watcher {
    /// Start watching `notes/` and `attachments/` under `forge_root`.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Watcher`] if the underlying notify watcher
    /// cannot be created or a directory cannot be watched.
    pub fn start(forge_root: &Path, debounce_ms: u64) -> Result<Self, StorageError> {
        let (storage_tx, storage_rx) = mpsc::sync_channel::<StorageEvent>(WATCHER_CHANNEL_BOUND);
        let (raw_tx, raw_rx) = mpsc::channel::<DebounceEventResult>();

        let duration = Duration::from_millis(debounce_ms);
        let mut debouncer = new_debouncer(duration, raw_tx)?;

        // Watch notes/ and attachments/ if they exist.
        let dirs = ["notes", "attachments"];
        for dir in &dirs {
            let dir_path = forge_root.join(dir);
            if dir_path.exists() {
                debouncer
                    .watcher()
                    .watch(&dir_path, RecursiveMode::Recursive)?;
            }
        }

        let forge_root_owned = forge_root.to_path_buf();

        // Spawn a thread to process debounced events.
        std::thread::spawn(move || {
            process_events(raw_rx, storage_tx, forge_root_owned);
        });

        Ok(Self {
            rx: storage_rx,
            _debouncer: debouncer,
        })
    }

    /// Access the receiver to poll for [`StorageEvent`]s.
    #[must_use]
    pub fn events(&self) -> &mpsc::Receiver<StorageEvent> {
        &self.rx
    }
}

// ── Event processing thread ───────────────────────────────────────────────────

/// Send `evt` on the bounded watcher → consumer channel, handling
/// both overflow (channel full) and disconnect (consumer dropped).
///
/// Recovery semantics: if the channel is full, the per-file event is
/// dropped and `pending_reconcile` is latched true. On the next call
/// where there is room, a single `ReconcileRequested` is flushed
/// before the requested event so the consumer learns to do a full
/// re-walk and recover whatever it missed. The latched flag also
/// dedupes repeated overflow logs into one warn per backlog.
///
/// Returns `false` only on disconnect — the caller should break out
/// of its event loop in that case (consumer is gone for good).
fn enqueue(
    tx: &mpsc::SyncSender<StorageEvent>,
    evt: StorageEvent,
    pending_reconcile: &mut bool,
) -> bool {
    let evt_is_reconcile = matches!(evt, StorageEvent::ReconcileRequested { .. });

    // If we owe a reconcile and the caller is sending something else,
    // try to flush the reconcile first. If even that won't fit we
    // keep the flag set and skip the current event too — the
    // reconcile signal IS the recovery, the per-file event would be
    // a no-op once it lands. `dropped_events: 0` here because the
    // overflow path already logged the count via the warn below and
    // does not thread an exact tally through; consumers learn the
    // important bit (a reconcile is needed) without a misleading
    // total.
    if *pending_reconcile && !evt_is_reconcile {
        match tx.try_send(StorageEvent::ReconcileRequested { dropped_events: 0 }) {
            Ok(()) => *pending_reconcile = false,
            Err(mpsc::TrySendError::Full(_)) => return true,
            Err(mpsc::TrySendError::Disconnected(_)) => return false,
        }
    }

    match tx.try_send(evt) {
        Ok(()) => {
            if evt_is_reconcile {
                *pending_reconcile = false;
            }
            true
        }
        Err(mpsc::TrySendError::Full(_)) => {
            if !*pending_reconcile {
                tracing::warn!(
                    audit = true,
                    bound = WATCHER_CHANNEL_BOUND,
                    "storage watcher: event channel full; consumer is falling behind. \
                     Dropping per-file events and queueing ReconcileRequested for \
                     recovery once the consumer drains."
                );
                *pending_reconcile = true;
            }
            true
        }
        Err(mpsc::TrySendError::Disconnected(_)) => false,
    }
}

#[allow(clippy::needless_pass_by_value)]
fn process_events(
    raw_rx: mpsc::Receiver<DebounceEventResult>,
    storage_tx: mpsc::SyncSender<StorageEvent>,
    forge_root: PathBuf,
) {
    let mut git_batch_mode = false;
    // Tracks how many debounced filesystem notifications were dropped
    // while `.git/index.lock` was held. Reset every time the lock
    // clears (signalling the end of a git operation); surfaced via
    // ReconcileRequested.dropped_events and a tracing::info! line so
    // operators see when batch-mode is masking changes.
    let mut git_batch_dropped: usize = 0;
    let mut git_batch_started_at: Option<std::time::Instant> = None;
    let mut pending_reconcile = false;

    for result in &raw_rx {
        let events = match result {
            Ok(evts) => evts,
            Err(errs) => {
                // Pre-#84 these were swallowed silently — `Err(_errs) => continue`.
                // Promoted to a warn so OS-level notify failures (watch
                // descriptors closing, kernel inotify queue overflow,
                // FSEvents rate-limiting, …) surface in the log.
                tracing::warn!(
                    audit = true,
                    errors = ?errs,
                    "storage watcher: notify-debouncer reported errors; some \
                     filesystem changes may have been missed"
                );
                continue;
            }
        };

        let lock_path = forge_root.join(".git").join("index.lock");
        let lock_exists = lock_path.exists();

        if lock_exists {
            if !git_batch_mode {
                git_batch_started_at = Some(std::time::Instant::now());
            }
            git_batch_mode = true;
            // Count what would otherwise have been forwarded — gives
            // the upcoming ReconcileRequested an exact tally of the
            // batch instead of just "many".
            git_batch_dropped = git_batch_dropped.saturating_add(events.len());
            continue;
        }

        if git_batch_mode {
            // Lock is gone — emit a dedicated reconcile signal so
            // downstream consumers don't have to special-case an
            // empty-string `FileModified` (issue #84). The dropped
            // count + held-duration tracing line gives operators a
            // post-mortem of what the git batch hid; the consumer
            // gets the count via ReconcileRequested.dropped_events.
            git_batch_mode = false;
            let dropped = std::mem::replace(&mut git_batch_dropped, 0);
            let held_ms = git_batch_started_at
                .take()
                .map(|t| u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX))
                .unwrap_or(0);
            tracing::info!(
                audit = true,
                dropped_events = dropped,
                held_ms,
                "storage watcher: .git/index.lock cleared after git batch; \
                 emitting ReconcileRequested to recover state",
            );
            if !enqueue(
                &storage_tx,
                StorageEvent::ReconcileRequested {
                    dropped_events: dropped,
                },
                &mut pending_reconcile,
            ) {
                return;
            }
        }

        for event in events {
            // Only process Any events (AnyContinuous are intermediate states).
            if event.kind != DebouncedEventKind::Any {
                continue;
            }

            let path = &event.path;

            if should_ignore(path) {
                continue;
            }

            // BL-082: skip symlink modify/create events. The watcher
            // sees a symlink as a regular file event, but indexing it
            // would either double-index (if the target is inside the
            // forge) or follow the link out of the sandbox (if the
            // target is outside). Reconcile already excludes symlinks
            // from the walk; this keeps the live event stream
            // consistent with that.
            //
            // The same `symlink_metadata` lookup also gives us the
            // file/dir discriminator we need below — directories have
            // their own watcher events but aren't a piece of file-as-
            // truth content the storage layer should index; we read
            // it once and route accordingly.
            let metadata = std::fs::symlink_metadata(path).ok();
            if let Some(ref meta) = metadata {
                if meta.file_type().is_symlink() {
                    tracing::debug!(
                        path = %path.display(),
                        "BL-082: ignoring file event for symlink",
                    );
                    continue;
                }
                if meta.is_dir() {
                    // The OS / notify-rs emits aggregate events for
                    // directories whose contents changed (e.g. files
                    // inside `notes/Ideas/`). Reading a directory with
                    // `std::fs::read` returns `Err("Is a directory")`,
                    // which the previous fall-through misclassified as
                    // a "file deleted via race". The fix: skip dir
                    // events outright — the per-file events for the
                    // children are already emitted separately by the
                    // recursive watcher.
                    tracing::debug!(
                        path = %path.display(),
                        "ignoring directory event (children fire their own events)",
                    );
                    continue;
                }
            }

            let Some(rel) = relative_path(&forge_root, path) else {
                continue;
            };

            let to_send = if metadata.is_some() {
                // File exists (we just stat'd it above) — read and
                // hash it, emit FileModified.
                match std::fs::read(path) {
                    Ok(bytes) => {
                        let hash = nexus_formats::sha256_hex(&bytes);
                        StorageEvent::FileModified {
                            path: rel,
                            content_hash: hash,
                        }
                    }
                    Err(_) => {
                        // Genuine race: stat succeeded, then the file
                        // disappeared before we could read it. Treat
                        // as a deletion.
                        StorageEvent::FileDeleted { path: rel }
                    }
                }
            } else {
                StorageEvent::FileDeleted { path: rel }
            };
            if !enqueue(&storage_tx, to_send, &mut pending_reconcile) {
                return;
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── should_ignore ──────────────────────────────────────────────────────

    #[test]
    fn should_ignore_git_directory() {
        assert!(should_ignore(Path::new("/forge/.git/config")));
        assert!(should_ignore(Path::new("/forge/notes/.git/COMMIT_EDITMSG")));
    }

    #[test]
    fn should_ignore_forge_temp() {
        assert!(should_ignore(Path::new("/forge/.forge/temp/scratch.md")));
    }

    #[test]
    fn should_ignore_node_modules() {
        assert!(should_ignore(Path::new(
            "/forge/node_modules/some-pkg/index.js"
        )));
    }

    #[test]
    fn should_ignore_swap_files() {
        assert!(should_ignore(Path::new("/forge/notes/my-note.md.swp")));
        assert!(should_ignore(Path::new("/forge/notes/.note.swp")));
    }

    #[test]
    fn should_ignore_backup_files() {
        assert!(should_ignore(Path::new("/forge/notes/my-note.md~")));
        assert!(should_ignore(Path::new("/forge/notes/old~")));
    }

    // ── Bounded channel + reconcile recovery ─────────────────────────────────
    //
    // Direct unit tests for the `enqueue` helper. Reproducing the full
    // notify-debouncer + filesystem-fanout overflow via integration test
    // would be timing-flaky on slow CI; the recovery semantics live entirely
    // in this function, so testing it in isolation gives deterministic
    // coverage of the contract.

    fn mod_evt(path: &str) -> StorageEvent {
        StorageEvent::FileModified {
            path: path.to_string(),
            content_hash: "deadbeef".into(),
        }
    }

    #[test]
    fn enqueue_passes_events_through_when_channel_has_room() {
        let (tx, rx) = mpsc::sync_channel::<StorageEvent>(4);
        let mut pending = false;

        assert!(enqueue(&tx, mod_evt("a"), &mut pending));
        assert!(enqueue(&tx, mod_evt("b"), &mut pending));
        assert!(!pending);

        let got: Vec<_> = (0..2).map(|_| rx.try_recv().unwrap()).collect();
        assert_eq!(got, vec![mod_evt("a"), mod_evt("b")]);
    }

    #[test]
    fn enqueue_sets_pending_reconcile_on_overflow_and_flushes_on_drain() {
        // Capacity 1 makes the overflow path deterministic.
        let (tx, rx) = mpsc::sync_channel::<StorageEvent>(1);
        let mut pending = false;

        // Fills the channel.
        assert!(enqueue(&tx, mod_evt("a"), &mut pending));
        assert!(!pending);

        // Overflow. Event is dropped; flag is latched; returns Ok (true).
        assert!(enqueue(&tx, mod_evt("b"), &mut pending));
        assert!(pending, "overflow must latch pending_reconcile");

        // Drain the channel so the next enqueue has room. The next call
        // should flush ReconcileRequested FIRST, then attempt the new
        // event. Capacity 1 means the new event itself overflows again
        // (and re-sets the flag), but the reconcile signal made it in.
        assert!(matches!(
            rx.try_recv().unwrap(),
            StorageEvent::FileModified { .. }
        ));
        assert!(enqueue(&tx, mod_evt("c"), &mut pending));
        // Reconcile took the only slot, so c overflowed and pending is set again.
        assert!(pending);

        assert!(matches!(
            rx.try_recv().unwrap(),
            StorageEvent::ReconcileRequested { .. }
        ));
        assert!(rx.try_recv().is_err(), "c should have been dropped");
    }

    #[test]
    fn enqueue_explicit_reconcile_clears_pending_flag() {
        let (tx, _rx) = mpsc::sync_channel::<StorageEvent>(2);
        let mut pending = true; // simulate a previously-latched overflow

        assert!(enqueue(
            &tx,
            StorageEvent::ReconcileRequested { dropped_events: 42 },
            &mut pending
        ));
        assert!(
            !pending,
            "an explicit ReconcileRequested send must clear the latched flag"
        );
    }

    #[test]
    fn enqueue_returns_false_on_consumer_disconnect() {
        let (tx, rx) = mpsc::sync_channel::<StorageEvent>(2);
        drop(rx);
        let mut pending = false;
        assert!(
            !enqueue(&tx, mod_evt("a"), &mut pending),
            "disconnect must surface as false so the producer can break"
        );
    }

    #[test]
    fn should_ignore_ds_store() {
        assert!(should_ignore(Path::new("/forge/notes/.DS_Store")));
        assert!(should_ignore(Path::new("/forge/.DS_Store")));
    }

    #[test]
    fn should_not_ignore_markdown() {
        assert!(!should_ignore(Path::new("/forge/notes/my-note.md")));
        assert!(!should_ignore(Path::new(
            "/forge/notes/daily/2026-04-12.md"
        )));
    }

    /// Issue #84. Pre-fix the substring matcher (`path.contains(".git")`)
    /// falsely ignored legitimate notes whose names contained the
    /// reserved directory name as a substring. The component-based
    /// matcher only fires on an exact path-component match.
    #[test]
    fn should_not_ignore_substring_lookalike_filenames() {
        assert!(
            !should_ignore(Path::new("/forge/notes/my-.git-history-notes.md")),
            "substring matching falsely ignored a legitimate note (#84 regression)"
        );
        assert!(!should_ignore(Path::new("/forge/notes/.forgetnot.md")));
        assert!(!should_ignore(Path::new(
            "/forge/notes/node_modules-rant.md"
        )));
        assert!(!should_ignore(Path::new("/forge/notes/target-list.md")));
    }

    #[test]
    fn should_not_ignore_attachments() {
        assert!(!should_ignore(Path::new("/forge/attachments/image.png")));
        assert!(!should_ignore(Path::new("/forge/attachments/doc.pdf")));
    }

    // ── relative_path ─────────────────────────────────────────────────────

    #[test]
    fn relative_path_strips_root() {
        let root = Path::new("/forge");
        let abs = Path::new("/forge/notes/my-note.md");
        assert_eq!(
            relative_path(root, abs),
            Some("notes/my-note.md".to_string())
        );
    }

    #[test]
    fn relative_path_returns_none_for_outside() {
        let root = Path::new("/forge");
        let abs = Path::new("/other/notes/my-note.md");
        assert_eq!(relative_path(root, abs), None);
    }

    // ── StorageEvent ───────────────────────────────────────────────────────

    // a..g pair up the variants (a==b, c differs, d==e, f==g) — the
    // single-char names are appropriate for a pure equality table.
    #[allow(clippy::many_single_char_names)]
    #[test]
    fn storage_event_variants_are_eq() {
        let a = StorageEvent::FileCreated {
            path: "notes/a.md".to_string(),
            content_hash: "abc123".to_string(),
        };
        let b = StorageEvent::FileCreated {
            path: "notes/a.md".to_string(),
            content_hash: "abc123".to_string(),
        };
        assert_eq!(a, b);

        let c = StorageEvent::FileDeleted {
            path: "notes/a.md".to_string(),
        };
        assert_ne!(a, c);

        let d = StorageEvent::FileModified {
            path: "notes/b.md".to_string(),
            content_hash: "def456".to_string(),
        };
        let e = StorageEvent::FileModified {
            path: "notes/b.md".to_string(),
            content_hash: "def456".to_string(),
        };
        assert_eq!(d, e);

        let f = StorageEvent::FileRenamed {
            from: "notes/old.md".to_string(),
            to: "notes/new.md".to_string(),
            content_hash: "ghi789".to_string(),
        };
        let g = StorageEvent::FileRenamed {
            from: "notes/old.md".to_string(),
            to: "notes/new.md".to_string(),
            content_hash: "ghi789".to_string(),
        };
        assert_eq!(f, g);
    }

    // ── Integration: watcher detects file creation ─────────────────────────

    #[test]
    fn watcher_detects_file_creation() {
        use std::time::Duration;
        use tempfile::TempDir;

        let dir = TempDir::new().expect("temp dir");
        let forge_root = dir.path();
        let notes_dir = forge_root.join("notes");
        std::fs::create_dir_all(&notes_dir).expect("create notes dir");

        let watcher = Watcher::start(forge_root, 50).expect("start watcher");

        // Give the watcher a moment to initialise before writing.
        std::thread::sleep(Duration::from_millis(100));

        // Write a new file.
        let note_path = notes_dir.join("hello.md");
        std::fs::write(&note_path, b"# Hello\n").expect("write note");

        // Wait up to 5 seconds for the event.
        let event = watcher
            .events()
            .recv_timeout(Duration::from_secs(5))
            .expect("expected a StorageEvent within 5 seconds");

        match event {
            StorageEvent::FileModified { path, content_hash } => {
                assert!(
                    path.contains("hello.md"),
                    "expected path to contain hello.md, got: {path}"
                );
                assert!(!content_hash.is_empty(), "content_hash should not be empty");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    /// Regression: writing files inside a directory used to emit a
    /// spurious `FileDeleted` for the *parent directory* — the OS
    /// debouncer aggregates child changes into a directory-level
    /// event, `path.exists()` returned true (the directory was still
    /// there), `std::fs::read(path)` returned `Err("Is a directory")`,
    /// and the previous fall-through misclassified the read failure
    /// as a deletion race. Every child write fired a phantom
    /// `notes/<dir>` deletion, fast enough to flood the activity
    /// timeline and drown out real events.
    ///
    /// Post-fix the watcher should never emit a `FileDeleted` whose
    /// `path` corresponds to a directory that still exists on disk.
    #[test]
    fn watcher_does_not_emit_phantom_deletions_for_directories() {
        use std::time::{Duration, Instant};
        use tempfile::TempDir;

        let dir = TempDir::new().expect("temp dir");
        let forge_root = dir.path();
        let notes_dir = forge_root.join("notes");
        let ideas_dir = notes_dir.join("Ideas");
        std::fs::create_dir_all(&ideas_dir).expect("create Ideas dir");

        let watcher = Watcher::start(forge_root, 50).expect("start watcher");
        std::thread::sleep(Duration::from_millis(100));

        // Write a fresh file inside the watched directory and update
        // it a few times to provoke directory-aggregate events.
        for i in 0..5 {
            let note_path = ideas_dir.join(format!("note-{i}.md"));
            std::fs::write(&note_path, format!("# Note {i}\n")).expect("write note");
            std::thread::sleep(Duration::from_millis(60));
        }

        // Drain the watcher for a generous window. Any event whose
        // path is `notes/Ideas` or `notes` and is `FileDeleted` is
        // the regression.
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match watcher.events().recv_timeout(remaining) {
                Ok(StorageEvent::FileDeleted { path }) => {
                    assert!(
                        !path.ends_with("Ideas") && path != "notes",
                        "phantom directory-level deletion: {path}",
                    );
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }
}
