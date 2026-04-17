//! Thread-confined wrapper around [`GitEngine`] for UI-driven async callers.
//!
//! # Why this exists
//!
//! `git2::Repository` — and therefore [`GitEngine`] — is deliberately neither
//! `Send` nor `Sync` (libgit2's internal state is not thread-safe). A Tauri
//! `#[tauri::command]` must be `async` and its captured state must be `Send`,
//! so a bare `GitEngine` cannot live inside a Tauri `State<...>`. Any UI that
//! naively called into the engine would block the frontend event loop because
//! the engine's synchronous `status` / `log` / `diff` calls can take tens of
//! milliseconds on large repositories.
//!
//! [`GitWorker`] resolves both problems at once by owning the engine on a
//! dedicated OS thread. Callers receive a cheap, `Send + Sync + Clone`
//! [`GitWorkerHandle`] that submits work via a bounded channel and blocks on
//! a reply. From an async caller, wrap each `handle.with(...)` call in
//! `tokio::task::spawn_blocking` (or the runtime equivalent) so the async
//! executor isn't blocked.
//!
//! # Microkernel fit
//!
//! Git is an invoker-local capability — the kernel does not expose git as an
//! IPC surface because no plugin or core subsystem currently calls git over
//! IPC (per [`crate`] module docs). `GitWorker` stays on the invoker side
//! (CLI, Tauri shell) and does **not** become a core plugin. Adding one later
//! would be a straight wrap of this worker's `handle.with(...)` API inside a
//! `CorePlugin::dispatch` implementation; nothing in this module has to
//! change.
//!
//! # Shape
//!
//! ```text
//! ┌─ caller thread ─────────┐        ┌─ worker thread ──────────┐
//! │ GitWorkerHandle::with  ─┼──Task──▶ loop { task(&mut engine) }│
//! │   blocks on reply_rx   ◀┼──reply─┤                          │
//! └────────────────────────┘        └──────────────────────────┘
//! ```
//!
//! The worker is strictly single-consumer: there is one engine, and tasks
//! run in submission order. If that becomes a bottleneck the fix is either
//! (a) spawn multiple workers for read-heavy ops against distinct repo
//! copies, or (b) split into reader / writer workers with an `RwLock`
//! facade — neither is needed today.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};

use crate::{GitEngine, GitError};

/// Channel capacity for pending git tasks. Small on purpose — callers should
/// see backpressure if they fire faster than git can service them rather than
/// queuing unbounded work that will eventually OOM the process.
const WORKER_QUEUE_DEPTH: usize = 32;

/// Boxed closure carried on the work channel. Each task owns its own reply
/// channel (allocated in [`GitWorkerHandle::with`]) so different calls can
/// return different types over the same transport.
type Task = Box<dyn FnOnce(&mut GitEngine) + Send + 'static>;

/// Messages the worker thread receives. `Shutdown` is a sentinel the
/// [`GitWorker`] owner sends from `Drop` to break the recv loop
/// deterministically — relying on "all senders dropped → recv returns Err"
/// doesn't work when outstanding [`GitWorkerHandle`] clones still hold a
/// `SyncSender<Msg>` and would otherwise keep the worker alive forever.
enum Msg {
    Task(Task),
    Shutdown,
}

/// A `git2::Repository` confined to a single OS thread, with a bounded
/// channel for submitting work from any caller thread.
///
/// Drop semantics: closing the last [`GitWorkerHandle`] or dropping the
/// owning [`GitWorker`] will close the channel, exit the worker loop, and
/// (for [`GitWorker`]) join the thread. A `Drop` impl on [`GitWorker`]
/// guarantees the thread is joined before the process exits so a crash
/// report includes the worker's stack.
#[derive(Debug)]
pub struct GitWorker {
    // The worker owns its own sender half so new handles can be minted on
    // demand without additional synchronisation. Drop sends `Shutdown`
    // through this sender and then joins the thread; handle clones made
    // earlier stay alive until their callers drop them, but any attempt
    // to send through them after Drop will fail once the worker's rx
    // falls out of scope.
    tx: SyncSender<Msg>,
    // `Option` so `Drop` can take and join the thread.
    join: Option<JoinHandle<()>>,
}

/// Cheap, cloneable handle to a running [`GitWorker`]. Hold this in a Tauri
/// `State`, a UI store, or any other `Send + Sync` context. Every call
/// blocks the caller thread until the worker replies — wrap in
/// `spawn_blocking` from async contexts.
#[derive(Clone, Debug)]
pub struct GitWorkerHandle {
    tx: SyncSender<Msg>,
}

impl GitWorker {
    /// Spawn a worker that opens the git repository at `repo_path`.
    ///
    /// The repo is opened on the worker thread (libgit2 resources never
    /// cross threads). This function blocks the caller until the open
    /// succeeds or fails, so `GitError::NotARepo` and similar open-time
    /// errors surface here instead of on the first [`GitWorkerHandle::with`]
    /// call.
    ///
    /// # Errors
    /// Returns [`GitError::NotARepo`] if `repo_path` is not inside a git
    /// working tree, [`GitError::Git`] for libgit2 open failures, or
    /// [`GitError::WorkerGone`] if the OS refuses to spawn the thread.
    pub fn spawn(repo_path: impl AsRef<Path>) -> Result<Self, GitError> {
        let repo_path: PathBuf = repo_path.as_ref().to_path_buf();
        let (tx, rx) = mpsc::sync_channel::<Msg>(WORKER_QUEUE_DEPTH);
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), GitError>>();

        let join = thread::Builder::new()
            .name("nexus-git-worker".to_string())
            .spawn(move || {
                let mut engine = match GitEngine::open(&repo_path) {
                    Ok(e) => {
                        // Signal readiness before draining the queue; if the
                        // spawner has already dropped (e.g. cancelled) the
                        // channel send will fail harmlessly.
                        if ready_tx.send(Ok(())).is_err() {
                            return;
                        }
                        e
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                // Blocking loop: every task ran to completion before we
                // read the next. A panicking closure will unwind the
                // worker; the `JoinHandle` surfaces that on shutdown and
                // any pending callers receive `GitError::WorkerGone` from
                // their reply channel going silent.
                while let Ok(msg) = rx.recv() {
                    match msg {
                        Msg::Task(task) => task(&mut engine),
                        Msg::Shutdown => break,
                    }
                }
            })
            .map_err(|e| GitError::WorkerGone(format!("spawn thread: {e}")))?;

        // Propagate open-time errors. If the worker thread died before
        // sending readiness (should not happen but we must not hang), treat
        // it as a worker-gone error and surface the join panic.
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                tx,
                join: Some(join),
            }),
            Ok(Err(e)) => {
                // Worker reported an open error; it has already returned.
                let _ = join.join();
                Err(e)
            }
            Err(_) => {
                let _ = join.join();
                Err(GitError::WorkerGone(
                    "worker exited before signalling readiness".to_string(),
                ))
            }
        }
    }

    /// Return a cheap, cloneable handle that submits work to this worker.
    ///
    /// The handle stays usable until the worker is dropped or its thread
    /// panics; after either, calls return [`GitError::WorkerGone`].
    #[must_use]
    pub fn handle(&self) -> GitWorkerHandle {
        GitWorkerHandle {
            tx: self.tx.clone(),
        }
    }
}

impl Drop for GitWorker {
    fn drop(&mut self) {
        // Send the explicit `Shutdown` sentinel so the worker exits its
        // recv loop even if outstanding `GitWorkerHandle` clones still
        // hold cloned senders. Without the sentinel, those clones would
        // keep the channel alive and `join` would deadlock here.
        //
        // The send is best-effort: if the channel is already full the
        // shutdown would block, but shutdown arriving after a full queue
        // of tasks is still fine — the worker processes every task ahead
        // of it and then exits on the sentinel. If the worker has already
        // panicked the send fails silently and we fall through to `join`,
        // which surfaces the panic to the log.
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl GitWorkerHandle {
    /// Run `f` on the worker thread and block the caller until it returns.
    ///
    /// Use for synchronous callers (CLI). From async callers, wrap in
    /// `tokio::task::spawn_blocking(move || handle.with(...))` so the
    /// async executor thread isn't blocked.
    ///
    /// # Errors
    /// Propagates whatever `f` returns. Additionally returns
    /// [`GitError::WorkerGone`] if the worker thread has exited (channel
    /// send failed) or if it panicked mid-task (reply channel closed
    /// before sending).
    pub fn with<F, T>(&self, f: F) -> Result<T, GitError>
    where
        F: FnOnce(&mut GitEngine) -> Result<T, GitError> + Send + 'static,
        T: Send + 'static,
    {
        let (reply_tx, reply_rx) = mpsc::channel::<Result<T, GitError>>();
        let task: Task = Box::new(move |engine| {
            // Ignore send errors: if the caller dropped the receiver we
            // have no one to deliver the result to, which is fine.
            let _ = reply_tx.send(f(engine));
        });
        self.tx
            .send(Msg::Task(task))
            .map_err(|_| GitError::WorkerGone("send task".to_string()))?;
        reply_rx
            .recv()
            .map_err(|_| GitError::WorkerGone("task panicked before reply".to_string()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    /// Initialise an empty git repo at `path` using the system `git` binary
    /// so the test doesn't depend on libgit2 init internals.
    fn init_repo(path: &Path) {
        let ok = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(path)
            .status()
            .expect("git init");
        assert!(ok.success(), "git init failed in {}", path.display());
    }

    #[test]
    fn spawn_opens_repo_and_handle_roundtrips() {
        let dir = tempdir().unwrap();
        init_repo(dir.path());

        let worker = GitWorker::spawn(dir.path()).expect("spawn");
        let handle = worker.handle();

        // `repo_root` is a cheap read that does not require commits; it
        // also proves the engine-on-worker round trip end-to-end.
        let root = handle
            .with(|engine| Ok(engine.repo_root().to_path_buf()))
            .expect("with");
        assert!(root.ends_with(dir.path().file_name().unwrap()) || root == dir.path());
    }

    #[test]
    fn spawn_returns_error_for_non_repo() {
        let dir = tempdir().unwrap();
        let err = GitWorker::spawn(dir.path()).unwrap_err();
        assert!(
            matches!(err, GitError::NotARepo(_)),
            "expected NotARepo, got {err:?}"
        );
    }

    #[test]
    fn handle_is_cheaply_cloneable_and_concurrent() {
        let dir = tempdir().unwrap();
        init_repo(dir.path());

        let worker = GitWorker::spawn(dir.path()).expect("spawn");
        let handle = worker.handle();

        // Fan out a handful of concurrent calls from different caller
        // threads; each should get a correct answer and none should
        // deadlock. The worker processes them serially, which is the
        // whole point — we're just verifying the handle is Send + Sync
        // in practice, not just by type signature.
        let joins: Vec<_> = (0..8)
            .map(|_| {
                let h = handle.clone();
                thread::spawn(move || h.with(|e| Ok(e.repo_root().to_path_buf())).unwrap())
            })
            .collect();
        for j in joins {
            let root = j.join().unwrap();
            assert!(root.exists());
        }
    }

    #[test]
    fn dropping_worker_closes_channel_and_joins_thread() {
        let dir = tempdir().unwrap();
        init_repo(dir.path());

        let worker = GitWorker::spawn(dir.path()).expect("spawn");
        let handle = worker.handle();

        // Drop the owner first; the handle should observe WorkerGone on
        // its next send because the worker thread has exited.
        drop(worker);
        let err = handle
            .with(|e| Ok(e.repo_root().to_path_buf()))
            .unwrap_err();
        assert!(
            matches!(err, GitError::WorkerGone(_)),
            "expected WorkerGone, got {err:?}"
        );
    }
}
