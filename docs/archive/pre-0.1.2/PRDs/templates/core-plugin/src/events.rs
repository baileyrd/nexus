//! Event subscription and handling.
//!
//! This module uses the opaque EventSubscription type exclusively.
//! It never imports broadcast::Receiver or any channel-specific type.
//! This ensures the plugin is compatible with future bus transport
//! changes (broadcast → type-based pub/sub). See PRD 01 §3.4, §3.6.

use nexus_core::{
    PluginContext, NexusEvent, EventSubscription,
};
use std::sync::Arc;

/// Handles event subscription and dispatch for this plugin.
pub struct EventHandler {
    subscription: EventSubscription,
}

impl EventHandler {
    /// Create a new event handler with a subscription from the plugin context.
    pub fn new(ctx: &dyn PluginContext) -> Self {
        EventHandler {
            subscription: ctx.subscribe_event(),
        }
    }

    /// Main event loop. Call this from a spawned task.
    ///
    /// Monitors both the event stream and the cancellation token.
    /// When the kernel triggers cancellation (during stop or shutdown),
    /// this loop exits cleanly.
    pub async fn run(&mut self, ctx: &dyn PluginContext) {
        loop {
            tokio::select! {
                _ = ctx.cancellation_token().cancelled() => {
                    // Kernel requested shutdown — exit cleanly.
                    tracing::info!("Cancellation received, stopping event loop.");
                    break;
                }
                Some(event) = self.subscription.recv() => {
                    self.handle_event(ctx, &event).await;
                }
            }
        }
    }

    /// Dispatch a single event.
    ///
    /// Match only the variants this plugin cares about.
    /// All other variants fall through to the wildcard arm.
    async fn handle_event(&self, ctx: &dyn PluginContext, event: &NexusEvent) {
        match event {
            // -------------------------------------------------------
            // Add your event handlers below. Examples:
            // -------------------------------------------------------

            NexusEvent::FileCreated(file_event) => {
                tracing::debug!("File created: {:?}", file_event.path);
                // TODO: Handle file creation.
            }

            NexusEvent::FileModified(file_event) => {
                tracing::debug!("File modified: {:?}", file_event.path);
                // TODO: Handle file modification.
            }

            // Ignore all other events — this is expected and correct.
            // As the system grows, new event variants will be added to
            // NexusEvent. Plugins should only match what they need.
            _ => {}
        }
    }
}
