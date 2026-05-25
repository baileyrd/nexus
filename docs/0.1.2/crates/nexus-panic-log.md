# nexus-panic-log

> Kind: lib · IPC plugin id: — · CorePlugin: no · Has settings: no · As of: 2026-05-25

## Overview

`nexus-panic-log` is a tiny, dependency-light leaf crate whose sole job is to install a process-wide panic hook that persists Rust panics to disk. On every panic it appends a structured, human-readable entry to `~/.nexus-shell/logs/panic.log` and then chains to the previously-installed hook so the normal stderr panic output is still emitted. There is no network, no telemetry, and no opt-in UI — the design assumption stated in the crate docs is that Nexus is a personal tool and a panic should survive a closed terminal so it can be inspected after the fact.

The crate is not a `CorePlugin` and participates in none of the kernel's plugin lifecycle, IPC, capability, or event machinery. It is a free-standing utility that frontend binaries call from the very first line of `main()`, before argument parsing or tracing setup, so that a crash during early initialization is still captured. Today two binaries install it: `nexus-cli` (`nexus_panic_log::install("nexus")`) and the Tauri shell (`nexus_panic_log::install("nexus-shell")`).

It is a leaf crate with no `nexus-*` dependencies, depending only on `std`, `chrono`, and `dirs`. That isolation is intentional: a panic-logging facility must be installable before anything else and must never pull in heavyweight or fallible subsystems. The workspace marks it `publish = false` (inherited from `[workspace.package]`), so it is an internal-only crate not intended for crates.io. It is listed as one of the workspace's four genuine leaf crates (alongside `nexus-types`, `nexus-plugin-api`, and `nexus-fuzz`) in `docs/0.1.2/crates.md`.

## Position in the dependency graph

- **Direct nexus-* dependencies:** None. This is a true leaf crate — it imports nothing from the rest of the workspace.
- **Notable external dependencies (+ why):**
  - `chrono` (workspace pin) — used only for `chrono::Utc::now().to_rfc3339()` to timestamp each entry.
  - `dirs` (`5`) — `dirs::home_dir()` resolves the user home directory; the crate docs note this matches the convention used in `shell/src-tauri/src/lib.rs`.
  - `tempfile` (dev-dependency, workspace pin) — used only by the test module for isolated temp directories.
- **Crates that depend on this one / who installs the hook:**
  - `crates/nexus-cli/Cargo.toml` → `crates/nexus-cli/src/main.rs:1779` calls `nexus_panic_log::install("nexus")` as the first statement of `main()`.
  - `shell/src-tauri/Cargo.toml` → `shell/src-tauri/src/main.rs:13` calls `nexus_panic_log::install("nexus-shell")` first in `main()`.
  - Note: the comment in `shell/src-tauri/src/main.rs` references the pattern used by `nexus-cli` and `nexus-tui`, but `nexus-tui` does **not** actually depend on or install this crate (confirmed: no references in `crates/nexus-tui/`). Only the CLI and the shell wire it up today.

## Public API surface

Everything lives in a single module, `crates/nexus-panic-log/src/lib.rs`. There is exactly one public item; all other functions are private helpers.

- **`pub fn install(binary_name: &'static str)`** — the entire public API. Installs the panic hook for the given binary label. Internally guarded by a `std::sync::Once` (`INSTALL_ONCE`), so it is idempotent: calling it multiple times is safe and only the first call takes effect per process. It takes the current hook via `std::panic::take_hook()`, sets a new boxed hook that (1) does a best-effort `write_entry(...)` and (2) always calls the captured default hook so stderr still shows the panic. The `binary_name` is captured by the closure and recorded in every log entry. Intended to be called as the first statement of `main()`.

Private helpers (not exported):

- `fn log_path() -> Option<PathBuf>` — resolves `~/.nexus-shell/logs/panic.log` via `dirs::home_dir()`, returning `None` if the home directory cannot be determined.
- `fn rotate_if_needed(path: &PathBuf)` — performs size-based rotation (see Internals). All errors swallowed.
- `fn write_entry(binary_name: &str, panic_info: &std::panic::PanicHookInfo<'_>) -> std::io::Result<()>` — the actual log-write routine. Returns `Err` on any I/O failure, but the hook ignores the result.
- `fn panic_message(panic_info: &std::panic::PanicHookInfo<'_>) -> String` — extracts the panic payload, handling `&str` and `String`, falling back to `"<non-string panic payload>"` for other payload types.

## IPC handlers

None — and none are expected. This crate is not a plugin: it has no `CorePlugin` impl, registers no IPC commands, and is never wired into `nexus-bootstrap`. It exposes a single ordinary Rust function (`install`) called directly from binary `main()` entry points. It does not participate in the `context.ipc_call(plugin_id, command, args)` path at all.

## Capabilities

None. The crate predates and sits outside the kernel capability system — it is not kernel-mediated. It performs direct filesystem I/O from within the panic hook (creating `~/.nexus-shell/logs/`, rotating, and appending) without any `fs.read` / `fs.write` capability check, because it runs before and independently of the kernel. This is by design: the panic hook must function even if the kernel never started or has already torn down.

## Settings / Config

No configurable settings — there is no `Config` struct, no TOML file, and no environment variable. All behavior is governed by hardcoded constants and a fixed path:

| Value | Definition | Source |
|-------|-----------|--------|
| Log path | `~/.nexus-shell/logs/panic.log` | `log_path()`, built from `dirs::home_dir()` |
| Rotated path | `~/.nexus-shell/logs/panic.log.1` | `path.with_extension("log.1")` |
| Rotation threshold | `MAX_LOG_BYTES = 1024 * 1024` (1 MiB) | `const MAX_LOG_BYTES: u64` |
| File ceiling | Two files (`panic.log` + `panic.log.1`) | rotation overwrites prior `.1` |

Note the slight discrepancy between the Cargo.toml `description` ("1 MB rotation") and the doc comment / constant ("1 MiB", `1024 * 1024`). The constant is authoritative: rotation triggers at 1 MiB (1,048,576 bytes), not 1 MB (1,000,000). The crate is not represented in `docs/0.1.2/settings/` because it has nothing to configure.

## Events

None. The crate does not publish or subscribe to any kernel events — it has no kernel dependency. The only side effect is the disk write.

## Internals & notable implementation details

**Panic hook mechanism.** `install` uses `std::sync::Once` to ensure the hook is installed exactly once per process. Inside `call_once`, it captures the existing hook via `std::panic::take_hook()` (which on a fresh process is the default stderr-printing hook), then installs a `Box`ed closure via `std::panic::set_hook`. On every panic the closure first does `let _ = write_entry(binary_name, panic_info)` (best-effort, error discarded) and then invokes `default_hook(panic_info)` so the standard panic message and any `RUST_BACKTRACE` behavior are preserved. Chaining rather than replacing is the key behavior: stderr output is never suppressed.

**Entry format.** Each entry is a small YAML-ish block delimited by a `---` line, with fields written one per line: `timestamp` (RFC 3339 UTC), `binary` (the `binary_name` passed to `install`), `location` (`file:line` from `panic_info.location()`, or `<unknown>`), `message` (the extracted payload string), and `backtrace:` followed by the output of `std::backtrace::Backtrace::force_capture()`. `force_capture()` always captures a backtrace regardless of the `RUST_BACKTRACE` environment variable.

**Log rotation logic.** Before each write, `write_entry` calls `rotate_if_needed`. That helper stat()s the file; if `fs::metadata` fails (e.g. the file does not exist yet) it returns early, and if the length is `<= MAX_LOG_BYTES` it returns without action. Once the file exceeds 1 MiB it renames `panic.log` → `panic.log.1`. On Windows it first `fs::remove_file`s the destination (because `rename` does not overwrite on Windows), guarded by `#[cfg(windows)]`; on Unix `fs::rename` overwrites the destination atomically. The result is a strict two-file ceiling — any older `.1` is overwritten and lost. All rotation errors are swallowed; the caller proceeds to write regardless.

**File path resolution.** `log_path()` returns `Option<PathBuf>`; `write_entry` converts a `None` (no home directory) into an `io::Error(NotFound, "no home directory")`. Before opening the file, `write_entry` creates the parent directory tree via `fs::create_dir_all(parent)?`. The file is opened with `OpenOptions::new().create(true).append(true)`.

**Failure handling / panic-in-hook safety.** Every fallible step inside the hook path is best-effort: `write_entry`'s `Result` is discarded by the closure, and `rotate_if_needed` swallows all its own errors. The module-level doc comment explicitly states the rationale — a panic-in-hook loop is worse than a missed log line, so the hook never propagates an error.

**Thread-safety.** Hook installation is serialized by `Once`. The hook closure itself is `Send + Sync` (it only captures `binary_name: &'static str` and the boxed default hook). Concurrent panics from multiple threads each open the file in append mode independently; on POSIX, `O_APPEND` writes are atomic for the small line-oriented writes used here, so interleaving of whole entries is avoided in practice, though the crate does not take an explicit lock. There is a benign TOCTOU window around rotation under concurrent panics (two threads could each decide to rotate), but the worst case is a redundant rename, not data corruption.

## Tests

The test module lives inline in `src/lib.rs` under `#[cfg(test)] mod tests` (there is no separate `tests/` directory). Because the real `write_entry` hardcodes `~/.nexus-shell/logs/panic.log`, the tests use a private test-only clone, `write_entry_to(path, binary_name, panic_info)`, that takes an arbitrary path so writes are redirected to a `tempfile::tempdir()`. This duplicates the production write logic against a caller-supplied path.

- **`writes_entry_on_panic`** — installs a custom hook that routes into `write_entry_to` against a temp path, triggers a panic inside `std::panic::catch_unwind`, then restores the previous hook. Asserts the panic occurred, the log file was created, and its contents contain `binary:    nexus-test`, the panic message `smoke-test panic`, and a `timestamp:` field. This exercises the entry-formatting path but, by design, not the real `~/.nexus-shell` path (to avoid touching the developer's home directory).
- **`rotation_renames_oversized_file`** — writes a file of `MAX_LOG_BYTES + 1` bytes, calls `rotate_if_needed` directly, and asserts the original `panic.log` no longer exists and `panic.log.1` does. This directly covers the rotation threshold and rename behavior.

Gaps worth noting: there is no test for the `<= MAX_LOG_BYTES` no-op branch, the missing-home-directory error path, the `<non-string panic payload>` fallback, hook idempotency via `Once`, or the Windows remove-before-rename branch. The smoke test validates the formatting code path through a test-only copy of `write_entry` rather than the production function, so the two could drift independently.
