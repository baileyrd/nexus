//! Local panic logging for Nexus binaries.
//!
//! Installs a [`std::panic::set_hook`] that appends a structured entry to
//! `~/.nexus-shell/logs/panic.log` on every panic, then chains to the
//! previously-installed hook so stderr output is preserved. No network, no
//! opt-in UI — Nexus is a personal tool and panics should survive a closed
//! terminal.
//!
//! Rotation policy: if the log file exceeds 1 MiB before a write, it is
//! renamed to `panic.log.1` (overwriting any prior `.1`). Two-file ceiling.
//!
//! Failures inside the hook are swallowed intentionally — a panic-in-hook
//! loop is worse than a missed log line.

use std::backtrace::Backtrace;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Once;

/// 1 MiB — rotation threshold.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

static INSTALL_ONCE: Once = Once::new();

/// Install the panic hook for `binary_name`.
///
/// Safe to call multiple times; only the first call takes effect per process.
/// Call this as the first statement of `main()`, before any code that might
/// panic (argument parsing, tracing setup, etc.).
pub fn install(binary_name: &'static str) {
    INSTALL_ONCE.call_once(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            // Best-effort write; never propagate an error.
            let _ = write_entry(binary_name, panic_info);
            // Always chain to the default hook so stderr still shows the panic.
            default_hook(panic_info);
        }));
    });
}

/// Resolve `~/.nexus-shell/logs/panic.log`. Matches the `dirs::home_dir`
/// convention used elsewhere in the Nexus codebase (see
/// `shell/src-tauri/src/lib.rs`).
fn log_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".nexus-shell").join("logs").join("panic.log"))
}

/// Rotate `panic.log` -> `panic.log.1` if the current file exceeds
/// [`MAX_LOG_BYTES`]. Errors are swallowed (caller continues regardless).
fn rotate_if_needed(path: &PathBuf) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.len() <= MAX_LOG_BYTES {
        return;
    }
    let rotated = path.with_extension("log.1");
    // `rename` overwrites the destination on Unix; on Windows we remove first.
    #[cfg(windows)]
    {
        let _ = fs::remove_file(&rotated);
    }
    let _ = fs::rename(path, &rotated);
}

/// Append one entry to the panic log. Returns `Err` on any I/O failure, but
/// the caller (the panic hook) ignores the result.
fn write_entry(
    binary_name: &str,
    panic_info: &std::panic::PanicHookInfo<'_>,
) -> std::io::Result<()> {
    let path = log_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no home directory"))?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    rotate_if_needed(&path);

    let timestamp = chrono::Utc::now().to_rfc3339();
    let location = panic_info
        .location()
        .map(|l| format!("{}:{}", l.file(), l.line()))
        .unwrap_or_else(|| "<unknown>".to_string());
    let message = panic_message(panic_info);
    let backtrace = Backtrace::force_capture();

    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;

    writeln!(file, "---")?;
    writeln!(file, "timestamp: {}", timestamp)?;
    writeln!(file, "binary:    {}", binary_name)?;
    writeln!(file, "location:  {}", location)?;
    writeln!(file, "message:   {}", message)?;
    writeln!(file, "backtrace:")?;
    writeln!(file, "{}", backtrace)?;
    Ok(())
}

/// Extract the panic payload as a string (handles `&str` and `String`).
fn panic_message(panic_info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = panic_info.payload();
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: write_entry produces a non-empty log file with the
    /// expected header fields. We invoke the hook machinery directly via
    /// `catch_unwind` + a custom hook that calls `write_entry_to` on a
    /// tempdir path.
    #[test]
    fn writes_entry_on_panic() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log_path = tmp.path().join("panic.log");

        // Install a hook that routes into a local write_entry override.
        let log_path_clone = log_path.clone();
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = write_entry_to(&log_path_clone, "nexus-test", info);
        }));

        let result = std::panic::catch_unwind(|| {
            panic!("smoke-test panic");
        });

        // Restore prior hook before asserting so test output stays clean.
        let _ = std::panic::take_hook();
        std::panic::set_hook(prev);

        assert!(result.is_err(), "panic should have occurred");
        let contents = fs::read_to_string(&log_path).expect("log file written");
        assert!(
            contents.contains("binary:    nexus-test"),
            "contents: {contents}"
        );
        assert!(
            contents.contains("smoke-test panic"),
            "contents: {contents}"
        );
        assert!(contents.contains("timestamp:"), "contents: {contents}");
    }

    /// Test-only variant of `write_entry` that writes to a caller-supplied
    /// path instead of the real `~/.nexus-shell/logs/panic.log`.
    fn write_entry_to(
        path: &PathBuf,
        binary_name: &str,
        panic_info: &std::panic::PanicHookInfo<'_>,
    ) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        rotate_if_needed(path);
        let timestamp = chrono::Utc::now().to_rfc3339();
        let location = panic_info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let message = panic_message(panic_info);
        let backtrace = Backtrace::force_capture();

        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "---")?;
        writeln!(file, "timestamp: {}", timestamp)?;
        writeln!(file, "binary:    {}", binary_name)?;
        writeln!(file, "location:  {}", location)?;
        writeln!(file, "message:   {}", message)?;
        writeln!(file, "backtrace:")?;
        writeln!(file, "{}", backtrace)?;
        Ok(())
    }

    #[test]
    fn rotation_renames_oversized_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let log = tmp.path().join("panic.log");
        // Create a >1MiB file.
        fs::write(&log, vec![b'x'; (MAX_LOG_BYTES + 1) as usize]).unwrap();
        rotate_if_needed(&log);
        assert!(!log.exists(), "original log should have been rotated away");
        assert!(log.with_extension("log.1").exists(), ".log.1 should exist");
    }
}
