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
    let spec = parse_register_adapter_spec(args)?;
    let plugin_id = parse_string_field(args, "plugin_id")?;
    let mut cfg = config.write().expect("DapHostConfig RwLock poisoned");
    let status = match cfg.register_contributed(spec, plugin_id) {
        Ok(()) => ("ok", true),
        Err(MergeSkipReason::TomlOverride) => ("toml_override", false),
        Err(MergeSkipReason::InvalidName) => ("invalid_name", false),
        Err(MergeSkipReason::InvalidCommand) => ("invalid_command", false),
    };
    Ok(json!({ "ok": status.1, "status": status.0 }))
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
    let name = parse_string_field(args, "name")?;
    let plugin_id = parse_string_field(args, "plugin_id")?;
    let mut cfg = config.write().expect("DapHostConfig RwLock poisoned");
    match cfg.unregister_contributed(&name, &plugin_id) {
        Ok(_removed) => Ok(json!({ "ok": true, "status": "ok" })),
        Err(UnregisterError::NotFound) => Ok(json!({ "ok": false, "status": "not_found" })),
        Err(UnregisterError::TomlEntry) => Ok(json!({ "ok": false, "status": "toml_entry" })),
        Err(UnregisterError::NotOwnedByPlugin { actual_owner }) => Ok(json!({
            "ok": false,
            "status": "not_owned_by_plugin",
            "actual_owner": actual_owner,
        })),
    }
}

fn parse_string_field(args: &Value, key: &str) -> Result<String, PluginError> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!("missing or empty required field `{key}`"),
        })
}

fn parse_register_adapter_spec(args: &Value) -> Result<DapAdapterSpec, PluginError> {
    let name = parse_string_field(args, "name")?;
    let command = parse_string_field(args, "command")?;
    let parse_args_list = args
        .get("args")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let file_types = args
        .get("file_types")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let adapter_type = args
        .get("adapter_type")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let disabled = args
        .get("disabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let env = args
        .get("env")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    // BL-113 — opaque shell-facing payload. The contribution-wire layer
    // packs `launch_config_schema` + cosmetic fields here; the host
    // round-trips it verbatim and never interprets it.
    let metadata = args.get("metadata").cloned();
    Ok(DapAdapterSpec {
        name,
        command,
        args: parse_args_list,
        adapter_type,
        file_types,
        disabled,
        env,
        metadata,
    })
}

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
                let arr: Vec<Value> = cfg
                    .adapters
                    .values()
                    .map(|spec| {
                        json!({
                            "name": spec.name,
                            "command": spec.command,
                            "args": spec.args,
                            "adapter_type": spec.adapter_type,
                            "file_types": spec.file_types,
                            "disabled": spec.disabled,
                            "connected": false,
                            "metadata": spec.metadata,
                        })
                    })
                    .collect();
                Ok(Value::Array(arr))
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
                let adapter = str_arg(args, "adapter")?;
                let program = str_arg(args, "program")?;
                let mode = opt_str(args, "mode");
                let cwd = opt_str(args, "cwd");
                let stop_on_entry = args
                    .get("stop_on_entry")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
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
                if let Some(a) = args.get("args").cloned() {
                    launch_args["args"] = a;
                }
                if let Some(e) = args.get("env").cloned() {
                    launch_args["env"] = e;
                }
                if let Some(extra) = args.get("extra").and_then(Value::as_object).cloned() {
                    if let Value::Object(map) = &mut launch_args {
                        for (k, v) in extra {
                            map.entry(k).or_insert(v);
                        }
                    }
                }
                Some(send_command_future(
                    pool,
                    config,
                    bus,
                    adapter,
                    "launch",
                    Some(launch_args),
                ))
            }

            HANDLER_ATTACH => {
                let adapter = str_arg(args, "adapter")?;
                let mut attach_args = serde_json::Map::new();
                if let Some(pid) = args.get("pid").and_then(Value::as_i64) {
                    attach_args.insert("pid".to_string(), json!(pid));
                }
                if let Some(port) = args.get("port").and_then(Value::as_i64) {
                    attach_args.insert("port".to_string(), json!(port));
                }
                if let Some(extra) = args.get("extra").and_then(Value::as_object).cloned() {
                    for (k, v) in extra {
                        attach_args.entry(k).or_insert(v);
                    }
                }
                Some(send_command_future(
                    pool,
                    config,
                    bus,
                    adapter,
                    "attach",
                    Some(Value::Object(attach_args)),
                ))
            }

            HANDLER_CONFIGURATION_DONE => {
                let adapter = str_arg(args, "adapter")?;
                Some(send_ack_future(
                    pool,
                    config,
                    bus,
                    adapter,
                    "configurationDone",
                    None,
                ))
            }

            HANDLER_DISCONNECT => {
                let adapter = str_arg(args, "adapter")?;
                let terminate_debuggee = args
                    .get("terminate_debuggee")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let payload = json!({
                    "restart": false,
                    "terminateDebuggee": terminate_debuggee,
                });
                Some(send_ack_future(
                    pool,
                    config,
                    bus,
                    adapter,
                    "disconnect",
                    Some(payload),
                ))
            }

            HANDLER_TERMINATE => {
                let adapter = str_arg(args, "adapter")?;
                Some(send_ack_future(
                    pool,
                    config,
                    bus,
                    adapter,
                    "terminate",
                    None,
                ))
            }

            HANDLER_SET_BREAKPOINTS => {
                let adapter = str_arg(args, "adapter")?;
                let source_path = str_arg(args, "source_path")?;
                let breakpoints = args
                    .get("breakpoints")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let specs: Vec<SourceBreakpointSpec> = breakpoints
                    .iter()
                    .filter_map(parse_source_breakpoint)
                    .collect();
                let wire_bps: Vec<Value> = specs.iter().map(spec_to_wire).collect();
                let payload = json!({
                    "source": { "path": source_path.clone() },
                    "breakpoints": wire_bps,
                });
                Some(Box::pin(async move {
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
                let adapter = str_arg(args, "adapter")?;
                let breakpoints = args
                    .get("breakpoints")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let payload = json!({ "breakpoints": breakpoints });
                Some(send_command_future(
                    pool,
                    config,
                    bus,
                    adapter,
                    "setFunctionBreakpoints",
                    Some(payload),
                ))
            }

            HANDLER_SET_EXCEPTION_BREAKPOINTS => {
                let adapter = str_arg(args, "adapter")?;
                let filters = args
                    .get("filters")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let payload = json!({ "filters": filters });
                Some(send_ack_future(
                    pool,
                    config,
                    bus,
                    adapter,
                    "setExceptionBreakpoints",
                    Some(payload),
                ))
            }

            HANDLER_CONTINUE => proxy_thread_request(args, config, pool, bus, "continue"),
            HANDLER_NEXT => proxy_thread_request(args, config, pool, bus, "next"),
            HANDLER_STEP_IN => proxy_thread_request(args, config, pool, bus, "stepIn"),
            HANDLER_STEP_OUT => proxy_thread_request(args, config, pool, bus, "stepOut"),
            HANDLER_PAUSE => proxy_thread_request(args, config, pool, bus, "pause"),

            HANDLER_THREADS => {
                let adapter = str_arg(args, "adapter")?;
                Some(send_command_future(
                    pool, config, bus, adapter, "threads", None,
                ))
            }

            HANDLER_STACK_TRACE => {
                let adapter = str_arg(args, "adapter")?;
                let thread_id = args.get("thread_id").and_then(Value::as_i64)?;
                let mut payload = json!({ "threadId": thread_id });
                if let Some(start) = args.get("start_frame").and_then(Value::as_i64) {
                    payload["startFrame"] = json!(start);
                }
                if let Some(levels) = args.get("levels").and_then(Value::as_i64) {
                    payload["levels"] = json!(levels);
                }
                Some(send_command_future(
                    pool,
                    config,
                    bus,
                    adapter,
                    "stackTrace",
                    Some(payload),
                ))
            }

            HANDLER_SCOPES => {
                let adapter = str_arg(args, "adapter")?;
                let frame_id = args.get("frame_id").and_then(Value::as_i64)?;
                let payload = json!({ "frameId": frame_id });
                Some(send_command_future(
                    pool,
                    config,
                    bus,
                    adapter,
                    "scopes",
                    Some(payload),
                ))
            }

            HANDLER_VARIABLES => {
                let adapter = str_arg(args, "adapter")?;
                let var_ref = args.get("variables_reference").and_then(Value::as_i64)?;
                let mut payload = json!({ "variablesReference": var_ref });
                if let Some(f) = opt_str(args, "filter") {
                    payload["filter"] = json!(f);
                }
                if let Some(s) = args.get("start").and_then(Value::as_i64) {
                    payload["start"] = json!(s);
                }
                if let Some(c) = args.get("count").and_then(Value::as_i64) {
                    payload["count"] = json!(c);
                }
                Some(send_command_future(
                    pool,
                    config,
                    bus,
                    adapter,
                    "variables",
                    Some(payload),
                ))
            }

            HANDLER_EVALUATE => {
                let adapter = str_arg(args, "adapter")?;
                let expression = str_arg(args, "expression")?;
                let mut payload = json!({ "expression": expression });
                if let Some(f) = args.get("frame_id").and_then(Value::as_i64) {
                    payload["frameId"] = json!(f);
                }
                if let Some(c) = opt_str(args, "context") {
                    payload["context"] = json!(c);
                }
                Some(send_command_future(
                    pool,
                    config,
                    bus,
                    adapter,
                    "evaluate",
                    Some(payload),
                ))
            }

            HANDLER_LIST_ADAPTERS => Some(Box::pin(async move {
                let cfg = config_or_err(config.as_ref())?;
                let connected: std::collections::HashSet<String> =
                    pool.connected_adapters().await.into_iter().collect();
                let arr: Vec<Value> = cfg
                    .adapters
                    .values()
                    .map(|spec| {
                        json!({
                            "name": spec.name,
                            "command": spec.command,
                            "args": spec.args,
                            "adapter_type": spec.adapter_type,
                            "file_types": spec.file_types,
                            "disabled": spec.disabled,
                            "connected": connected.contains(&spec.name),
                            "metadata": spec.metadata,
                        })
                    })
                    .collect();
                Ok(Value::Array(arr))
            })),

            _ => None,
        }
    }
}

/// Build a `dispatch_async` future that sends `command` to the
/// adapter and returns the response `body` (or `Null`). Republishes
/// any drained events on each attempt.
fn send_command_future(
    pool: Arc<ConnectionPool>,
    config: Option<Arc<DapHostConfig>>,
    bus: Option<Arc<EventBus>>,
    adapter: String,
    command: &'static str,
    payload: Option<Value>,
) -> CorePluginFuture {
    Box::pin(async move {
        let cfg = config_or_err(config.as_ref())?;
        let result = pool
            .call_with_reconnect(&adapter, &cfg, move |client| {
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
            .map_err(map_client_err)?;
        Ok(result)
    })
}

/// Same as [`send_command_future`] but returns the canonical
/// `{ "ok": true }` ack — used by fire-and-forget verbs that don't
/// carry a meaningful response body.
fn send_ack_future(
    pool: Arc<ConnectionPool>,
    config: Option<Arc<DapHostConfig>>,
    bus: Option<Arc<EventBus>>,
    adapter: String,
    command: &'static str,
    payload: Option<Value>,
) -> CorePluginFuture {
    Box::pin(async move {
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
        Ok(json!({ "ok": true }))
    })
}

fn proxy_thread_request(
    args: &Value,
    config: Option<Arc<DapHostConfig>>,
    pool: Arc<ConnectionPool>,
    bus: Option<Arc<EventBus>>,
    command: &'static str,
) -> Option<CorePluginFuture> {
    let adapter = str_arg(args, "adapter")?;
    let thread_id = args.get("thread_id").and_then(Value::as_i64)?;
    let payload = json!({ "threadId": thread_id });
    Some(send_ack_future(
        pool,
        config,
        bus,
        adapter,
        command,
        Some(payload),
    ))
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

fn parse_source_breakpoint(v: &Value) -> Option<SourceBreakpointSpec> {
    let line = v.get("line").and_then(Value::as_i64)?;
    Some(SourceBreakpointSpec {
        line,
        condition: v
            .get("condition")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        hit_condition: v
            .get("hit_condition")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        log_message: v
            .get("log_message")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    })
}

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

fn str_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
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
    fn async_handler_without_required_args_returns_none() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        // Missing adapter
        assert!(plugin.dispatch_async(HANDLER_LAUNCH, &json!({})).is_none());
        // Missing thread_id
        assert!(plugin
            .dispatch_async(HANDLER_CONTINUE, &json!({ "adapter": "x" }))
            .is_none());
        // Missing frame_id
        assert!(plugin
            .dispatch_async(HANDLER_SCOPES, &json!({ "adapter": "x" }))
            .is_none());
        // Missing variables_reference
        assert!(plugin
            .dispatch_async(HANDLER_VARIABLES, &json!({ "adapter": "x" }))
            .is_none());
        // Missing expression
        assert!(plugin
            .dispatch_async(HANDLER_EVALUATE, &json!({ "adapter": "x" }))
            .is_none());
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
    fn parse_source_breakpoint_minimal_line_only() {
        let v = json!({"line": 42});
        let bp = parse_source_breakpoint(&v).unwrap();
        assert_eq!(bp.line, 42);
        assert!(bp.condition.is_none());
        assert!(bp.hit_condition.is_none());
        assert!(bp.log_message.is_none());
    }

    #[test]
    fn parse_source_breakpoint_full_record() {
        let v = json!({
            "line": 10,
            "condition": "i > 0",
            "hit_condition": "> 5",
            "log_message": "hit"
        });
        let bp = parse_source_breakpoint(&v).unwrap();
        assert_eq!(bp.line, 10);
        assert_eq!(bp.condition.as_deref(), Some("i > 0"));
        assert_eq!(bp.hit_condition.as_deref(), Some("> 5"));
        assert_eq!(bp.log_message.as_deref(), Some("hit"));
    }

    #[test]
    fn parse_source_breakpoint_rejects_missing_line() {
        assert!(parse_source_breakpoint(&json!({"condition": "x"})).is_none());
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
