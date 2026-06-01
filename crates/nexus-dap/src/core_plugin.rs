//! Core plugin for the DAP host (`com.nexus.dap`).
//!
//! Loads `<forge>/.forge/dap.toml` at init time, exposes IPC handlers
//! that proxy DAP requests to the right child adapter, and
//! republishes adapter-pushed events on the kernel event bus.
//!
//! # IPC surface
//!
//! | id | name | summary |
//! |---|---|---|
//! | 1 | `list_adapters` | configured + connected adapters |
//! | 2 | `launch` | spawn + send `launch` |
//! | 3 | `attach` | spawn + send `attach` |
//! | 4 | `configuration_done` | post-breakpoint handshake |
//! | 5 | `disconnect` | graceful tear-down |
//! | 6 | `terminate` | force-stop the debuggee |
//! | 7 | `set_breakpoints` | replace per-source breakpoints |
//! | 8 | `set_function_breakpoints` | function-name breakpoints |
//! | 9 | `set_exception_breakpoints` | exception filters |
//! | 10 | `continue` | resume |
//! | 11 | `next` | step over |
//! | 12 | `step_in` | step in |
//! | 13 | `step_out` | step out |
//! | 14 | `pause` | request a stop |
//! | 15 | `threads` | enumerate threads |
//! | 16 | `stack_trace` | frames for a thread |
//! | 17 | `scopes` | scopes for a frame |
//! | 18 | `variables` | resolve a `variablesReference` |
//! | 19 | `evaluate` | REPL / watch evaluation |
//! | 20 | `register_adapter` | BL-113 plugin-contributed adapter add |
//! | 21 | `unregister_adapter` | BL-113 plugin-contributed adapter remove |
//!
//! # Bus events
//!
//! Every adapter event fans out as `com.nexus.dap.<event>` with the
//! adapter `body` preserved verbatim. Known events: `initialized`,
//! `stopped`, `continued`, `exited`, `terminated`, `thread`,
//! `output`, `breakpoint`, `module`, `process`, `capabilities`. Unknown
//! event names pass through unchanged.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde_json::{json, Value};

use crate::client::{DapClient, DapClientError, SourceBreakpointSpec};
use crate::config::{DapAdapterSpec, MergeSkipReason, UnregisterError};
use crate::ipc::{
    DapAdapterArgs, DapAdapterEntry, DapAttachArgs, DapEvaluateArgs, DapLaunchArgs, DapOk,
    DapRegisterAdapterArgs, DapRegisterAdapterReply, DapScopesArgs, DapSetBreakpointsArgs,
    DapSetExceptionBreakpointsArgs, DapSetFunctionBreakpointsArgs, DapSourceBreakpoint,
    DapStackTraceArgs, DapThreadArgs, DapUnregisterAdapterArgs, DapUnregisterAdapterReply,
    DapVariablesArgs,
};
use crate::pool::{ConnectionPool, PoolConfig};
use crate::{DapConfigError, DapHostConfig};

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = "com.nexus.dap";

pub const HANDLER_LIST_ADAPTERS: u32 = 1;
pub const HANDLER_LAUNCH: u32 = 2;
pub const HANDLER_ATTACH: u32 = 3;
pub const HANDLER_CONFIGURATION_DONE: u32 = 4;
pub const HANDLER_DISCONNECT: u32 = 5;
pub const HANDLER_TERMINATE: u32 = 6;
pub const HANDLER_SET_BREAKPOINTS: u32 = 7;
pub const HANDLER_SET_FUNCTION_BREAKPOINTS: u32 = 8;
pub const HANDLER_SET_EXCEPTION_BREAKPOINTS: u32 = 9;
pub const HANDLER_CONTINUE: u32 = 10;
pub const HANDLER_NEXT: u32 = 11;
pub const HANDLER_STEP_IN: u32 = 12;
pub const HANDLER_STEP_OUT: u32 = 13;
pub const HANDLER_PAUSE: u32 = 14;
pub const HANDLER_THREADS: u32 = 15;
pub const HANDLER_STACK_TRACE: u32 = 16;
pub const HANDLER_SCOPES: u32 = 17;
pub const HANDLER_VARIABLES: u32 = 18;
pub const HANDLER_EVALUATE: u32 = 19;
pub const HANDLER_REGISTER_ADAPTER: u32 = 20;
pub const HANDLER_UNREGISTER_ADAPTER: u32 = 21;

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::dap::register`. Order
/// matches the pre-SD-06 bootstrap registration.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("list_adapters", HANDLER_LIST_ADAPTERS),
    ("launch", HANDLER_LAUNCH),
    ("attach", HANDLER_ATTACH),
    ("configuration_done", HANDLER_CONFIGURATION_DONE),
    ("disconnect", HANDLER_DISCONNECT),
    ("terminate", HANDLER_TERMINATE),
    ("set_breakpoints", HANDLER_SET_BREAKPOINTS),
    ("set_function_breakpoints", HANDLER_SET_FUNCTION_BREAKPOINTS),
    (
        "set_exception_breakpoints",
        HANDLER_SET_EXCEPTION_BREAKPOINTS,
    ),
    ("continue", HANDLER_CONTINUE),
    ("next", HANDLER_NEXT),
    ("step_in", HANDLER_STEP_IN),
    ("step_out", HANDLER_STEP_OUT),
    ("pause", HANDLER_PAUSE),
    ("threads", HANDLER_THREADS),
    ("stack_trace", HANDLER_STACK_TRACE),
    ("scopes", HANDLER_SCOPES),
    ("variables", HANDLER_VARIABLES),
    ("evaluate", HANDLER_EVALUATE),
    ("register_adapter", HANDLER_REGISTER_ADAPTER),
    ("unregister_adapter", HANDLER_UNREGISTER_ADAPTER),
];

/// Async IPC verbs require `dispatch_async`. Listed once so the sync
/// `dispatch` arm can route them with a clear error.
const ASYNC_HANDLERS: &[u32] = &[
    HANDLER_LAUNCH,
    HANDLER_ATTACH,
    HANDLER_CONFIGURATION_DONE,
    HANDLER_DISCONNECT,
    HANDLER_TERMINATE,
    HANDLER_SET_BREAKPOINTS,
    HANDLER_SET_FUNCTION_BREAKPOINTS,
    HANDLER_SET_EXCEPTION_BREAKPOINTS,
    HANDLER_CONTINUE,
    HANDLER_NEXT,
    HANDLER_STEP_IN,
    HANDLER_STEP_OUT,
    HANDLER_PAUSE,
    HANDLER_THREADS,
    HANDLER_STACK_TRACE,
    HANDLER_SCOPES,
    HANDLER_VARIABLES,
    HANDLER_EVALUATE,
];

/// Core plugin that manages connections to DAP adapters.
///
/// The active adapter set lives behind a [`RwLock`] so the
/// BL-113 `register_adapter` / `unregister_adapter` IPC verbs can
/// mutate it at runtime after a plugin activates / deactivates.
/// Async dispatch handlers snapshot the config at dispatch time
/// (see [`snapshot_config`]) so an in-flight command keeps the
/// adapter view it started with even if the master config mutates
/// underneath.
pub struct DapCorePlugin {
    forge_root: PathBuf,
    event_bus: Option<Arc<EventBus>>,
    config: Arc<RwLock<DapHostConfig>>,
    pool: Arc<ConnectionPool>,
}

impl DapCorePlugin {
    /// Create a new (unstarted) DAP host plugin for the given forge root.
    #[must_use]
    pub fn new(forge_root: PathBuf, event_bus: Option<Arc<EventBus>>) -> Self {
        let pool = Arc::new(ConnectionPool::new(PoolConfig::default()));
        Self {
            forge_root,
            event_bus,
            config: Arc::new(RwLock::new(DapHostConfig::default())),
            pool,
        }
    }
}

/// Snapshot the host config behind an `Arc<RwLock>` into a fresh
/// `Arc<DapHostConfig>` so async dispatch keeps its existing
/// pass-by-Arc helper signatures unchanged.
fn snapshot_config(cell: &Arc<RwLock<DapHostConfig>>) -> Arc<DapHostConfig> {
    Arc::new(cell.read().expect("DapHostConfig RwLock poisoned").clone())
}

/// BL-113 Phase 1b — sync IPC handler for `register_adapter`.
///
/// Parses `args` into a [`DapAdapterSpec`] + `plugin_id`, takes the
/// host config's write lock, delegates the merge to
/// [`DapHostConfig::register_contributed`], and returns a
/// `{ok, status}` envelope. Validation errors are surfaced as a
/// "skip" status (not a `PluginError`) so the caller can decide
/// whether to log + continue or escalate.
///
/// **Trust model (ADR 0027 §Open Question #3, confirmed Phase 1b):**
/// no capability gate at the verb level. Plugins author manifest
/// contributions; the bootstrap-side wiring helper
/// (`nexus-bootstrap::dap_contribution_wiring::wire_dap_contributions`)
/// is the only intended caller in tree. A plugin with `ipc.call`
/// could reach this verb directly today, but doing so bypasses the
/// manifest pipeline (no `contributed_by` provenance via the proper
/// lifecycle path, no marketplace install record); the resulting
/// adapter still can't *spawn* anything its contributing plugin
/// doesn't hold `process.spawn` for, because spawn capability is
/// checked at the `launch` / `attach` boundary, not here. Hard
/// enforcement at the verb level needs kernel-side caller-identity
/// threading; filed as a hardening follow-up.
fn handle_register_adapter(
    config: &Arc<RwLock<DapHostConfig>>,
    args: &Value,
) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via typed `DapRegisterAdapterArgs`
    // (`deny_unknown_fields`). The prior hand-rolled
    // `parse_register_adapter_spec` + `parse_string_field` silently
    // accepted unknown fields and let typos like
    // `{ commandd: "..." }` through.
    let typed: DapRegisterAdapterArgs =
        serde_json::from_value(args.clone()).map_err(|e| PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!("register_adapter: invalid args: {e}"),
        })?;
    if typed.name.is_empty() {
        return Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "register_adapter: missing or empty required field `name`".to_string(),
        });
    }
    if typed.command.is_empty() {
        return Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "register_adapter: missing or empty required field `command`".to_string(),
        });
    }
    if typed.plugin_id.is_empty() {
        return Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "register_adapter: missing or empty required field `plugin_id`".to_string(),
        });
    }
    let spec = DapAdapterSpec {
        name: typed.name,
        command: typed.command,
        args: typed.args,
        adapter_type: typed.adapter_type,
        file_types: typed.file_types,
        disabled: typed.disabled,
        env: typed.env,
        metadata: typed.metadata,
    };
    let mut cfg = config.write().expect("DapHostConfig RwLock poisoned");
    let reply = match cfg.register_contributed(spec, typed.plugin_id) {
        Ok(()) => DapRegisterAdapterReply {
            ok: true,
            status: "ok".to_string(),
        },
        Err(MergeSkipReason::TomlOverride) => DapRegisterAdapterReply {
            ok: false,
            status: "toml_override".to_string(),
        },
        Err(MergeSkipReason::InvalidName) => DapRegisterAdapterReply {
            ok: false,
            status: "invalid_name".to_string(),
        },
        Err(MergeSkipReason::InvalidCommand) => DapRegisterAdapterReply {
            ok: false,
            status: "invalid_command".to_string(),
        },
    };
    serde_json::to_value(&reply).map_err(|e| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: format!("register_adapter: serialize reply: {e}"),
    })
}

/// BL-113 Phase 1b — sync IPC handler for `unregister_adapter`.
///
/// Parses `name` + `plugin_id` out of `args` and delegates to
/// [`DapHostConfig::unregister_contributed`]. Authorisation is
/// enforced inside the config method (the `plugin_id` must match the
/// contributing plugin recorded at register time). On
/// `NotOwnedByPlugin` the reply carries `actual_owner` so the caller
/// can log who actually contributed the entry.
fn handle_unregister_adapter(
    config: &Arc<RwLock<DapHostConfig>>,
    args: &Value,
) -> Result<Value, PluginError> {
    // #190 / R7 — strict-parse via typed `DapUnregisterAdapterArgs`,
    // typed `DapUnregisterAdapterReply`.
    let typed: DapUnregisterAdapterArgs =
        serde_json::from_value(args.clone()).map_err(|e| PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!("unregister_adapter: invalid args: {e}"),
        })?;
    if typed.name.is_empty() {
        return Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "unregister_adapter: missing or empty required field `name`".to_string(),
        });
    }
    if typed.plugin_id.is_empty() {
        return Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: "unregister_adapter: missing or empty required field `plugin_id`".to_string(),
        });
    }
    let mut cfg = config.write().expect("DapHostConfig RwLock poisoned");
    let reply = match cfg.unregister_contributed(&typed.name, &typed.plugin_id) {
        Ok(_removed) => DapUnregisterAdapterReply {
            ok: true,
            status: "ok".to_string(),
            actual_owner: None,
        },
        Err(UnregisterError::NotFound) => DapUnregisterAdapterReply {
            ok: false,
            status: "not_found".to_string(),
            actual_owner: None,
        },
        Err(UnregisterError::TomlEntry) => DapUnregisterAdapterReply {
            ok: false,
            status: "toml_entry".to_string(),
            actual_owner: None,
        },
        Err(UnregisterError::NotOwnedByPlugin { actual_owner }) => DapUnregisterAdapterReply {
            ok: false,
            status: "not_owned_by_plugin".to_string(),
            actual_owner: Some(actual_owner),
        },
    };
    serde_json::to_value(&reply).map_err(|e| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: format!("unregister_adapter: serialize reply: {e}"),
    })
}

// #190 — `parse_string_field` and `parse_register_adapter_spec`
// helpers removed; both `handle_register_adapter` and
// `handle_unregister_adapter` now strict-parse via typed
// `DapRegisterAdapterArgs` / `DapUnregisterAdapterArgs` and inline
// the non-empty checks.

impl CorePlugin for DapCorePlugin {
    fn on_init(&mut self) -> Result<(), PluginError> {
        let toml_path = self.forge_root.join(".forge").join("dap.toml");
        let loaded = match DapHostConfig::read_from(&toml_path) {
            Ok(cfg) => {
                tracing::info!(
                    plugin_id = PLUGIN_ID,
                    adapters = cfg.adapters.len(),
                    "loaded dap.toml"
                );
                cfg
            }
            Err(DapConfigError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                tracing::debug!(
                    plugin_id = PLUGIN_ID,
                    "no dap.toml found — DAP host has no adapters configured"
                );
                DapHostConfig::default()
            }
            Err(e) => {
                tracing::warn!(
                    plugin_id = PLUGIN_ID,
                    error = %e,
                    "failed to parse dap.toml — DAP host disabled"
                );
                DapHostConfig::default()
            }
        };
        *self.config.write().expect("DapHostConfig RwLock poisoned") = loaded;
        Ok(())
    }

    fn on_start(&mut self) -> Result<(), PluginError> {
        let adapter_count = self
            .config
            .read()
            .expect("DapHostConfig RwLock poisoned")
            .adapters
            .len();
        if let Some(bus) = &self.event_bus {
            let _ = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.dap.started",
                json!({ "configured_adapters": adapter_count }),
            );
        }
        tracing::info!(
            plugin_id = PLUGIN_ID,
            configured_adapters = adapter_count,
            "DAP host started (connections are lazy)"
        );
        Ok(())
    }

    fn on_stop(&mut self) {
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
                tracing::info!(plugin_id = PLUGIN_ID, "DAP host stopped");
                return;
            }
            std::thread::sleep(poll_interval);
        }
        tracing::warn!(
            audit = true,
            plugin_id = PLUGIN_ID,
            timeout_secs = SHUTDOWN_DEADLINE.as_secs(),
            "DAP host shutdown timed out; abandoning the join — child processes \
             may be stranded until the host process exits"
        );
    }

    fn dispatch(&mut self, handler_id: u32, args: &Value) -> Result<Value, PluginError> {
        match handler_id {
            HANDLER_LIST_ADAPTERS => {
                // The sync `list_adapters` reports the configured set
                // only — connected state needs an async pool call.
                // Callers that want the merged view should hit the
                // async handler (we route both ids through
                // `dispatch_async` below; this sync arm is the
                // fallback for the rare invoker that bypasses async).
                let cfg = self.config.read().expect("DapHostConfig RwLock poisoned");
                let entries: Vec<DapAdapterEntry> = cfg
                    .adapters
                    .values()
                    .map(|spec| adapter_entry(spec, false))
                    .collect();
                serde_json::to_value(&entries).map_err(|e| PluginError::ExecutionFailed {
                    plugin_id: PLUGIN_ID.to_string(),
                    reason: format!("list_adapters: serialize reply: {e}"),
                })
            }
            HANDLER_REGISTER_ADAPTER => handle_register_adapter(&self.config, args),
            HANDLER_UNREGISTER_ADAPTER => handle_unregister_adapter(&self.config, args),
            id if ASYNC_HANDLERS.contains(&id) => Err(PluginError::ExecutionFailed {
                plugin_id: PLUGIN_ID.to_string(),
                reason: format!("handler_id {id} requires dispatch_async"),
            }),
            _ => Err(PluginError::ExecutionFailed {
                plugin_id: PLUGIN_ID.to_string(),
                reason: format!("unknown handler_id {handler_id}"),
            }),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn dispatch_async(&mut self, handler_id: u32, args: &Value) -> Option<CorePluginFuture> {
        let pool = Arc::clone(&self.pool);
        // BL-113 Phase 1b — async dispatchers consume an immutable
        // snapshot of the host config taken at dispatch time. A
        // concurrent `register_adapter` / `unregister_adapter` mutates
        // the master config but won't affect this in-flight command's
        // adapter view (the snapshot is per-future, not shared).
        let config = Some(snapshot_config(&self.config));
        let bus = self.event_bus.clone();

        match handler_id {
            HANDLER_LAUNCH => {
                // #190 / R7 — strict-parse via typed `DapLaunchArgs`.
                let parsed: Result<DapLaunchArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let DapLaunchArgs {
                        adapter,
                        program,
                        mode,
                        args: launch_args_vec,
                        cwd,
                        env,
                        stop_on_entry,
                        extra,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("launch: invalid args: {e}"),
                    })?;
                    let mut launch_args = json!({
                        "program": program,
                        "stopOnEntry": stop_on_entry,
                    });
                    if let Some(m) = mode {
                        launch_args["mode"] = json!(m);
                    }
                    if let Some(c) = cwd {
                        launch_args["cwd"] = json!(c);
                    }
                    if !launch_args_vec.is_empty() {
                        launch_args["args"] = json!(launch_args_vec);
                    }
                    if !env.is_empty() {
                        launch_args["env"] = json!(env);
                    }
                    if let Some(Value::Object(extra_map)) = extra {
                        if let Value::Object(map) = &mut launch_args {
                            for (k, v) in extra_map {
                                map.entry(k).or_insert(v);
                            }
                        }
                    }
                    run_send_command(pool, config, bus, adapter, "launch", Some(launch_args)).await
                }))
            }

            HANDLER_ATTACH => {
                // #190 / R7 — strict-parse via typed `DapAttachArgs`.
                let parsed: Result<DapAttachArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let DapAttachArgs {
                        adapter,
                        pid,
                        port,
                        extra,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("attach: invalid args: {e}"),
                    })?;
                    let mut attach_args = serde_json::Map::new();
                    if let Some(pid) = pid {
                        attach_args.insert("pid".to_string(), json!(pid));
                    }
                    if let Some(port) = port {
                        attach_args.insert("port".to_string(), json!(port));
                    }
                    if let Some(Value::Object(extra_map)) = extra {
                        for (k, v) in extra_map {
                            attach_args.entry(k).or_insert(v);
                        }
                    }
                    run_send_command(
                        pool,
                        config,
                        bus,
                        adapter,
                        "attach",
                        Some(Value::Object(attach_args)),
                    )
                    .await
                }))
            }

            HANDLER_CONFIGURATION_DONE => {
                // #190 / R7 — strict-parse via typed `DapAdapterArgs`.
                let parsed: Result<DapAdapterArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let DapAdapterArgs { adapter, .. } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("configuration_done: invalid args: {e}"),
                        })?;
                    run_send_ack(pool, config, bus, adapter, "configurationDone", None).await
                }))
            }

            HANDLER_DISCONNECT => {
                // #190 / R7 — strict-parse via typed `DapAdapterArgs`.
                let parsed: Result<DapAdapterArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let DapAdapterArgs {
                        adapter,
                        terminate_debuggee,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("disconnect: invalid args: {e}"),
                    })?;
                    let payload = json!({
                        "restart": false,
                        "terminateDebuggee": terminate_debuggee,
                    });
                    run_send_ack(pool, config, bus, adapter, "disconnect", Some(payload)).await
                }))
            }

            HANDLER_TERMINATE => {
                // #190 / R7 — strict-parse via typed `DapAdapterArgs`.
                let parsed: Result<DapAdapterArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let DapAdapterArgs { adapter, .. } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("terminate: invalid args: {e}"),
                        })?;
                    run_send_ack(pool, config, bus, adapter, "terminate", None).await
                }))
            }

            HANDLER_SET_BREAKPOINTS => {
                // #190 / R7 — strict-parse via typed `DapSetBreakpointsArgs`.
                let parsed: Result<DapSetBreakpointsArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let DapSetBreakpointsArgs {
                        adapter,
                        source_path,
                        breakpoints,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("set_breakpoints: invalid args: {e}"),
                    })?;
                    let specs: Vec<SourceBreakpointSpec> =
                        breakpoints.iter().map(typed_to_spec).collect();
                    let wire_bps: Vec<Value> = specs.iter().map(spec_to_wire).collect();
                    let payload = json!({
                        "source": { "path": source_path.clone() },
                        "breakpoints": wire_bps,
                    });
                    let cfg = config_or_err(config.as_ref())?;
                    let result = pool
                        .call_with_reconnect(&adapter, &cfg, move |client| {
                            let bus = bus.clone();
                            let payload = payload.clone();
                            let source_path = source_path.clone();
                            let specs = specs.clone();
                            Box::pin(async move {
                                let lock = client.lock().await;
                                let r = lock.send_request("setBreakpoints", Some(payload)).await?;
                                lock.remember_breakpoints(&source_path, specs).await;
                                republish_pending(&lock, bus.as_ref()).await;
                                Ok(r.unwrap_or(Value::Null))
                            })
                        })
                        .await
                        .map_err(map_client_err)?;
                    Ok(result)
                }))
            }

            HANDLER_SET_FUNCTION_BREAKPOINTS => {
                // #190 / R7 — strict-parse via typed
                // `DapSetFunctionBreakpointsArgs`. Function breakpoints
                // are sent verbatim — the typed `name` / `condition`
                // rows match DAP's wire shape.
                let parsed: Result<DapSetFunctionBreakpointsArgs, _> =
                    serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let DapSetFunctionBreakpointsArgs {
                        adapter,
                        breakpoints,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("set_function_breakpoints: invalid args: {e}"),
                    })?;
                    let payload = json!({ "breakpoints": breakpoints });
                    run_send_command(
                        pool,
                        config,
                        bus,
                        adapter,
                        "setFunctionBreakpoints",
                        Some(payload),
                    )
                    .await
                }))
            }

            HANDLER_SET_EXCEPTION_BREAKPOINTS => {
                // #190 / R7 — strict-parse via typed
                // `DapSetExceptionBreakpointsArgs`.
                let parsed: Result<DapSetExceptionBreakpointsArgs, _> =
                    serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let DapSetExceptionBreakpointsArgs { adapter, filters } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("set_exception_breakpoints: invalid args: {e}"),
                        })?;
                    let payload = json!({ "filters": filters });
                    run_send_ack(
                        pool,
                        config,
                        bus,
                        adapter,
                        "setExceptionBreakpoints",
                        Some(payload),
                    )
                    .await
                }))
            }

            HANDLER_CONTINUE => Some(proxy_thread_request(args, config, pool, bus, "continue")),
            HANDLER_NEXT => Some(proxy_thread_request(args, config, pool, bus, "next")),
            HANDLER_STEP_IN => Some(proxy_thread_request(args, config, pool, bus, "stepIn")),
            HANDLER_STEP_OUT => Some(proxy_thread_request(args, config, pool, bus, "stepOut")),
            HANDLER_PAUSE => Some(proxy_thread_request(args, config, pool, bus, "pause")),

            HANDLER_THREADS => {
                // #190 / R7 — strict-parse via typed `DapAdapterArgs`.
                let parsed: Result<DapAdapterArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let DapAdapterArgs { adapter, .. } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("threads: invalid args: {e}"),
                        })?;
                    run_send_command(pool, config, bus, adapter, "threads", None).await
                }))
            }

            HANDLER_STACK_TRACE => {
                // #190 / R7 — strict-parse via typed `DapStackTraceArgs`.
                let parsed: Result<DapStackTraceArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let DapStackTraceArgs {
                        adapter,
                        thread_id,
                        start_frame,
                        levels,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("stack_trace: invalid args: {e}"),
                    })?;
                    let mut payload = json!({ "threadId": thread_id });
                    if let Some(start) = start_frame {
                        payload["startFrame"] = json!(start);
                    }
                    if let Some(levels) = levels {
                        payload["levels"] = json!(levels);
                    }
                    run_send_command(pool, config, bus, adapter, "stackTrace", Some(payload)).await
                }))
            }

            HANDLER_SCOPES => {
                // #190 / R7 — strict-parse via typed `DapScopesArgs`.
                let parsed: Result<DapScopesArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let DapScopesArgs { adapter, frame_id } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("scopes: invalid args: {e}"),
                        })?;
                    let payload = json!({ "frameId": frame_id });
                    run_send_command(pool, config, bus, adapter, "scopes", Some(payload)).await
                }))
            }

            HANDLER_VARIABLES => {
                // #190 / R7 — strict-parse via typed `DapVariablesArgs`.
                let parsed: Result<DapVariablesArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let DapVariablesArgs {
                        adapter,
                        variables_reference,
                        filter,
                        start,
                        count,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("variables: invalid args: {e}"),
                    })?;
                    let mut payload = json!({ "variablesReference": variables_reference });
                    if let Some(f) = filter {
                        payload["filter"] = json!(f);
                    }
                    if let Some(s) = start {
                        payload["start"] = json!(s);
                    }
                    if let Some(c) = count {
                        payload["count"] = json!(c);
                    }
                    run_send_command(pool, config, bus, adapter, "variables", Some(payload)).await
                }))
            }

            HANDLER_EVALUATE => {
                // #190 / R7 — strict-parse via typed `DapEvaluateArgs`.
                let parsed: Result<DapEvaluateArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let DapEvaluateArgs {
                        adapter,
                        expression,
                        frame_id,
                        context,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("evaluate: invalid args: {e}"),
                    })?;
                    let mut payload = json!({ "expression": expression });
                    if let Some(f) = frame_id {
                        payload["frameId"] = json!(f);
                    }
                    if let Some(c) = context {
                        payload["context"] = json!(c);
                    }
                    run_send_command(pool, config, bus, adapter, "evaluate", Some(payload)).await
                }))
            }

            HANDLER_LIST_ADAPTERS => Some(Box::pin(async move {
                let cfg = config_or_err(config.as_ref())?;
                let connected: std::collections::HashSet<String> =
                    pool.connected_adapters().await.into_iter().collect();
                let entries: Vec<DapAdapterEntry> = cfg
                    .adapters
                    .values()
                    .map(|spec| adapter_entry(spec, connected.contains(&spec.name)))
                    .collect();
                serde_json::to_value(&entries).map_err(|e| PluginError::ExecutionFailed {
                    plugin_id: PLUGIN_ID.to_string(),
                    reason: format!("list_adapters: serialize reply: {e}"),
                })
            })),

            _ => None,
        }
    }
}

/// Inlined async body for command verbs that return the adapter's
/// response body (or `Null`). Republishes any drained events on each
/// attempt. Called from inside a `Box::pin(async move { ... })`.
async fn run_send_command(
    pool: Arc<ConnectionPool>,
    config: Option<Arc<DapHostConfig>>,
    bus: Option<Arc<EventBus>>,
    adapter: String,
    command: &'static str,
    payload: Option<Value>,
) -> Result<Value, PluginError> {
    let cfg = config_or_err(config.as_ref())?;
    pool.call_with_reconnect(&adapter, &cfg, move |client| {
        let bus = bus.clone();
        let payload = payload.clone();
        Box::pin(async move {
            let lock = client.lock().await;
            let r = lock.send_request(command, payload).await?;
            republish_pending(&lock, bus.as_ref()).await;
            Ok(r.unwrap_or(Value::Null))
        })
    })
    .await
    .map_err(map_client_err)
}

/// Same as [`run_send_command`] but returns the canonical typed
/// `DapOk { ok: true }` ack — used by fire-and-forget verbs that
/// don't carry a meaningful response body.
async fn run_send_ack(
    pool: Arc<ConnectionPool>,
    config: Option<Arc<DapHostConfig>>,
    bus: Option<Arc<EventBus>>,
    adapter: String,
    command: &'static str,
    payload: Option<Value>,
) -> Result<Value, PluginError> {
    let cfg = config_or_err(config.as_ref())?;
    pool.call_with_reconnect(&adapter, &cfg, move |client| {
        let bus = bus.clone();
        let payload = payload.clone();
        Box::pin(async move {
            let lock = client.lock().await;
            let _ = lock.send_request(command, payload).await?;
            republish_pending(&lock, bus.as_ref()).await;
            Ok(())
        })
    })
    .await
    .map_err(map_client_err)?;
    serde_json::to_value(DapOk { ok: true }).map_err(|e| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: format!("serialize ack reply: {e}"),
    })
}

/// Build the shared `continue` / `next` / `step_in` / `step_out` /
/// `pause` future. #190 / R7 — strict-parse via typed `DapThreadArgs`.
fn proxy_thread_request(
    args: &Value,
    config: Option<Arc<DapHostConfig>>,
    pool: Arc<ConnectionPool>,
    bus: Option<Arc<EventBus>>,
    command: &'static str,
) -> CorePluginFuture {
    let parsed: Result<DapThreadArgs, _> = serde_json::from_value(args.clone());
    Box::pin(async move {
        let DapThreadArgs { adapter, thread_id } =
            parsed.map_err(|e| PluginError::ExecutionFailed {
                plugin_id: PLUGIN_ID.to_string(),
                reason: format!("{command}: invalid args: {e}"),
            })?;
        let payload = json!({ "threadId": thread_id });
        run_send_ack(pool, config, bus, adapter, command, Some(payload)).await
    })
}

/// Drain any adapter-pushed events and republish them on the kernel
/// bus. Idempotent — safe to call repeatedly.
async fn republish_pending(client: &DapClient, bus: Option<&Arc<EventBus>>) {
    let pending = client.drain_events().await;
    if pending.is_empty() {
        return;
    }
    let Some(bus) = bus else {
        return;
    };
    for e in pending {
        let topic = format!("com.nexus.dap.{}", e.event);
        if let Err(err) = bus.publish_plugin(PLUGIN_ID, &topic, e.body) {
            tracing::warn!(
                plugin_id = PLUGIN_ID,
                topic = %topic,
                error = %err,
                "failed to republish dap event"
            );
        }
    }
}

/// Project a typed [`DapSourceBreakpoint`] (`snake_case` wire) into
/// the internal cached [`SourceBreakpointSpec`]. Owned by the client
/// for post-reconnect breakpoint resync.
fn typed_to_spec(b: &DapSourceBreakpoint) -> SourceBreakpointSpec {
    SourceBreakpointSpec {
        line: b.line,
        condition: b.condition.clone(),
        hit_condition: b.hit_condition.clone(),
        log_message: b.log_message.clone(),
    }
}

/// Convert a cached [`SourceBreakpointSpec`] to the camelCase wire
/// shape DAP `setBreakpoints` expects.
fn spec_to_wire(b: &SourceBreakpointSpec) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("line".to_string(), json!(b.line));
    if let Some(c) = &b.condition {
        obj.insert("condition".to_string(), json!(c));
    }
    if let Some(h) = &b.hit_condition {
        obj.insert("hitCondition".to_string(), json!(h));
    }
    if let Some(m) = &b.log_message {
        obj.insert("logMessage".to_string(), json!(m));
    }
    Value::Object(obj)
}

/// Build a typed `list_adapters` row from a stored
/// [`DapAdapterSpec`] + the live connected-state flag.
fn adapter_entry(spec: &DapAdapterSpec, connected: bool) -> DapAdapterEntry {
    DapAdapterEntry {
        name: spec.name.clone(),
        command: spec.command.clone(),
        args: spec.args.clone(),
        adapter_type: spec.adapter_type.clone(),
        file_types: spec.file_types.clone(),
        disabled: spec.disabled,
        connected,
        metadata: spec.metadata.clone(),
    }
}

fn config_or_err(config: Option<&Arc<DapHostConfig>>) -> Result<Arc<DapHostConfig>, PluginError> {
    config.cloned().ok_or_else(|| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: "DAP host config not loaded".to_string(),
    })
}

#[allow(clippy::needless_pass_by_value)]
fn map_client_err(e: DapClientError) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_plugin(dir: &std::path::Path) -> DapCorePlugin {
        DapCorePlugin::new(dir.to_path_buf(), None)
    }

    #[test]
    fn plugin_id_is_correct() {
        assert_eq!(PLUGIN_ID, "com.nexus.dap");
    }

    #[test]
    fn handler_ids_are_unique_and_contiguous() {
        let ids = [
            HANDLER_LIST_ADAPTERS,
            HANDLER_LAUNCH,
            HANDLER_ATTACH,
            HANDLER_CONFIGURATION_DONE,
            HANDLER_DISCONNECT,
            HANDLER_TERMINATE,
            HANDLER_SET_BREAKPOINTS,
            HANDLER_SET_FUNCTION_BREAKPOINTS,
            HANDLER_SET_EXCEPTION_BREAKPOINTS,
            HANDLER_CONTINUE,
            HANDLER_NEXT,
            HANDLER_STEP_IN,
            HANDLER_STEP_OUT,
            HANDLER_PAUSE,
            HANDLER_THREADS,
            HANDLER_STACK_TRACE,
            HANDLER_SCOPES,
            HANDLER_VARIABLES,
            HANDLER_EVALUATE,
            HANDLER_REGISTER_ADAPTER,
            HANDLER_UNREGISTER_ADAPTER,
        ];
        // Unique
        let mut sorted = ids.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate handler id");
        // Contiguous 1..=21
        assert_eq!(*sorted.first().unwrap(), 1);
        assert_eq!(*sorted.last().unwrap(), 21);
        assert_eq!(sorted.len(), 21);
    }

    #[test]
    fn on_init_with_no_dap_toml_succeeds() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        assert!(plugin.on_init().is_ok());
        assert!(plugin.config.read().unwrap().adapters.is_empty());
    }

    #[test]
    fn on_init_with_valid_dap_toml_loads_config() {
        let dir = tempdir().unwrap();
        let forge_dir = dir.path().join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();
        std::fs::write(
            forge_dir.join("dap.toml"),
            r#"
[[adapters]]
name = "rust"
command = "codelldb"
file_types = ["rs"]
"#,
        )
        .unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let cfg = plugin.config.read().unwrap();
        assert!(cfg.adapters.contains_key("rust"));
    }

    #[test]
    fn on_init_with_invalid_dap_toml_falls_back_to_empty() {
        let dir = tempdir().unwrap();
        let forge_dir = dir.path().join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();
        std::fs::write(forge_dir.join("dap.toml"), "not valid toml = = =").unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        assert!(plugin.config.read().unwrap().adapters.is_empty());
    }

    #[test]
    fn sync_list_adapters_returns_array() {
        let dir = tempdir().unwrap();
        let forge_dir = dir.path().join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();
        std::fs::write(
            forge_dir.join("dap.toml"),
            r#"
[[adapters]]
name = "rust"
command = "codelldb"
file_types = ["rs"]

[[adapters]]
name = "node"
command = "js-debug"
file_types = ["ts", "js"]
disabled = true
"#,
        )
        .unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let result = plugin.dispatch(HANDLER_LIST_ADAPTERS, &json!({})).unwrap();
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        let names: Vec<&str> = arr.iter().map(|v| v["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"rust"));
        assert!(names.contains(&"node"));
        // No connections yet — `connected: false` for every row.
        for row in arr {
            assert_eq!(row["connected"], json!(false));
        }
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
        // #190 / R7 — previously `str_arg(args, "adapter")?` and similar
        // returned `None` from `dispatch_async`, the kernel fell back
        // to sync dispatch, and the sync arm errored with the
        // misleading "handler_id N requires dispatch_async". Now each
        // typed `DapXxxArgs` parse surfaces the missing-field error
        // through the future as a clean
        // `PluginError::ExecutionFailed { reason: "<verb>: invalid
        // args: …" }`.
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();

        let cases: &[(u32, Value)] = &[
            // Missing adapter
            (HANDLER_LAUNCH, json!({})),
            // Missing thread_id
            (HANDLER_CONTINUE, json!({ "adapter": "x" })),
            // Missing frame_id
            (HANDLER_SCOPES, json!({ "adapter": "x" })),
            // Missing variables_reference
            (HANDLER_VARIABLES, json!({ "adapter": "x" })),
            // Missing expression
            (HANDLER_EVALUATE, json!({ "adapter": "x" })),
        ];
        for (id, args) in cases {
            let fut = plugin
                .dispatch_async(*id, args)
                .expect("dispatch_async must return a future (not None)");
            let err = runtime
                .block_on(fut)
                .expect_err("missing fields must error");
            assert!(
                err.to_string().contains("invalid args"),
                "handler {id} did not surface a strict-parse error: {err}"
            );
        }
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

    #[test]
    fn typed_to_spec_minimal_line_only() {
        let typed = DapSourceBreakpoint {
            line: 42,
            condition: None,
            hit_condition: None,
            log_message: None,
        };
        let bp = typed_to_spec(&typed);
        assert_eq!(bp.line, 42);
        assert!(bp.condition.is_none());
        assert!(bp.hit_condition.is_none());
        assert!(bp.log_message.is_none());
    }

    #[test]
    fn typed_to_spec_full_record() {
        let typed = DapSourceBreakpoint {
            line: 10,
            condition: Some("i > 0".to_string()),
            hit_condition: Some("> 5".to_string()),
            log_message: Some("hit".to_string()),
        };
        let bp = typed_to_spec(&typed);
        assert_eq!(bp.line, 10);
        assert_eq!(bp.condition.as_deref(), Some("i > 0"));
        assert_eq!(bp.hit_condition.as_deref(), Some("> 5"));
        assert_eq!(bp.log_message.as_deref(), Some("hit"));
    }

    #[test]
    fn set_breakpoints_rejects_breakpoint_without_line() {
        // #190 / R7 — strict-parse via `DapSourceBreakpoint` rejects a
        // row missing the required `line` field (previously
        // `parse_source_breakpoint` silently filtered it out).
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let fut = plugin
            .dispatch_async(
                HANDLER_SET_BREAKPOINTS,
                &json!({
                    "adapter": "x",
                    "source_path": "/tmp/x.rs",
                    "breakpoints": [{ "condition": "y" }],
                }),
            )
            .expect("dispatch_async must return a future");
        let err = runtime.block_on(fut).expect_err("missing line must error");
        assert!(err.to_string().contains("invalid args"));
    }

    #[test]
    fn spec_to_wire_emits_camel_case_keys() {
        let s = SourceBreakpointSpec {
            line: 5,
            condition: Some("c".to_string()),
            hit_condition: Some("h".to_string()),
            log_message: Some("l".to_string()),
        };
        let w = spec_to_wire(&s);
        let obj = w.as_object().unwrap();
        assert_eq!(obj["line"], json!(5));
        assert_eq!(obj["condition"], json!("c"));
        assert_eq!(obj["hitCondition"], json!("h"));
        assert_eq!(obj["logMessage"], json!("l"));
        // snake_case keys absent (DAP wire is camelCase).
        assert!(!obj.contains_key("hit_condition"));
        assert!(!obj.contains_key("log_message"));
    }

    #[test]
    fn async_async_handlers_set_covers_every_non_list_id() {
        // Sanity check: every handler except LIST_ADAPTERS,
        // REGISTER_ADAPTER, and UNREGISTER_ADAPTER must be in the
        // async set. Helps the sync dispatch arm route correctly.
        for id in 2u32..=19u32 {
            assert!(
                ASYNC_HANDLERS.contains(&id),
                "handler {id} missing from ASYNC_HANDLERS"
            );
        }
        // The BL-113 verbs are sync — they mutate the config map under
        // a write lock; no async pool interaction needed.
        assert!(!ASYNC_HANDLERS.contains(&HANDLER_REGISTER_ADAPTER));
        assert!(!ASYNC_HANDLERS.contains(&HANDLER_UNREGISTER_ADAPTER));
    }

    // ── BL-113 Phase 1b — register_adapter / unregister_adapter IPC ────────────

    fn register_args(name: &str, command: &str, plugin_id: &str) -> Value {
        json!({
            "name": name,
            "command": command,
            "args": [],
            "file_types": ["rs"],
            "disabled": false,
            "env": {},
            "plugin_id": plugin_id,
        })
    }

    #[test]
    fn register_adapter_ipc_inserts_and_reports_ok() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let reply = plugin
            .dispatch(
                HANDLER_REGISTER_ADAPTER,
                &register_args("rust", "codelldb", "community.rust"),
            )
            .unwrap();
        assert_eq!(reply["ok"], json!(true));
        assert_eq!(reply["status"], json!("ok"));
        let cfg = plugin.config.read().unwrap();
        assert!(cfg.adapters.contains_key("rust"));
        assert_eq!(cfg.contributed_by["rust"], "community.rust");
    }

    #[test]
    fn register_adapter_ipc_rejects_collision_with_toml() {
        let dir = tempdir().unwrap();
        let forge_dir = dir.path().join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();
        std::fs::write(
            forge_dir.join("dap.toml"),
            r#"
[[adapters]]
name = "rust"
command = "codelldb-from-toml"
"#,
        )
        .unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let reply = plugin
            .dispatch(
                HANDLER_REGISTER_ADAPTER,
                &register_args("rust", "codelldb-from-plugin", "community.rust"),
            )
            .unwrap();
        assert_eq!(reply["ok"], json!(false));
        assert_eq!(reply["status"], json!("toml_override"));
        // TOML entry untouched.
        let cfg = plugin.config.read().unwrap();
        assert_eq!(cfg.adapters["rust"].command, "codelldb-from-toml");
        assert!(!cfg.contributed_by.contains_key("rust"));
    }

    #[test]
    fn register_adapter_ipc_rejects_missing_fields() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        // Missing `command`.
        let err = plugin
            .dispatch(
                HANDLER_REGISTER_ADAPTER,
                &json!({
                    "name": "rust",
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
    fn unregister_adapter_ipc_round_trip_with_owner_match() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        plugin
            .dispatch(
                HANDLER_REGISTER_ADAPTER,
                &register_args("rust", "codelldb", "community.rust"),
            )
            .unwrap();
        let reply = plugin
            .dispatch(
                HANDLER_UNREGISTER_ADAPTER,
                &json!({ "name": "rust", "plugin_id": "community.rust" }),
            )
            .unwrap();
        assert_eq!(reply["ok"], json!(true));
        assert_eq!(reply["status"], json!("ok"));
        assert!(plugin.config.read().unwrap().adapters.is_empty());
    }

    #[test]
    fn unregister_adapter_ipc_surfaces_each_skip_reason() {
        let dir = tempdir().unwrap();
        let forge_dir = dir.path().join(".forge");
        std::fs::create_dir_all(&forge_dir).unwrap();
        std::fs::write(
            forge_dir.join("dap.toml"),
            r#"
[[adapters]]
name = "toml-pinned"
command = "x"
"#,
        )
        .unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        plugin
            .dispatch(
                HANDLER_REGISTER_ADAPTER,
                &register_args("contrib", "x", "plugin.owner"),
            )
            .unwrap();

        // not_found
        let reply = plugin
            .dispatch(
                HANDLER_UNREGISTER_ADAPTER,
                &json!({ "name": "ghost", "plugin_id": "anyone" }),
            )
            .unwrap();
        assert_eq!(reply["status"], json!("not_found"));

        // toml_entry
        let reply = plugin
            .dispatch(
                HANDLER_UNREGISTER_ADAPTER,
                &json!({ "name": "toml-pinned", "plugin_id": "anyone" }),
            )
            .unwrap();
        assert_eq!(reply["status"], json!("toml_entry"));

        // not_owned_by_plugin (includes actual_owner)
        let reply = plugin
            .dispatch(
                HANDLER_UNREGISTER_ADAPTER,
                &json!({ "name": "contrib", "plugin_id": "plugin.intruder" }),
            )
            .unwrap();
        assert_eq!(reply["status"], json!("not_owned_by_plugin"));
        assert_eq!(reply["actual_owner"], json!("plugin.owner"));

        // Original entries untouched after failed unregister attempts.
        let cfg = plugin.config.read().unwrap();
        assert!(cfg.adapters.contains_key("toml-pinned"));
        assert!(cfg.adapters.contains_key("contrib"));
    }

    #[test]
    fn async_dispatch_snapshot_sees_register_at_dispatch_time() {
        // A snapshot taken from the RwLock reflects the state at the
        // moment of the read; a later register doesn't retroactively
        // appear in the snapshot.
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        let snap_before = snapshot_config(&plugin.config);
        plugin
            .dispatch(
                HANDLER_REGISTER_ADAPTER,
                &register_args("rust", "codelldb", "community.rust"),
            )
            .unwrap();
        let snap_after = snapshot_config(&plugin.config);
        assert!(snap_before.adapters.is_empty());
        assert!(snap_after.adapters.contains_key("rust"));
    }
}
