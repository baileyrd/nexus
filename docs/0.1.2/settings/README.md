# Settings Reference

> **As of:** 2026-05-21. Sources: `crates/nexus-formats/src/config/`, `crates/nexus-kernel/src/config.rs`, `crates/nexus-ai/src/config.rs`, `crates/nexus-mcp/src/config.rs`, `crates/nexus-notifications/src/config.rs`, `crates/nexus-lsp,dap/src/config.rs`, `crates/nexus-plugins/src/manifest.rs`, `crates/nexus-plugins/src/settings.rs`, `shell/src-tauri/src/persistence.rs`. (ACP is greenfield — no config loader per ADR 0027 §Phase 4.)

This directory documents **every** configuration surface in Nexus. The promise: if a setting exists at runtime, it's listed here. If it should be a setting but isn't yet, it's in [`hardcoded-rust.md`](hardcoded-rust.md) or [`hardcoded-shell.md`](hardcoded-shell.md) with a suggested key.

## Index

| File | Scope |
|------|-------|
| [`forge-config.md`](forge-config.md) | Per-forge TOML/JSON files under `<forge>/.forge/` — `app.toml`, `workspace.json`, `ai.toml`, `mcp.toml`, `notifications.toml`, `lsp.toml`, `dap.toml`, `config.toml` (multi-section: `[audio]` / `[collab]` / `[digests]` / `[notifications.<channel>]` / `[mcp]`) |
| [`plugin-manifests.md`](plugin-manifests.md) | `plugin.toml` schema for community plugins (and the parallel `plugin.json` shape the shell expects) |
| [`env-vars.md`](env-vars.md) | Every env var the code reads at runtime |
| [`hardcoded-rust.md`](hardcoded-rust.md) | Rust-side hardcoded values flagged for promotion to settings or named constants |
| [`hardcoded-shell.md`](hardcoded-shell.md) | Shell-side hardcoded values flagged for promotion to settings or named constants |
| [`plugin-manifest-defaults.md`](plugin-manifest-defaults.md) | Defaults baked into plugin manifests — backend `MANIFEST_TOML`, scaffolds, shell `definePlugin` keybindings/priorities/schema defaults |

## Categories at a glance

### Persistent config files

Every TOML/JSON file the code reads at runtime as config (not test fixtures, not Cargo.toml). All deserialize via serde; all run `${ENV_VAR}` substitution before parse (`crates/nexus-formats/src/config/env_subst.rs`).

| File | Path | Struct (file:line) | Loader |
|------|------|---------------------|--------|
| **app.toml** | `<forge>/.forge/app.toml` | `AppConfig` — `nexus-formats/src/config/app.rs:9` | `load_app_config` — `config/mod.rs:38` |
| **workspace.json** | `<forge>/.forge/workspace.json` | `WorkspaceState` — `nexus-formats/src/config/workspace.rs:8` | `load_workspace_state` — `config/mod.rs:56` |
| **ai.toml** | `<forge>/.forge/ai.toml` | `AiConfig` — `nexus-ai/src/config.rs:10` / `nexus-formats/src/config/ai.rs` | `load_ai_config` — `config/mod.rs:92` |
| **mcp.toml** | `<forge>/.forge/mcp.toml` | `McpHostConfig` — `nexus-mcp/src/config.rs:144` | `McpHostConfig::read_from` — `config.rs:221` |
| **notifications.toml** | `<forge>/.forge/notifications.toml` | `NotificationsConfig` — `nexus-notifications/src/config.rs:272` | `NotificationsConfig::load` — `config.rs:300` |
| **lsp.toml** | `<forge>/.forge/lsp.toml` | `LspHostConfig` — `nexus-lsp/src/config.rs:106` | `LspHostConfig::load` |
| **dap.toml** | `<forge>/.forge/dap.toml` | `DapHostConfig` — `nexus-dap/src/config.rs:115` | `DapHostConfig::load` |
| **config.toml** | `<forge>/.forge/config.toml` | Multi-section (`[audio]` `nexus-audio/src/config.rs:66`, `[collab]` `nexus-collab/src/core_plugin.rs:75`, `[digests]` `nexus-workflow/src/digests.rs:51`, `[notifications.*]`, `[mcp]`) | Per-section loader on each subsystem |
| **kernel config.toml** | `<forge>/.nexus/config.toml` | `KernelConfig` — `nexus-kernel/src/config.rs:12` | `KernelConfig::load` — line 79 |

Missing file ⇒ defaults returned (no error). Schemas at [`forge-config.md`](forge-config.md).

### Shell-side persistence

The Tauri shell maintains its own state outside the forge (so it survives `--forge-path` switches).

| File | Path | Struct (file:line) | Loader |
|------|------|---------------------|--------|
| **shell-state.json** | `<app_config_dir>/shell-state.json` | `ShellState` — `shell/src-tauri/src/persistence.rs:50` | `get_shell_state` Tauri command |
| **plugin granted_caps.json** | `~/.nexus-shell/plugins/<plugin>/granted_caps.json` | sealed `GrantedCaps` — `shell/src-tauri/src/lib.rs:364` | `get_plugin_granted_capabilities` |
| **community plugin.json** | `~/.nexus-shell/plugins/<plugin>/plugin.json` | `CommunityPluginManifest` — `shell/src-tauri/src/lib.rs:100` | `scan_plugin_directory` |

### Per-plugin settings schemas

Plugins declare their own settings shape via a JSON Schema referenced from the manifest:

```toml
# plugin.toml
[settings]
schema = "settings-schema.json"
```

Loaded by `SettingsManager::register_schema` (`crates/nexus-plugins/src/settings.rs:42`); persisted per-plugin to `<plugin_dir>/settings.json`; validated against the schema on every write.

### Env vars

Full table in [`env-vars.md`](env-vars.md). Categories:

- **Provider detection** — `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `OLLAMA_BASE_URL`
- **Local embeddings** — `NEXUS_LOCAL_EMBEDDINGS`, `NEXUS_LOCAL_EMBEDDING_MODEL`
- **Forge selection** — `NEXUS_FORGE_PATH`
- **TLS pinning** — `NEXUS_TLS_PINNING`
- **Keyring bypass** — `NEXUS_NO_KEYRING`
- **Logging** — `RUST_LOG`, `NEXUS_TUI_LOG`
- **Shell editor integration** — `VISUAL`, `EDITOR`, `SHELL`
- **Color** — `NO_COLOR`
- **System** — `HOME`, `USERPROFILE`, `PATH`, `TEMP`
- **WSLg quirks** — `WEBKIT_DISABLE_COMPOSITING_MODE`, `WEBKIT_DISABLE_DMABUF_RENDERER`, `GDK_BACKEND` (baked into `shell/`'s tauri:dev — do **not** strip)

### `<forge>/.forge/` layout

| Path | Purpose | Creator |
|------|---------|---------|
| `.forge/index.db` | SQLite entity index | `StorageEngine::init` |
| `.forge/search/` | Tantivy FTS segments | `StorageEngine` |
| `.forge/app.toml` | Application config | user / shell |
| `.forge/workspace.json` | Shell workspace state | shell |
| `.forge/ai.toml` | AI provider config | user |
| `.forge/mcp.toml` | MCP server registry | user / plugins |
| `.forge/lsp.toml` | LSP server specs | user |
| `.forge/dap.toml` | DAP adapter specs | user |
| `.forge/notifications.toml` | Notification channels + routing | user |
| `.forge/config.toml` | Multi-section TOML for non-standalone subsystems (`[audio]`, `[collab]`, `[digests]`, `[notifications.<channel>]`, `[mcp]`, …) | user / plugins |
| `.forge/notifications/inbox.db` | SQLite inbox | `nexus-notifications/src/lib.rs:84` |
| `.forge/procmgr.sqlite` | Terminal process manager | `nexus-bootstrap/src/plugins/terminal.rs:26` |
| `.forge/sessions.sqlite` | Terminal session scrollback | bootstrap terminal:72 |
| `.forge/agent/transcripts.sqlite` | Agent conversation transcripts | bootstrap |
| `.forge/agents/<agent_id>/` | Per-agent memory (history.jsonl etc.) | `nexus-agent/src/memory.rs:40` |
| `.forge/ai-runtime/runs.db` | AI runtime execution logs (reserved) | bootstrap |
| `.forge/ai-activity.log` | AI surface activity log (chat, ask, cmd-i, ghost) | `nexus-ai/src/activity_log.rs:42` |
| `.forge/comments/` | Inline-comment sidecars (`<note>.md.json`) | `nexus-comments/src/store.rs:62` |
| `.forge/templates/` | User-authored `.template.md` files (recursive) | `nexus-templates/src/registry.rs:61` |
| `.forge/skills/` | Authored prompt-template skills (surfaced via MCP) | `nexus-mcp/src/server.rs:1180` |
| `.forge/digests/last_fired.json` | Last-fired timestamps for the digest scheduler | `nexus-workflow/src/digests.rs:131` |
| `.forge/.audio/models/` | Whisper / local-audio model cache (override via `[audio] local_model_dir`) | `nexus-audio/src/config.rs:138` |
| `.forge/.editor/crdt/` | CRDT conflict snapshots | `nexus-crdt/src/state.rs:106` |
| `.forge/.editor/undo/<sha>.json` | Per-file persisted undo stacks | `nexus-editor/src/handlers/session.rs:336` |
| `.forge/.kernel/audit.db` | Audit log SQLite | `nexus-bootstrap::audit_sqlite` |
| `.forge/kv.sqlite3` | KV store | `nexus-bootstrap/src/lib.rs:204` |
| `.forge/plugins/` | Community WASM/JS bundles | manual install |
| `.forge/logs/` | Runtime logs | bootstrap |
| `.forge/temp/` | Transient | runtime |
| `.forge/.lock` | Exclusive forge lock | `StorageEngine` |
| `.forge/.gitignore` | Default ignore rules for rebuildable indexes / per-machine state | `nexus-cli/src/commands/crdt.rs:208` |

`.gitattributes` (at the forge root, not inside `.forge/`) is also written by `nexus crdt init` per `nexus-cli/src/commands/crdt.rs:133` to register the CRDT merge driver.

> **Removed:** `.forge/acp.toml` was previously listed as reserved. ADR 0027 §Phase 4 keeps ACP greenfield with no flat-TOML loader — see `crates/nexus-acp/src/lib.rs:13` and `crates/nexus-bootstrap/src/acp_contribution_wiring.rs:8`. Do not introduce one.

## How to add a setting

1. Identify the right Config struct in the relevant `nexus-<service>` crate.
2. Add a field with a `#[serde(default = "…")]` and a typed default helper.
3. If user-facing, register a `SettingsSchema` so the settings UI surfaces it.
4. Document it here (add a row to the relevant `.md`) — settings without a doc row are considered hidden and should fail review.
5. If it replaces a hardcoded value, remove the entry from [`hardcoded-rust.md`](hardcoded-rust.md) or [`hardcoded-shell.md`](hardcoded-shell.md).

## How to NOT add a setting

- **Do not** introduce a new top-level TOML file in `.forge/` without an entry in the table above.
- **Do not** add an env var without an entry in [`env-vars.md`](env-vars.md).
- **Do not** ship a Tauri command in `shell/src-tauri/` that mutates user-visible state — route through `kernel_invoke` → `ipc_call` so the setting lives in a service crate's Config struct.
