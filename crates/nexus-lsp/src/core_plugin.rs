//! Core plugin for the LSP host (`com.nexus.lsp`).
//!
//! Loads `<forge>/.forge/lsp.toml` at init time, exposes IPC handlers
//! that proxy LSP requests to the right child server, and republishes
//! server-pushed notifications on the kernel event bus.
//!
//! # IPC surface
//!
//! | id | name | args |
//! |---|---|---|
//! | 1 | `list_servers` | — |
//! | 2 | `open_file` | `{path, content, language_id?, version?}` |
//! | 3 | `close_file` | `{path}` |
//! | 4 | `change_file` | `{path, content, version}` |
//! | 5 | `completions` | `{path, line, character}` |
//! | 6 | `hover` | `{path, line, character}` |
//! | 7 | `definition` | `{path, line, character}` |
//! | 8 | `references` | `{path, line, character, include_declaration?}` |
//! | 9 | `rename` | `{path, line, character, new_name}` |
//! | 10 | `code_actions` | `{path, range}` |
//! | 11 | `format` | `{path}` |
//! | 12 | `execute_command` | `{path, command, arguments?}` |
//!
//! Handlers 2..=12 require the file path to map to a configured server
//! via [`LspHostConfig::server_for_path`]; calls for an unrouted path
//! return JSON `null`. The path is the *routing* hint — `execute_command`
//! itself targets the server-side command registry, not a document.
//!
//! # Bus events
//!
//! Server-pushed notifications fan out as
//! `com.nexus.lsp.<lsp_method_with_dots>`, e.g.
//! `com.nexus.lsp.textDocument.publishDiagnostics`. The original LSP
//! `params` payload travels verbatim. A poll loop driven by the
//! tokio runtime in [`LspCorePlugin::dispatch_async`] drains every
//! known client's notification queue on every request — chatty
//! enough to keep the diagnostic latency low without spawning a
//! dedicated background task that competes with kernel shutdown.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde_json::{json, Value};

use crate::client::LspClientError;
use crate::config::{LspServerSpec, MergeSkipReason, UnregisterError};
use crate::ipc::{
    LspChangeFileArgs, LspCodeActionsArgs, LspExecuteCommandArgs, LspOk, LspOpenFileArgs,
    LspOpenFileReply, LspPathArgs, LspPositionArgs, LspReferencesArgs, LspRegisterServerArgs,
    LspRegisterServerReply, LspRenameArgs, LspServerEntry, LspUnregisterServerArgs,
    LspUnregisterServerReply,
};
use crate::pool::{ConnectionPool, PoolConfig};
use crate::{LspConfigError, LspHostConfig};

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = "com.nexus.lsp";

/// Sync handler — list configured servers.
pub const HANDLER_LIST_SERVERS: u32 = 1;
/// Async — open a document on the routed server.
pub const HANDLER_OPEN_FILE: u32 = 2;
/// Async — close a document.
pub const HANDLER_CLOSE_FILE: u32 = 3;
/// Async — push a `didChange`.
pub const HANDLER_CHANGE_FILE: u32 = 4;
/// Async — request completions.
pub const HANDLER_COMPLETIONS: u32 = 5;
/// Async — request hover.
pub const HANDLER_HOVER: u32 = 6;
/// Async — request definition.
pub const HANDLER_DEFINITION: u32 = 7;
/// Async — request references.
pub const HANDLER_REFERENCES: u32 = 8;
/// Async — request rename.
pub const HANDLER_RENAME: u32 = 9;
/// Async — request code actions.
pub const HANDLER_CODE_ACTIONS: u32 = 10;
/// Async — request formatting.
pub const HANDLER_FORMAT: u32 = 11;
/// Async — `workspace/executeCommand`. Targets the server routed for
/// the supplied `path` (so the editor can dispatch a command-only
/// code action against the same server that produced it). Powers
/// the BL-077 follow-up: code actions whose `edit` field is missing
/// but whose `command` field carries a server-side action name.
pub const HANDLER_EXECUTE_COMMAND: u32 = 12;
/// BL-113 Phase 2b plugin-contributed server add.
pub const HANDLER_REGISTER_SERVER: u32 = 13;
/// BL-113 Phase 2b plugin-contributed server remove.
pub const HANDLER_UNREGISTER_SERVER: u32 = 14;

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::lsp::register`. Order
/// matches the pre-SD-06 bootstrap registration.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("list_servers", HANDLER_LIST_SERVERS),
    ("open_file", HANDLER_OPEN_FILE),
    ("close_file", HANDLER_CLOSE_FILE),
    ("change_file", HANDLER_CHANGE_FILE),
    ("completions", HANDLER_COMPLETIONS),
    ("hover", HANDLER_HOVER),
    ("definition", HANDLER_DEFINITION),
    ("references", HANDLER_REFERENCES),
    ("rename", HANDLER_RENAME),
    ("code_actions", HANDLER_CODE_ACTIONS),
    ("format", HANDLER_FORMAT),
    ("execute_command", HANDLER_EXECUTE_COMMAND),
    ("register_server", HANDLER_REGISTER_SERVER),
    ("unregister_server", HANDLER_UNREGISTER_SERVER),
];

/// Core plugin that manages connections to LSP servers.
///
/// The active server set lives behind a [`RwLock`] so the
/// BL-113 `register_server` / `unregister_server` IPC verbs can
/// mutate it at runtime after a plugin activates / deactivates.
/// Async dispatch handlers snapshot the config at dispatch time
/// (see [`snapshot_config`]) so an in-flight command keeps the
/// server view it started with even if the master config mutates
/// underneath.
pub struct LspCorePlugin {
    forge_root: PathBuf,
    event_bus: Option<Arc<EventBus>>,
    config: Arc<RwLock<LspHostConfig>>,
    pool: Arc<ConnectionPool>,
}

impl LspCorePlugin {
    /// Create a new (unstarted) LSP host plugin for the given forge root.
    #[must_use]
    pub fn new(forge_root: PathBuf, event_bus: Option<Arc<EventBus>>) -> Self {
        let pool = Arc::new(ConnectionPool::new(
            PoolConfig::default(),
            forge_root.clone(),
        ));
        Self {
            forge_root,
            event_bus,
            config: Arc::new(RwLock::new(LspHostConfig::default())),
            pool,
        }
    }
}

/// Snapshot the host config behind an `Arc<RwLock>` into a fresh
/// `Arc<LspHostConfig>` so async dispatch keeps its existing
/// pass-by-Arc helper signatures unchanged.
fn snapshot_config(cell: &Arc<RwLock<LspHostConfig>>) -> Arc<LspHostConfig> {
    Arc::new(read_config(cell).clone())
}

/// #199 / R16 — recover from a poisoned `LspHostConfig` lock rather
/// than `.expect()`-panicking. With `panic = "abort"` in the release
/// profile, that would convert a prior writer-side panic into a
/// whole-process abort. The lock is only ever mutated by whole-value
/// replacement (`reload`) or via `LspHostConfig` methods that don't
/// leave torn state on panic, so reading the inner value on poison is
/// safe (same posture as `nexus-dap`'s `read_config`/`write_config`,
/// #199).
fn read_config(config: &RwLock<LspHostConfig>) -> std::sync::RwLockReadGuard<'_, LspHostConfig> {
    match config.read() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::error!("LspHostConfig RwLock poisoned — recovering (see #199)");
            poisoned.into_inner()
        }
    }
}

/// Write-lock counterpart of [`read_config`]; see its doc comment.
fn write_config(config: &RwLock<LspHostConfig>) -> std::sync::RwLockWriteGuard<'_, LspHostConfig> {
    match config.write() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::error!("LspHostConfig RwLock poisoned — recovering (see #199)");
            poisoned.into_inner()
        }
    }
}

/// BL-113 Phase 2b — sync IPC handler for `register_server`. Parses
/// `args` into an [`LspServerSpec`] + `plugin_id`, takes the host
/// config's write lock, delegates the merge to
/// [`LspHostConfig::register_contributed`], and returns a
/// `{ok, status}` envelope. Validation errors are surfaced as a
/// "skip" status (not a `PluginError`) so the caller can decide
/// whether to log + continue or escalate.
///
/// Trust model (ADR 0027 §Open Question #3): no capability gate at
/// the verb level. Plugins author manifest contributions; the
/// bootstrap-side wiring helper
/// (`nexus-bootstrap::lsp_contribution_wiring::wire_lsp_contributions`)
/// is the only intended caller. Runtime usage capabilities
/// (`process.spawn` for spawning the language server) ride on the
/// contributing plugin's existing grants and are checked at the
/// `start` boundary, not here. Hard enforcement at the verb level
/// is filed as a hardening follow-up.
fn handle_register_server(
    config: &Arc<RwLock<LspHostConfig>>,
    args: &Value,
) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via typed `LspRegisterServerArgs`
    // (`deny_unknown_fields`). The prior hand-rolled
    // `parse_register_server_spec` + `parse_string_field` silently
    // ignored unknown fields and let typos like `{ commandd: "..." }`
    // through. Non-empty checks for `name` / `command` / `plugin_id`
    // are now inlined on the typed fields.
    let typed: LspRegisterServerArgs =
        serde_json::from_value(args.clone()).map_err(|e| PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!("register_server: invalid args: {e}"),
        })?;
    if typed.name.is_empty() {
        return Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "register_server: missing or empty required field `name`".to_string(),
        });
    }
    if typed.command.is_empty() {
        return Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "register_server: missing or empty required field `command`".to_string(),
        });
    }
    if typed.plugin_id.is_empty() {
        return Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "register_server: missing or empty required field `plugin_id`".to_string(),
        });
    }
    let spec = LspServerSpec {
        name: typed.name,
        command: typed.command,
        args: typed.args,
        file_types: typed.file_types,
        root_markers: typed.root_markers,
        disabled: typed.disabled,
        env: typed.env,
    };
    let mut cfg = write_config(config);
    let reply = match cfg.register_contributed(spec, typed.plugin_id) {
        Ok(()) => LspRegisterServerReply {
            ok: true,
            status: "ok".to_string(),
        },
        Err(MergeSkipReason::TomlOverride) => LspRegisterServerReply {
            ok: false,
            status: "toml_override".to_string(),
        },
        Err(MergeSkipReason::InvalidName) => LspRegisterServerReply {
            ok: false,
            status: "invalid_name".to_string(),
        },
        Err(MergeSkipReason::InvalidCommand) => LspRegisterServerReply {
            ok: false,
            status: "invalid_command".to_string(),
        },
    };
    serde_json::to_value(&reply).map_err(|e| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: format!("register_server: serialize reply: {e}"),
    })
}

/// BL-113 Phase 2b — sync IPC handler for `unregister_server`.
/// Parses `name` + `plugin_id` out of `args` and delegates to
/// [`LspHostConfig::unregister_contributed`]. Authorisation is
/// enforced inside the config method (the `plugin_id` must match the
/// contributing plugin recorded at register time). On
/// `NotOwnedByPlugin` the reply carries `actual_owner` so the caller
/// can log who actually contributed the entry.
fn handle_unregister_server(
    config: &Arc<RwLock<LspHostConfig>>,
    args: &Value,
) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via typed `LspUnregisterServerArgs`,
    // typed `LspUnregisterServerReply`. The prior `parse_string_field`
    // chain silently passed unknown fields through.
    let typed: LspUnregisterServerArgs =
        serde_json::from_value(args.clone()).map_err(|e| PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!("unregister_server: invalid args: {e}"),
        })?;
    if typed.name.is_empty() {
        return Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "unregister_server: missing or empty required field `name`".to_string(),
        });
    }
    if typed.plugin_id.is_empty() {
        return Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "unregister_server: missing or empty required field `plugin_id`".to_string(),
        });
    }
    let mut cfg = write_config(config);
    let reply = match cfg.unregister_contributed(&typed.name, &typed.plugin_id) {
        Ok(_removed) => LspUnregisterServerReply {
            ok: true,
            status: "ok".to_string(),
            actual_owner: None,
        },
        Err(UnregisterError::NotFound) => LspUnregisterServerReply {
            ok: false,
            status: "not_found".to_string(),
            actual_owner: None,
        },
        Err(UnregisterError::TomlEntry) => LspUnregisterServerReply {
            ok: false,
            status: "toml_entry".to_string(),
            actual_owner: None,
        },
        Err(UnregisterError::NotOwnedByPlugin { actual_owner }) => LspUnregisterServerReply {
            ok: false,
            status: "not_owned_by_plugin".to_string(),
            actual_owner: Some(actual_owner),
        },
    };
    serde_json::to_value(&reply).map_err(|e| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: format!("unregister_server: serialize reply: {e}"),
    })
}

// #190 — `parse_string_field` and `parse_register_server_spec` helpers
// removed; both `handle_register_server` and `handle_unregister_server`
// now strict-parse via typed `LspRegisterServerArgs` /
// `LspUnregisterServerArgs` and inline the non-empty checks.

impl CorePlugin for LspCorePlugin {
    fn on_init(&mut self) -> Result<(), PluginError> {
        let toml_path = self.forge_root.join(".forge").join("lsp.toml");
        let loaded = match LspHostConfig::read_from(&toml_path) {
            Ok(cfg) => {
                tracing::info!(
                    plugin_id = PLUGIN_ID,
                    servers = cfg.servers.len(),
                    "loaded lsp.toml"
                );
                cfg
            }
            Err(LspConfigError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                tracing::debug!(
                    plugin_id = PLUGIN_ID,
                    "no lsp.toml found — LSP host has no servers configured"
                );
                LspHostConfig::default()
            }
            Err(e) => {
                tracing::warn!(
                    plugin_id = PLUGIN_ID,
                    error = %e,
                    "failed to parse lsp.toml — LSP host disabled"
                );
                LspHostConfig::default()
            }
        };
        *write_config(&self.config) = loaded;
        Ok(())
    }

    fn on_start(&mut self) -> Result<(), PluginError> {
        let server_count = read_config(&self.config).servers.len();
        if let Some(bus) = &self.event_bus {
            let _ = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.lsp.started",
                json!({ "configured_servers": server_count }),
            );
        }
        tracing::info!(
            plugin_id = PLUGIN_ID,
            configured_servers = server_count,
            "LSP host started (connections are lazy)"
        );
        Ok(())
    }

    fn on_stop(&mut self) {
        // Same shutdown shape as McpHostPlugin: spawn a current-thread
        // runtime so we don't need an outer reactor, hard-cap the join
        // so a misbehaving server can't hang kernel shutdown.
        const SHUTDOWN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);
        let pool = Arc::clone(&self.pool);
        let handle = std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                rt.block_on(async move {
                    pool.shutdown_all().await;
                });
            }
        });
        let start = std::time::Instant::now();
        let poll_interval = std::time::Duration::from_millis(50);
        while start.elapsed() < SHUTDOWN_DEADLINE {
            if handle.is_finished() {
                let _ = handle.join();
                tracing::info!(plugin_id = PLUGIN_ID, "LSP host stopped");
                return;
            }
            std::thread::sleep(poll_interval);
        }
        tracing::warn!(
            audit = true,
            plugin_id = PLUGIN_ID,
            timeout_secs = SHUTDOWN_DEADLINE.as_secs(),
            "LSP host shutdown timed out; abandoning the join — child processes \
             may be stranded until the host process exits"
        );
    }

    fn dispatch(&mut self, handler_id: u32, args: &Value) -> Result<Value, PluginError> {
        match handler_id {
            HANDLER_LIST_SERVERS => {
                // #190 / R7 — materialize into typed `Vec<LspServerEntry>`
                // so the schemars schema generator sees the same fields
                // the runtime emits.
                let cfg = read_config(&self.config);
                let arr: Vec<LspServerEntry> = cfg
                    .servers
                    .values()
                    .map(|spec| LspServerEntry {
                        name: spec.name.clone(),
                        command: spec.command.clone(),
                        args: spec.args.clone(),
                        file_types: spec.file_types.clone(),
                        disabled: spec.disabled,
                    })
                    .collect();
                serde_json::to_value(&arr).map_err(|e| PluginError::ExecutionFailed {
                    plugin_id: PLUGIN_ID.to_string(),
                    reason: format!("list_servers: serialize: {e}"),
                })
            }
            HANDLER_REGISTER_SERVER => handle_register_server(&self.config, args),
            HANDLER_UNREGISTER_SERVER => handle_unregister_server(&self.config, args),
            HANDLER_OPEN_FILE
            | HANDLER_CLOSE_FILE
            | HANDLER_CHANGE_FILE
            | HANDLER_COMPLETIONS
            | HANDLER_HOVER
            | HANDLER_DEFINITION
            | HANDLER_REFERENCES
            | HANDLER_RENAME
            | HANDLER_CODE_ACTIONS
            | HANDLER_FORMAT
            | HANDLER_EXECUTE_COMMAND => Err(PluginError::ExecutionFailed {
                plugin_id: PLUGIN_ID.to_string(),
                reason: format!("handler_id {handler_id} requires dispatch_async"),
            }),
            _ => Err(PluginError::ExecutionFailed {
                plugin_id: PLUGIN_ID.to_string(),
                reason: format!("unknown handler_id {handler_id}"),
            }),
        }
    }

    #[allow(clippy::too_many_lines)] // dispatch_async fans out across 10 verbs; splitting per-verb hurts readability
    fn dispatch_async(&mut self, handler_id: u32, args: &Value) -> Option<CorePluginFuture> {
        let pool = Arc::clone(&self.pool);
        // BL-113 Phase 2b — async dispatchers consume an immutable
        // snapshot of the host config taken at dispatch time. A
        // concurrent `register_server` / `unregister_server` mutates
        // the master config but won't affect this in-flight command's
        // server view (the snapshot is per-future, not shared).
        let config = Some(snapshot_config(&self.config));
        let bus = self.event_bus.clone();

        match handler_id {
            HANDLER_OPEN_FILE => {
                // #190 / R7 — strict-parse args via typed `LspOpenFileArgs`,
                // typed reply via `LspOpenFileReply`. Parse errors surface
                // through the future as `PluginError::ExecutionFailed`
                // instead of the prior quiet `str_arg(...)?`-returns-`None`
                // fallback.
                let parsed: Result<LspOpenFileArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let LspOpenFileArgs {
                        path,
                        content,
                        language_id,
                        version,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("open_file: invalid args: {e}"),
                    })?;
                    let version = version.unwrap_or(1);
                    let cfg = config_or_err(config.as_ref())?;
                    let Some(server) = cfg.server_for_path(&path) else {
                        return Ok(Value::Null);
                    };
                    let server_name = server.name.clone();
                    let lang = language_id.unwrap_or_else(|| infer_language_id(&path));
                    let uri = file_uri_from_path(&path);
                    pool.call_with_reconnect(&server_name, &cfg, move |client| {
                        let bus = bus.clone();
                        let uri = uri.clone();
                        let lang = lang.clone();
                        let content = content.clone();
                        Box::pin(async move {
                            let lock = client.lock().await;
                            lock.did_open(&uri, &lang, version, &content).await?;
                            republish_pending(&lock, bus.as_ref()).await;
                            Ok(())
                        })
                    })
                    .await
                    .map_err(map_client_err)?;
                    let reply = LspOpenFileReply {
                        uri: file_uri_from_path(&path),
                        server: server_name,
                    };
                    serde_json::to_value(&reply).map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("open_file: serialize reply: {e}"),
                    })
                }))
            }

            HANDLER_CLOSE_FILE => {
                // #190 / R7 — strict-parse via typed `LspPathArgs`,
                // typed `LspOk` reply.
                let parsed: Result<LspPathArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let LspPathArgs { path } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("close_file: invalid args: {e}"),
                        })?;
                    let cfg = config_or_err(config.as_ref())?;
                    let Some(server) = cfg.server_for_path(&path) else {
                        return Ok(Value::Null);
                    };
                    let server_name = server.name.clone();
                    let uri = file_uri_from_path(&path);
                    pool.call_with_reconnect(&server_name, &cfg, move |client| {
                        let bus = bus.clone();
                        let uri = uri.clone();
                        Box::pin(async move {
                            let lock = client.lock().await;
                            lock.did_close(&uri).await?;
                            republish_pending(&lock, bus.as_ref()).await;
                            Ok(())
                        })
                    })
                    .await
                    .map_err(map_client_err)?;
                    serde_json::to_value(&LspOk { ok: true }).map_err(|e| {
                        PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("close_file: serialize reply: {e}"),
                        }
                    })
                }))
            }

            HANDLER_CHANGE_FILE => {
                // #190 / R7 — strict-parse via typed `LspChangeFileArgs`.
                // Note: `LspChangeFileArgs.version` is `i64` (required);
                // the prior `args.get("version").as_i64().unwrap_or(2)`
                // silently defaulted to 2 on missing/malformed input,
                // which would have produced a stale-version did_change
                // call. Strict parse means callers must supply a version
                // explicitly.
                let parsed: Result<LspChangeFileArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let LspChangeFileArgs {
                        path,
                        content,
                        version,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("change_file: invalid args: {e}"),
                    })?;
                    let cfg = config_or_err(config.as_ref())?;
                    let Some(server) = cfg.server_for_path(&path) else {
                        return Ok(Value::Null);
                    };
                    let server_name = server.name.clone();
                    let uri = file_uri_from_path(&path);
                    pool.call_with_reconnect(&server_name, &cfg, move |client| {
                        let bus = bus.clone();
                        let uri = uri.clone();
                        let content = content.clone();
                        Box::pin(async move {
                            let lock = client.lock().await;
                            lock.did_change(&uri, version, &content).await?;
                            republish_pending(&lock, bus.as_ref()).await;
                            Ok(())
                        })
                    })
                    .await
                    .map_err(map_client_err)?;
                    serde_json::to_value(&LspOk { ok: true }).map_err(|e| {
                        PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("change_file: serialize reply: {e}"),
                        }
                    })
                }))
            }

            HANDLER_COMPLETIONS => {
                proxy_position_request(args, config, pool, bus, "textDocument/completion")
            }
            HANDLER_HOVER => proxy_position_request(args, config, pool, bus, "textDocument/hover"),
            HANDLER_DEFINITION => {
                proxy_position_request(args, config, pool, bus, "textDocument/definition")
            }
            HANDLER_REFERENCES => {
                // #190 / R7 — strict-parse via typed `LspReferencesArgs`.
                let parsed: Result<LspReferencesArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let LspReferencesArgs {
                        path,
                        line,
                        character,
                        include_declaration,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("references: invalid args: {e}"),
                    })?;
                    let cfg = config_or_err(config.as_ref())?;
                    let Some(server) = cfg.server_for_path(&path) else {
                        return Ok(Value::Null);
                    };
                    let server_name = server.name.clone();
                    let uri = file_uri_from_path(&path);
                    let payload = json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                        "context": { "includeDeclaration": include_declaration },
                    });
                    proxy_request(
                        &pool,
                        &cfg,
                        &server_name,
                        bus,
                        "textDocument/references",
                        payload,
                    )
                    .await
                    .map_err(map_client_err)
                }))
            }
            HANDLER_RENAME => {
                // #190 / R7 — strict-parse via typed `LspRenameArgs`.
                let parsed: Result<LspRenameArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let LspRenameArgs {
                        path,
                        line,
                        character,
                        new_name,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("rename: invalid args: {e}"),
                    })?;
                    let cfg = config_or_err(config.as_ref())?;
                    let Some(server) = cfg.server_for_path(&path) else {
                        return Ok(Value::Null);
                    };
                    let server_name = server.name.clone();
                    let uri = file_uri_from_path(&path);
                    let payload = json!({
                        "textDocument": { "uri": uri },
                        "position": { "line": line, "character": character },
                        "newName": new_name,
                    });
                    proxy_request(
                        &pool,
                        &cfg,
                        &server_name,
                        bus,
                        "textDocument/rename",
                        payload,
                    )
                    .await
                    .map_err(map_client_err)
                }))
            }
            HANDLER_CODE_ACTIONS => {
                // #190 / R7 — strict-parse via typed `LspCodeActionsArgs`.
                // The `range` field is intentionally `serde_json::Value`
                // on the typed struct since the LSP `Range` shape isn't
                // mirrored. Strict on `{ path, range }`; typos like
                // `{ rangee: ... }` now error.
                let parsed: Result<LspCodeActionsArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let LspCodeActionsArgs { path, range } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("code_actions: invalid args: {e}"),
                        })?;
                    let cfg = config_or_err(config.as_ref())?;
                    let Some(server) = cfg.server_for_path(&path) else {
                        return Ok(Value::Null);
                    };
                    let server_name = server.name.clone();
                    let uri = file_uri_from_path(&path);
                    let payload = json!({
                        "textDocument": { "uri": uri },
                        "range": range,
                        "context": { "diagnostics": [] },
                    });
                    proxy_request(
                        &pool,
                        &cfg,
                        &server_name,
                        bus,
                        "textDocument/codeAction",
                        payload,
                    )
                    .await
                    .map_err(map_client_err)
                }))
            }
            HANDLER_FORMAT => {
                // #190 / R7 — strict-parse via typed `LspPathArgs` (just
                // `{ path }`). Note: formatting options (tabSize,
                // insertSpaces) are hardcoded in the payload — that's
                // pre-existing behavior; a follow-up could thread them
                // through a richer args shape.
                let parsed: Result<LspPathArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let LspPathArgs { path } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("format: invalid args: {e}"),
                        })?;
                    let cfg = config_or_err(config.as_ref())?;
                    let Some(server) = cfg.server_for_path(&path) else {
                        return Ok(Value::Null);
                    };
                    let server_name = server.name.clone();
                    let uri = file_uri_from_path(&path);
                    let payload = json!({
                        "textDocument": { "uri": uri },
                        "options": {
                            "tabSize": 4,
                            "insertSpaces": true,
                        },
                    });
                    proxy_request(
                        &pool,
                        &cfg,
                        &server_name,
                        bus,
                        "textDocument/formatting",
                        payload,
                    )
                    .await
                    .map_err(map_client_err)
                }))
            }
            HANDLER_EXECUTE_COMMAND => {
                // #190 / R7 — strict-parse via typed `LspExecuteCommandArgs`.
                // `arguments` defaults to empty Vec when omitted (typed
                // default), preserving the prior behavior of forwarding
                // an empty array for servers that require the field.
                let parsed: Result<LspExecuteCommandArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let LspExecuteCommandArgs {
                        path,
                        command,
                        arguments,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("execute_command: invalid args: {e}"),
                    })?;
                    let cfg = config_or_err(config.as_ref())?;
                    let Some(server) = cfg.server_for_path(&path) else {
                        return Ok(Value::Null);
                    };
                    let server_name = server.name.clone();
                    let payload = json!({
                        "command": command,
                        "arguments": arguments,
                    });
                    proxy_request(
                        &pool,
                        &cfg,
                        &server_name,
                        bus,
                        "workspace/executeCommand",
                        payload,
                    )
                    .await
                    .map_err(map_client_err)
                }))
            }
            _ => None,
        }
    }
}

/// Build the closure for completions / hover / definition — the three
/// position-only requests share an identical wire shape.
///
/// #190 / R7 — strict-parse args via typed `LspPositionArgs`
/// (`deny_unknown_fields`). Parse errors surface through the future
/// as `PluginError::ExecutionFailed` instead of the prior quiet
/// `str_arg(...)?`-returns-`None` mode. The reply body is the raw
/// LSP-protocol JSON the upstream server emits; that's per-LSP-spec
/// and stays untyped on purpose.
fn proxy_position_request(
    args: &Value,
    config: Option<Arc<LspHostConfig>>,
    pool: Arc<ConnectionPool>,
    bus: Option<Arc<EventBus>>,
    method: &'static str,
) -> Option<CorePluginFuture> {
    let parsed: Result<LspPositionArgs, _> = serde_json::from_value(args.clone());
    Some(Box::pin(async move {
        let LspPositionArgs {
            path,
            line,
            character,
        } = parsed.map_err(|e| PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!("{method}: invalid args: {e}"),
        })?;
        let cfg = config_or_err(config.as_ref())?;
        let Some(server) = cfg.server_for_path(&path) else {
            return Ok(Value::Null);
        };
        let server_name = server.name.clone();
        let uri = file_uri_from_path(&path);
        let payload = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
        });
        proxy_request(&pool, &cfg, &server_name, bus, method, payload)
            .await
            .map_err(map_client_err)
    }))
}

/// Send an LSP request through the pool's reconnect loop. The
/// closure body is called once per attempt; transient failures
/// drop the entry and trigger a fresh connect with document
/// resync handled by the pool. Notifications drained per attempt
/// so server-pushed diagnostics still fan out even when an
/// earlier attempt failed mid-flight.
async fn proxy_request(
    pool: &ConnectionPool,
    cfg: &LspHostConfig,
    server_name: &str,
    bus: Option<Arc<EventBus>>,
    method: &'static str,
    payload: Value,
) -> Result<Value, LspClientError> {
    pool.call_with_reconnect(server_name, cfg, move |client| {
        let bus = bus.clone();
        let payload = payload.clone();
        Box::pin(async move {
            let lock = client.lock().await;
            let r = lock.send_request(method, payload).await?;
            republish_pending(&lock, bus.as_ref()).await;
            Ok(r)
        })
    })
    .await
}

/// Drain any server-pushed notifications and republish them on the
/// kernel bus. Idempotent — safe to call repeatedly.
async fn republish_pending(client: &crate::client::LspClient, bus: Option<&Arc<EventBus>>) {
    let pending = client.drain_notifications().await;
    if pending.is_empty() {
        return;
    }
    let Some(bus) = bus else {
        return;
    };
    for n in pending {
        // Map LSP method names like `textDocument/publishDiagnostics`
        // to a dotted topic suitable for the kernel bus's
        // namespace check (`com.nexus.lsp.<…>`).
        let topic = format!("com.nexus.lsp.{}", n.method.replace('/', "."));
        if let Err(e) = bus.publish_plugin(PLUGIN_ID, &topic, n.params) {
            tracing::warn!(
                plugin_id = PLUGIN_ID,
                topic = %topic,
                error = %e,
                "failed to republish lsp notification"
            );
        }
    }
}

// #190 — `str_arg` removed; all callers now strict-parse via typed
// `Lsp*Args` shapes from `crate::ipc`.

fn config_or_err(config: Option<&Arc<LspHostConfig>>) -> Result<Arc<LspHostConfig>, PluginError> {
    config.cloned().ok_or_else(|| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: "LSP host config not loaded".to_string(),
    })
}

#[allow(clippy::needless_pass_by_value)]
fn map_client_err(e: LspClientError) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: e.to_string(),
    }
}

fn file_uri_from_path(path: &str) -> String {
    if path.starts_with("file://") {
        path.to_string()
    } else {
        format!("file://{path}")
    }
}

/// Best-effort languageId from extension. Servers usually accept
/// anything they recognise; unknown extensions get the extension
/// itself which most servers tolerate.
fn infer_language_id(path: &str) -> String {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "rs" => "rust".to_string(),
        "ts" => "typescript".to_string(),
        "tsx" => "typescriptreact".to_string(),
        "js" | "mjs" | "cjs" => "javascript".to_string(),
        "jsx" => "javascriptreact".to_string(),
        "py" => "python".to_string(),
        "go" => "go".to_string(),
        "rb" => "ruby".to_string(),
        "java" => "java".to_string(),
        "c" => "c".to_string(),
        "h" | "hpp" | "cc" | "cpp" | "cxx" => "cpp".to_string(),
        "json" => "json".to_string(),
        "toml" => "toml".to_string(),
        "yaml" | "yml" => "yaml".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_plugin(dir: &std::path::Path) -> LspCorePlugin {
        LspCorePlugin::new(dir.to_path_buf(), None)
    }

    #[test]
    fn plugin_id_is_correct() {
        assert_eq!(PLUGIN_ID, "com.nexus.lsp");
    }

    #[test]
    fn on_init_with_no_lsp_toml_succeeds() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        assert!(plugin.on_init().is_ok());
        assert!(plugin.config.read().unwrap().servers.is_empty());
    }

    #[test]
    fn on_init_with_valid_lsp_toml_loads_config() {
        let dir = tempdir().unwrap();
        let forge_dir = dir.path().join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();
        std::fs::write(
            forge_dir.join("lsp.toml"),
            r#"
[[servers]]
name = "rust-analyzer"
command = "rust-analyzer"
file_types = ["rs"]
"#,
        )
        .unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let cfg = plugin.config.read().unwrap();
        assert!(cfg.servers.contains_key("rust-analyzer"));
    }

    #[test]
    fn on_init_with_invalid_lsp_toml_falls_back_to_empty() {
        let dir = tempdir().unwrap();
        let forge_dir = dir.path().join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();
        std::fs::write(forge_dir.join("lsp.toml"), "not valid toml = = =").unwrap();
        let mut plugin = make_plugin(dir.path());
        // Doesn't error — host stays disabled but plugin loads.
        plugin.on_init().unwrap();
        assert!(plugin.config.read().unwrap().servers.is_empty());
    }

    #[test]
    fn list_servers_returns_array() {
        let dir = tempdir().unwrap();
        let forge_dir = dir.path().join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();
        std::fs::write(
            forge_dir.join("lsp.toml"),
            r#"
[[servers]]
name = "ra"
command = "rust-analyzer"
file_types = ["rs"]

[[servers]]
name = "ts"
command = "typescript-language-server"
file_types = ["ts", "tsx"]
disabled = true
"#,
        )
        .unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let result = plugin.dispatch(HANDLER_LIST_SERVERS, &json!({})).unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        let names: Vec<&str> = arr.iter().map(|v| v["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"ra"));
        assert!(names.contains(&"ts"));
    }

    #[test]
    fn unknown_handler_returns_error() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        assert!(plugin.dispatch(999, &json!({})).is_err());
    }

    #[test]
    fn async_handler_without_required_args_surfaces_strict_parse_error() {
        // #190 / R7 — previously `str_arg(args, "path")?` and
        // `args.get("line").as_i64()?` returned `None` from
        // `dispatch_async`, the kernel fell back to sync dispatch,
        // and the sync arm errored with the misleading
        // "handler_id N requires dispatch_async". Now the typed
        // `LspPositionArgs` parse surfaces the missing-field error
        // through the future as a clean
        // `PluginError::ExecutionFailed { reason: "<method>: invalid
        // args: …" }`.
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        // Missing every field — strict parse hits "missing field `path`".
        let fut = plugin
            .dispatch_async(HANDLER_HOVER, &json!({}))
            .expect("dispatch_async must return a future (not None)");
        let err = runtime
            .block_on(fut)
            .expect_err("missing fields must error");
        assert!(err.to_string().contains("invalid args"));
        // Path supplied, but missing `line` / `character`.
        let fut = plugin
            .dispatch_async(HANDLER_HOVER, &json!({ "path": "/tmp/x.rs" }))
            .expect("dispatch_async must return a future (not None)");
        let err = runtime
            .block_on(fut)
            .expect_err("missing line/character must error");
        assert!(err.to_string().contains("invalid args"));
    }

    #[test]
    fn file_uri_passthrough_on_already_uri() {
        assert_eq!(file_uri_from_path("file:///tmp/x.rs"), "file:///tmp/x.rs");
        assert_eq!(file_uri_from_path("/tmp/x.rs"), "file:///tmp/x.rs");
    }

    #[test]
    fn infer_language_id_known_extensions() {
        assert_eq!(infer_language_id("/x.rs"), "rust");
        assert_eq!(infer_language_id("/x.tsx"), "typescriptreact");
        assert_eq!(infer_language_id("/x.go"), "go");
        // Unknown extension passes through as-is.
        assert_eq!(infer_language_id("/x.zig"), "zig");
        // No extension yields empty string.
        assert_eq!(infer_language_id("/Makefile"), "");
    }

    #[test]
    fn on_start_succeeds_without_config() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        assert!(plugin.on_start().is_ok());
    }

    #[test]
    fn on_stop_is_safe_with_no_clients() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        plugin.on_stop();
    }

    // ── BL-113 Phase 2b — register_server / unregister_server IPC ──────────────

    fn register_args(name: &str, command: &str, plugin_id: &str) -> Value {
        json!({
            "name": name,
            "command": command,
            "args": [],
            "file_types": ["rs"],
            "root_markers": [],
            "disabled": false,
            "env": {},
            "plugin_id": plugin_id,
        })
    }

    #[test]
    fn register_server_ipc_inserts_and_reports_ok() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let reply = plugin
            .dispatch(
                HANDLER_REGISTER_SERVER,
                &register_args("rust-analyzer", "rust-analyzer", "community.rust"),
            )
            .unwrap();
        assert_eq!(reply["ok"], json!(true));
        assert_eq!(reply["status"], json!("ok"));
        let cfg = plugin.config.read().unwrap();
        assert!(cfg.servers.contains_key("rust-analyzer"));
        assert_eq!(cfg.contributed_by["rust-analyzer"], "community.rust");
    }

    #[test]
    fn register_server_ipc_rejects_collision_with_toml() {
        let dir = tempdir().unwrap();
        let forge_dir = dir.path().join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();
        std::fs::write(
            forge_dir.join("lsp.toml"),
            r#"
[[servers]]
name = "ra"
command = "rust-analyzer-from-toml"
"#,
        )
        .unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let reply = plugin
            .dispatch(
                HANDLER_REGISTER_SERVER,
                &register_args("ra", "rust-analyzer-from-plugin", "community.rust"),
            )
            .unwrap();
        assert_eq!(reply["ok"], json!(false));
        assert_eq!(reply["status"], json!("toml_override"));
        let cfg = plugin.config.read().unwrap();
        assert_eq!(cfg.servers["ra"].command, "rust-analyzer-from-toml");
        assert!(!cfg.contributed_by.contains_key("ra"));
    }

    #[test]
    fn register_server_ipc_rejects_missing_fields() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let err = plugin
            .dispatch(
                HANDLER_REGISTER_SERVER,
                &json!({
                    "name": "ra",
                    "plugin_id": "community.rust",
                }),
            )
            .unwrap_err();
        let PluginError::ExecutionFailed { reason, .. } = err else {
            panic!("expected ExecutionFailed");
        };
        assert!(reason.contains("command"), "reason was: {reason}");
    }

    #[test]
    fn unregister_server_ipc_round_trip_with_owner_match() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        plugin
            .dispatch(
                HANDLER_REGISTER_SERVER,
                &register_args("ra", "rust-analyzer", "community.rust"),
            )
            .unwrap();
        let reply = plugin
            .dispatch(
                HANDLER_UNREGISTER_SERVER,
                &json!({ "name": "ra", "plugin_id": "community.rust" }),
            )
            .unwrap();
        assert_eq!(reply["ok"], json!(true));
        assert_eq!(reply["status"], json!("ok"));
        assert!(plugin.config.read().unwrap().servers.is_empty());
    }

    #[test]
    fn unregister_server_ipc_surfaces_each_skip_reason() {
        let dir = tempdir().unwrap();
        let forge_dir = dir.path().join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();
        std::fs::write(
            forge_dir.join("lsp.toml"),
            r#"
[[servers]]
name = "toml-pinned"
command = "x"
"#,
        )
        .unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        plugin
            .dispatch(
                HANDLER_REGISTER_SERVER,
                &register_args("contrib", "x", "plugin.owner"),
            )
            .unwrap();

        let reply = plugin
            .dispatch(
                HANDLER_UNREGISTER_SERVER,
                &json!({ "name": "ghost", "plugin_id": "anyone" }),
            )
            .unwrap();
        assert_eq!(reply["status"], json!("not_found"));

        let reply = plugin
            .dispatch(
                HANDLER_UNREGISTER_SERVER,
                &json!({ "name": "toml-pinned", "plugin_id": "anyone" }),
            )
            .unwrap();
        assert_eq!(reply["status"], json!("toml_entry"));

        let reply = plugin
            .dispatch(
                HANDLER_UNREGISTER_SERVER,
                &json!({ "name": "contrib", "plugin_id": "plugin.intruder" }),
            )
            .unwrap();
        assert_eq!(reply["status"], json!("not_owned_by_plugin"));
        assert_eq!(reply["actual_owner"], json!("plugin.owner"));

        let cfg = plugin.config.read().unwrap();
        assert!(cfg.servers.contains_key("toml-pinned"));
        assert!(cfg.servers.contains_key("contrib"));
    }
}
