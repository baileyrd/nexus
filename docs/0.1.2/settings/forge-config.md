# Forge Config Files

> Every TOML/JSON file under `<forge>/.forge/` and `<forge>/.nexus/` that the code reads as config. Each is rebuilt from defaults if absent; none should be hand-edited at runtime while the kernel is booted (use the relevant `settings_write` / IPC handler to mutate).

## `<forge>/.forge/app.toml`

`AppConfig` — `crates/nexus-formats/src/config/app.rs:9`.

```toml
[core]              # CoreSettings
[editor]            # EditorSettings
[preview]           # PreviewSettings
[search]            # SearchSettings
[plugins]           # PluginSettings
[git]               # GitSettings — adds (P2-06) poll_interval_secs + auto_commit_tick_secs overrides
[dream_cycle]       # DreamCycleSettings

[settings]          # BTreeMap<String, toml::Value> — flat key→value bag
# any plugin can stash typed values here
```

Loader: `load_app_config(forge_root)` — `nexus-formats/src/config/mod.rs:38`. Missing file ⇒ `AppConfig::default()` (line 113).

### `[settings]` cascade keys (P2 cascades)

The shell's `configStore` mirrors the `[settings]` table verbatim into a per-forge in-memory store. These keys are produced/consumed by Phase 2 cascades:

| Key | Value | Owner | Notes |
|-----|-------|-------|-------|
| `nexus.keybindings.overrides` | `{ [commandId]: chord }` | shell `KeybindingRegistry` | P2-01 — runtime keybinding overrides; localStorage cache mirrors per-machine, app.toml is portable. |
| `nexus.priority.<scope>.<entryId>` | `number` | shell registries | P2-02 — scope ∈ `slot` / `activityBar` / `panelArea` / `statusBar`. Lower = earlier in the sort. Live re-sort on change. |
| `nexus.bases.fileExtensions` | `string[]` | `nexus.bases` plugin | P2-03 — bare extensions (no leading dot). |
| `nexus.canvas.fileExtensions` | `string[]` | `nexus.canvas` plugin | P2-03 |
| `nexus.editor.fileExtensions` | `string[]` | `nexus.editor` plugin | P2-03 |

## `<forge>/.forge/workspace.json`

`WorkspaceState` — `crates/nexus-formats/src/config/workspace.rs:8`.

```json
{
  "active_file": "notes/foo.md",
  "open_files": [{"path": "...", "scroll": 0}],
  "sidebar_collapsed": false,
  "panel_layout": "default",
  "recent_files": ["..."],
  "search_query": "",
  "theme": "nexus-dark"
}
```

Loader: `load_workspace_state(forge_root)` — `config/mod.rs:56`.

## `<forge>/.forge/ai.toml`

`AiConfig` — `crates/nexus-ai/src/config.rs:10`.

```toml
provider = "anthropic"             # "anthropic" | "openai" | "ollama" | "llama-cpp" — see config.rs:81 for env-driven detection
model = "claude-sonnet-4-6"        # default in nexus-formats/src/config/ai.rs:35
api_key = "${ANTHROPIC_API_KEY}"   # env-substituted
base_url = "https://api.anthropic.com"
max_tokens = 4096                  # default 4096
context_window = 8192              # default 8192
reserved_response_tokens = 1024    # default 1024
privacy = "off"                    # PrivacyPolicy: Off | RedactPii | LocalOnly
injection_policy = "off"           # InjectionPolicy: Off | OnDemand | Always
local_embedding_model = "bge-small-en-v1.5-int8"   # via fastembed
tls_pinning_enabled = false        # opt-in BL-102

# P2-04 — per-provider defaults used when `model` is unset.
# Omit any field to fall back to the provider's built-in constant.
anthropic_model        = "claude-sonnet-4-20250514"   # nexus_ai::anthropic::DEFAULT_MODEL
openai_chat_model      = "gpt-4o"                     # nexus_ai::openai::DEFAULT_CHAT_MODEL
openai_embedding_model = "text-embedding-3-small"     # nexus_ai::openai::DEFAULT_EMBEDDING_MODEL
ollama_chat_model      = "llama3.2"                   # nexus_ai::ollama::DEFAULT_CHAT_MODEL
ollama_embedding_model = "nomic-embed-text"           # nexus_ai::ollama::DEFAULT_EMBEDDING_MODEL
ollama_temperature     = 0.2                          # FIM `/api/generate` — nexus_ai::ollama::DEFAULT_FIM_TEMPERATURE
ollama_base_url        = "http://localhost:11434"     # P2-05 — nexus_ai::ollama::DEFAULT_BASE_URL
indexing_debounce_secs = 2                            # P2-06 — IndexingDaemon debounce window; nexus_ai::indexing_daemon::DEFAULT_DEBOUNCE
```

Defaults via `serde(default = …)` helpers in the Config struct. Loader: `load_ai_config(forge_root)` — `config/mod.rs:92`.

## `<forge>/.forge/mcp.toml`

`McpHostConfig` — `crates/nexus-mcp/src/config.rs:144`.

```toml
[servers.<name>]                    # BTreeMap<String, McpServerSpec> — ordered for stable serialization
command = "/path/to/mcp-server"
args = ["--option", "value"]
env = { KEY = "value" }
working_dir = "/optional"
transport = "stdio"                 # also "streamable-http" with `url = "..."`

[timeouts]                          # P2-06 — optional per-field overrides; each falls back to nexus_mcp::{client,server,auth}::DEFAULT_*
connect_secs  = 15                  # rmcp initialize handshake
shutdown_secs = 5                   # graceful close before child SIGKILL
ipc_secs      = 30                  # inbound IPC into kernel-side plugins
ai_ipc_secs   = 120                 # inbound IPC into ai/* handlers
oauth_secs    = 30                  # OAuth token POST

[contributed_by]                    # HashMap<server_name, plugin_id> — runtime-only
"some.server" = "com.example.plugin"
```

Loader: `McpHostConfig::read_from(path)` — line 221. Missing file ⇒ empty config.

## `<forge>/.forge/lsp.toml` / `dap.toml` / `acp.toml`

Same shape — host config registry of named server/adapter specs + a `contributed_by` map for BL-113 plugin contributions.

- LSP: `LspHostConfig` — `crates/nexus-lsp/src/config.rs:106`. Each spec carries command/args/env/file-types/initialization options.
- DAP: `DapHostConfig` — `crates/nexus-dap/src/config.rs:115`. Each spec carries adapter command/args/env.
- ACP: `AcpHostConfig` — `crates/nexus-acp/src/config.rs:81`. Each spec carries agent command/args/env.

## `<forge>/.forge/notifications.toml`

`NotificationsConfig` — `crates/nexus-notifications/src/config.rs:272`.

```toml
[sources.<id>]                       # BTreeMap<String, SourceConfig>
# Which kernel-bus topics trigger which channels

[channels.desktop]
enabled = true

[channels.discord]
webhook_url = "..."

[channels.telegram]
bot_token = "..."
chat_id = "..."
max_bytes = 4096                      # optional override — default see nexus-notifications/src/lib.rs::DEFAULT_TELEGRAM_MAX_BYTES

[channels.email]
smtp_host = "..."
smtp_port = 587
smtp_user = "..."
smtp_pass = "${SMTP_PASS}"
from = "nexus@example.com"
to = ["alerts@example.com"]

[channels.webhook]                    # C90 / #443 — generic HTTP POST (Slack/ntfy/Gotify/Matrix/any)
url = "https://hooks.slack.example/services/x"
body_template = '{"text": "{title}: {message}"}'   # optional — {title}/{message} JSON-escaped on substitution; unset ⇒ {"title","message"}

[channels.webhook.headers]            # optional — extra HTTP headers, e.g. an auth token
Authorization = "Bearer ${WEBHOOK_TOKEN}"

[inbox]
max_rows = 1000                       # default — see nexus-notifications/src/inbox.rs:47
max_age_days = 30                     # default — line 50
```

Loader: `NotificationsConfig::load` — `config.rs:300`.

## `<forge>/.forge/config.toml`

Shared TOML file read by several subsystems for blocks they own. Each block is optional; absent ⇒ defaults.

```toml
[audio]                                                 # AudioConfig — crates/nexus-audio/src/config.rs:64
stt_backend = "platform"                                # "local" | "provider" | "platform" (default: platform)
tts_backend = "platform"
local_model_size = "base.en"
local_model_dir = "/abs/path"                           # default: <forge>/.forge/.audio/models
provider_api_key = ""                                   # falls back to OPENAI_API_KEY
provider_base_url = ""                                  # default: nexus_audio::provider_backend::DEFAULT_BASE_URL
provider_stt_model = "whisper-1"
provider_tts_model = "tts-1"
provider_tts_voice = "alloy"
whisper_model_url = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{size}.bin"  # P2-05 — template; {size} substituted at download
creds_lookup_timeout_secs = 2                           # P2-06 — com.nexus.ai::resolve_credentials deadline

[collab]                                                # CollabConfig — crates/nexus-bootstrap/src/collab.rs:61
enabled = false
relay_url = "ws://relay.example:7700/"                  # required when enabled = true
token = "shared-secret"
peer_id = "alice@laptop"
display_name = "Alice"
initial_delay_ms = 1000                                 # ReconnectConfig.initial_delay override
max_delay_ms = 30000                                    # ReconnectConfig.max_delay override (P2-06: collab.backoff_max)
backoff_factor = 2.0                                    # P2-06 — ReconnectConfig.backoff_factor override
buffer_capacity = 256                                   # ReconnectConfig.buffer_capacity override
handshake_timeout_secs = 10                             # P2-06 forward-compat — pending ReconnectingClient knob

[memory]                                                # MemoryConfig — crates/nexus-bootstrap/src/memory_capture.rs (C37, #390)
capture_enabled = true                                  # master switch for the passive bus-capture pump; default true (preserves pre-C37 always-on behavior)
capture_exclude_plugins = ["com.nexus.terminal"]        # source-plugin-id prefixes to skip capturing; default [] (capture everything)
event_retention_max_rows = 20000                        # prune captured (source="event") rows beyond this count, oldest first; default null = unbounded

[digests]                                               # DigestConfig — crates/nexus-workflow/src/digests.rs:56
enabled = false                                         # master switch; cron loop only fires when true
daily_cron = "0 7 * * *"                                # 5-field cron; null disables daily
weekly_cron = "0 7 * * 1"                               # 5-field cron; null disables weekly
scope_path = "Inbox"                                    # forge-relative subtree; null = whole forge
digests_dir = "Digests"                                 # forge-relative output dir

[webhooks]                                              # WebhookConfig — crates/nexus-workflow/src/webhook.rs:61
enabled = false                                         # master switch; HTTP listener only spawns when true
bind = "127.0.0.1:18080"                                # host:port
```

Loaders: `AudioConfig::load(forge_root)` — `config.rs:155`; `nexus_bootstrap::collab::load_config(forge_root)` — `collab.rs:100`; `nexus_bootstrap::memory_capture::load_config(forge_root)` — `memory_capture.rs` (C37, #390 — private to the module, read internally by `start_capture`); `nexus_bootstrap::load_digest_config(forge_root)` — `lib.rs:488`; `nexus_bootstrap::load_webhook_config(forge_root)` — `lib.rs:923`. Missing file / missing block ⇒ defaults. The `enabled = false` default for both `[digests]` and `[webhooks]` keeps the cron loop and HTTP listener dormant; manual `com.nexus.workflow::run_digest` IPC calls still work regardless. `[memory].capture_enabled` defaults `true` instead — unlike those two, passive memory capture has run unconditionally since it shipped, so the default preserves existing behavior rather than introducing an opt-in gate.

## `<forge>/.nexus/config.toml`

`KernelConfig` — `crates/nexus-kernel/src/config.rs:12`.

```toml
forge_root = "/path/to/forge"          # set at boot; informational
event_bus_capacity = 2048              # tokio broadcast channel capacity
plugin_search_paths = []               # additional dirs scanned by the plugin loader
hot_reload_enabled = true              # plugin file-watcher — parsed but not read by anything (C80); use `nexus plugin dev <dir>` instead, see plugins/community.md#hot-reload
lifecycle_timeout_secs = 30            # on_init / on_start budget per plugin
tls_pinning_enabled = false            # global TLS pinning kill-switch
require_signatures = false             # reject unsigned community plugins
```

Loader: `KernelConfig::load(forge_root)` — line 79. Missing file ⇒ defaults (forge_root injected).

## Plugin-side per-plugin settings

Each plugin can declare a JSON Schema via `[settings] schema = "settings.json"` in its `plugin.toml` (see [`plugin-manifests.md`](plugin-manifests.md)). Persisted to `<plugin_dir>/settings.json`. Validated on write by `SettingsManager` (`crates/nexus-plugins/src/settings.rs:42`).

## Shell-side state

`ShellState` — `shell/src-tauri/src/persistence.rs:50`. Path: `<app_config_dir>/shell-state.json`.

```json
{
  "version": 1,
  "last_forge_path": "/path/to/forge",
  "recent_forge_paths": ["..."],
  "remote_forge_recents": [{"uri": "ssh://...", "label": "..."}]
}
```

- `recent_forge_paths` capped at 8 (`persistence.rs:29`).
- `remote_forge_recents` capped at 8 (`persistence.rs:32`).
- Read/write via Tauri commands `get_shell_state` / `save_shell_state` / `write_last_forge_path` / `forget_forge_path`.
