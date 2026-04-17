//! Errors surfaced by the terminal subsystem.

/// Failures from PTY spawning, I/O, or child-process control.
#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    /// PTY allocation failed (`openpty`). Usually signals resource
    /// exhaustion (open-file-descriptor limit) or a kernel / OS refusal.
    #[error("allocate pty: {0}")]
    PtyAlloc(String),

    /// The configured shell executable could not be spawned (not on PATH,
    /// permission denied, unsupported on this platform, …).
    #[error("spawn shell '{shell}': {reason}")]
    Spawn {
        /// The shell command that failed to spawn.
        shell: String,
        /// Human-readable failure reason.
        reason: String,
    },

    /// Raw I/O error reading from or writing to the PTY master.
    #[error("pty i/o: {0}")]
    Io(#[from] std::io::Error),

    /// Session was asked to do something after it had already been killed
    /// or its child had exited.
    #[error("session '{0}' is not running")]
    NotRunning(String),

    /// Session detection or validation of the user's shell failed. Includes
    /// the fallback that will be used.
    #[error("shell detection failed: {reason}")]
    ShellDetection {
        /// Why detection failed.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_running_display_includes_session_id() {
        let err = TerminalError::NotRunning("my-session".into());
        assert!(err.to_string().contains("my-session"));
    }

    #[test]
    fn spawn_display_includes_shell_and_reason() {
        let err = TerminalError::Spawn {
            shell: "/bin/nope".into(),
            reason: "no such file".into(),
        };
        let s = err.to_string();
        assert!(s.contains("/bin/nope"));
        assert!(s.contains("no such file"));
    }
}
