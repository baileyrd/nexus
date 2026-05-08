//! Event bus: tokio broadcast channel wrapper.

use std::sync::Arc;

use tokio::sync::broadcast;

use crate::error::{RecvError, Result};
use crate::event::{EventFilter, EventMetadata, NexusEvent, PublishedEvent};

/// The kernel's event bus. Fans out `PublishedEvent`s to all subscribers
/// via a bounded tokio broadcast channel.
///
/// Owned by the `Kernel` struct; subscribers receive handles via
/// `EventBus::subscribe`. Publishers must go through the kernel
/// (`publish_kernel` is `pub(crate)` so plugins can't reach it directly).
#[derive(Debug)]
pub struct EventBus {
    sender: broadcast::Sender<Arc<PublishedEvent>>,
}

/// Return true iff `type_id` lies within `plugin_id`'s namespace —
/// that is, `type_id == plugin_id` (the bare-id form) or `type_id`
/// equals `plugin_id` followed by a `.` and a non-empty suffix.
///
/// Plain `starts_with` is unsafe here: `"com.foo".starts_with("com.fo")`
/// is true, so a plugin id `com.fo` could spoof `com.foo.event`. The
/// `.`-separator check is what actually anchors the namespace boundary.
/// See issue #79.
/// Test whether `type_id` lies within the kernel namespace owned by
/// `plugin_id`. Exposed `pub` (was `pub(crate)`) so the BL-103 fuzz
/// crate can drive it directly without simulating a full event-bus
/// publish; the function itself is pure (no allocations, no I/O) and
/// the contract — equal to plugin id, or `<plugin_id>.<suffix>`,
/// nothing else — is the namespace anti-spoofing guarantee for
/// `EventFilter::CustomPrefix` subscribers.
#[doc(hidden)]
pub fn type_id_in_namespace(type_id: &str, plugin_id: &str) -> bool {
    if type_id == plugin_id {
        return true;
    }
    type_id
        .strip_prefix(plugin_id)
        .is_some_and(|rest| rest.starts_with('.'))
}

/// Topics that any plugin may publish to even though they don't lie in
/// the caller's namespace. These are kernel-owned shared channels for
/// cross-plugin fan-out (the activity timeline is the canonical case —
/// terminal, git, storage, ai, workflow all append to it). The kernel
/// still populates `emitting_plugin` and `metadata.source_plugin_id`
/// from the caller, so subscribers can attribute each entry to its
/// real source; spoofing the topic name doesn't spoof attribution.
const KERNEL_OWNED_SHARED_TOPICS: &[&str] =
    &[nexus_types::activity::ACTIVITY_APPENDED_TOPIC];

/// Return true iff `type_id` is one of the [`KERNEL_OWNED_SHARED_TOPICS`].
#[doc(hidden)]
pub fn is_kernel_owned_shared_topic(type_id: &str) -> bool {
    KERNEL_OWNED_SHARED_TOPICS.contains(&type_id)
}

impl EventBus {
    /// Create a new bus with the given ring buffer capacity.
    ///
    /// # Panics
    /// Panics if `capacity == 0`. `KernelConfig::load` rejects 0 from
    /// disk, but `KernelConfig::for_testing` and direct struct
    /// construction can pass 0; failing fast here surfaces the bad
    /// value at the construction site instead of inside
    /// `tokio::broadcast::channel`'s panic. See issue #81.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        assert!(
            capacity > 0,
            "EventBus capacity must be > 0; tokio::broadcast::channel(0) panics"
        );
        let (sender, _receiver) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publish an event emitted by a plugin. Enforces two invariants:
    ///
    /// - Only `NexusEvent::Custom` is accepted — plugins cannot emit kernel events.
    /// - The `type_id` must lie within `source_plugin_id`'s namespace
    ///   (either equal to it, or extending it with a `.`-separated suffix —
    ///   see [`type_id_in_namespace`]). This is the kernel's anti-spoofing
    ///   guarantee for `EventFilter::CustomPrefix` subscribers.
    ///
    /// The kernel populates `emitting_plugin` from `source_plugin_id`; the
    /// plugin cannot override it.
    ///
    /// # Errors
    /// - `BusError::PluginPublishingKernelEvent` if `event` is not `Custom`.
    /// - `BusError::TypeIdNamespaceMismatch` if `type_id` is not in
    ///   `source_plugin_id`'s namespace.
    pub fn publish_plugin(
        &self,
        source_plugin_id: &str,
        type_id: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        use crate::error::BusError;

        if !type_id_in_namespace(type_id, source_plugin_id)
            && !is_kernel_owned_shared_topic(type_id)
        {
            return Err(BusError::TypeIdNamespaceMismatch {
                plugin_id: source_plugin_id.to_string(),
                type_id: type_id.to_string(),
            }
            .into());
        }

        let event = NexusEvent::Custom {
            type_id: type_id.to_string(),
            emitting_plugin: source_plugin_id.to_string(),
            payload,
        };
        let metadata = EventMetadata {
            event_id: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            source_plugin_id: source_plugin_id.to_string(),
            span_id: current_span_id(),
        };
        let published = Arc::new(PublishedEvent { metadata, event });
        let _ = self.sender.send(published);
        if let Some(m) = crate::metrics::global() {
            m.record_event_publish(source_plugin_id);
            m.record_event_bus_queue_depth(self.sender.len() as u64);
        }
        Ok(())
    }

    /// Publish a kernel-tier event on behalf of a core plugin.
    ///
    /// Core plugins (native Rust, `trust_level = "core"`) may publish any
    /// first-class [`NexusEvent`] variant, including forge file events.
    /// The metadata `source_plugin_id` is set to `plugin_id`.
    ///
    /// To emit plugin-namespaced custom events use [`publish_plugin`] instead.
    ///
    /// # Errors
    /// Currently infallible. The underlying `broadcast::Sender::send` returns
    /// `Err` only when there are zero active subscribers — a normal,
    /// non-error condition for a fan-out bus — which is silently ignored.
    /// The `Result` return is preserved so a future bus implementation
    /// (with explicit shutdown semantics) can surface real failures
    /// without an API break.
    pub fn publish_core(&self, plugin_id: &str, event: NexusEvent) -> Result<()> {
        let metadata = EventMetadata {
            event_id: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            source_plugin_id: plugin_id.to_string(),
            span_id: current_span_id(),
        };
        let published = Arc::new(PublishedEvent { metadata, event });
        let _ = self.sender.send(published);
        if let Some(m) = crate::metrics::global() {
            m.record_event_publish(plugin_id);
            m.record_event_bus_queue_depth(self.sender.len() as u64);
        }
        Ok(())
    }

    /// Publish a kernel-owned event. Not callable from plugins.
    ///
    /// # Errors
    /// Currently infallible. See [`publish_core`](Self::publish_core)
    /// for the rationale on the preserved `Result` return type.
    #[allow(dead_code, clippy::unnecessary_wraps)] // wired up by nexus-plugins (PRD 04)
    pub(crate) fn publish_kernel(&self, event: NexusEvent) -> Result<()> {
        let metadata = EventMetadata {
            event_id: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            source_plugin_id: "kernel".to_string(),
            span_id: current_span_id(),
        };
        let published = Arc::new(PublishedEvent { metadata, event });
        // broadcast::Sender::send returns the number of active receivers,
        // or an error if there are none — that's not an error condition for us.
        let _ = self.sender.send(published);
        if let Some(m) = crate::metrics::global() {
            m.record_event_bus_queue_depth(self.sender.len() as u64);
        }
        Ok(())
    }

    /// Subscribe to events matching the filter. The subscription is dropped
    /// automatically when it goes out of scope.
    #[must_use]
    pub fn subscribe(&self, filter: EventFilter) -> EventSubscription {
        EventSubscription {
            receiver: self.sender.subscribe(),
            filter,
        }
    }

    /// Number of active subscribers (useful for debug/metrics).
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

/// A subscription handle returned by `EventBus::subscribe`. Dropped
/// subscriptions auto-unsubscribe (tokio broadcast semantics).
pub struct EventSubscription {
    receiver: broadcast::Receiver<Arc<PublishedEvent>>,
    filter: EventFilter,
}

impl EventSubscription {
    /// Receive the next event matching the filter. Non-matching events are
    /// skipped internally.
    ///
    /// # Errors
    /// - `RecvError::Lagged(n)` — subscriber fell behind; `n` events lost.
    /// - `RecvError::Closed` — bus is shut down.
    pub async fn recv(&mut self) -> std::result::Result<Arc<PublishedEvent>, RecvError> {
        loop {
            let event = match self.receiver.recv().await {
                Ok(e) => e,
                Err(broadcast::error::RecvError::Lagged(n)) => return Err(RecvError::Lagged(n)),
                Err(broadcast::error::RecvError::Closed) => return Err(RecvError::Closed),
            };
            if matches_filter(&event.event, &self.filter) {
                return Ok(event);
            }
            // non-matching: keep looping
        }
    }

    /// Try to receive without blocking. Returns `Ok(None)` if no matching
    /// events are currently available.
    ///
    /// # Errors
    /// - `RecvError::Lagged(n)` — subscriber fell behind.
    /// - `RecvError::Closed` — bus is shut down.
    pub fn try_recv(&mut self) -> std::result::Result<Option<Arc<PublishedEvent>>, RecvError> {
        loop {
            let event = match self.receiver.try_recv() {
                Ok(e) => e,
                Err(broadcast::error::TryRecvError::Empty) => return Ok(None),
                Err(broadcast::error::TryRecvError::Lagged(n)) => return Err(RecvError::Lagged(n)),
                Err(broadcast::error::TryRecvError::Closed) => return Err(RecvError::Closed),
            };
            if matches_filter(&event.event, &self.filter) {
                return Ok(Some(event));
            }
        }
    }
}

/// Check whether an event matches a filter.
fn matches_filter(event: &NexusEvent, filter: &EventFilter) -> bool {
    match filter {
        EventFilter::All => true,
        EventFilter::Variant(name) => variant_name(event) == name.as_str(),
        EventFilter::CustomPrefix(prefix) => {
            if let NexusEvent::Custom { type_id, .. } = event {
                type_id.starts_with(prefix.as_str())
            } else {
                false
            }
        }
        EventFilter::CustomExact(wanted) => {
            if let NexusEvent::Custom { type_id, .. } = event {
                type_id == wanted
            } else {
                false
            }
        }
    }
}

/// Get the variant name of a `NexusEvent` for filter matching.
#[allow(clippy::match_same_arms)]
fn variant_name(event: &NexusEvent) -> &'static str {
    match event {
        NexusEvent::PluginLoaded { .. }     => "PluginLoaded",
        NexusEvent::PluginStarted { .. }    => "PluginStarted",
        NexusEvent::PluginStopped { .. }    => "PluginStopped",
        NexusEvent::PluginCrashed { .. }    => "PluginCrashed",
        NexusEvent::CapabilityGranted { .. } => "CapabilityGranted",
        NexusEvent::CapabilityDenied { .. }  => "CapabilityDenied",
        NexusEvent::Custom { .. }            => "Custom",
    }
}

/// Get the current `tracing` span id as a stringified numeric id,
/// if any.
///
/// Pre-#81 this returned the `Debug` repr (e.g. `"Id(1)"`), which
/// leaked through to subscribers serializing event metadata —
/// `Id(1)` is not a useful identifier downstream of the bus.
/// `Id::into_u64()` gives the underlying numeric id, which is what
/// callers actually want to correlate spans across logs/events.
fn current_span_id() -> Option<String> {
    // tracing::Span::current() always returns a span, but it's the None span
    // when no actual span is active. We use its metadata or None.
    let span = tracing::Span::current();
    if span.is_disabled() {
        None
    } else {
        span.id().map(|id| id.into_u64().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: a kernel-owned event for test publishing.
    fn plugin_loaded_event(id: &str) -> NexusEvent {
        NexusEvent::PluginLoaded {
            plugin_id: id.to_string(),
            version: "1.0.0".to_string(),
        }
    }

    #[tokio::test]
    async fn publish_and_receive_single_event() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe(EventFilter::All);

        bus.publish_kernel(plugin_loaded_event("com.test")).unwrap();

        let published = sub.recv().await.unwrap();
        match &published.event {
            NexusEvent::PluginLoaded { plugin_id, .. } => assert_eq!(plugin_id, "com.test"),
            _ => panic!("wrong event variant"),
        }
        assert_eq!(published.metadata.source_plugin_id, "kernel");
    }

    #[tokio::test]
    async fn filter_variant_skips_non_matching_events() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe(EventFilter::Variant("PluginStarted".to_string()));

        // Publish a PluginLoaded event — should be skipped by the filter
        bus.publish_kernel(plugin_loaded_event("com.a")).unwrap();

        // Publish a PluginStarted event — should be received
        bus.publish_kernel(NexusEvent::PluginStarted {
            plugin_id: "com.b".to_string(),
        }).unwrap();

        let published = sub.recv().await.unwrap();
        match &published.event {
            NexusEvent::PluginStarted { plugin_id } => assert_eq!(plugin_id, "com.b"),
            _ => panic!("filter let wrong event through"),
        }
    }

    #[tokio::test]
    async fn filter_custom_prefix_matches_custom_events() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe(EventFilter::CustomPrefix("com.test.".to_string()));

        bus.publish_kernel(NexusEvent::Custom {
            type_id: "com.test.ping".to_string(),
            emitting_plugin: "com.test".to_string(),
            payload: serde_json::json!({}),
        }).unwrap();

        let published = sub.recv().await.unwrap();
        match &published.event {
            NexusEvent::Custom { type_id, .. } => assert_eq!(type_id, "com.test.ping"),
            _ => panic!("wrong variant"),
        }
    }

    #[tokio::test]
    async fn try_recv_returns_none_when_empty() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe(EventFilter::All);
        let result = sub.try_recv().unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn subscriber_count_reflects_active_subscriptions() {
        let bus = EventBus::new(16);
        assert_eq!(bus.subscriber_count(), 0);

        let _sub1 = bus.subscribe(EventFilter::All);
        assert_eq!(bus.subscriber_count(), 1);

        {
            let _sub2 = bus.subscribe(EventFilter::All);
            assert_eq!(bus.subscriber_count(), 2);
        }
        // sub2 dropped
        assert_eq!(bus.subscriber_count(), 1);
    }

    #[tokio::test]
    async fn metadata_has_fresh_uuid_per_publish() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe(EventFilter::All);

        bus.publish_kernel(plugin_loaded_event("com.a")).unwrap();
        bus.publish_kernel(plugin_loaded_event("com.b")).unwrap();

        let e1 = sub.recv().await.unwrap();
        let e2 = sub.recv().await.unwrap();
        assert_ne!(e1.metadata.event_id, e2.metadata.event_id);
    }

    #[tokio::test]
    async fn slow_subscriber_gets_lagged_error() {
        // Capacity of 2; publish 5 events without consuming; should lag.
        let bus = EventBus::new(2);
        let mut sub = bus.subscribe(EventFilter::All);

        for i in 0..5 {
            bus.publish_kernel(plugin_loaded_event(&format!("com.test.{i}"))).unwrap();
        }

        // First recv should return Lagged, not an actual event.
        let result = sub.recv().await;
        match result {
            Err(RecvError::Lagged(n)) => assert!(n >= 1, "expected at least 1 lagged, got {n}"),
            other => panic!("expected Lagged error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn recv_returns_closed_when_bus_dropped() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe(EventFilter::All);

        drop(bus);

        let result = sub.recv().await;
        assert!(matches!(result, Err(RecvError::Closed)));
    }

    #[tokio::test]
    async fn publish_plugin_emits_custom_event() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe(EventFilter::All);

        bus.publish_plugin(
            "com.example.plugin",
            "com.example.plugin.ping",
            serde_json::json!({"key": "value"}),
        )
        .unwrap();

        let published = sub.recv().await.unwrap();
        assert_eq!(published.metadata.source_plugin_id, "com.example.plugin");
        match &published.event {
            NexusEvent::Custom { type_id, emitting_plugin, .. } => {
                assert_eq!(type_id, "com.example.plugin.ping");
                assert_eq!(emitting_plugin, "com.example.plugin");
            }
            _ => panic!("expected Custom event"),
        }
    }

    #[test]
    fn publish_plugin_rejects_namespace_mismatch() {
        let bus = EventBus::new(16);
        let result = bus.publish_plugin(
            "com.legit.plugin",
            "com.evil.spoofed",
            serde_json::json!({}),
        );
        assert!(matches!(
            result,
            Err(crate::error::Error::Bus(
                crate::error::BusError::TypeIdNamespaceMismatch { .. }
            ))
        ));
    }

    /// Regression for issue #79. The pre-fix check was `type_id.starts_with(plugin_id)`,
    /// which let `com.foo` publish `com.foobar.event` because the substring
    /// `"com.foo"` is a prefix of `"com.foobar.event"`. A subscriber
    /// filtering on `EventFilter::CustomPrefix("com.foobar.")` would
    /// receive the spoofed event. The fix anchors the namespace boundary
    /// on a `.` separator (or strict equality).
    #[test]
    fn publish_plugin_rejects_substring_prefix_spoof() {
        let bus = EventBus::new(16);
        let result = bus.publish_plugin(
            "com.foo",
            "com.foobar.event",
            serde_json::json!({}),
        );
        assert!(
            matches!(
                result,
                Err(crate::error::Error::Bus(
                    crate::error::BusError::TypeIdNamespaceMismatch { .. }
                ))
            ),
            "com.foo must NOT be allowed to publish com.foobar.event",
        );
    }

    #[test]
    fn publish_plugin_allows_dotted_suffix() {
        let bus = EventBus::new(16);
        bus.publish_plugin(
            "com.foo",
            "com.foo.event",
            serde_json::json!({}),
        )
        .expect("dotted suffix is the legitimate namespace shape");
    }

    #[test]
    fn publish_plugin_allows_bare_plugin_id_as_type_id() {
        let bus = EventBus::new(16);
        bus.publish_plugin(
            "com.foo",
            "com.foo",
            serde_json::json!({}),
        )
        .expect("bare plugin_id as type_id is unambiguously the plugin's");
    }

    #[tokio::test]
    async fn publish_plugin_allows_kernel_owned_shared_topic() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe(EventFilter::All);
        bus.publish_plugin(
            "com.nexus.terminal",
            nexus_types::activity::ACTIVITY_APPENDED_TOPIC,
            serde_json::json!({"surface": "process"}),
        )
        .expect("kernel-owned shared topic is publishable from any plugin");

        let published = sub.recv().await.unwrap();
        assert_eq!(published.metadata.source_plugin_id, "com.nexus.terminal");
        match &published.event {
            NexusEvent::Custom {
                type_id,
                emitting_plugin,
                ..
            } => {
                assert_eq!(
                    type_id,
                    nexus_types::activity::ACTIVITY_APPENDED_TOPIC
                );
                // Attribution still tracks the real caller — the shared
                // topic is not an anonymity escape hatch.
                assert_eq!(emitting_plugin, "com.nexus.terminal");
            }
            _ => panic!("expected Custom event"),
        }
    }

    #[test]
    fn type_id_in_namespace_unit_cases() {
        // Adversarial substring-prefix shapes (issue #79).
        assert!(!type_id_in_namespace("com.foobar.event", "com.foo"));
        assert!(!type_id_in_namespace("com.foo2.event", "com.foo"));
        assert!(!type_id_in_namespace("com.foo-bar.event", "com.foo"));

        // Reverse: longer plugin_id must not appear within shorter type_id.
        assert!(!type_id_in_namespace("com.foo", "com.foobar"));

        // Disjoint ids are rejected.
        assert!(!type_id_in_namespace("com.evil.spoofed", "com.legit.plugin"));

        // Legitimate suffix shapes pass.
        assert!(type_id_in_namespace("com.foo.event", "com.foo"));
        assert!(type_id_in_namespace("com.foo.deeply.nested.event", "com.foo"));
        assert!(type_id_in_namespace("com.foo", "com.foo"));
    }

    /// Issue #81. Pre-fix the `tokio::broadcast::channel(0)` panic
    /// surfaced as a generic "channel capacity must be > 0" message
    /// from inside tokio, with no hint about where the bad config
    /// came from. Now `EventBus::new(0)` panics at the construction
    /// site with a message naming the bus and the constraint.
    #[test]
    #[should_panic(expected = "EventBus capacity must be > 0")]
    fn new_panics_on_zero_capacity() {
        let _ = EventBus::new(0);
    }

    #[tokio::test]
    async fn lagged_subscriber_can_recover_and_keep_receiving() {
        let bus = EventBus::new(2);
        let mut sub = bus.subscribe(EventFilter::All);

        // Cause a lag
        for i in 0..5 {
            bus.publish_kernel(plugin_loaded_event(&format!("com.test.{i}"))).unwrap();
        }

        // First recv — lagged
        assert!(matches!(sub.recv().await, Err(RecvError::Lagged(_))));

        // Subsequent recvs should return actual events from what's still in the buffer
        let event = sub.recv().await.unwrap();
        assert!(matches!(event.event, NexusEvent::PluginLoaded { .. }));
    }
}
