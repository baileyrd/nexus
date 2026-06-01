//! Core plugin for the MCP host subsystem (`com.nexus.mcp.host`).
//!
//! Loads `<forge>/.forge/mcp.toml` at init time and exposes IPC handlers
//! so any other plugin or invoker can enumerate and call tools on external
//! MCP servers without linking the `rmcp` crate directly.
//!
//! # Connection lifecycle
//!
//! Connections are established lazily on the first IPC call that targets
//! a particular server — not eagerly at `on_start`. This avoids blocking
//! startup if a configured server is slow to respond or unavailable.
//!
//! # IPC surface
//!
//! | handler | name | args |
//! |---------|------|------|
//! | 1 | `list_servers` | — |
//! | 2 | `list_tools` | `{"server": "..."}` |
//! | 3 | `call_tool` | `{"server": "...", "tool": "...", "arguments": {...}}` |
//! | 4 | `list_resources` | `{"server": "..."}` |
//! | 5 | `list_prompts` | `{"server": "..."}` |
//! | 6 | `connect` | `{"server": "..."}` |
//! | 7 | `disconnect` | `{"server": "..."}` |
//! | 8 | `register_tool` | `{"name": "...", "description": "...", "input_schema": {...}, "plugin_id": "...", "command": "..."}` |
//! | 9 | `unregister_tool` | `{"name": "..."}` |
//! | 10 | `list_dynamic_tools` | — |
//!
//! Handlers 8–10 (DG-39 / PRD-14 §10) manage the in-process
//! [`dynamic_tools`](crate::dynamic_tools) registry that
//! `NexusMcpServer` consults to expose plugin-published tools.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde_json::json;

use crate::config::{McpMergeSkipReason, McpServerSpec, McpTransport, McpUnregisterError};
use crate::ipc::{
    McpCallToolArgs, McpCallToolReply, McpConnectReply, McpDisconnectMissReply, McpPromptEntry,
    McpRegisterServerArgs, McpRegisterServerReply, McpRegisterToolReply, McpResourceEntry,
    McpServerArgs, McpServerEntry, McpToolEntry, McpUnregisterServerArgs, McpUnregisterServerReply,
    McpUnregisterToolArgs, McpUnregisterToolReply,
};
use crate::pool::{ConnectionPool, PoolConfig};
use crate::{McpClientError, McpHostConfig};

/// Reverse-DNS identifier for this plugin.
pub const PLUGIN_ID: &str = "com.nexus.mcp.host";

/// IPC handler (sync): returns list of configured servers.
pub const HANDLER_LIST_SERVERS: u32 = 1;
/// IPC handler (async): connect to server if needed, then list its tools.
pub const HANDLER_LIST_TOOLS: u32 = 2;
/// IPC handler (async): connect to server if needed, then call a tool.
pub const HANDLER_CALL_TOOL: u32 = 3;
/// IPC handler (async): list resources exposed by a server.
pub const HANDLER_LIST_RESOURCES: u32 = 4;
/// IPC handler (async): list prompts exposed by a server.
pub const HANDLER_LIST_PROMPTS: u32 = 5;
/// IPC handler (async): explicitly establish a connection to a server.
pub const HANDLER_CONNECT: u32 = 6;
/// IPC handler (async): disconnect from a server and free its process.
pub const HANDLER_DISCONNECT: u32 = 7;
/// IPC handler (sync, DG-39): register a tool with the in-process
/// MCP dynamic-tool registry. Args carry `name` / `description` /
/// `input_schema` / `plugin_id` / `command`.
pub const HANDLER_REGISTER_TOOL: u32 = 8;
/// IPC handler (sync, DG-39): unregister a previously-registered
/// dynamic tool. Args carry `name`.
pub const HANDLER_UNREGISTER_TOOL: u32 = 9;
/// IPC handler (sync, DG-39): list every tool currently in the
/// dynamic-tool registry (no args).
pub const HANDLER_LIST_DYNAMIC_TOOLS: u32 = 10;
/// IPC handler (sync, BL-113 Phase 3b): register a plugin-contributed
/// external MCP server.
pub const HANDLER_REGISTER_SERVER: u32 = 11;
/// IPC handler (sync, BL-113 Phase 3b): unregister a previously
/// plugin-contributed external MCP server.
pub const HANDLER_UNREGISTER_SERVER: u32 = 12;

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::mcp::register`. Order
/// matches the pre-SD-06 bootstrap registration so the emitted
/// manifest is byte-identical.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("list_servers", HANDLER_LIST_SERVERS),
    ("list_tools", HANDLER_LIST_TOOLS),
    ("call_tool", HANDLER_CALL_TOOL),
    ("list_resources", HANDLER_LIST_RESOURCES),
    ("list_prompts", HANDLER_LIST_PROMPTS),
    ("connect", HANDLER_CONNECT),
    ("disconnect", HANDLER_DISCONNECT),
    ("register_tool", HANDLER_REGISTER_TOOL),
    ("unregister_tool", HANDLER_UNREGISTER_TOOL),
    ("list_dynamic_tools", HANDLER_LIST_DYNAMIC_TOOLS),
    ("register_server", HANDLER_REGISTER_SERVER),
    ("unregister_server", HANDLER_UNREGISTER_SERVER),
];

/// Core plugin that manages connections to external MCP servers.
///
/// The active server set lives behind a [`RwLock`] so the BL-113
/// `register_server` / `unregister_server` IPC verbs can mutate it at
/// runtime. Async dispatch handlers snapshot the config at dispatch
/// time so an in-flight command keeps the server view it started
/// with even if the master config mutates underneath.
pub struct McpHostPlugin {
    forge_root: PathBuf,
    event_bus: Option<Arc<EventBus>>,
    config: Arc<RwLock<McpHostConfig>>,
    pool: Arc<ConnectionPool>,
}

impl McpHostPlugin {
    /// Create a new (unstarted) MCP host plugin for the given forge root.
    #[must_use]
    pub fn new(forge_root: PathBuf, event_bus: Option<Arc<EventBus>>) -> Self {
        Self {
            forge_root,
            event_bus,
            config: Arc::new(RwLock::new(McpHostConfig::default())),
            pool: Arc::new(ConnectionPool::new(PoolConfig::default())),
        }
    }
}

/// Snapshot the host config behind an `Arc<RwLock>` into a fresh
/// `Arc<McpHostConfig>` so async dispatch keeps its existing
/// pass-by-Arc helper signatures unchanged.
fn snapshot_config(cell: &Arc<RwLock<McpHostConfig>>) -> Arc<McpHostConfig> {
    Arc::new(cell.read().expect("McpHostConfig RwLock poisoned").clone())
}

fn parse_transport(raw: &str) -> McpTransport {
    match raw.trim().to_ascii_lowercase().as_str() {
        "http" => McpTransport::Http,
        "ws" | "websocket" => McpTransport::Websocket,
        _ => McpTransport::Stdio,
    }
}

fn parse_register_server(
    args: &serde_json::Value,
) -> Result<(String, McpServerSpec, String), PluginError> {
    // #190 / R7 — strict-parse via typed `McpRegisterServerArgs`
    // (`deny_unknown_fields`) instead of the prior hand-rolled
    // field-by-field reads off `serde_json::Value`. Defaults for
    // `transport` ("stdio") / `command` ("") / `args` / `env` /
    // `disabled` are carried by the serde struct itself; the field
    // order matches the old shape so callers don't need to change.
    let typed: McpRegisterServerArgs =
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
    if typed.plugin_id.is_empty() {
        return Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "register_server: missing or empty required field `plugin_id`".to_string(),
        });
    }
    let spec = McpServerSpec {
        transport: parse_transport(&typed.transport),
        command: typed.command,
        args: typed.args,
        env: typed.env.into_iter().collect(),
        url: typed.url,
        disabled: typed.disabled,
        ..McpServerSpec::default()
    };
    Ok((typed.name, spec, typed.plugin_id))
}

// #190 — `required_string` helper removed; the register/unregister
// server handlers now strict-parse via typed `McpRegisterServerArgs`
// / `McpUnregisterServerArgs`, and inline the non-empty check on
// the resulting fields.

/// BL-113 Phase 3b — sync IPC handler for `register_server` on
/// `com.nexus.mcp.host`. Same shape as the DAP / LSP register
/// handlers, adapted for MCP's `BTreeMap<name, McpServerSpec>` keying
/// and per-transport validator.
///
/// Trust model (ADR 0027 §Open Question #3): no capability gate at
/// the verb level. Plugins author manifest contributions; the
/// bootstrap-side wiring helper
/// (`nexus-bootstrap::mcp_contribution_wiring::wire_mcp_contributions`)
/// is the only intended caller. Runtime capabilities for MCP server
/// operation (`process.spawn` for stdio transport, `net.connect` for
/// http/websocket) ride on the contributing plugin's existing grants
/// and are checked at the `connect` boundary, not here. Hard
/// enforcement at the verb level is filed as a hardening follow-up.
fn handle_register_server(
    config: &Arc<RwLock<McpHostConfig>>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    let (name, spec, plugin_id) = parse_register_server(args)?;
    let mut cfg = config.write().expect("McpHostConfig RwLock poisoned");
    // #190 / R7 — reply migrates from ad-hoc `json!({"ok": …, "status":
    // …})` to the typed `McpRegisterServerReply` (`deny_unknown_fields`).
    let reply = match cfg.register_contributed(name, spec, plugin_id) {
        Ok(()) => McpRegisterServerReply {
            ok: true,
            status: "ok".to_string(),
            reason: None,
        },
        Err(McpMergeSkipReason::TomlOverride) => McpRegisterServerReply {
            ok: false,
            status: "toml_override".to_string(),
            reason: None,
        },
        Err(McpMergeSkipReason::InvalidName) => McpRegisterServerReply {
            ok: false,
            status: "invalid_name".to_string(),
            reason: None,
        },
        Err(McpMergeSkipReason::Invalid(reason)) => McpRegisterServerReply {
            ok: false,
            status: "invalid".to_string(),
            reason: Some(reason),
        },
    };
    serde_json::to_value(&reply).map_err(|e| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: format!("register_server: serialize reply: {e}"),
    })
}

fn handle_unregister_server(
    config: &Arc<RwLock<McpHostConfig>>,
    args: &serde_json::Value,
) -> Result<serde_json::Value, PluginError> {
    // #190 / R7 — strict-parse via typed `McpUnregisterServerArgs`. The
    // typed struct's `deny_unknown_fields` catches typos that the old
    // `required_string` chain silently passed through.
    let typed: McpUnregisterServerArgs =
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
    let mut cfg = config.write().expect("McpHostConfig RwLock poisoned");
    let reply = match cfg.unregister_contributed(&typed.name, &typed.plugin_id) {
        Ok(_removed) => McpUnregisterServerReply {
            ok: true,
            status: "ok".to_string(),
            actual_owner: None,
        },
        Err(McpUnregisterError::NotFound) => McpUnregisterServerReply {
            ok: false,
            status: "not_found".to_string(),
            actual_owner: None,
        },
        Err(McpUnregisterError::TomlEntry) => McpUnregisterServerReply {
            ok: false,
            status: "toml_entry".to_string(),
            actual_owner: None,
        },
        Err(McpUnregisterError::NotOwnedByPlugin { actual_owner }) => McpUnregisterServerReply {
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

impl CorePlugin for McpHostPlugin {
    fn on_init(&mut self) -> Result<(), PluginError> {
        let mcp_toml = self.forge_root.join(".forge").join("mcp.toml");
        let loaded = match McpHostConfig::read_from(&mcp_toml) {
            Ok(cfg) => {
                tracing::info!(
                    plugin_id = PLUGIN_ID,
                    servers = cfg.servers.len(),
                    "loaded mcp.toml"
                );
                cfg
            }
            Err(crate::McpConfigError::Io { .. }) => {
                tracing::debug!(
                    plugin_id = PLUGIN_ID,
                    "no mcp.toml found — MCP host has no external servers"
                );
                McpHostConfig::default()
            }
            Err(e) => {
                tracing::warn!(plugin_id = PLUGIN_ID, error = %e, "failed to parse mcp.toml");
                McpHostConfig::default()
            }
        };
        *self.config.write().expect("McpHostConfig RwLock poisoned") = loaded;
        Ok(())
    }

    fn on_start(&mut self) -> Result<(), PluginError> {
        let server_count = self
            .config
            .read()
            .expect("McpHostConfig RwLock poisoned")
            .servers
            .len();

        if let Some(bus) = &self.event_bus {
            if let Err(err) = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.mcp.host.started",
                json!({ "configured_servers": server_count }),
            ) {
                tracing::warn!(
                    plugin_id = PLUGIN_ID,
                    %err,
                    "failed to publish mcp.host.started lifecycle event",
                );
            }
        }
        tracing::info!(
            plugin_id = PLUGIN_ID,
            configured_servers = server_count,
            "MCP host started (connections are lazy)"
        );
        Ok(())
    }

    fn on_stop(&mut self) {
        // Best-effort: drop pool — McpClient's Drop sends graceful close.
        // A misbehaving MCP child that ignores the close signal would
        // pre-#85 hang `join()` indefinitely and block kernel shutdown.
        // Now we hard-cap the join with a poll loop; a child that
        // doesn't release the runtime in time gets stranded (the OS
        // reclaims at process exit) but kernel shutdown proceeds.
        let pool = Arc::clone(&self.pool);
        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            if let Ok(rt) = rt {
                rt.block_on(async move {
                    pool.shutdown_all().await;
                });
            }
        });
        const SHUTDOWN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);
        let start = std::time::Instant::now();
        let poll = std::time::Duration::from_millis(50);
        while start.elapsed() < SHUTDOWN_DEADLINE {
            if handle.is_finished() {
                let _ = handle.join();
                tracing::info!(plugin_id = PLUGIN_ID, "MCP host stopped");
                return;
            }
            std::thread::sleep(poll);
        }
        tracing::warn!(
            audit = true,
            plugin_id = PLUGIN_ID,
            timeout_secs = SHUTDOWN_DEADLINE.as_secs(),
            "MCP host shutdown timed out; abandoning the join — child processes \
             may be stranded until the host process exits"
        );
    }

    fn dispatch(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_LIST_SERVERS => {
                // #190 / R7 — materialize into the typed wire shape so the
                // schemars schema generator sees the same fields the runtime
                // emits. `McpServerEntry` already carries `deny_unknown_fields`.
                let cfg = self.config.read().expect("McpHostConfig RwLock poisoned");
                let arr: Vec<McpServerEntry> = cfg
                    .servers
                    .iter()
                    .map(|(name, spec)| McpServerEntry {
                        name: name.clone(),
                        command: spec.command.clone(),
                        args: spec.args.clone(),
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
            HANDLER_REGISTER_TOOL => {
                // #190 / R7 — `DynamicTool` already deserializes via its
                // own typed deser; the args side is therefore already
                // strict. The reply migrates from ad-hoc
                // `json!({"ok": true})` to the typed `McpRegisterToolReply`
                // so the schemars generator sees the wire shape.
                let tool: crate::dynamic_tools::DynamicTool = serde_json::from_value(args.clone())
                    .map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("register_tool: invalid args: {e}"),
                    })?;
                crate::dynamic_tools::global().register(tool).map_err(|e| {
                    PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("register_tool: {e}"),
                    }
                })?;
                serde_json::to_value(&McpRegisterToolReply { ok: true }).map_err(|e| {
                    PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("register_tool: serialize reply: {e}"),
                    }
                })
            }
            HANDLER_UNREGISTER_TOOL => {
                // #190 / R7 — strict-parse args via typed
                // `McpUnregisterToolArgs` (rejects typos like
                // `{ namee: "foo" }` instead of silently meaning
                // "missing 'name' arg") and emit the typed
                // `McpUnregisterToolReply` reply.
                let McpUnregisterToolArgs { name } =
                    serde_json::from_value(args.clone()).map_err(|e| {
                        PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("unregister_tool: invalid args: {e}"),
                        }
                    })?;
                let removed = crate::dynamic_tools::global().unregister(&name);
                serde_json::to_value(&McpUnregisterToolReply { removed, name }).map_err(|e| {
                    PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("unregister_tool: serialize reply: {e}"),
                    }
                })
            }
            HANDLER_LIST_DYNAMIC_TOOLS => {
                let tools = crate::dynamic_tools::global().list();
                Ok(serde_json::to_value(&tools).unwrap_or(serde_json::Value::Array(vec![])))
            }
            HANDLER_LIST_TOOLS
            | HANDLER_CALL_TOOL
            | HANDLER_LIST_RESOURCES
            | HANDLER_LIST_PROMPTS
            | HANDLER_CONNECT
            | HANDLER_DISCONNECT => Err(PluginError::ExecutionFailed {
                plugin_id: PLUGIN_ID.to_string(),
                reason: format!("handler_id {handler_id} requires dispatch_async"),
            }),
            _ => Err(PluginError::ExecutionFailed {
                plugin_id: PLUGIN_ID.to_string(),
                reason: format!("unknown handler_id {handler_id}"),
            }),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn dispatch_async(
        &mut self,
        handler_id: u32,
        args: &serde_json::Value,
    ) -> Option<CorePluginFuture> {
        let pool = Arc::clone(&self.pool);
        // BL-113 Phase 3b — async dispatchers consume an immutable
        // snapshot of the host config taken at dispatch time. A
        // concurrent `register_server` / `unregister_server` mutates
        // the master config but won't affect this in-flight command's
        // server view (the snapshot is per-future, not shared).
        let config = Some(snapshot_config(&self.config));

        match handler_id {
            HANDLER_CONNECT => {
                // #190 / R7 — strict-parse args + typed reply via
                // `McpServerArgs` / `McpConnectReply` (both
                // `deny_unknown_fields`). A parse error needs to be
                // surfaced through the future to avoid the prior
                // quiet-fail mode where `str_arg(...)?` returned `None`
                // and the kernel fell back to sync dispatch.
                let parsed: Result<McpServerArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let McpServerArgs { server } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("connect: invalid args: {e}"),
                        })?;
                    let cfg = config_or_err(config.as_ref())?;
                    pool.get_or_connect(&server, &cfg)
                        .await
                        .map_err(map_client_err)?;
                    let reply = McpConnectReply { ok: true, server };
                    serde_json::to_value(&reply).map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("connect: serialize reply: {e}"),
                    })
                }))
            }

            HANDLER_DISCONNECT => {
                // #190 / R7 — see HANDLER_CONNECT for the parse pattern.
                // Reply shape is `McpConnectReply` for the success branch
                // and the distinct `McpDisconnectMissReply` for the
                // not-connected branch — the wire shapes are different
                // enough that one union type would obscure the contract.
                let parsed: Result<McpServerArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let McpServerArgs { server } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("disconnect: invalid args: {e}"),
                        })?;
                    if pool.disconnect(&server).await {
                        serde_json::to_value(&McpConnectReply { ok: true, server }).map_err(|e| {
                            PluginError::ExecutionFailed {
                                plugin_id: PLUGIN_ID.to_string(),
                                reason: format!("disconnect: serialize reply: {e}"),
                            }
                        })
                    } else {
                        serde_json::to_value(&McpDisconnectMissReply {
                            ok: false,
                            server,
                            reason: "not connected".to_string(),
                        })
                        .map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("disconnect: serialize reply: {e}"),
                        })
                    }
                }))
            }

            HANDLER_LIST_TOOLS => {
                // #190 / R7 — strict-parse args via `McpServerArgs`, reply
                // via `Vec<McpToolEntry>`. Same parse-error-surfacing
                // posture as `HANDLER_CONNECT` above.
                let parsed: Result<McpServerArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let McpServerArgs { server } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("list_tools: invalid args: {e}"),
                        })?;
                    let cfg = config_or_err(config.as_ref())?;
                    let client = pool
                        .get_or_connect(&server, &cfg)
                        .await
                        .map_err(map_client_err)?;
                    let lock = client.lock().await;
                    let tools = lock.list_tools().await.map_err(map_client_err)?;
                    let arr: Vec<McpToolEntry> = tools
                        .iter()
                        .map(|t| {
                            // rmcp stores the schema as Arc<JsonObject>;
                            // wrap it as a JSON object so consumers (the
                            // AI tool bridge) can pass it through to the
                            // model verbatim.
                            let input_schema =
                                serde_json::Value::Object(t.input_schema.as_ref().clone());
                            McpToolEntry {
                                name: t.name.to_string(),
                                description: t.description.as_ref().map(|d| d.to_string()),
                                input_schema: Some(input_schema),
                            }
                        })
                        .collect();
                    serde_json::to_value(&arr).map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("list_tools: serialize reply: {e}"),
                    })
                }))
            }

            HANDLER_LIST_RESOURCES => {
                let parsed: Result<McpServerArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let McpServerArgs { server } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("list_resources: invalid args: {e}"),
                        })?;
                    let cfg = config_or_err(config.as_ref())?;
                    let client = pool
                        .get_or_connect(&server, &cfg)
                        .await
                        .map_err(map_client_err)?;
                    let lock = client.lock().await;
                    let resources = lock.list_resources().await.map_err(map_client_err)?;
                    // `Resource` is `rmcp::model::Annotated<RawResource>` which
                    // only exposes the inner via `Deref`, so we read fields by
                    // reference + clone rather than moving out.
                    let arr: Vec<McpResourceEntry> = resources
                        .iter()
                        .map(|r| McpResourceEntry {
                            uri: r.uri.clone(),
                            name: Some(r.name.clone()),
                            description: r.description.clone(),
                            mime_type: r.mime_type.clone(),
                        })
                        .collect();
                    serde_json::to_value(&arr).map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("list_resources: serialize reply: {e}"),
                    })
                }))
            }

            HANDLER_LIST_PROMPTS => {
                let parsed: Result<McpServerArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let McpServerArgs { server } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("list_prompts: invalid args: {e}"),
                        })?;
                    let cfg = config_or_err(config.as_ref())?;
                    let client = pool
                        .get_or_connect(&server, &cfg)
                        .await
                        .map_err(map_client_err)?;
                    let lock = client.lock().await;
                    let prompts = lock.list_prompts().await.map_err(map_client_err)?;
                    let arr: Vec<McpPromptEntry> = prompts
                        .iter()
                        .map(|p| McpPromptEntry {
                            name: p.name.clone(),
                            description: p.description.clone(),
                        })
                        .collect();
                    serde_json::to_value(&arr).map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("list_prompts: serialize reply: {e}"),
                    })
                }))
            }

            HANDLER_CALL_TOOL => {
                // #190 / R7 — strict-parse via typed `McpCallToolArgs`
                // (`deny_unknown_fields`). Same parse-error-surfacing
                // posture as the other dispatch_async branches.
                let parsed: Result<McpCallToolArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let McpCallToolArgs {
                        server,
                        tool,
                        arguments,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("call_tool: invalid args: {e}"),
                    })?;
                    // `call_tool(name, Option<Map>)`: pass `None` when
                    // the caller omitted `arguments` (typed default
                    // = empty map) to preserve the prior wire shape
                    // — some MCP servers distinguish "no arguments
                    // field" from "empty object".
                    let tool_args = if arguments.is_empty() {
                        None
                    } else {
                        Some(arguments)
                    };
                    let cfg = config_or_err(config.as_ref())?;
                    let client = pool
                        .get_or_connect(&server, &cfg)
                        .await
                        .map_err(map_client_err)?;
                    let lock = client.lock().await;
                    let result = lock
                        .call_tool(tool.clone(), tool_args)
                        .await
                        .map_err(map_client_err)?;
                    // Issue #85. Cap the aggregated tool response so a
                    // misbehaving / malicious MCP server can't stream
                    // gigabyte responses into our memory. We measure
                    // the per-content-item size as we accumulate so
                    // the early items still surface even if the tail
                    // is rejected.
                    const MAX_TOOL_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
                    const MAX_TOOL_RESPONSE_ITEMS: usize = 1024;
                    let mut content: Vec<serde_json::Value> = Vec::new();
                    let mut total_bytes: usize = 0;
                    let mut truncated = false;
                    for item in &result.content {
                        if content.len() >= MAX_TOOL_RESPONSE_ITEMS {
                            truncated = true;
                            break;
                        }
                        let Ok(v) = serde_json::to_value(item) else {
                            continue;
                        };
                        let item_bytes = serde_json::to_vec(&v).map(|b| b.len()).unwrap_or(0);
                        if total_bytes.saturating_add(item_bytes) > MAX_TOOL_RESPONSE_BYTES {
                            truncated = true;
                            break;
                        }
                        total_bytes = total_bytes.saturating_add(item_bytes);
                        content.push(v);
                    }
                    if truncated {
                        tracing::warn!(
                            audit = true,
                            plugin_id = PLUGIN_ID,
                            server = %server,
                            tool = %tool,
                            item_count = content.len(),
                            byte_count = total_bytes,
                            "MCP tool response truncated to fit response cap \
                             ({MAX_TOOL_RESPONSE_ITEMS} items / \
                             {MAX_TOOL_RESPONSE_BYTES} bytes)"
                        );
                    }
                    let reply = McpCallToolReply {
                        content,
                        is_error: result.is_error.unwrap_or(false),
                        truncated,
                    };
                    serde_json::to_value(&reply).map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("call_tool: serialize reply: {e}"),
                    })
                }))
            }

            _ => None,
        }
    }
}

fn config_or_err(config: Option<&Arc<McpHostConfig>>) -> Result<Arc<McpHostConfig>, PluginError> {
    config.cloned().ok_or_else(|| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: "MCP host config not loaded".to_string(),
    })
}

// Used as a function pointer by `Result::map_err`, which forces the
// by-value signature; wrapping in a closure would re-trip
// `redundant_closure`.
#[allow(clippy::needless_pass_by_value)]
fn map_client_err(e: McpClientError) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_plugin(dir: &std::path::Path) -> McpHostPlugin {
        McpHostPlugin::new(dir.to_path_buf(), None)
    }

    #[test]
    fn plugin_id_is_correct() {
        assert_eq!(PLUGIN_ID, "com.nexus.mcp.host");
    }

    #[test]
    fn on_init_with_no_mcp_toml_succeeds() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        assert!(plugin.on_init().is_ok());
        let cfg = plugin.config.read().unwrap();
        assert!(cfg.servers.is_empty());
    }

    #[test]
    fn on_init_with_valid_mcp_toml_loads_config() {
        let dir = tempdir().unwrap();
        let forge_dir = dir.path().join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();
        std::fs::write(
            forge_dir.join("mcp.toml"),
            r#"
[servers.test]
command = "echo"
args = ["hello"]
"#,
        )
        .unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let cfg = plugin.config.read().unwrap();
        assert!(cfg.servers.contains_key("test"));
    }

    #[test]
    fn list_servers_returns_empty_without_config() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let result = plugin.dispatch(HANDLER_LIST_SERVERS, &json!({}));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), serde_json::Value::Array(vec![]));
    }

    #[test]
    fn list_servers_returns_configured_servers() {
        let dir = tempdir().unwrap();
        let forge_dir = dir.path().join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();
        std::fs::write(
            forge_dir.join("mcp.toml"),
            r#"
[servers.fs]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]

[servers.gh]
command = "uvx"
args = ["mcp-server-github"]
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
        assert!(names.contains(&"fs"));
        assert!(names.contains(&"gh"));
    }

    #[test]
    fn async_handler_without_server_arg_surfaces_strict_parse_error() {
        // #190 / R7 — previously `str_arg(args, "server")?` quietly
        // returned `None` from `dispatch_async`, the kernel then fell
        // back to sync dispatch, and the sync arm errored with the
        // misleading "handler_id N requires dispatch_async". Now the
        // missing `server` field surfaces through the future as a
        // clean `PluginError::ExecutionFailed { reason: "list_tools:
        // invalid args: …" }`.
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let fut = plugin
            .dispatch_async(HANDLER_LIST_TOOLS, &json!({}))
            .expect(
            "dispatch_async must return a future (not None) so the parse error reaches the caller",
        );
        let err = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(fut)
            .expect_err("missing 'server' field must error");
        let msg = err.to_string();
        assert!(
            msg.contains("invalid args") && msg.contains("server"),
            "error should mention missing 'server' field, got: {msg}",
        );
    }

    #[test]
    fn unknown_handler_returns_error() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let result = plugin.dispatch(999, &json!({}));
        assert!(result.is_err());
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
        plugin.on_stop(); // must not panic
    }

    // ── DG-39 dynamic-tool dispatch tests ────────────────────────────────────
    //
    // Note: the registry is process-global, so every test below uses a
    // uniquely-named tool to avoid cross-test interference. Each test
    // cleans up by unregistering at the end.

    #[test]
    fn register_tool_inserts_into_global_registry() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let name = "dg39_register_inserts";
        let result = plugin
            .dispatch(
                HANDLER_REGISTER_TOOL,
                &json!({
                    "name": name,
                    "description": "test",
                    "input_schema": { "type": "object", "properties": {} },
                    "plugin_id": "com.example.test",
                    "command": "do_thing",
                }),
            )
            .unwrap();
        assert_eq!(result, json!({ "ok": true }));
        let entry = crate::dynamic_tools::global().lookup(name);
        assert!(entry.is_some());
        assert!(crate::dynamic_tools::global().unregister(name));
    }

    #[test]
    fn register_tool_rejects_reserved_prefix() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let err = plugin
            .dispatch(
                HANDLER_REGISTER_TOOL,
                &json!({
                    "name": "nexus_evil_override",
                    "description": "naughty",
                    "input_schema": {},
                    "plugin_id": "com.example.test",
                    "command": "do_thing",
                }),
            )
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("reserved") || msg.contains("nexus_"),
            "expected reserved-prefix error, got: {msg}"
        );
    }

    #[test]
    fn unregister_tool_returns_removed_flag() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let name = "dg39_unregister_flag";
        plugin
            .dispatch(
                HANDLER_REGISTER_TOOL,
                &json!({
                    "name": name,
                    "description": "test",
                    "input_schema": {},
                    "plugin_id": "com.example.test",
                    "command": "do_thing",
                }),
            )
            .unwrap();
        let result = plugin
            .dispatch(HANDLER_UNREGISTER_TOOL, &json!({ "name": name }))
            .unwrap();
        assert_eq!(result["removed"], json!(true));
        // Second unregister reports false.
        let again = plugin
            .dispatch(HANDLER_UNREGISTER_TOOL, &json!({ "name": name }))
            .unwrap();
        assert_eq!(again["removed"], json!(false));
    }

    #[test]
    fn list_dynamic_tools_returns_registered_entries() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let name = "dg39_list_returns";
        plugin
            .dispatch(
                HANDLER_REGISTER_TOOL,
                &json!({
                    "name": name,
                    "description": "test desc",
                    "input_schema": { "type": "object", "properties": {} },
                    "plugin_id": "com.example.test",
                    "command": "do_thing",
                }),
            )
            .unwrap();
        let arr = plugin
            .dispatch(HANDLER_LIST_DYNAMIC_TOOLS, &json!({}))
            .unwrap();
        let tools = arr.as_array().unwrap();
        assert!(
            tools.iter().any(|t| t["name"] == json!(name)),
            "registered tool '{name}' not in list: {arr}"
        );
        plugin
            .dispatch(HANDLER_UNREGISTER_TOOL, &json!({ "name": name }))
            .unwrap();
    }

    // ── BL-113 Phase 3b — register_server / unregister_server IPC ──────────────

    fn register_server_args(name: &str, command: &str, plugin_id: &str) -> serde_json::Value {
        json!({
            "name": name,
            "transport": "stdio",
            "command": command,
            "args": [],
            "env": {},
            "disabled": false,
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
                &register_server_args("fs", "filesystem-mcp", "community.fs"),
            )
            .unwrap();
        assert_eq!(reply["ok"], json!(true));
        assert_eq!(reply["status"], json!("ok"));
        let cfg = plugin.config.read().unwrap();
        assert!(cfg.servers.contains_key("fs"));
        assert_eq!(cfg.contributed_by["fs"], "community.fs");
    }

    #[test]
    fn register_server_ipc_rejects_collision_with_toml() {
        let dir = tempdir().unwrap();
        let forge_dir = dir.path().join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();
        std::fs::write(
            forge_dir.join("mcp.toml"),
            r#"
[servers.fs]
command = "filesystem-from-toml"
"#,
        )
        .unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let reply = plugin
            .dispatch(
                HANDLER_REGISTER_SERVER,
                &register_server_args("fs", "filesystem-from-plugin", "community.fs"),
            )
            .unwrap();
        assert_eq!(reply["ok"], json!(false));
        assert_eq!(reply["status"], json!("toml_override"));
        let cfg = plugin.config.read().unwrap();
        assert_eq!(cfg.servers["fs"].command, "filesystem-from-toml");
        assert!(!cfg.contributed_by.contains_key("fs"));
    }

    #[test]
    fn register_server_ipc_surfaces_invalid_for_empty_command() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        // Empty command on stdio transport triggers the spec validator.
        let reply = plugin
            .dispatch(
                HANDLER_REGISTER_SERVER,
                &json!({
                    "name": "bad",
                    "transport": "stdio",
                    "command": "",
                    "plugin_id": "community.bad",
                }),
            )
            .unwrap();
        assert_eq!(reply["status"], json!("invalid"));
        let reason = reply["reason"].as_str().unwrap_or("");
        assert!(reason.contains("empty command"), "reason was: {reason}");
        assert!(plugin.config.read().unwrap().servers.is_empty());
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
                &register_server_args("fs", "filesystem-mcp", "community.fs"),
            )
            .unwrap();
        let reply = plugin
            .dispatch(
                HANDLER_UNREGISTER_SERVER,
                &json!({ "name": "fs", "plugin_id": "community.fs" }),
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
            forge_dir.join("mcp.toml"),
            r#"
[servers.toml-pinned]
command = "x"
"#,
        )
        .unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        plugin
            .dispatch(
                HANDLER_REGISTER_SERVER,
                &register_server_args("contrib", "x", "plugin.owner"),
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
