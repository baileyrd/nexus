//! Plugin lifecycle implementation.
//!
//! The kernel drives this plugin through a state machine:
//!   Discovered → Loaded → Initialized → Started → Stopped → Unloaded
//!
//! Each on_* method corresponds to a state transition. Timeouts are enforced
//! by the kernel (see PRD 01 §4.1 for timeout values).

use nexus_core::{
    PluginContext, PluginLifecycle, PluginError,
};
use crate::events::EventHandler;
use crate::state::PluginState;

pub struct MyPlugin {
    state: Option<PluginState>,
    event_handler: Option<EventHandler>,
}

impl MyPlugin {
    pub fn new() -> Self {
        MyPlugin {
            state: None,
            event_handler: None,
        }
    }
}

#[async_trait::async_trait]
impl PluginLifecycle for MyPlugin {
    async fn on_load(&mut self) -> Result<(), PluginError> {
        // Called when the binary is loaded into memory.
        // Do minimal work here — no I/O, no network, no event subscriptions.
        // Use this for in-memory struct initialization only.
        Ok(())
    }

    async fn on_init(&mut self, ctx: &dyn PluginContext) -> Result<(), PluginError> {
        // Called after dependencies are initialized and capabilities are granted.
        // This is where you:
        //   1. Restore state from the KV store (important for hot-reload).
        //   2. Set up event subscriptions.
        //   3. Register IPC commands.

        // Restore persisted state (if any) for hot-reload continuity.
        // See PRD 01 §7.2 for the state preservation contract.
        self.state = Some(PluginState::restore(ctx).await?);

        // Subscribe to events — uses opaque EventSubscription (PRD 01 §3.4).
        // The underlying bus transport may change; this code won't need to.
        self.event_handler = Some(EventHandler::new(ctx));

        Ok(())
    }

    async fn on_start(&mut self) -> Result<(), PluginError> {
        // Called when the plugin transitions to Started.
        // Begin active work: start background tasks, open connections, etc.
        //
        // If you need to spawn long-running work, use tokio::spawn() and
        // monitor ctx.cancellation_token() in the spawned task.
        Ok(())
    }

    async fn on_stop(&mut self) -> Result<(), PluginError> {
        // Called on graceful shutdown. The kernel enforces a 5-second timeout.
        //
        // Persist state here — this is your last chance before unload.
        // The KV store remains available during on_stop().
        if let Some(state) = &self.state {
            state.persist().await?;
        }
        Ok(())
    }

    async fn on_shutdown(&mut self) -> Result<(), PluginError> {
        // Final cleanup: close file handles, release locks, drop resources.
        // Called after on_stop(). The kernel enforces a 2-second timeout.
        //
        // Do NOT do I/O here — the KV store may already be closed.
        // This is for in-memory cleanup only.
        self.event_handler = None;
        self.state = None;
        Ok(())
    }
}
