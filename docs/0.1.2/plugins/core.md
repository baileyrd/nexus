# Core Plugins

> Native Rust plugins registered at bootstrap. Full access — no WASM sandbox, no community capability gate. Authored in `crates/nexus-<service>/` and wired into the kernel via `crates/nexus-bootstrap/src/plugins/mod.rs`.

## CorePlugin trait

Defined in `crates/nexus-plugins/src/loader.rs`. Service crates implement this trait on a `*CorePlugin` struct (one per crate that registers IPC).

```rust
pub trait CorePlugin: Send + Sync {
    fn on_init(&mut self) -> Result<()> { Ok(()) }
    fn on_start(&mut self) -> Result<()> { Ok(()) }
    fn on_stop(&mut self) {}
    fn on_enable(&mut self) -> Result<()> { Ok(()) }
    fn on_disable(&mut self) {}
    fn on_settings_changed(&mut self, _settings: &Value) -> Result<()> { Ok(()) }

    fn dispatch(&mut self, handler_id: u32, args: &Value) -> Result<Value> {
        Err(PluginError::HandlerIsAsyncOnly { handler_id })
    }
    fn dispatch_async(&mut self, handler_id: u32, args: &Value)
        -> Option<CorePluginFuture> { None }
    fn wire_context(&mut self, _ctx: Arc<KernelPluginContext>) {}
}
```

- Every method has a default — plugins implement only what they actually surface.
- `dispatch` — synchronous IPC handler. Default returns `PluginError::HandlerIsAsyncOnly { handler_id }`, so async-only plugins can omit it entirely.
- `dispatch_async` — futures-returning handler; override for handlers that need `await`. The dispatcher tries `dispatch_async` first and falls back to sync.
- `wire_context` — kernel calls this exactly once with a per-plugin `KernelPluginContext` that the plugin uses for nested `ipc_call`, `emit_event`, `settings()`.

## 23 in-tree core plugins

Listed in registration order (the order matters for deterministic boot). Source of truth: `crates/nexus-bootstrap/src/plugins/mod.rs::register_all`.

1. **com.nexus.security** (`crates/nexus-security/`) — keyring vault, audit log, TLS pinning
2. **com.nexus.storage** (`crates/nexus-storage/`) — file-as-truth, SQLite, Tantivy, watcher, graph, bases SQL
3. **com.nexus.database** (`crates/nexus-database/`) — pure-compute formulas/views (no SQL)
4. **com.nexus.editor** (`crates/nexus-editor/`) — block-tree + CM6 sessions
5. **com.nexus.theme** (`crates/nexus-theme/`) — CSS variable cascade
6. **com.nexus.ai.runtime** (`crates/nexus-ai-runtime/`) — task scheduler / observation. Registered before `ai` so the shared tokio pool handle is published in time for `ai`'s indexing daemon.
7. **com.nexus.ai** (`crates/nexus-ai/`) — provider traits, RAG, tool loop
8. **com.nexus.skills** (`crates/nexus-skills/`) — `.skill.md` registry
9. **com.nexus.templates** (`crates/nexus-templates/`) — `.template.md` registry
10. **com.nexus.formats** (`crates/nexus-formats/`) — pure markdown/canvas/notion parsing
11. **com.nexus.workflow** (`crates/nexus-workflow/`) — `.workflow.toml` + triggers
12. **com.nexus.linkpreview** (`crates/nexus-linkpreview/`) — OG/Twitter-card fetch
13. **com.nexus.notifications** (`crates/nexus-notifications/`) — multi-channel + inbox
14. **com.nexus.audio** (`crates/nexus-audio/`) — STT/TTS providers
15. **com.nexus.comments** (`crates/nexus-comments/`) — block-anchored threads
16. **com.nexus.agent** (`crates/nexus-agent/`) — archetypes, plan/run, transcripts
17. **com.nexus.mcp.host** (`crates/nexus-mcp/`) — external MCP server connections
18. **com.nexus.lsp** (`crates/nexus-lsp/`) — LSP server host
19. **com.nexus.dap** (`crates/nexus-dap/`) — DAP adapter host
20. **com.nexus.acp** (`crates/nexus-acp/`) — ACP agent host
21. **com.nexus.git** (`crates/nexus-git/`) — libgit2 over forge root
22. **com.nexus.terminal** (`crates/nexus-terminal/`) — PTY sessions, saved/ad-hoc commands, REPL
23. **com.nexus.collab** (`crates/nexus-collab/`) — WebSocket relay. Registered last so every preceding plugin's events are available to the relay bridge.

## Authoring a new core plugin (in-tree)

1. Add `nexus-<feature>` to the workspace `Cargo.toml` `members`.
2. Implement `CorePlugin` on a struct in `crates/nexus-<feature>/src/core_plugin.rs`.
3. Declare the IPC surface as an `IPC_HANDLERS: &[(&str, u32)]` slice on the crate. The manifest is built inline at bootstrap by `core_manifest_with_ipc(...)` in `crates/nexus-bootstrap/src/plugins/mod.rs` from this slice — no `plugin.toml` file or `MANIFEST_TOML` string is required for core plugins.
4. Register from `crates/nexus-bootstrap/src/plugins/mod.rs` in the right order (storage must come before any plugin that uses it during `on_init`).
5. Add `[[handler]]` rows for every IPC handler to `crates/nexus-bootstrap/cap_matrix.toml`. `cap_matrix_complete --ignored` will catch any you missed.
6. If you introduce a new boundary type, add the `ts-export` feature and a row in `scripts/check_ipc_drift.sh`.
7. Update `dep_invariants.rs` if your crate must be inaccessible to a particular invoker.

## Trust posture

Core plugins are granted `Capability::ALL` automatically — they are part of the trusted base. The check happens at `nexus-kernel::IpcDispatcher::dispatch` based on `manifest.trust_level == TrustLevel::Core`. This is why community plugins cannot impersonate a core plugin: a community manifest declaring `trust_level = "core"` is rejected at load (`PluginLoader::load`).
