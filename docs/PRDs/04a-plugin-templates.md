# Plugin Templates — PRD v1.0

**Version:** 1.0  
**Date:** April 2026  
**Status:** Implementation-Ready  
**Subsystem:** Plugin Development Templates  
**Parent PRDs:** [01 — Kernel & Event System](01-kernel-event-system.md), [04 — Plugin System](04-plugin-system.md)

---

## Executive Summary

This PRD specifies the official plugin templates for Nexus — both as `cargo-generate` scaffolds and as working reference crates in the repository. The templates encode the conventions, lifecycle hooks, event subscription patterns, capability declarations, and project structure that every plugin must follow. Two variants exist — **core** and **community** — that share the same structure but differ in default capability scope and manifest flags.

The goal is to eliminate boilerplate guesswork, enforce the opaque `EventSubscription` pattern from day one (see PRD 01 §3.4), and give plugin authors a compilable starting point that passes all lint and test checks out of the box.

---

## 1. Design Rationale

### 1.1 Why Templates Matter Early

Every plugin in the system will implement the same `PluginLifecycle` trait, subscribe to events through the same `EventSubscription` API, declare capabilities in the same manifest format, and follow the same directory layout. Without templates, each plugin author will reverse-engineer these patterns from the PRDs and get subtle things wrong — especially the opaque subscription wrapper, capability escalation boundaries, and graceful shutdown sequencing.

Templates are cheaper than documentation at preventing mistakes because they give you code that already compiles.

### 1.2 Why Minimal Differentiation Between Core and Community

Core and community plugins share the same `PluginLifecycle` trait, the same `PluginContext` API, and the same event bus. The only differences are:

- **Capability defaults:** Core plugins receive all capabilities by default; community plugins declare required and optional capabilities explicitly.
- **Manifest trust level:** Core plugins set `trust_level = "core"`, which the kernel uses to skip user approval dialogs for capability grants.
- **Access to kernel internals:** Core plugins may use `pub(crate)` kernel APIs not exposed through `PluginContext`. The template does not scaffold these — they're added per-plugin as needed.

Everything else — directory structure, lifecycle hooks, event patterns, testing setup, build configuration — is identical. This keeps the plugin ecosystem uniform and avoids a class system where core plugins look fundamentally different from community contributions.

---

## 2. Template Structure

Both core and community templates produce the following directory layout:

```
{{plugin-name}}/
├── Cargo.toml
├── manifest.toml
├── src/
│   ├── lib.rs          # Plugin entry point, create_plugin() export
│   ├── plugin.rs       # PluginLifecycle implementation
│   ├── events.rs       # Event subscription and handling
│   └── state.rs        # Plugin state management (KV store helpers)
├── tests/
│   ├── lifecycle_test.rs
│   └── events_test.rs
├── assets/
│   ├── icon-light.svg
│   └── icon-dark.svg
└── README.md
```

### 2.1 cargo-generate Configuration

Each template includes a `cargo-generate.toml` at the root:

```toml
[template]
cargo_generate_version = ">=0.18.0"

[placeholders.plugin-name]
type = "string"
prompt = "Plugin name (kebab-case):"
regex = "^[a-z][a-z0-9-]*$"

[placeholders.plugin-id]
type = "string"
prompt = "Plugin ID (reverse-dns, e.g., com.nexus.my-plugin):"
regex = "^[a-z][a-z0-9.\\-]*$"

[placeholders.author]
type = "string"
prompt = "Author name:"

[placeholders.description]
type = "string"
prompt = "One-line description:"
```

---

## 3. manifest.toml Specification

### 3.1 Core Plugin Manifest

```toml
[plugin]
id = "{{plugin-id}}"
name = "{{plugin-name}}"
version = "0.1.0"
author = "{{author}}"
description = "{{description}}"
kernel_api_version = "1.0.0"
trust_level = "core"

[capabilities]
# Core plugins receive all capabilities by default.
# List here only for documentation — the kernel grants all regardless.
required = []
optional = []

[dependencies]
# Other plugin IDs this plugin requires to be loaded first.
plugins = []

[build]
binary_name = "{{plugin-name}}"
```

### 3.2 Community Plugin Manifest

```toml
[plugin]
id = "{{plugin-id}}"
name = "{{plugin-name}}"
version = "0.1.0"
author = "{{author}}"
description = "{{description}}"
kernel_api_version = "1.0.0"
trust_level = "community"

[capabilities]
# Community plugins must declare every capability they need.
# Required capabilities block plugin load if denied.
# Optional capabilities allow degraded operation if denied.
required = ["FileRead"]
optional = ["FileWrite"]

[dependencies]
plugins = []

[build]
binary_name = "{{plugin-name}}"
```

---

## 4. Source File Specifications

### 4.1 lib.rs — Plugin Entry Point

```rust
mod plugin;
mod events;
mod state;

use plugin::MyPlugin;
use nexus_core::PluginLifecycle;

/// Entry point called by the kernel to instantiate this plugin.
/// This is the only `#[no_mangle]` export required.
#[no_mangle]
pub extern "C" fn create_plugin() -> Box<dyn PluginLifecycle> {
    Box::new(MyPlugin::new())
}
```

### 4.2 plugin.rs — Lifecycle Implementation

```rust
use nexus_core::{
    PluginContext, PluginLifecycle, PluginError,
    NexusEvent, EventSubscription,
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

#[async_trait]
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
        self.state = Some(PluginState::restore(ctx).await?);

        // Subscribe to events — uses opaque EventSubscription (PRD 01 §3.4).
        // The underlying bus transport may change; this code won't need to.
        self.event_handler = Some(EventHandler::new(ctx));

        Ok(())
    }

    async fn on_start(&mut self) -> Result<(), PluginError> {
        // Called when the plugin transitions to Started.
        // Begin active work: start background tasks, open connections, etc.
        Ok(())
    }

    async fn on_stop(&mut self) -> Result<(), PluginError> {
        // Called on graceful shutdown. Persist state before exiting.
        // The kernel enforces a 5-second timeout on this method.
        if let Some(state) = &self.state {
            state.persist().await?;
        }
        Ok(())
    }

    async fn on_shutdown(&mut self) -> Result<(), PluginError> {
        // Final cleanup: close file handles, release locks, drop resources.
        // Called after on_stop(). The kernel enforces a 2-second timeout.
        self.event_handler = None;
        self.state = None;
        Ok(())
    }
}
```

### 4.3 events.rs — Event Subscription and Handling

```rust
use nexus_core::{
    PluginContext, NexusEvent, EventSubscription,
};
use std::sync::Arc;

/// Handles event subscription and dispatch for this plugin.
/// Uses the opaque EventSubscription type — never references
/// broadcast::Receiver or any channel-specific type directly.
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
    /// Monitors both the event stream and the cancellation token.
    pub async fn run(&mut self, ctx: &dyn PluginContext) {
        loop {
            tokio::select! {
                _ = ctx.cancellation_token().cancelled() => {
                    // Kernel requested shutdown — exit cleanly.
                    break;
                }
                Some(event) = self.subscription.recv() => {
                    self.handle_event(ctx, &event).await;
                }
            }
        }
    }

    /// Dispatch a single event. Match only the variants this plugin cares about.
    async fn handle_event(&self, ctx: &dyn PluginContext, event: &NexusEvent) {
        match event {
            // --- Add your event handlers here ---

            NexusEvent::FileCreated(file_event) => {
                // Example: react to new files.
                // tracing::info!("File created: {:?}", file_event.path);
            }

            // Ignore all other events.
            _ => {}
        }
    }
}
```

### 4.4 state.rs — State Management

```rust
use nexus_core::{PluginContext, PluginError};

/// Plugin state that persists across hot-reloads via the kernel KV store.
///
/// On init: call restore() to load previous state.
/// On stop: call persist() to save current state.
/// This ensures hot-reload continuity (PRD 01 §7.2).
pub struct PluginState {
    // Add your plugin's persistent fields here.
    // Example:
    // pub last_processed_file: Option<String>,
}

impl PluginState {
    const KV_KEY: &'static str = "plugin_state";

    /// Restore state from the KV store, or create fresh state if none exists.
    pub async fn restore(ctx: &dyn PluginContext) -> Result<Self, PluginError> {
        match ctx.kv_get(Self::KV_KEY).await {
            Ok(Some(bytes)) => {
                // Deserialize from stored bytes.
                // Use serde_json or bincode — handle missing fields gracefully
                // for forward compatibility when new fields are added.
                serde_json::from_slice(&bytes)
                    .map_err(|e| PluginError::InitFailed(
                        format!("State deserialization failed: {}. Starting fresh.", e)
                    ))
                    .or_else(|_| Ok(Self::default()))
            }
            Ok(None) => {
                // No prior state — first run or state was cleared.
                Ok(Self::default())
            }
            Err(e) => {
                // KV store error — log and start fresh rather than failing init.
                tracing::warn!("Failed to restore state: {:?}. Starting fresh.", e);
                Ok(Self::default())
            }
        }
    }

    /// Persist current state to the KV store.
    pub async fn persist(&self) -> Result<(), PluginError> {
        // Serialize and store. Called during on_stop() before shutdown.
        let _bytes = serde_json::to_vec(self)
            .map_err(|e| PluginError::StopFailed(
                format!("State serialization failed: {}", e)
            ))?;
        // ctx.kv_set(Self::KV_KEY, bytes).await
        //     .map_err(|e| PluginError::StopFailed(format!("KV write failed: {}", e)))?;
        Ok(())
    }

    fn default() -> Self {
        PluginState {
            // Initialize default field values here.
        }
    }
}
```

---

## 5. Test Specifications

### 5.1 lifecycle_test.rs

```rust
use nexus_core::test_utils::MockPluginContext;

#[tokio::test]
async fn test_plugin_lifecycle_happy_path() {
    let mut plugin = create_plugin();
    let ctx = MockPluginContext::new();

    // Full lifecycle: load → init → start → stop → shutdown.
    plugin.on_load().await.unwrap();
    plugin.on_init(&ctx).await.unwrap();
    plugin.on_start().await.unwrap();
    plugin.on_stop().await.unwrap();
    plugin.on_shutdown().await.unwrap();
}

#[tokio::test]
async fn test_plugin_state_persists_across_reload() {
    let ctx = MockPluginContext::new();

    // First run: init and stop (persists state).
    let mut plugin = create_plugin();
    plugin.on_load().await.unwrap();
    plugin.on_init(&ctx).await.unwrap();
    plugin.on_start().await.unwrap();
    plugin.on_stop().await.unwrap();
    plugin.on_shutdown().await.unwrap();

    // Second run: init should restore state from KV store.
    let mut plugin2 = create_plugin();
    plugin2.on_load().await.unwrap();
    plugin2.on_init(&ctx).await.unwrap();
    // Assert state was restored.
    plugin2.on_shutdown().await.unwrap();
}
```

### 5.2 events_test.rs

```rust
use nexus_core::test_utils::{MockPluginContext, MockEventBus};

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

    // Verify handler processes it without panic.
    // (Plugin-specific assertions go here.)
}

#[tokio::test]
async fn test_event_handler_ignores_unrelated_events() {
    let bus = MockEventBus::new();
    let ctx = MockPluginContext::with_bus(&bus);

    let mut handler = EventHandler::new(&ctx);

    // Publish an event this plugin doesn't handle.
    bus.publish(NexusEvent::KernelStarted).await.unwrap();

    // Should not panic or produce side effects.
}
```

---

## 6. Cargo.toml Specification

```toml
[package]
name = "{{plugin-name}}"
version = "0.1.0"
edition = "2021"
authors = ["{{author}}"]
description = "{{description}}"

[lib]
crate-type = ["cdylib"]

[dependencies]
nexus-core = { path = "../../nexus-core" }
async-trait = "0.1"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tracing = "0.1"
tokio = { version = "1.35", features = ["macros", "rt-multi-thread"] }
futures = "0.3"

[dev-dependencies]
nexus-core = { path = "../../nexus-core", features = ["test-utils"] }
tokio = { version = "1.35", features = ["test-util"] }
```

---

## 7. Conventions Enforced by the Templates

### 7.1 Event Subscription

Templates always use `EventSubscription` via `ctx.subscribe_event()`. No template
file imports `tokio::sync::broadcast` or references channel-specific types. This
ensures the migration from broadcast to type-based pub/sub (PRD 01 §3.6) requires
zero plugin-side changes.

### 7.2 State Persistence

Templates include the `state.rs` pattern with `restore()` / `persist()` from day
one. Even plugins that initially have no state get the scaffolding, because state
requirements tend to emerge during development and retrofitting persistence after
the fact leads to hot-reload bugs.

### 7.3 Graceful Shutdown

Templates wire up the `CancellationToken` in the event loop and persist state in
`on_stop()`. The separation between `on_stop()` (save state, drain work) and
`on_shutdown()` (release resources) is explicit in the template comments.

### 7.4 Error Resilience

`PluginState::restore()` never fails fatally — if deserialization fails or the KV
store errors, it logs a warning and starts with fresh state. This prevents a
corrupted state blob from permanently bricking a plugin.

### 7.5 Capability Documentation

Community plugin manifests include inline comments explaining the distinction
between `required` (blocks load if denied) and `optional` (allows degraded
operation) capabilities. Core plugin manifests note that capabilities are granted
implicitly and the list is for documentation only.

---

## 8. Integration with `nexus plugin scaffold`

The CLI command `nexus plugin scaffold` (PRD 05) should:

1. Prompt for `--type core|community` (default: `community`).
2. Run `cargo generate` against the appropriate template.
3. Fill placeholders from CLI flags or interactive prompts.
4. Run `cargo check` on the generated project to verify it compiles.
5. Print next-steps guidance.

```bash
$ nexus plugin scaffold --name my-analyzer --type community --author "Jane Doe"

Created: nexus-plugins/my-analyzer/
  manifest.toml  — community plugin, requires [FileRead]
  src/lib.rs     — entry point
  src/plugin.rs  — lifecycle stubs
  src/events.rs  — event handler with EventSubscription
  src/state.rs   — KV-backed state persistence

Next steps:
  cd nexus-plugins/my-analyzer
  $EDITOR manifest.toml           # declare your capabilities
  $EDITOR src/events.rs           # add event handlers
  cargo nexus-plugin-dev          # hot-reload dev server
```

---

## 9. Acceptance Criteria

### 9.1 Templates

- [ ] Core and community templates both compile with `cargo check` out of the box.
- [ ] Both templates pass `cargo test` with no modifications.
- [ ] `cargo clippy` produces zero warnings on generated code.
- [ ] Templates use `EventSubscription` exclusively — no `broadcast::Receiver` imports.
- [ ] Templates include `CancellationToken` wiring in the event loop.
- [ ] Templates include `PluginState::restore()` / `persist()` pattern.
- [ ] `cargo-generate.toml` validates plugin name (kebab-case) and ID (reverse-dns).
- [ ] Generated `manifest.toml` differs only in `trust_level` and capability defaults.

### 9.2 CLI Integration

- [ ] `nexus plugin scaffold --type core` generates from the core template.
- [ ] `nexus plugin scaffold --type community` (or no `--type`) generates from the community template.
- [ ] `nexus plugin scaffold` runs `cargo check` post-generation and reports errors.
- [ ] All placeholders are filled from flags or interactive prompts.

### 9.3 Documentation

- [ ] Template source files include comments explaining why each pattern exists, with cross-references to PRD sections.
- [ ] README.md in each template covers build, test, dev-server, and deployment steps.

---

## 10. Dependencies

### 10.1 What This Specification Depends On

| Dependency | PRD | Why |
|---|---|---|
| `PluginLifecycle` trait | 01 — Kernel §4 | Lifecycle hooks the template implements |
| `EventSubscription` type | 01 — Kernel §3.4 | Opaque subscription wrapper used in event handlers |
| `PluginContext` trait | 01 — Kernel §6 | API surface the template codes against |
| `CancellationToken` | 01 — Kernel §9.3 | Graceful shutdown wiring |
| Plugin manifest format | 04 — Plugin System §3 | `manifest.toml` schema |
| `nexus plugin scaffold` CLI | 05 — CLI | Scaffolding command that invokes these templates |

### 10.2 What Depends On This Specification

| Consumer | Why |
|---|---|
| Every core plugin (PRDs 08–16) | Will be scaffolded from the core template |
| Community plugin ecosystem | Will be scaffolded from the community template |
| Plugin documentation (PRD 04 §16) | References these templates as the starting point |

---

**Document Version:** 1.0  
**Last Updated:** April 2026  
**Status:** Ready for Implementation
