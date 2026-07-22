//! BL-135 — source-tag → channel-list router.
//!
//! Producers tag each [`crate::Notification`] with a `source` name
//! (e.g. `"workflow"`, `"agent"`, `"ai_runtime"`) and a [`Severity`]
//! instead of hardcoding a [`Channel`]. The router consults the
//! loaded [`crate::NotificationsConfig`] to translate the tag into
//! the concrete channels to dispatch on.
//!
//! Explicit-channel callers (the v1 `send(channel: "discord", …)`
//! callsite) bypass the router entirely — the override path lands
//! straight on the transport map. See
//! [`crate::core_plugin::NotificationsCorePlugin::dispatch_send`].

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use crate::config::{NotificationsConfig, ResolvedSource, Severity};
use crate::Channel;

/// Thread-safe view of the loaded routing rules.
///
/// Wraps `Arc<RwLock<…>>` over the resolved-source map so the file
/// watcher can swap in a fresh config without taking the dispatch
/// path offline.
#[derive(Clone, Default)]
pub struct Router {
    inner: Arc<RwLock<RouterState>>,
}

#[derive(Default)]
struct RouterState {
    sources: BTreeMap<String, ResolvedSource>,
}

/// Outcome of a routing decision. Returned by [`Router::resolve`]
/// so callers can distinguish "no matching source" (silent drop —
/// caller may want to fall back to a default channel) from
/// "matched but filtered out by severity / quiet_hours" (intentional
/// drop — caller should *not* fall back).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Source name is not in the loaded config.
    UnknownSource,
    /// Source matched but the event was filtered (severity below
    /// `min_severity`, or current time inside `quiet_hours`).
    Filtered,
    /// Source matched; dispatch to these channels.
    Routed(Vec<Channel>),
}

impl Router {
    /// Build an empty router. Used by tests and as the boot-time
    /// default before the bootstrap loads a real config.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Build a router from an already-loaded config. Equivalent to
    /// `Router::empty()` followed by `swap_config(config)` but skips
    /// the lock dance.
    ///
    /// # Errors
    /// Forwards [`crate::config::ConfigError::QuietHours`] from the
    /// underlying `resolve_sources` call.
    pub fn from_config(config: &NotificationsConfig) -> Result<Self, crate::config::ConfigError> {
        let sources = config.resolve_sources()?;
        Ok(Self {
            inner: Arc::new(RwLock::new(RouterState { sources })),
        })
    }

    /// Replace the routing rules with a freshly loaded config.
    /// Concurrent in-flight dispatches see the old rules until they
    /// release their read guard; subsequent dispatches see the new
    /// rules. Used by the file-watcher live-reload path.
    ///
    /// # Errors
    /// Forwards [`crate::config::ConfigError::QuietHours`] from the
    /// underlying `resolve_sources` call.
    pub fn swap_config(
        &self,
        config: &NotificationsConfig,
    ) -> Result<(), crate::config::ConfigError> {
        let sources = config.resolve_sources()?;
        // #199 / R16 — recover from a poisoned lock instead of
        // `.expect()`-panicking. With `panic = "abort"` in the release
        // profile, that would convert a prior writer-side panic into a
        // whole-process abort. `state.sources = sources` is a single
        // field replacement (no partial-write state to observe), so
        // reading/writing the inner value on poison is safe.
        let mut state = match self.inner.write() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::error!("notifications router state lock poisoned — recovering (see #199)");
                poisoned.into_inner()
            }
        };
        state.sources = sources;
        Ok(())
    }

    /// Look up the routing for a source-tagged notification. Returns
    /// [`Resolution::UnknownSource`] when the tag isn't configured,
    /// [`Resolution::Filtered`] when severity / quiet_hours drops the
    /// event, and [`Resolution::Routed`] otherwise.
    #[must_use]
    pub fn resolve(&self, source: &str, severity: Severity, min_of_day: u16) -> Resolution {
        // #199 / R16 — recover from a poisoned lock rather than
        // propagating the panic (see `swap_config` above).
        let state = match self.inner.read() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::error!("notifications router state lock poisoned — recovering (see #199)");
                poisoned.into_inner()
            }
        };
        let Some(rule) = state.sources.get(source) else {
            return Resolution::UnknownSource;
        };
        if severity < rule.min_severity {
            return Resolution::Filtered;
        }
        if let Some(qh) = rule.quiet_hours {
            if qh.contains(min_of_day) {
                return Resolution::Filtered;
            }
        }
        if rule.channels.is_empty() {
            return Resolution::Filtered;
        }
        Resolution::Routed(rule.channels.clone())
    }

    /// Snapshot the registered source names. Used by the
    /// observability/MCP surface and unit tests.
    #[must_use]
    pub fn source_names(&self) -> Vec<String> {
        // #199 / R16 — recover from a poisoned lock rather than
        // propagating the panic (see `swap_config` above).
        let state = match self.inner.read() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::error!("notifications router state lock poisoned — recovering (see #199)");
                poisoned.into_inner()
            }
        };
        state.sources.keys().cloned().collect()
    }
}

/// Current local time as a 0..=1439 minute-of-day. Pulled into a
/// helper so unit tests can pin time-of-day without depending on the
/// system clock. The router itself takes `min_of_day` as a parameter.
#[must_use]
pub fn current_min_of_day() -> u16 {
    use chrono::Timelike;
    let now = chrono::Local::now().time();
    u16::try_from(now.hour() * 60 + now.minute()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(text: &str) -> NotificationsConfig {
        NotificationsConfig::parse(text).unwrap()
    }

    #[test]
    fn unknown_source_resolves_to_unknown() {
        let r = Router::from_config(&cfg("")).unwrap();
        assert_eq!(
            r.resolve("nope", Severity::Info, 600),
            Resolution::UnknownSource
        );
    }

    #[test]
    fn routes_to_configured_channels() {
        let r = Router::from_config(&cfg(r#"
[sources.workflow]
route = ["desktop", "discord"]
"#))
        .unwrap();
        assert_eq!(
            r.resolve("workflow", Severity::Info, 600),
            Resolution::Routed(vec![Channel::Desktop, Channel::Discord]),
        );
    }

    #[test]
    fn filters_by_min_severity() {
        let r = Router::from_config(&cfg(r#"
[sources.workflow]
route = ["desktop"]
min_severity = "warn"
"#))
        .unwrap();
        assert_eq!(
            r.resolve("workflow", Severity::Info, 600),
            Resolution::Filtered
        );
        assert_eq!(
            r.resolve("workflow", Severity::Warn, 600),
            Resolution::Routed(vec![Channel::Desktop]),
        );
        assert_eq!(
            r.resolve("workflow", Severity::Error, 600),
            Resolution::Routed(vec![Channel::Desktop]),
        );
    }

    #[test]
    fn filters_by_quiet_hours() {
        let r = Router::from_config(&cfg(r#"
[sources.workflow]
route = ["desktop"]
quiet_hours = "22:00-08:00"
"#))
        .unwrap();
        // 23:00 — inside the quiet window.
        assert_eq!(
            r.resolve("workflow", Severity::Info, 23 * 60),
            Resolution::Filtered,
        );
        // 12:00 — outside.
        assert_eq!(
            r.resolve("workflow", Severity::Info, 12 * 60),
            Resolution::Routed(vec![Channel::Desktop]),
        );
    }

    #[test]
    fn empty_route_filters() {
        let r = Router::from_config(&cfg(r#"
[sources.workflow]
route = []
"#))
        .unwrap();
        assert_eq!(
            r.resolve("workflow", Severity::Info, 600),
            Resolution::Filtered
        );
    }

    #[test]
    fn swap_config_swaps_rules() {
        let r = Router::from_config(&cfg(r#"
[sources.workflow]
route = ["desktop"]
"#))
        .unwrap();
        assert!(matches!(
            r.resolve("workflow", Severity::Info, 600),
            Resolution::Routed(_)
        ));
        r.swap_config(&cfg(r#"
[sources.agent]
route = ["discord"]
"#))
            .unwrap();
        assert_eq!(
            r.resolve("workflow", Severity::Info, 600),
            Resolution::UnknownSource
        );
        assert_eq!(
            r.resolve("agent", Severity::Info, 600),
            Resolution::Routed(vec![Channel::Discord]),
        );
    }

    #[test]
    fn source_names_reflect_loaded_config() {
        let r = Router::from_config(&cfg(r#"
[sources.alpha]
route = ["desktop"]

[sources.beta]
route = ["discord"]
"#))
        .unwrap();
        let mut names = r.source_names();
        names.sort();
        assert_eq!(names, vec!["alpha".to_string(), "beta".to_string()]);
    }
}
