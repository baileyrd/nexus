# Plugin Manifests

> `plugin.toml` is the contract between a plugin and the kernel. Schema source: `crates/nexus-plugins/src/manifest.rs:15-540`. The shell parses a parallel `plugin.json` shape for community/JS plugins — schema at `shell/src-tauri/src/lib.rs:100-134`.

## `plugin.toml` — full schema

```toml
id = "com.example.my-plugin"           # reverse-DNS plugin id; must match folder name
name = "My Plugin"                     # display name
version = "0.1.0"                      # semver
trust_level = "community"              # "core" | "community"; community manifests are rejected with trust_level="core"
api_version = "0.1"                    # minimum kernel API version

runtime = "wasm"                       # "native" | "wasm" | "script"

[capabilities]
required = ["fs.read", "kv.read"]      # must-have or load fails
optional = ["net.http"]                # opt-in at install time

[wasm]                                 # required if runtime="wasm"
module = "plugin.wasm"
memory_mb = 16                         # default 16
fuel = 10_000_000                      # wasmtime fuel; default 10_000_000
max_execution_ms = 5000                # epoch-deadline per call; default 5000

[script]                               # required if runtime="script"
entry = "index.js"

[settings]
schema = "settings.json"               # path to JSON Schema; validated by SettingsManager

[lifecycle]                            # hook enablement flags
on_init = true
on_start = true
on_stop = true
on_enable = false
on_disable = false
on_settings_changed = false

[activation]                           # lazy-activation (script plugins)
on_command = ["my.command"]
on_uri_scheme = ["myscheme:"]
on_file_open = ["*.foo"]
# … see manifest.rs:545-565 for full ActivationConfig

[[dependencies]]                       # other plugins this one needs loaded first
plugin_id = "com.nexus.storage"
version_req = ">=0.1, <0.2"

[signature]                            # ed25519 signature (BL-099); required when KernelConfig.require_signatures = true
public_key = "base64..."
signature = "base64..."

# ── Registrations: every extension point ────────────────────────────────
[registrations]
cli_subcommands = [...]
ipc_commands = [...]
event_subscribers = [...]
ui_commands = [...]
ui_panels = [...]
ui_settings_tabs = [...]
ui_ribbon_items = [...]
ui_status_items = [...]
slash_commands = [...]
menu_items = [...]
uri_handlers = [...]
protocol_hosts = { dap = [...], lsp = [...], mcp = [...], acp = [...] }
```

Field-level reference is in the `PluginManifest`, `PluginRuntime`, `ManifestCapabilities`, `WasmConfig`, `ScriptConfig`, `SettingsConfig`, `LifecycleConfig`, `ActivationConfig`, `Registrations`, `PluginDependency`, `PluginSignature`, `ProtocolHostsContribution` types in `manifest.rs`.

## `plugin.json` — shell-side community plugin manifest

Shell-side schema: `CommunityPluginManifest` — `shell/src-tauri/src/lib.rs:100-134`.

```json
{
  "id": "com.example.my-plugin",
  "name": "My Plugin",
  "version": "0.1.0",
  "main": "index.js",
  "enabled": true,
  "description": "...",
  "author": "...",
  "api_version": "0.1",
  "capabilities": ["fs.read"]
}
```

Injected at scan time (not authored by plugin author):
- `dir` — absolute plugin directory
- `manifest_path` — absolute path to `plugin.json`
- `verification_status` — `"unverified" | "verified" | "tampered"` based on signature check

Lives at `~/.nexus-shell/plugins/<plugin>/plugin.json`. Scanned by `scan_plugin_directory` Tauri command.

## Granted capability state (per-plugin)

`<plugin_dir>/granted_caps.json` — `shell/src-tauri/src/lib.rs:364`:

```json
{
  "version": "0.1.0",
  "granted": ["fs.read", "net.http"]
}
```

Sealed with `chacha20poly1305` per BL-101; key in OS keyring under `nexus-shell:<plugin_id>:granted-caps`. Use `NEXUS_NO_KEYRING=1` to disable sealing (development only).

## Registrations — every extension point

`Registrations` — `manifest.rs:158-189`.

| Field | Type | What it does |
|-------|------|---------------|
| `cli_subcommands` | `Vec<CliSubcommand>` | adds `nexus <subcommand>` |
| `ipc_commands` | `Vec<IpcCommand>` | this plugin's IPC handlers |
| `event_subscribers` | `Vec<EventSubscriber>` | what topics this plugin listens to |
| `ui_commands` | `Vec<UiCommand>` | command palette entries |
| `ui_panels` | `Vec<UiPanel>` | shell panes (sidebar / right / bottom) |
| `ui_settings_tabs` | `Vec<UiSettingsTab>` | tabs in the settings modal |
| `ui_ribbon_items` | `Vec<UiRibbonItem>` | activity-bar icons |
| `ui_status_items` | `Vec<UiStatusItem>` | status-bar pieces |
| `slash_commands` | `Vec<SlashCommand>` | editor slash menu |
| `menu_items` | `Vec<MenuItem>` | context / app menus |
| `uri_handlers` | `Vec<UriHandler>` | `<scheme>:` deeplinks |
| `protocol_hosts` | `ProtocolHostsContribution` | DAP/LSP/MCP/ACP adapter contributions (BL-113) |

`ProtocolHostsContribution` (`manifest.rs:196-221`) is gated by `protocol.host.contribute` at the host's `register_*` handler — only invoker plugins can drive it.

## Manifest invariants enforced at load

- `id` must be reverse-DNS, must match folder name (`shell/src-tauri/src/lib.rs::assertValidPluginId`).
- `trust_level = "core"` rejected for any manifest the community loader sees.
- `wasm` config required iff `runtime = "wasm"`.
- `script` config required iff `runtime = "script"`.
- `signature` required iff `KernelConfig.require_signatures = true`.
- Per-kind `registrations` list capped at 1024 entries (`manifest.rs:1765`).
- Dependency `version_req` parsed with `semver` — invalid ⇒ load failure.
