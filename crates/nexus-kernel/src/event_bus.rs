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

impl EventBus {
    /// Create a new bus with the given ring buffer capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _receiver) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Publish an event emitted by a plugin. Enforces two invariants:
    ///
    /// - Only `NexusEvent::Custom` is accepted — plugins cannot emit kernel events.
    /// - The `type_id` must start with `source_plugin_id` (namespace anti-spoofing).
    ///
    /// The kernel populates `emitting_plugin` from `source_plugin_id`; the
    /// plugin cannot override it.
    ///
    /// # Errors
    /// - `BusError::PluginPublishingKernelEvent` if `event` is not `Custom`.
    /// - `BusError::TypeIdNamespaceMismatch` if `type_id` doesn't start with `source_plugin_id`.
    pub fn publish_plugin(
        &self,
        source_plugin_id: &str,
        type_id: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        use crate::error::BusError;

        if !type_id.starts_with(source_plugin_id) {
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
    /// Returns `BusError::Closed` if the bus has been shut down.
    pub fn publish_core(&self, plugin_id: &str, event: NexusEvent) -> Result<()> {
        let metadata = EventMetadata {
            event_id: uuid::Uuid::new_v4(),
            timestamp: chrono::Utc::now(),
            source_plugin_id: plugin_id.to_string(),
            span_id: current_span_id(),
        };
        let published = Arc::new(PublishedEvent { metadata, event });
        let _ = self.sender.send(published);
        Ok(())
    }

    /// Publish a kernel-owned event. Not callable from plugins.
    ///
    /// # Errors
    /// Returns `BusError::Closed` if the bus has been shut down.
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

/// Get the current `tracing` span id, if any.
#[allow(dead_code)] // used by publish_kernel (see above)
fn current_span_id() -> Option<String> {
    // tracing::Span::current() always returns a span, but it's the None span
    // when no actual span is active. We use its metadata or None.
    let span = tracing::Span::current();
    if span.is_disabled() {
        None
    } else {
        span.id().map(|id| format!("{id:?}"))
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
