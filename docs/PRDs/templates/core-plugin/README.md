# {{plugin-name}}

{{description}}

## Development

```bash
# Build
cargo build --release

# Run tests
cargo test

# Lint
cargo clippy

# Start hot-reload dev server
cargo nexus-plugin-dev
```

## Project Structure

```
src/
├── lib.rs      — Entry point (create_plugin export)
├── plugin.rs   — PluginLifecycle implementation
├── events.rs   — Event subscription and handling
└── state.rs    — KV-backed state persistence
```

## Plugin Lifecycle

This plugin follows the Nexus plugin lifecycle state machine:

1. **on_load()** — Binary loaded into memory. Minimal initialization only.
2. **on_init(ctx)** — Dependencies ready. Restore state, subscribe to events, register IPC.
3. **on_start()** — Plugin is live. Begin active work.
4. **on_stop()** — Graceful shutdown. Persist state (5s timeout).
5. **on_shutdown()** — Final cleanup. Release resources (2s timeout).

## Event Handling

Edit `src/events.rs` to add handlers for the `NexusEvent` variants your plugin
needs. The template uses `EventSubscription` (an opaque stream type) so your
handlers are compatible with future event bus changes.

## State Persistence

Edit `src/state.rs` to add fields that should survive hot-reloads. State is
automatically saved to the kernel KV store on stop and restored on init.
