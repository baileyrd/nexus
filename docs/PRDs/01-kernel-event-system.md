# Nexus Kernel & Event System — PRD v1.0

**Version:** 1.0  
**Date:** April 2026  
**Status:** ✅ Shipped — Complete (see [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md), 2026-04-18)  
**Subsystem:** Core Kernel & Event Bus

---

## Executive Summary

The Kernel & Event System is the foundational microkernel that implements the event bus, plugin lifecycle management, capability system, and storage primitives for Nexus. This subsystem is the control plane—intentionally minimal—that all other plugins depend upon. It enforces type safety, async-first concurrency, and permission boundaries at the architectural level, not as an afterthought.

---

## 1. Architecture & Design Rationale

### 1.1 Why Microkernel Over Monolith

The microkernel architecture provides:
- **Loose coupling:** Plugins communicate only via typed events, not direct function calls.
- **Hot-reload capability:** Plugins can be unloaded and reloaded without kernel restart.
- **Fault isolation:** A crashing plugin does not crash the entire system.
- **Clear boundaries:** Capabilities enforce permission segregation between core and community plugins.
- **Developer agility:** Teams can build and ship plugins independently once the kernel interface is stable.

### 1.2 Why Broadcast Over Request/Response

Broadcast (publish/subscribe) is the event pattern because:
- **Natural fan-out:** One event can notify N subscribers without O(N) coordination overhead.
- **Decoupling:** Publishers don't need to know or wait for subscribers to complete.
- **Ordering guarantees:** All subscribers see events in the same sequence on a single channel.
- **Resilience:** Slow subscribers don't block fast ones; backpressure is explicit, not implicit.
- **Debuggability:** Event history is easy to replay and analyze in a ring buffer.

Request/response (RPC) will be handled by plugins via dedicated IPC event types, not the kernel.

### 1.3 Why Typed Events

Using a Rust `enum` for `NexusEvent` instead of dynamic/untyped events:
- **Type safety:** Compiler catches event payload mismatches.
- **Documentation:** Each variant shows exactly what data it carries.
- **Performance:** No runtime type checking or allocation for variant dispatch.
- **Tooling:** IDEs can auto-complete and jump to event definitions.

---

## 2. NexusEvent Enum & Metadata

### 2.1 Event Type Definitions

All events flow through a single `NexusEvent` enum. Each variant includes a payload and metadata.

```rust
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Global event type for all inter-plugin communication.
/// Variants are organized by subsystem for clarity.
#[derive(Clone, Debug)]
pub enum NexusEvent {
    // --- Filesystem events ---
    FileCreated(FileEvent),
    FileModified(FileEvent),
    FileDeleted(FileEvent),
    DirectoryCreated(FileEvent),
    DirectoryDeleted(FileEvent),

    // --- Editor events ---
    EditorOpened(EditorEvent),
    EditorClosed(EditorEvent),
    EditorBufferChanged(EditorEvent),
    EditorCursorMoved(EditorEvent),
    EditorSelectionChanged(EditorEvent),
    EditorSyntaxError(EditorEvent),

    // --- Workspace events ---
    WorkspaceOpened(WorkspaceEvent),
    WorkspaceClosed(WorkspaceEvent),
    WorkspaceProjectAdded(WorkspaceEvent),
    WorkspaceProjectRemoved(WorkspaceEvent),
    WorkspaceSettingsChanged(WorkspaceEvent),

    // --- Plugin lifecycle events ---
    PluginLoading(PluginEvent),
    PluginLoaded(PluginEvent),
    PluginInitializing(PluginEvent),
    PluginInitialized(PluginEvent),
    PluginStarting(PluginEvent),
    PluginStarted(PluginEvent),
    PluginStopping(PluginEvent),
    PluginStopped(PluginEvent),
    PluginUnloading(PluginEvent),
    PluginUnloaded(PluginEvent),
    PluginError(PluginEvent),
    PluginCapabilityRequested(PluginEvent),
    PluginCapabilityGranted(PluginEvent),
    PluginCapabilityDenied(PluginEvent),

    // --- AI & LLM events ---
    AICompletionRequested(AIEvent),
    AICompletionReceived(AIEvent),
    AIErrorOccurred(AIEvent),
    AIModelLoaded(AIEvent),

    // --- Terminal events ---
    TerminalOpened(TerminalEvent),
    TerminalClosed(TerminalEvent),
    TerminalInput(TerminalEvent),
    TerminalOutput(TerminalEvent),

    // --- Process manager events ---
    ProcessStarted(ProcessEvent),
    ProcessExited(ProcessEvent),
    ProcessOutput(ProcessEvent),
    ProcessError(ProcessEvent),

    // --- Database events ---
    DatabaseConnected(DatabaseEvent),
    DatabaseDisconnected(DatabaseEvent),
    DatabaseQueryExecuted(DatabaseEvent),
    DatabaseIndexUpdated(DatabaseEvent),

    // --- Search events ---
    SearchIndexing(SearchEvent),
    SearchIndexed(SearchEvent),
    SearchQueryExecuted(SearchEvent),
    SearchResultsReady(SearchEvent),

    // --- Sync/Collaboration events ---
    SyncStarted(SyncEvent),
    SyncCompleted(SyncEvent),
    SyncConflict(SyncEvent),
    SyncError(SyncEvent),

    // --- Custom plugin events ---
    PluginCustom(CustomEvent),

    // --- System events ---
    KernelStarting,
    KernelStarted,
    KernelShuttingDown,
    SystemPaused,
    SystemResumed,
}

/// Event metadata attached to all events.
#[derive(Clone, Debug)]
pub struct EventMetadata {
    /// Unique ID for this event instance.
    pub event_id: Uuid,
    /// Timestamp when the event was created.
    pub timestamp: DateTime<Utc>,
    /// ID of the plugin that emitted this event.
    pub source_plugin_id: String,
    /// For correlation across multiple events in a flow.
    pub correlation_id: Uuid,
    /// OpenTelemetry span ID for distributed tracing.
    pub span_id: String,
    /// Severity level for filtering and routing.
    pub severity: EventSeverity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventSeverity {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

// --- Event payload structures (each subsystem) ---

#[derive(Clone, Debug)]
pub struct FileEvent {
    pub metadata: EventMetadata,
    pub path: std::path::PathBuf,
    pub size_bytes: Option<u64>,
    pub mime_type: Option<String>,
}

#[derive(Clone, Debug)]
pub struct EditorEvent {
    pub metadata: EventMetadata,
    pub editor_id: String,
    pub file_path: std::path::PathBuf,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub text_delta: Option<String>,
    pub language: Option<String>,
}

#[derive(Clone, Debug)]
pub struct WorkspaceEvent {
    pub metadata: EventMetadata,
    pub workspace_id: String,
    pub project_id: Option<String>,
    pub setting_key: Option<String>,
    pub setting_value: Option<serde_json::Value>,
}

#[derive(Clone, Debug)]
pub struct PluginEvent {
    pub metadata: EventMetadata,
    pub plugin_id: String,
    pub plugin_version: String,
    pub capability: Option<String>,
    pub reason: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AIEvent {
    pub metadata: EventMetadata,
    pub model: String,
    pub tokens_used: Option<usize>,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TerminalEvent {
    pub metadata: EventMetadata,
    pub terminal_id: String,
    pub command: Option<String>,
    pub output: Option<String>,
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug)]
pub struct ProcessEvent {
    pub metadata: EventMetadata,
    pub process_id: u32,
    pub command: String,
    pub output: Option<String>,
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug)]
pub struct DatabaseEvent {
    pub metadata: EventMetadata,
    pub connection_string: String,
    pub query: Option<String>,
    pub rows_affected: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct SearchEvent {
    pub metadata: EventMetadata,
    pub index_name: String,
    pub query: Option<String>,
    pub result_count: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct SyncEvent {
    pub metadata: EventMetadata,
    pub resource_id: String,
    pub change_count: Option<usize>,
    pub conflict_info: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CustomEvent {
    pub metadata: EventMetadata,
    pub event_type: String,
    pub payload: serde_json::Value,
}
```

### 2.2 Event Metadata Design

Every event carries metadata for observability and correlation:
- **event_id:** Unique identifier for audit logs and tracing.
- **timestamp:** UTC creation time; used to detect event ordering issues.
- **source_plugin_id:** Which plugin emitted this event; used for filtering.
- **correlation_id:** Links multiple events in a single user action or transaction.
- **span_id:** OpenTelemetry integration for distributed tracing across async tasks.
- **severity:** Guides filtering, alerting, and log aggregation.

---

## 3. Event Bus Internals

### 3.1 Channel Architecture & Sizing

The event bus uses `tokio::sync::broadcast` to implement the event stream. Critically,
the broadcast channel is an **implementation detail** that must not leak through the
`PluginContext` trait boundary. All subscriber-facing APIs return an opaque
`EventSubscription` type (see §3.5) so the underlying transport can be swapped
(e.g., to type-based pub/sub routing) without breaking plugin code.

```rust
use tokio::sync::broadcast;
use std::sync::Arc;

pub struct EventBus {
    /// Broadcast sender; all events flow through this channel.
    tx: broadcast::Sender<Arc<NexusEvent>>,
    /// Bounded ring buffer; max 10,000 events in memory at once.
    /// When full, oldest events are dropped.
    /// Sizing rationale: ~100 KB per event (with metadata) = 1 GB max memory.
    capacity: usize,
    /// Track slow subscribers; alert if any lag > 1000 events.
    slow_subscriber_threshold: usize,
}

impl EventBus {
    /// Create a new event bus with a bounded ring buffer.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(10_000);
        EventBus {
            tx,
            capacity: 10_000,
            slow_subscriber_threshold: 1_000,
        }
    }

    /// Publish an event to all subscribers.
    /// Returns error only if all subscribers have disconnected.
    pub async fn publish(&self, event: NexusEvent) -> Result<(), PublishError> {
        self.tx.send(Arc::new(event))
            .map_err(|_| PublishError::NoSubscribers)?;
        Ok(())
    }

    /// Subscribe to all future events (not historical).
    /// Returns an opaque EventSubscription, not the raw broadcast::Receiver.
    pub fn subscribe(&self) -> EventSubscription {
        EventSubscription::from_broadcast(self.tx.subscribe())
    }
}

#[derive(Debug)]
pub enum PublishError {
    /// No active subscribers; event dropped.
    NoSubscribers,
    /// Channel was closed during publish.
    ChannelClosed,
}
```

### 3.2 Backpressure Handling

Subscribers receive events in a broadcast channel. If a subscriber is slow:
- The channel will not block the publisher (broadcast is non-blocking).
- If a subscriber lags > 1,000 events behind, it receives `broadcast::error::RecvError::Lagged`.
- Subscribers must handle this by:
  - Reconnecting and fetching state from persistent storage.
  - Logging the lag incident for debugging.
  - Optionally alerting the kernel to slow down other subscribers.

Dead subscribers are automatically cleaned up when they drop their receiver.

### 3.3 Event History & Replay

The bounded ring buffer retains the last 10,000 events. The kernel exposes a replay API:

```rust
pub trait EventHistory {
    /// Retrieve events since a specific timestamp.
    async fn events_since(
        &self,
        since: DateTime<Utc>,
        filter: Option<EventFilter>,
    ) -> Result<Vec<Arc<NexusEvent>>, HistoryError>;

    /// Retrieve the last N events matching a filter.
    async fn last_n_events(
        &self,
        n: usize,
        filter: Option<EventFilter>,
    ) -> Result<Vec<Arc<NexusEvent>>, HistoryError>;

    /// Clear history (for testing or on kernel shutdown).
    async fn clear(&mut self) -> Result<(), HistoryError>;
}

pub struct EventFilter {
    pub source_plugin_id: Option<String>,
    pub event_type: Option<String>,
    pub severity: Option<EventSeverity>,
    pub correlation_id: Option<Uuid>,
}
```

### 3.4 Opaque Subscription Type

The `EventSubscription` wrapper ensures no channel-specific type leaks through the
plugin API. Plugins consume events via a generic async stream interface. The kernel
can change the underlying transport (broadcast, mpsc fan-out, type-based routing)
without any plugin code changes.

```rust
use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;

/// Opaque event subscription returned to plugins.
/// Hides the underlying channel implementation so the bus
/// transport can be swapped without breaking the plugin API.
pub struct EventSubscription {
    inner: Pin<Box<dyn Stream<Item = Arc<NexusEvent>> + Send>>,
}

impl EventSubscription {
    /// Wrap a tokio broadcast receiver (current implementation).
    pub(crate) fn from_broadcast(
        rx: broadcast::Receiver<Arc<NexusEvent>>,
    ) -> Self {
        use tokio_stream::wrappers::BroadcastStream;
        use tokio_stream::StreamExt;
        let stream = BroadcastStream::new(rx)
            .filter_map(|result| result.ok());
        EventSubscription {
            inner: Box::pin(stream),
        }
    }

    /// Receive the next event. Returns None if the bus is closed.
    pub async fn recv(&mut self) -> Option<Arc<NexusEvent>> {
        use futures::StreamExt;
        self.inner.next().await
    }
}
```

**Why this matters:** The PRD's initial implementation uses `tokio::sync::broadcast`,
but the intended migration path is toward type-based pub/sub routing (see §3.6).
By boxing the stream behind `EventSubscription` from day one, that migration becomes
a kernel-internal refactor with zero plugin-side changes. Without this wrapper,
`broadcast::Receiver` leaks into every plugin's select loop, and changing the bus
requires updating every plugin in lockstep.

### 3.5 Channel Alternative Analysis (Historical)

**Why tokio::broadcast?**
- Non-blocking send (important for hot paths).
- Built-in ring buffer with automatic old-event eviction.
- Tight integration with Tokio runtime.
- Good for many-to-many publish/subscribe.

**Alternatives considered:**
- **flume:** More memory-efficient for large payloads; however, tokio::broadcast is standard and sufficient.
- **crossbeam:** Good for sync code; we are async-first, so overkill.
- **async-broadcast:** Equivalent to tokio::broadcast; tokio is preferred for ecosystem consistency.

### 3.6 Migration Path: Broadcast → Type-Based Pub/Sub

The initial broadcast implementation is the simplest correct starting point, but it
has known scaling limitations. As plugin count and event volume grow, the system
should migrate to **type-based pub/sub routing**, where the kernel dispatches events
only to subscribers that declared interest in specific event variants.

#### Why type-based over topic-based

Topic-based routing (one channel per subsystem category, e.g., "filesystem," "editor")
reduces noise compared to broadcast but still delivers irrelevant events within a
category. It also forces arbitrary grouping decisions — does `DatabaseIndexUpdated`
belong to the "database" topic or the "search" topic?

Type-based routing maps directly to `NexusEvent` enum discriminants. A plugin declares
"I want `FileCreated` and `AICompletionReceived`" and receives exactly those two
streams. No subscriber-side filtering, no wasted delivery, and the routing granularity
matches the type system that already exists.

Wildcard subscriptions ("give me all filesystem events") are supported as syntactic
sugar — the kernel registers the subscriber for each variant in the category.

#### What the migration involves

Because `EventSubscription` (§3.4) hides the channel implementation, the migration
is **kernel-internal only** — no plugin code changes.

**Kernel changes (~100-200 lines):**
- `EventBus` replaces the single `broadcast::Sender` with a
  `HashMap<Discriminant<NexusEvent>, Vec<mpsc::Sender<Arc<NexusEvent>>>>`.
- `publish()` looks up the event's discriminant and fans out to registered senders
  (O(1) lookup + O(subscribers) send).
- `subscribe()` and `subscribe_filtered()` register mpsc senders under the relevant
  discriminants and return `EventSubscription` wrapping the mpsc receiver.

**Event history changes:**
- The broadcast ring buffer goes away. Event history must be maintained as a
  separate concern — either a `VecDeque` in memory or the SQLite-backed persistent
  log (recommended, since `rusqlite` is already a dependency).
- The `EventHistory` trait (§3.3) is already architecturally separated, so no API
  change is needed.

**New subscription API (additive, not breaking):**
```rust
// Subscribe to specific event types (preferred for new plugins).
ctx.subscribe_to::<FileCreated>()
ctx.subscribe_to_category(EventCategory::Filesystem)

// Original subscribe_event() remains as "subscribe to everything"
// convenience — internally registers for all discriminants.
ctx.subscribe_event()
```

#### When to migrate

The broadcast bus should be replaced when any of the following thresholds are hit:
- Sustained event throughput exceeds 50,000 events/sec (broadcast lock contention).
- More than 10 active plugins (wasted delivery cost becomes measurable).
- Plugins begin spending >5% of CPU time in event match/discard logic.

---

## 4. Plugin Lifecycle State Machine

### 4.1 State Diagram

```
   [Discovered] ──(load)──> [Loaded] ──(init)──> [Initialized] ──(start)──> [Started]
       ^                        ^                     ^                        |
       |                        |                     |                        |
       └────────────────────────┴─────────────────────┴────────(reload)────────┘
                                                       
                                                [Started]
                                                   |
                                              (stop)│
                                                   v
                                             [Stopped]
                                                   |
                                             (unload)│
                                                   v
                                            [Unloaded]
```

Error states exist at each transition. Timeouts apply:

| Transition | Timeout | Cleanup |
|-----------|---------|---------|
| load() → Loaded | 5s | Unload file handles, report error |
| init() → Initialized | 10s | Invoke on_shutdown, unload, report error |
| start() → Started | 10s | Invoke on_stop, then unload |
| stop() → Stopped | 5s | Force-cancel async tasks, unload |
| unload() → Unloaded | 2s | Drop all plugin resources |

### 4.2 State Transitions & Error Handling

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginState {
    Discovered,
    Loaded,
    Initialized,
    Started,
    Stopped,
    Unloaded,
    Error,
}

pub struct PluginInstance {
    pub id: String,
    pub version: String,
    pub state: PluginState,
    pub capabilities_granted: Vec<String>,
    pub dependencies: Vec<String>,
}

pub trait PluginLifecycle: Send + Sync {
    /// Called when the plugin is loaded into memory.
    async fn on_load(&mut self) -> Result<(), PluginError>;

    /// Called after all dependencies are initialized.
    async fn on_init(&mut self, ctx: &PluginContext) -> Result<(), PluginError>;

    /// Called when the plugin is started.
    async fn on_start(&mut self) -> Result<(), PluginError>;

    /// Called when the plugin is stopping (graceful shutdown).
    async fn on_stop(&mut self) -> Result<(), PluginError>;

    /// Called when the plugin is being unloaded.
    async fn on_shutdown(&mut self) -> Result<(), PluginError>;
}

/// Errors during plugin lifecycle.
#[derive(Debug)]
pub enum PluginError {
    LoadFailed(String),
    InitFailed(String),
    StartFailed(String),
    StopFailed(String),
    ShutdownFailed(String),
    DependencyMissing(String),
    CircularDependency(Vec<String>),
    CapabilityDenied(String),
    Timeout,
    Panic(String),
}
```

### 4.3 Dependency Resolution Algorithm

The kernel uses topological sort (Kahn's algorithm) to order plugin initialization:

```rust
pub fn resolve_plugin_order(plugins: &[PluginManifest]) 
    -> Result<Vec<String>, PluginError> 
{
    // 1. Build dependency graph.
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    let mut in_degree: HashMap<String, usize> = HashMap::new();

    for plugin in plugins {
        graph.entry(plugin.id.clone()).or_insert_with(Vec::new);
        *in_degree.entry(plugin.id.clone()).or_insert(0);
        
        for dep in &plugin.dependencies {
            graph.entry(dep.clone()).or_insert_with(Vec::new);
            graph[&plugin.id].push(dep.clone());
            *in_degree.get_mut(&dep).unwrap_or_else(|| {
                in_degree.insert(dep.clone(), 0);
                in_degree.get_mut(dep).unwrap()
            }) += 1;
        }
    }

    // 2. Kahn's algorithm: identify sources (in_degree = 0).
    let mut queue: Vec<String> = in_degree
        .iter()
        .filter(|(_, &degree)| degree == 0)
        .map(|(id, _)| id.clone())
        .collect();

    let mut order = Vec::new();

    while let Some(node) = queue.pop() {
        order.push(node.clone());
        for neighbor in graph[&node].clone() {
            *in_degree.get_mut(&neighbor).unwrap() -= 1;
            if in_degree[&neighbor] == 0 {
                queue.push(neighbor);
            }
        }
    }

    // 3. Detect cycles: if order.len() != plugins.len(), cycle exists.
    if order.len() != plugins.len() {
        let remaining: Vec<String> = in_degree
            .iter()
            .filter(|(_, &degree)| degree > 0)
            .map(|(id, _)| id.clone())
            .collect();
        return Err(PluginError::CircularDependency(remaining));
    }

    Ok(order)
}
```

### 4.4 Panic & Crash Handling

If a plugin panics during lifecycle:
1. The kernel catches the panic via `catch_unwind`.
2. The plugin is marked as `Error`.
3. Dependent plugins are notified via `PluginError` event.
4. The plugin is forced into `Stopped` → `Unloaded`.
5. No automatic restart (manual restart via kernel API).

---

## 5. Capability System

### 5.1 Capability Definitions

Capabilities are coarse-grained permissions that allow or deny plugin access to kernel primitives.

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    // --- Filesystem ---
    FileRead,
    FileWrite,
    FileDelete,
    DirectoryWatch,
    
    // --- Editor ---
    EditorAccess,
    EditorWrite,
    
    // --- Workspace ---
    WorkspaceAccess,
    WorkspaceModify,
    
    // --- Process execution ---
    ProcessExecute,
    ProcessKill,
    
    // --- Network ---
    NetworkHttp,
    NetworkWebsocket,
    
    // --- Storage & Database ---
    StorageRead,
    StorageWrite,
    DatabaseAccess,
    
    // --- System ---
    SystemHotkey,
    SystemNativeMenu,
    SystemTray,
    SystemProtocolHandler,
    
    // --- IPC & Plugins ---
    PluginIpc,
    PluginCapabilityRequest,
    
    // --- Custom capability for plugins to define their own permissions ---
    Custom(String),
}

#[derive(Clone, Debug)]
pub struct CapabilitySet {
    /// Set of granted capabilities.
    granted: HashSet<Capability>,
    /// Whether the plugin can request capabilities dynamically post-init.
    can_escalate: bool,
    /// Audit log of capability requests.
    request_history: Vec<(DateTime<Utc>, Capability, bool)>,
}

impl CapabilitySet {
    pub fn has(&self, cap: &Capability) -> bool {
        self.granted.contains(cap)
    }

    pub fn grant(&mut self, cap: Capability) {
        self.granted.insert(cap);
    }

    pub fn deny(&mut self, cap: Capability) {
        self.granted.remove(&cap);
    }
}
```

### 5.2 Grant/Deny Flow

1. **Plugin manifest declares required capabilities** in `plugin.toml`:
   ```toml
   [plugin]
   id = "my-editor-plugin"
   required_capabilities = ["FileRead", "EditorAccess"]
   optional_capabilities = ["FileWrite"]
   ```

2. **Kernel loads and parses manifest**, compares declared capabilities to a **whitelist**:
   - **Core plugins** (shipped with Nexus) get all capabilities by default.
   - **Community plugins** can only request whitelisted capabilities.

3. **Dynamic capability requests** at runtime:
   ```rust
   pub trait PluginContext {
       async fn request_capability(&self, cap: Capability) 
           -> Result<(), CapabilityError>;
   }
   ```

4. **User approval dialog** (if enabled) shows the plugin requesting a capability.
   - User can **Grant Once**, **Grant Always**, or **Deny**.
   - Denial is logged and emits `PluginCapabilityDenied` event.

5. **Capability escalation prevention**: If a plugin tries to access a capability it doesn't have, the kernel raises `CapabilityError::Denied` and does not perform the action.

---

## 6. PluginContext API

The `PluginContext` trait is the main interface each plugin receives. It provides access to the kernel's services.

```rust
pub trait PluginContext: Send + Sync {
    // --- Event Bus ---
    /// Publish an event to all interested subscribers.
    async fn publish_event(&self, event: NexusEvent) -> Result<(), PublishError>;
    /// Subscribe to all future events. Returns an opaque stream (§3.4)
    /// so the underlying bus transport can be swapped without plugin changes.
    fn subscribe_event(&self) -> EventSubscription;
    /// Subscribe with a filter. Same opaque return type.
    async fn subscribe_event_filtered(
        &self,
        filter: EventFilter,
    ) -> EventSubscription;

    // --- Filesystem ---
    async fn read_file(&self, path: &Path) -> Result<Vec<u8>, IoError>;
    async fn write_file(&self, path: &Path, data: &[u8]) -> Result<(), IoError>;
    async fn delete_file(&self, path: &Path) -> Result<(), IoError>;
    async fn watch_directory(&self, path: &Path) -> Result<DirWatcher, IoError>;

    // --- Plugin Management ---
    fn plugin_id(&self) -> String;
    fn plugin_version(&self) -> String;
    async fn request_capability(&self, cap: Capability) 
        -> Result<(), CapabilityError>;
    async fn query_plugin(&self, plugin_id: &str) 
        -> Result<PluginInstance, PluginError>;

    // --- Storage Primitives ---
    async fn kv_get(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError>;
    async fn kv_set(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError>;
    async fn kv_delete(&self, key: &str) -> Result<(), StorageError>;

    // --- SQLite Index ---
    async fn index_query(&self, sql: &str) 
        -> Result<Vec<serde_json::Value>, IndexError>;
    async fn index_insert(&self, table: &str, row: serde_json::Value) 
        -> Result<(), IndexError>;

    // --- IPC (for plugins to expose RPC-like commands) ---
    async fn register_ipc_command(
        &self,
        command: String,
        handler: Box<dyn IpcHandler>,
    ) -> Result<(), IpcError>;
    async fn call_ipc_command(
        &self,
        target_plugin: &str,
        command: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, IpcError>;

    // --- Logging & Tracing ---
    fn logger(&self) -> &tracing::Span;
}

pub trait IpcHandler: Send + Sync {
    async fn handle(&self, payload: serde_json::Value) 
        -> Result<serde_json::Value, IpcError>;
}

#[derive(Debug)]
pub enum CapabilityError {
    Denied(String),
    NotFound,
    AlreadyGranted,
}

#[derive(Debug)]
pub enum IpcError {
    PluginNotFound,
    CommandNotFound,
    SerializationError(String),
    Timeout,
    HandlerPanic(String),
}
```

---

## 7. Hot-Reload Mechanism

### 7.1 Reload Process

Hot-reload allows a plugin to be updated without restarting the kernel:

1. **New version detected** (file watcher or manual trigger).
2. **Load new binary** into a separate in-memory namespace.
3. **Run on_init()** on the new version.
4. **Request capabilities** (same as initial load).
5. **Start new version** with on_start().
6. **Drain old subscribers** by replaying critical state to new subscribers.
7. **Swap references** atomically (Arc pointer swap).
8. **Stop & unload old version**.

```rust
pub async fn reload_plugin(
    &mut self,
    plugin_id: &str,
    new_binary: Vec<u8>,
) -> Result<(), ReloadError> {
    // 1. Verify old plugin exists and is Running.
    let old_plugin = self.get_plugin(plugin_id)?;
    if old_plugin.state != PluginState::Started {
        return Err(ReloadError::NotRunning);
    }

    // 2. Load and validate new binary.
    let new_plugin = self.load_plugin_from_bytes(new_binary)?;

    // 3. Check version compatibility.
    if new_plugin.version == old_plugin.version {
        return Err(ReloadError::SameVersion);
    }

    // 4. Initialize new plugin.
    self.init_plugin(&new_plugin).await?;

    // 5. Gracefully stop old, swap, and start new.
    self.stop_plugin(plugin_id).await?;
    self.swap_plugin(plugin_id, new_plugin).await?;
    self.start_plugin(plugin_id).await?;

    Ok(())
}
```

### 7.2 State Preservation

State is preserved across reload via:
- **Key-value store:** Plugins persist state to `ctx.kv_set()` before shutdown.
- **Replay on startup:** On new version's `on_init()`, plugin calls `ctx.kv_get()` to restore state.
- **Event history:** Plugin can query `event_history.events_since(last_timestamp)` to catch missed events during reload.

### 7.3 Version Compatibility

The kernel does not enforce API versioning automatically. Plugins are responsible for:
- Declaring required kernel version in manifest.
- Validating data schema during deserialization.
- Gracefully handling missing keys from older state blobs.

---

## 8. Error Handling & Supervision

### 8.1 Event Publication Errors

If event publishing fails:
- **All subscribers disconnected:** Event is dropped; kernel logs a warning.
- **Channel closed:** Indicates kernel is shutting down; return error to caller.

Plugins must be defensive: never assume an event was processed.

### 8.2 Plugin Crashes

If a plugin crashes or panics:
1. Panic is caught at task boundary via `catch_unwind`.
2. `PluginError` event is published.
3. Plugin state → `Error`.
4. Dependent plugins receive error; they decide how to respond.
5. Kernel does not auto-restart (manual intervention required).

### 8.3 Event Handler Panics

If a plugin's event handler panics:
1. The panic is isolated to that plugin's task.
2. Other subscribers continue processing.
3. `PluginError` is logged and can be queried via kernel API.
4. The panic does not propagate to the kernel itself.

### 8.4 Supervision Strategy

Plugins run in isolated Tokio tasks. The kernel maintains a task handle for each plugin:
```rust
pub struct PluginTask {
    plugin_id: String,
    task_handle: tokio::task::JoinHandle<()>,
    cancellation_token: CancellationToken,
}
```

On kernel shutdown, all cancellation tokens are triggered, and the kernel waits up to 30 seconds for tasks to complete gracefully. If any task does not complete, it is forcefully aborted.

---

## 9. Concurrency Model

### 9.1 Task Architecture

The kernel spawns one main task per plugin:
```rust
tokio::spawn(async move {
    if let Err(e) = plugin.run(ctx, rx).await {
        error!("Plugin {} crashed: {:?}", plugin_id, e);
    }
});
```

Each plugin's main task is responsible for:
- Subscribing to events.
- Running a select loop over event channels and timeouts.
- Spawning child tasks for long-running operations.
- Cleaning up on cancellation.

### 9.2 Task Spawning Strategy

Plugins should spawn child tasks for:
- **Long-running operations** (process execution, network requests).
- **Heavy computation** (search indexing, AI inference).

Use `tokio::spawn_blocking` for CPU-bound work to avoid starving the async runtime.

### 9.3 Cancellation & Graceful Shutdown

Each plugin receives a `CancellationToken` (via tokio-util):
```rust
pub trait PluginContext {
    fn cancellation_token(&self) -> CancellationToken;
}
```

On plugin stop, the kernel triggers the token. The plugin's main loop should monitor it:
```rust
let mut subscription = ctx.subscribe_event();

loop {
    tokio::select! {
        _ = ctx.cancellation_token().cancelled() => {
            // Graceful shutdown: flush state, close connections, exit.
            break;
        }
        Some(event) = subscription.recv() => {
            // Handle event.
        }
    }
}
```

### 9.4 Shutdown Sequence

On kernel shutdown:
1. Emit `KernelShuttingDown` event (plugins see this and can save state).
2. Trigger all plugin cancellation tokens in reverse dependency order.
3. Wait up to 30 seconds for each plugin's `on_stop()` to complete.
4. Invoke `on_shutdown()` for cleanup (close files, release locks).
5. Unload all plugins.
6. Stop the event bus.

---

## 10. Performance Requirements

| Metric | Target | Notes |
|--------|--------|-------|
| Event publish latency | < 1 ms (p50), < 5 ms (p99) | Non-blocking send; no waiting for subscribers. |
| Event subscribe latency | < 100 µs | Creating a receiver is instant. |
| Event throughput | 100,000 events/sec on a single core | Broadcast overhead is minimal. |
| Plugin load time | < 2 sec | Loading binary and validating manifest. |
| Plugin init time | < 5 sec | Depends on plugin; kernel enforces timeout. |
| Memory per event | ~100 KB (rough estimate) | 10,000-event ring buffer ~1 GB. |
| Task creation overhead | < 10 µs | Tokio task spawn cost. |
| Capability lookup | O(1) | HashSet membership test. |

---

## 11. Testing Strategy

### 11.1 Event Bus Tests

```rust
#[tokio::test]
async fn test_event_publish_subscribe() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    let event = NexusEvent::KernelStarted;
    bus.publish(event.clone()).await.unwrap();

    let received = rx.recv().await.unwrap();
    assert_eq!(*received, event);
}

#[tokio::test]
async fn test_slow_subscriber_lagged() {
    let bus = EventBus::new();
    let mut rx = bus.subscribe();

    // Publish 11,000 events to trigger lagging.
    for i in 0..11_000 {
        let event = NexusEvent::KernelStarted; // Dummy event.
        bus.publish(event).await.ok();
    }

    // Subscriber should see Lagged error.
    match rx.recv().await {
        Err(broadcast::error::RecvError::Lagged(_)) => {},
        _ => panic!("Expected Lagged"),
    }
}
```

### 11.2 Plugin Lifecycle Tests

```rust
#[tokio::test]
async fn test_plugin_circular_dependency_detected() {
    let plugins = vec![
        PluginManifest {
            id: "a".to_string(),
            dependencies: vec!["b".to_string()],
            ..Default::default()
        },
        PluginManifest {
            id: "b".to_string(),
            dependencies: vec!["a".to_string()],
            ..Default::default()
        },
    ];

    let result = resolve_plugin_order(&plugins);
    assert!(matches!(result, Err(PluginError::CircularDependency(_))));
}

#[tokio::test]
async fn test_plugin_timeout_during_init() {
    // Mock a plugin that hangs during init.
    // Verify kernel times out after 10 sec and marks plugin as Error.
    todo!()
}
```

### 11.3 Capability System Tests

```rust
#[test]
fn test_capability_denied() {
    let mut caps = CapabilitySet::new();
    caps.grant(Capability::FileRead);
    
    assert!(caps.has(&Capability::FileRead));
    assert!(!caps.has(&Capability::FileWrite));
}
```

### 11.4 Mock Strategies

- **MockPluginContext:** Implement PluginContext trait for testing plugins in isolation.
- **MockEventBus:** Track published events; allow tests to inject events.
- **In-memory storage:** Use HashMap instead of disk for fast test setup.

---

## 12. Event Versioning & Backward Compatibility

### 12.1 Event Schema Versioning

Events are versioned via the enum structure. If a new field is added to `FileEvent`:
```rust
pub struct FileEvent {
    pub metadata: EventMetadata,
    pub path: PathBuf,
    pub size_bytes: Option<u64>,
    pub mime_type: Option<String>,
    pub owner_uid: Option<u32>,  // NEW FIELD
}
```

Plugins must handle the new field gracefully (it's `Option`, so default is `None`).

### 12.2 Plugin API Versioning

The kernel API (PluginContext trait) is versioned:
```rust
pub const KERNEL_API_VERSION: &str = "1.0.0";
```

Plugins declare their required API version in the manifest:
```toml
kernel_api_version = "1.0.0"
```

The kernel checks compatibility before initializing. If incompatible, the plugin is not loaded.

---

## 13. Plugin Developer Workflow

> **Full template specification:** See [PRD 04a — Plugin Templates](04a-plugin-templates.md)
> for the complete template PRD, including cargo-generate configs, manifest schemas,
> source file specifications, and acceptance criteria. Working template code is in
> `templates/core-plugin/` and `templates/community-plugin/`.

### 13.1 Creating a Plugin

**Option A: Via `nexus plugin scaffold` (recommended)**
```bash
# Community plugin (default)
nexus plugin scaffold --name my-analyzer --author "Jane Doe"

# Core plugin
nexus plugin scaffold --name my-analyzer --type core --author "Jane Doe"
```

**Option B: Via `cargo generate`**
```bash
# Community plugin
cargo generate --path templates/community-plugin
cd my-analyzer

# Core plugin
cargo generate --path templates/core-plugin
cd my-analyzer
```

Both methods produce the same project structure with lifecycle stubs, opaque
`EventSubscription` wiring (§3.4), KV-backed state persistence, cancellation
token handling, and passing tests out of the box.

**Build & Deploy:**
```bash
cargo build --release
cp target/release/my_analyzer ~/.nexus/plugins/
# Kernel auto-discovers and loads on next startup.
```

### 13.2 Plugin Directory Structure

```
my-plugin/
├── Cargo.toml
├── manifest.toml
├── src/
│   ├── lib.rs          # Entry point (create_plugin export)
│   ├── plugin.rs       # PluginLifecycle implementation
│   ├── events.rs       # Event subscription and handling
│   └── state.rs        # KV-backed state persistence
├── tests/
│   ├── lifecycle_test.rs
│   └── events_test.rs
├── assets/
│   ├── icon-light.svg
│   └── icon-dark.svg
└── README.md
```

---

## 14. Debugging Tools & Developer Experience

### 14.1 Event Inspector

A built-in debug command shows real-time events:
```bash
nexus debug events --filter source=my-plugin
# Output:
# 2026-04-11T12:34:56Z [INFO] my-plugin: FileModified { path: "/src/main.rs", ... }
# 2026-04-11T12:34:57Z [INFO] my-plugin: EditorBufferChanged { ... }
```

### 14.2 Plugin State Viewer

```bash
nexus debug plugins
# Output:
# ID              | Version | State       | Capabilities
# my-plugin       | 0.1.0   | Started     | FileRead, EditorAccess
# ai-assistant    | 1.2.0   | Started     | NetworkHttp, AICompletionRequested
```

### 14.3 Capability Audit Log

```bash
nexus debug capabilities --plugin my-plugin
# Output:
# Request              | Result | Timestamp
# FileWrite            | DENIED | 2026-04-11T12:34:00Z
# EditorAccess         | GRANTED (core) | 2026-04-11T12:30:00Z
```

### 14.4 Tracing Integration

All kernel operations are instrumented with `tracing`:
```rust
#[tracing::instrument(skip(ctx))]
async fn handle_event(ctx: &PluginContext, event: &NexusEvent) {
    info!("Handling event: {:?}", event);
    // ...
}
```

Developers can enable debug logging:
```bash
RUST_LOG=nexus=debug nexus
```

---

## 15. Error Messages & User Experience

### 15.1 Plugin Load Failure

**User sees:**
```
ERROR: Plugin 'my-plugin' failed to load.
  Reason: Binary not found at ~/.nexus/plugins/my_plugin
  Hint: Run 'nexus plugin install my-plugin' or ensure the binary is in the plugins directory.
```

### 15.2 Capability Denial

**User sees:**
```
PERMISSION DENIED: Plugin 'my-plugin' requested access to 'FileWrite'.
  Decision: User denied on 2026-04-11T12:35:00Z
  To grant: nexus plugin grant my-plugin --capability FileWrite
```

### 15.3 Plugin Crash

**User sees:**
```
ERROR: Plugin 'ai-assistant' crashed.
  Error: panicked at 'unwrap on None'
  Stack: [backtrace URL]
  Action: Restart with 'nexus plugin restart ai-assistant'
```

---

## 16. Acceptance Criteria

### 16.1 Event Bus
- [ ] Publish 10,000 events/sec with < 1 ms p50 latency.
- [ ] Subscribers can filter events by type, source, or correlation ID.
- [ ] Slow subscribers (lagged > 1,000 events) receive `Lagged` error and can recover.
- [ ] Ring buffer retains 10,000 events and drops oldest on overflow.
- [ ] Event metadata (timestamp, span_id, correlation_id) is correctly propagated.

### 16.2 Plugin Lifecycle
- [ ] Plugin discovery scans `~/.nexus/plugins/` on startup.
- [ ] Topological sort correctly resolves dependency order.
- [ ] Circular dependencies are detected and reported.
- [ ] Plugins timeout after 10 sec during init/start; marked as Error.
- [ ] Plugin panic is caught and isolated; kernel continues.
- [ ] Hot-reload preserves plugin state via key-value store.
- [ ] Dependencies are started before dependents.

### 16.3 Capability System
- [ ] Core plugins have all capabilities.
- [ ] Community plugins can only use whitelisted capabilities.
- [ ] Runtime capability requests trigger user approval dialog (if enabled).
- [ ] Access without capability raises `CapabilityError::Denied` and logs audit entry.
- [ ] Capability escalation is prevented (plugin cannot grant itself new capabilities).

### 16.4 PluginContext API
- [ ] All methods are thread-safe and async.
- [ ] Event subscription returns an opaque `EventSubscription`, not a channel-specific type.
- [ ] Event subscription returns all future events, not historical.
- [ ] File I/O respects capability checks.
- [ ] KV store is persistent across plugin reloads.
- [ ] IPC commands can be registered and called between plugins.
- [ ] Tracing span is available for observability.

### 16.5 Error Handling
- [ ] Event publication error does not crash kernel.
- [ ] Plugin crash does not crash kernel.
- [ ] Event handler panic is isolated to that plugin's task.
- [ ] Kernel gracefully shuts down; all plugins stop within 30 sec.

### 16.6 Testing
- [ ] Unit tests cover event bus, lifecycle, capabilities, dependency resolution.
- [ ] Integration tests cover plugin loading, hot-reload, error scenarios.
- [ ] Test coverage > 80%.

---

## 17. Dependencies

### 17.1 What This Subsystem Depends On

| Dependency | Version | Purpose |
|------------|---------|---------|
| tokio | 1.35+ | Async runtime, broadcast channel, task spawning |
| tracing | 0.1 | Structured logging and distributed tracing |
| serde | 1.0 | Serialization for events and storage |
| uuid | 1.0 | Unique IDs for events and correlation |
| chrono | 0.4 | Timestamps and time utilities |
| notify | 5.0 | Directory watching for file changes |
| rusqlite | 0.29+ | SQLite index management |
| thiserror | 1.0 | Error handling macros |

### 17.2 What Depends On This Subsystem

| Plugin/Subsystem | Dependency Type |
|------------------|-----------------|
| **All plugins** | Hard (PluginContext trait) |
| Editor Plugin | Events (FileModified, EditorBufferChanged) |
| AI Assistant Plugin | Events (AICompletionRequested, AICompletionReceived) |
| Terminal Plugin | Events (TerminalOpened, TerminalOutput, ProcessStarted) |
| File Watcher Plugin | Events (FileCreated, FileDeleted, DirectoryChanged) |
| Workspace Manager | Events (WorkspaceOpened, WorkspaceProjectAdded) |

---

## Conclusion

The Kernel & Event System is the foundational layer of Nexus. Its design prioritizes:
- **Type safety** (Rust enum for events, compile-time verification).
- **Scalability** (async-first, non-blocking broadcast, bounded memory).
- **Isolation** (plugins run in separate tasks; crashes don't propagate).
- **Observability** (event tracing, capability audits, debug tools).

This PRD provides sufficient detail for implementation of the core kernel crate (`nexus-core`) and serves as the specification for all plugins.

---

**Document Version:** 1.0  
**Last Updated:** April 2026  
**Status:** Ready for Implementation
