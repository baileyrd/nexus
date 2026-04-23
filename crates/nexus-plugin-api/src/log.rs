//! Plugin log level — independent of `tracing::Level` to avoid leaking the
//! tracing crate into the stable plugin API surface.

/// Log severity for plugin-emitted messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, export_to = "../../../packages/nexus-extension-api/src/generated/"))]
pub enum LogLevel {
    /// Fine-grained tracing information.
    Trace,
    /// Debugging information.
    Debug,
    /// General informational messages.
    Info,
    /// Warnings that do not prevent operation.
    Warn,
    /// Error conditions.
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_levels_are_distinct() {
        assert_ne!(LogLevel::Trace, LogLevel::Debug);
        assert_ne!(LogLevel::Debug, LogLevel::Info);
        assert_ne!(LogLevel::Info, LogLevel::Warn);
        assert_ne!(LogLevel::Warn, LogLevel::Error);
    }

    #[test]
    fn log_level_is_copy() {
        let a = LogLevel::Info;
        let b = a;
        assert_eq!(a, b);
    }
}
