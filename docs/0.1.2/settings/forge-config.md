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

## `<forge>/.forge/terminal.toml`

`TerminalConfig` — `crates/nexus-terminal/src/config.rs`. Loaded at bootstrap (`crates/nexus-bootstrap/src/plugins/terminal.rs`); a missing file yields a permissive (no-op) policy, so a forge without this file behaves exactly as before.

Today it carries one block, `[spawn]` (`nexus_types::SpawnPolicy`), the **authoritative** env-hygiene + resource-governance default applied to every session the terminal spawns (`create_session`, `run_saved`, `repl_start`).

```toml
[spawn]
# Discard the inherited parent environment entirely; the child starts
# from an empty env plus TERM/COLORTERM and any caller-supplied vars.
clean_env = false
# When non-empty, only inherited keys named here survive (case-insensitive).
env_allowlist = ["PATH", "HOME", "LANG"]
# Inherited keys named here are removed, applied after the allowlist.
env_denylist = ["AWS_SECRET_ACCESS_KEY", "GITHUB_TOKEN"]
# Optional wall-clock runtime budget (seconds). The session is killed
# once it outlives this; omit for no limit. Enforced by the terminal's
# memory poller, so the kill lands within ~one poll interval (~1s) of
# the deadline. Requires the memory monitor to be running (it is in the
# default bootstrap).
timeout_secs = 3600
# Optional CPU-time budget (seconds) — the session is killed once the
# child has CONSUMED this much CPU time (user + system), independent of
# wall-clock. Same poller-based enforcement and single-process caveat as
# the RSS memory cap. Omit for no limit.
cpu_secs = 600
# Best-effort confinement of the child's INITIAL working directory: a
# spawn whose working_dir canonicalizes outside this root is rejected;
# a session with no working_dir spawns here. Omit for no confinement.
root_dir = "/srv/forge"
# Best-effort allowlist of shell programs (matched by exact path or
# basename). A spawn of any other program is rejected. Omit for no
# restriction; an empty list denies every spawn.
command_allowlist = ["bash", "/usr/bin/zsh"]
```

> **Confinement and allowlist are best-effort, not enforcement.** `root_dir` constrains only the starting directory — the child can `cd` out immediately, and the check is TOCTOU. `command_allowlist` gates only the immediate program — an allowed shell can still run anything via `sh -c`, `$(...)`, symlinks, or a wrapper. They stop accidents and casual misuse, not a determined process.

**Authority & precedence.** The forge file is the policy authority. A per-call `create_session` `policy` argument may only ever *tighten* this default via `SpawnPolicy::tighten` (clean_env OR-ed, denylists unioned, allowlists intersected, the shorter `timeout_secs` / `cpu_secs` winning) — an IPC caller can never weaken a forge-mandated restriction. The filter applies to the *inherited* environment only; a session's explicit `env` vars and the service-mandated `TERM`/`COLORTERM` are layered on top afterwards and are exempt.

> **Not a security boundary.** This is env hygiene and resource governance, not isolation: a spawned child can still open sockets and read any file its uid can reach. It exists to stop accidental leakage of parent secrets into child processes and to let a forge mandate a clean environment.

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

Loaders: `AudioConfig::load(forge_root)` — `config.rs:155`; `nexus_bootstrap::collab::load_config(forge_root)` — `collab.rs:100`; `nexus_bootstrap::load_digest_config(forge_root)` — `lib.rs:488`; `nexus_bootstrap::load_webhook_config(forge_root)` — `lib.rs:923`. Missing file / missing block ⇒ defaults. The `enabled = false` default for both `[digests]` and `[webhooks]` keeps the cron loop and HTTP listener dormant; manual `com.nexus.workflow::run_digest` IPC calls still work regardless.

## `<forge>/.nexus/config.toml`

`KernelConfig` — `crates/nexus-kernel/src/config.rs:12`.

```toml
forge_root = "/path/to/forge"          # set at boot; informational
event_bus_capacity = 2048              # tokio broadcast channel capacity
plugin_search_paths = []               # additional dirs scanned by the plugin loader
hot_reload_enabled = true              # plugin file-watcher
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
