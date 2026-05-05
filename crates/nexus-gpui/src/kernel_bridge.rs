//! Tokio ↔ gpui async bridge.
//!
//! gpui has its own executor; Nexus IPC calls are async and require a tokio
//! runtime. This module owns both and exposes a `call` helper that gpui
//! background tasks can use without knowing about the runtime boundary.
//!
//! # Bridge contract
//!
//! `KernelBridge` is `Send + Sync` and cheap to clone via `Arc`. A gpui
//! background task:
//!
//! ```ignore
//! cx.background_executor().spawn({
//!     let bridge = Arc::clone(&bridge);
//!     async move { bridge.call("com.nexus.storage", "graph_stats", json!({})) }
//! }).detach();
//! ```
//!
//! `call` uses [`tokio::runtime::Runtime::block_on`] which is safe to invoke
//! from any thread — it does not require the caller to already be inside a
//! tokio context.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::Value;
use tokio::runtime::Runtime as TokioRuntime;

// Import the trait so `.ipc_call()` is visible on `KernelPluginContext`.
use nexus_kernel::PluginContext;

use nexus_bootstrap::Runtime as NexusRuntime;

pub struct KernelBridge {
    /// Nexus kernel + plugin registry. Locked briefly per IPC call.
    runtime: Arc<Mutex<NexusRuntime>>,
    /// Dedicated tokio runtime for driving async IPC futures.
    tokio: Arc<TokioRuntime>,
}

impl KernelBridge {
    pub fn new(runtime: NexusRuntime) -> Result<Self> {
        let tokio = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .thread_name("nexus-ipc")
            .build()
            .context("failed to start nexus IPC tokio runtime")?;

        Ok(Self {
            runtime: Arc::new(Mutex::new(runtime)),
            tokio: Arc::new(tokio),
        })
    }

    /// Make a synchronous (blocking) IPC call from any thread.
    ///
    /// Intended for use inside `cx.background_executor().spawn(async { ... })`
    /// blocks where the caller is already off the main gpui thread.
    pub fn call(&self, plugin: &str, command: &str, args: Value) -> Result<Value> {
        let rt = self.runtime.lock().expect("runtime lock poisoned");
        self.tokio
            .block_on(rt.context.ipc_call(plugin, command, args, Duration::from_secs(30)))
            .context("ipc_call failed")
    }

    /// Convenience: call without arguments.
    pub fn call_empty(&self, plugin: &str, command: &str) -> Result<Value> {
        self.call(plugin, command, serde_json::json!({}))
    }
}
