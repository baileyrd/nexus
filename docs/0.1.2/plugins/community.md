# Community Plugins

> 3rd-party plugins loaded at runtime. WASM-sandboxed via wasmtime (ADR 0016) or JS-sandboxed via iframe (ADR 0015). Capability-gated at every kernel-mediated call.

## Three runtime variants

`PluginRuntime` enum (`crates/nexus-plugins/src/manifest.rs`):

- `Native` — Rust core plugin. Community plugins **cannot** declare this; reserved for in-tree.
- `Wasm` — `.wasm` module loaded into a `wasmtime::Store` with fuel + epoch-deadline + memory cap. Calls Nexus through `host_fns.rs` imports.
- `Script` — JS module loaded into an iframe `WebView` by the shell. Calls Nexus through the `@nexus/extension-api` SDK.

## WASM sandbox (`crates/nexus-plugins/src/sandbox.rs`)

```
wasmtime::Engine
    │
    └── wasmtime::Store
            │
            ├── PluginData (host-side state: pluginId, capabilities, kv handle)
            ├── PluginEventForwarder (kernel-bus subscriptions)
            └── fuel + epoch_deadline (per-call budget)
```

Each WASM call is fueled — at fuel exhaustion the call traps. Each call also runs under an epoch deadline (`manifest.wasm.max_execution_ms`, default 5000ms). Host imports exposed (`host_fns.rs`):

- `ipc_call(plugin_id, command, args, args_len, out, out_len)` → integer status
- `publish_event(topic, payload, payload_len)` → integer status
- `log(level, msg, msg_len)` → void
- `kv_get(key, key_len, out, out_len)` → integer status
- `kv_set(key, key_len, value, value_len)` → integer status
- … plus capability check + error code helpers

Negative status codes for host errors:
- `-1001` = `HOST_CAPABILITY_DENIED`
- `-1002` = `HOST_BUFFER_OVERFLOW`

## JS / Script sandbox (`shell/src/host/sandbox/`)

Iframe-based per ADR 0015. The host owns the iframe; the plugin gets a `PluginAPI` proxy from `@nexus/extension-api`:

```typescript
import { definePlugin } from '@nexus/extension-api';

export default definePlugin({
  id: 'com.example.my-plugin',
  activate(ctx) {
    ctx.ipc.call('com.nexus.storage', 'read_file', { path: 'notes/foo.md' })
       .then((value) => console.log(value));

    ctx.events.subscribe('com.nexus.git.state', (event) => { ... });
  },
});
```

The orchestrator-assigned `pluginId` is bound at handshake time per F-8.1.2 — a plugin **cannot** spoof a different id by passing one through; `assertValidPluginId` rejects empty / colon-bearing ids on every host-side call.

## Install flow

1. User downloads `<plugin>/` directory with `plugin.toml` + `plugin.wasm` (or `plugin.json` + `index.js` for JS) into `<forge>/.forge/plugins/` (WASM) or `~/.nexus-shell/plugins/` (JS).
2. `PluginLoader::load(plugin_dir)` parses the manifest. Rejected if `trust_level = "core"`.
3. Capability check UI shows the user `manifest.capabilities.required` and `optional`, with HIGH-risk verbs highlighted.
4. User grants a subset → sealed under `chacha20poly1305` and persisted to `<plugin_dir>/granted_caps.json` (key in OS keyring; `NEXUS_NO_KEYRING=1` to bypass for development).
5. Plugin loads. Lifecycle hooks fire per `manifest.lifecycle` config: `on_init` → `on_start` → (running) → `on_stop`.

## At runtime

Every `ipc_call` from the plugin checks:
1. `ipc.call` capability (unconditional).
2. The handler's `cap_matrix.toml` row — either `caps = [...]` (caller must hold all) or `unrestricted` (no extra check). Any args-aware `policy = "name"` runs after.
3. Per-call audit log entry (`com.nexus.security::query_audit_log` to inspect).

Fuel + epoch checks happen per-call inside the sandbox.

## Hot reload

`PluginLoader::hot_reload` watches plugin directories (`notify-debouncer-mini`). On change:
1. `on_stop` the current instance.
2. Parse the new manifest. If invalid → rollback to last-good (`hot_reload.rs`).
3. Re-instantiate sandbox. If load throws → rollback.
4. `on_init` + `on_start` the new instance.

A crash quarantine counter is incremented per failure; after 3 crashes the plugin is disabled until manually re-enabled.

**Reaching it (C80).** The machinery above ships with `hot_reload: true` as `PluginManagerConfig`'s default, but every production frontend used to override it to `false` (`App::plugins()`, `crates/nexus-cli/src/app.rs`) or never constructed a `PluginManager` with a watcher at all — the mechanism was real but unreachable. `nexus plugin dev <dir>` (`crates/nexus-cli/src/commands/plugin.rs`) is the fix: a long-running CLI session that builds its own `hot_reload: true` manager rooted at `<dir>` (same one-subdirectory-per-plugin layout as `.forge/plugins/`), loads everything found, and polls `poll_reloads()` every 250ms until Ctrl+C, printing a line per hot-swap. It's deliberately a standalone session — no live forge kernel/storage boot required — so `nexus plugin dev ~/my-plugin-workspace` works the same regardless of `--forge-path`.

This only covers WASM community plugins, where `reload_plugin` can safely rebuild the whole sandbox from scratch (no live state to preserve). Script (JS) plugins run in-realm with commands/subscriptions/DOM already registered by `activate()`, and the shell has no general mechanism to unwind that — so the shell-side half of C80 is narrower: the Plugins modal's **Rescan** button (`shell/src/host/communityPluginLoader.ts::rescanCommunityPlugins`, replacing the old "drop a folder and restart" hint) discovers and activates *newly dropped* community plugins without a restart, but does not attempt to hot-swap an already-loaded plugin's changed code — editing an existing script plugin still needs a reload.

The forge-scoped `hot_reload_enabled` setting (`<forge>/.forge/app.toml`, see [`settings/forge-config.md`](../settings/forge-config.md)) remains unwired — there's no clean place to route it into the CLI dev-mode path, which is deliberately forge-independent. Not fixed by C80; flagged here so it doesn't read as shipped.

## Signing (BL-099)

Manifests can carry an ed25519 signature:

```toml
[signature]
public_key = "base64..."
signature  = "base64..."
```

When `KernelConfig.require_signatures = true`, unsigned manifests are rejected at load. Public keys are matched against a trusted-keys list maintained by the marketplace (not yet wired — manual at v0.1.2).

## Authoring quickstart

```bash
nexus plugin scaffold --type wasm com.example.my-plugin
# emits plugin.toml + Rust skeleton; cargo build to .wasm
```

For JS:

```bash
nexus plugin scaffold --type script com.example.my-plugin
# emits plugin.json + index.ts with @nexus/extension-api import
```

Reference manifests: `crates/nexus-plugins/src/manifest.rs` (Rust types), [`../settings/plugin-manifests.md`](../settings/plugin-manifests.md) (TOML schema).
