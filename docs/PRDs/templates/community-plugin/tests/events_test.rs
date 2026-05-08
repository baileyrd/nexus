//! Event handling tests.
//!
//! These tests verify that the plugin correctly processes events
//! it subscribes to and ignores events it doesn't care about.

use nexus_core::test_utils::{MockPluginContext, MockEventBus};
use nexus_core::{NexusEvent, FileEvent, EventMetadata};

use {{crate_name}}::events::EventHandler;

#[tokio::test]
async fn test_event_handler_processes_file_created() {
    let bus = MockEventBus::new();
    let ctx = MockPluginContext::with_bus(&bus);

    let mut handler = EventHandler::new(&ctx);

    // Publish a FileCreated event.
    let event = NexusEvent::FileCreated(FileEvent {
        metadata: EventMetadata::test_default(),
        path: "/test/file.rs".into(),
        size_bytes: Some(1024),
        mime_type: Some("text/x-rust".to_string()),
    });
    bus.publish(event).await.unwrap();

    // TODO: Add plugin-specific assertions.
    // Example: verify a side effect occurred, a counter incremented, etc.
}

#[tokio::test]
async fn test_event_handler_ignores_unrelated_events() {
    let bus = MockEventBus::new();
    let ctx = MockPluginContext::with_bus(&bus);

    let _handler = EventHandler::new(&ctx);

    // Publish events this plugin doesn't handle.
    bus.publish(NexusEvent::KernelStarted).await.unwrap();
    bus.publish(NexusEvent::TerminalOpened(Default::default())).await.unwrap();

    // Should not panic or produce side effects.
}

#[tokio::test]
async fn test_event_loop_exits_on_cancellation() {
    let bus = MockEventBus::new();
    let ctx = MockPluginContext::with_bus(&bus);

    let mut handler = EventHandler::new(&ctx);

    // Trigger cancellation immediately.
    ctx.cancel();

    // The event loop should exit without blocking.
    handler.run(&ctx).await;
}
