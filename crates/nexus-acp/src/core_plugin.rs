//! Core plugin for the ACP host (`com.nexus.acp`).
//!
//! Holds an [`AcpHostConfig`] populated at runtime by the BL-113
//! contribution wiring, lazily connects to configured agents via the
//! [`ConnectionPool`], proxies request/response traffic over IPC, and
//! republishes agent-pushed notifications on the kernel event bus.
//!
//! # Status (0.1.2): experimental — no in-tree consumer
//!
//! ACP's IPC surface is fully wired and unit-tested, but no in-tree
//! shell plugin invokes `com.nexus.acp::*` today. The only user-facing
//! entry point is the inbound `nexus acp serve` CLI subcommand
//! (`crates/nexus-cli/src/commands/acp.rs`) plus the
//! `first-party-acp-echo` example plugin. Until a shell consumer
//! lands, this plugin is best treated as experimental scaffolding.
//! See `docs/0.1.2/plugins/assessment/PHASE5_DECISIONS.md` §5.1.
//!
//! # IPC surface
//!
//! | id | name | sync? | args |
//! |---|---|---|---|
//! | 1 | `list_agents` | sync | — |
//! | 2 | `initialize` | async | `{agent}` |
//! | 3 | `propose` | async | `{agent, action, params?}` |
//! | 4 | `accept` | async | `{agent, proposal_id, reason?}` |
//! | 5 | `reject` | async | `{agent, proposal_id, reason?}` |
//! | 6 | `register_server` | sync | `{name, command, …, plugin_id}` |
//! | 7 | `unregister_server` | sync | `{name, plugin_id}` |
//! | 8 | `disconnect` | async | `{agent}` |
//!
//! # Bus events
//!
//! Agent-pushed notifications fan out as
//! `com.nexus.acp.<acp_method_with_dots>` — e.g. an `agent/output`
//! notification becomes `com.nexus.acp.agent.output`. The original
//! JSON-RPC `params` payload travels verbatim.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde_json::{json, Value};

use crate::client::AcpClientError;
use crate::config::{AcpAdapterSpec, AcpHostConfig, MergeSkipReason, UnregisterError};
use crate::ipc::{
    AcpAgentArgs, AcpAgentEntry, AcpDecisionArgs, AcpProposeArgs, AcpRegisterServerArgs,
    AcpRegisterServerReply, AcpUnregisterServerArgs, AcpUnregisterServerReply,
};
use crate::pool::{ConnectionPool, PoolConfig};

/// Reverse-DNS identifier.
pub const PLUGIN_ID: &str = "com.nexus.acp";

/// Sync — list configured agents + their connected status.
pub const HANDLER_LIST_AGENTS: u32 = 1;
/// Async — open + handshake an agent (forces a lazy connect).
pub const HANDLER_INITIALIZE: u32 = 2;
/// Async — submit an action proposal to the agent.
pub const HANDLER_PROPOSE: u32 = 3;
/// Async — approve a previously-proposed action.
pub const HANDLER_ACCEPT: u32 = 4;
/// Async — reject a previously-proposed action.
pub const HANDLER_REJECT: u32 = 5;
/// BL-113 Phase 4 — plugin-contributed agent register.
pub const HANDLER_REGISTER_SERVER: u32 = 6;
/// BL-113 Phase 4 — plugin-contributed agent unregister.
pub const HANDLER_UNREGISTER_SERVER: u32 = 7;
/// Async — close the agent connection without graceful exit (the
/// pool's reconnect path can re-establish next call).
pub const HANDLER_DISCONNECT: u32 = 8;

/// SD-06 — single source of truth for `(command-name, handler-id)`
/// pairs consumed by `nexus_bootstrap::plugins::acp::register`. Order
/// matches the pre-SD-06 bootstrap registration.
pub const IPC_HANDLERS: &[(&str, u32)] = &[
    ("list_agents", HANDLER_LIST_AGENTS),
    ("initialize", HANDLER_INITIALIZE),
    ("propose", HANDLER_PROPOSE),
    ("accept", HANDLER_ACCEPT),
    ("reject", HANDLER_REJECT),
    ("disconnect", HANDLER_DISCONNECT),
    ("register_server", HANDLER_REGISTER_SERVER),
    ("unregister_server", HANDLER_UNREGISTER_SERVER),
];

/// Core plugin that manages connections to ACP-speaking agents.
pub struct AcpCorePlugin {
    event_bus: Option<Arc<EventBus>>,
    config: Arc<RwLock<AcpHostConfig>>,
    pool: Arc<ConnectionPool>,
}

impl AcpCorePlugin {
    /// Create a new (unstarted) ACP host plugin for the given forge
    /// root. The forge root is the working directory each spawned
    /// agent inherits.
    #[must_use]
    pub fn new(forge_root: PathBuf, event_bus: Option<Arc<EventBus>>) -> Self {
        let pool = Arc::new(ConnectionPool::new(PoolConfig::default(), forge_root));
        Self {
            event_bus,
            config: Arc::new(RwLock::new(AcpHostConfig::new())),
            pool,
        }
    }
}

fn snapshot_config(cell: &Arc<RwLock<AcpHostConfig>>) -> Arc<AcpHostConfig> {
    Arc::new(cell.read().expect("AcpHostConfig RwLock poisoned").clone())
}

/// BL-113 Phase 4 — sync handler for `register_server`. Same
/// authorisation model as the LSP / DAP / MCP handlers (declarative
/// pipeline; no verb-level capability gate).
///
/// #190 — strict-parse via typed `AcpRegisterServerArgs`. The prior
/// hand-rolled `parse_register_server_spec` + `parse_string_field`
/// helpers silently accepted unknown fields and let typos like
/// `{ commandd: "..." }` through.
fn handle_register_server(
    config: &Arc<RwLock<AcpHostConfig>>,
    args: &Value,
) -> Result<Value, PluginError> {
    let typed: AcpRegisterServerArgs = serde_json::from_value(args.clone()).map_err(|e| {
        PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!("register_server: invalid args: {e}"),
        }
    })?;
    // Only reject *empty* strings at the parse boundary —
    // whitespace-only values flow through to `register_contributed`
    // and surface as a `MergeSkipReason::Invalid{Name,Command}`
    // "skip" status, matching the prior hand-rolled behaviour the
    // tests depend on.
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
    let spec = AcpAdapterSpec {
        name: typed.name,
        command: typed.command,
        args: typed.args,
        capabilities: typed.capabilities,
        disabled: typed.disabled,
        env: typed.env,
        metadata: typed.metadata,
    };
    let mut cfg = config.write().expect("AcpHostConfig RwLock poisoned");
    let reply = match cfg.register_contributed(spec, typed.plugin_id) {
        Ok(()) => AcpRegisterServerReply {
            ok: true,
            status: "ok".to_string(),
        },
        Err(MergeSkipReason::AlreadyRegistered) => AcpRegisterServerReply {
            ok: false,
            status: "already_registered".to_string(),
        },
        Err(MergeSkipReason::InvalidName) => AcpRegisterServerReply {
            ok: false,
            status: "invalid_name".to_string(),
        },
        Err(MergeSkipReason::InvalidCommand) => AcpRegisterServerReply {
            ok: false,
            status: "invalid_command".to_string(),
        },
    };
    serde_json::to_value(&reply).map_err(|e| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: format!("register_server: serialize reply: {e}"),
    })
}

fn handle_unregister_server(
    config: &Arc<RwLock<AcpHostConfig>>,
    args: &Value,
) -> Result<Value, PluginError> {
    // #190 — strict-parse via typed `AcpUnregisterServerArgs`.
    let typed: AcpUnregisterServerArgs = serde_json::from_value(args.clone()).map_err(|e| {
        PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!("unregister_server: invalid args: {e}"),
        }
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
    let mut cfg = config.write().expect("AcpHostConfig RwLock poisoned");
    let reply = match cfg.unregister_contributed(&typed.name, &typed.plugin_id) {
        Ok(_removed) => AcpUnregisterServerReply {
            ok: true,
            status: "ok".to_string(),
            actual_owner: None,
        },
        Err(UnregisterError::NotFound) => AcpUnregisterServerReply {
            ok: false,
            status: "not_found".to_string(),
            actual_owner: None,
        },
        Err(UnregisterError::NotOwnedByPlugin { actual_owner }) => AcpUnregisterServerReply {
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

impl CorePlugin for AcpCorePlugin {
    fn on_init(&mut self) -> Result<(), PluginError> {
        // Empty registry by design — ACP has no flat-TOML loader.
        // The contribution wiring runs after this and populates the
        // registry through `register_server`.
        tracing::debug!(
            plugin_id = PLUGIN_ID,
            "ACP host initialised (registry starts empty; contributions arrive after plugin load)"
        );
        Ok(())
    }

    fn on_start(&mut self) -> Result<(), PluginError> {
        let agent_count = self
            .config
            .read()
            .expect("AcpHostConfig RwLock poisoned")
            .adapters
            .len();
        if let Some(bus) = &self.event_bus {
            let _ = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.acp.started",
                json!({ "registered_agents": agent_count }),
            );
        }
        tracing::info!(
            plugin_id = PLUGIN_ID,
            registered_agents = agent_count,
            "ACP host started (connections are lazy)"
        );
        Ok(())
    }

    fn on_stop(&mut self) {
        // Same shutdown shape as LspCorePlugin: spawn a current-thread
        // runtime so we don't need an outer reactor, hard-cap the join
        // so a misbehaving agent can't hang kernel shutdown.
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
                tracing::info!(plugin_id = PLUGIN_ID, "ACP host stopped");
                return;
            }
            std::thread::sleep(poll_interval);
        }
        tracing::warn!(
            audit = true,
            plugin_id = PLUGIN_ID,
            timeout_secs = SHUTDOWN_DEADLINE.as_secs(),
            "ACP host shutdown timed out; abandoning the join — child processes \
             may be stranded until the host process exits"
        );
    }

    fn dispatch(&mut self, handler_id: u32, args: &Value) -> Result<Value, PluginError> {
        match handler_id {
            HANDLER_LIST_AGENTS => {
                // #190 — typed `Vec<AcpAgentEntry>` reply via shared
                // `agent_entry` projector. The sync handler can't
                // await `pool.connected_agents()`, so `connected`
                // is always `false`; an async list variant can land
                // when a real use case needs the merged view.
                let cfg = self.config.read().expect("AcpHostConfig RwLock poisoned");
                let entries: Vec<AcpAgentEntry> = cfg
                    .adapters
                    .values()
                    .map(|spec| agent_entry(spec, false))
                    .collect();
                serde_json::to_value(&entries).map_err(|e| PluginError::ExecutionFailed {
                    plugin_id: PLUGIN_ID.to_string(),
                    reason: format!("list_agents: serialize reply: {e}"),
                })
            }
            HANDLER_REGISTER_SERVER => handle_register_server(&self.config, args),
            HANDLER_UNREGISTER_SERVER => handle_unregister_server(&self.config, args),
            HANDLER_INITIALIZE | HANDLER_PROPOSE | HANDLER_ACCEPT | HANDLER_REJECT
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

    fn dispatch_async(&mut self, handler_id: u32, args: &Value) -> Option<CorePluginFuture> {
        let pool = Arc::clone(&self.pool);
        let config = Some(snapshot_config(&self.config));
        let bus = self.event_bus.clone();

        match handler_id {
            HANDLER_INITIALIZE => {
                // #190 — strict-parse via typed `AcpAgentArgs`.
                let parsed: Result<AcpAgentArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let AcpAgentArgs { agent } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("initialize: invalid args: {e}"),
                        })?;
                    let cfg = config_or_err(config.as_ref())?;
                    let result = pool
                        .call_with_reconnect(&agent, &cfg, move |client| {
                            let bus = bus.clone();
                            Box::pin(async move {
                                let lock = client.lock().await;
                                let caps = lock.server_capabilities().await.unwrap_or(Value::Null);
                                republish_pending(&lock, bus.as_ref()).await;
                                Ok(caps)
                            })
                        })
                        .await
                        .map_err(map_client_err)?;
                    Ok(json!({ "agent": agent, "capabilities": result }))
                }))
            }
            HANDLER_PROPOSE => {
                // #190 — strict-parse via typed `AcpProposeArgs`.
                let parsed: Result<AcpProposeArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let AcpProposeArgs {
                        agent,
                        action,
                        params,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("propose: invalid args: {e}"),
                    })?;
                    let cfg = config_or_err(config.as_ref())?;
                    proxy_request(&pool, &cfg, &agent, bus, &action, params)
                        .await
                        .map_err(map_client_err)
                }))
            }
            HANDLER_ACCEPT | HANDLER_REJECT => {
                // #190 — strict-parse via typed `AcpDecisionArgs`.
                let parsed: Result<AcpDecisionArgs, _> = serde_json::from_value(args.clone());
                let method = if handler_id == HANDLER_ACCEPT {
                    "accept"
                } else {
                    "reject"
                };
                Some(Box::pin(async move {
                    let AcpDecisionArgs {
                        agent,
                        proposal_id,
                        reason,
                    } = parsed.map_err(|e| PluginError::ExecutionFailed {
                        plugin_id: PLUGIN_ID.to_string(),
                        reason: format!("{method}: invalid args: {e}"),
                    })?;
                    let cfg = config_or_err(config.as_ref())?;
                    let mut payload = json!({ "proposalId": proposal_id });
                    if let Some(r) = reason {
                        payload["reason"] = Value::String(r);
                    }
                    proxy_request(&pool, &cfg, &agent, bus, method, payload)
                        .await
                        .map_err(map_client_err)
                }))
            }
            HANDLER_DISCONNECT => {
                // #190 — strict-parse via typed `AcpAgentArgs`.
                let parsed: Result<AcpAgentArgs, _> = serde_json::from_value(args.clone());
                Some(Box::pin(async move {
                    let AcpAgentArgs { agent } =
                        parsed.map_err(|e| PluginError::ExecutionFailed {
                            plugin_id: PLUGIN_ID.to_string(),
                            reason: format!("disconnect: invalid args: {e}"),
                        })?;
                    let dropped = pool.disconnect(&agent).await;
                    Ok(json!({ "agent": agent, "dropped": dropped }))
                }))
            }
            _ => None,
        }
    }
}

/// Send a request through the pool's reconnect loop. Transient
/// failures drop the entry and trigger a fresh connect on the next
/// attempt. Drains pending notifications per attempt so agent-pushed
/// events still fan out even when an earlier attempt failed mid-flight.
async fn proxy_request(
    pool: &ConnectionPool,
    cfg: &AcpHostConfig,
    agent_name: &str,
    bus: Option<Arc<EventBus>>,
    method: &str,
    payload: Value,
) -> Result<Value, AcpClientError> {
    let method = method.to_string();
    pool.call_with_reconnect(agent_name, cfg, move |client| {
        let bus = bus.clone();
        let payload = payload.clone();
        let method = method.clone();
        Box::pin(async move {
            let lock = client.lock().await;
            let r = lock.send_request(&method, payload).await?;
            republish_pending(&lock, bus.as_ref()).await;
            Ok(r)
        })
    })
    .await
}

async fn republish_pending(client: &crate::client::AcpClient, bus: Option<&Arc<EventBus>>) {
    let pending = client.drain_notifications().await;
    if pending.is_empty() {
        return;
    }
    let Some(bus) = bus else {
        return;
    };
    for n in pending {
        let topic = format!("com.nexus.acp.{}", n.method.replace('/', "."));
        if let Err(e) = bus.publish_plugin(PLUGIN_ID, &topic, n.params) {
            tracing::warn!(
                plugin_id = PLUGIN_ID,
                topic = %topic,
                error = %e,
                "failed to republish acp notification"
            );
        }
    }
}

/// Project a stored [`AcpAdapterSpec`] + live `connected` flag into
/// the typed `list_agents` row.
fn agent_entry(spec: &AcpAdapterSpec, connected: bool) -> AcpAgentEntry {
    AcpAgentEntry {
        name: spec.name.clone(),
        command: spec.command.clone(),
        args: spec.args.clone(),
        capabilities: spec.capabilities.clone(),
        disabled: spec.disabled,
        connected,
        metadata: spec.metadata.clone(),
    }
}

fn config_or_err(config: Option<&Arc<AcpHostConfig>>) -> Result<Arc<AcpHostConfig>, PluginError> {
    config.cloned().ok_or_else(|| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: "ACP host config not loaded".to_string(),
    })
}

#[allow(clippy::needless_pass_by_value)]
fn map_client_err(e: AcpClientError) -> PluginError {
    PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_plugin(dir: &std::path::Path) -> AcpCorePlugin {
        AcpCorePlugin::new(dir.to_path_buf(), None)
    }

    #[test]
    fn plugin_id_is_correct() {
        assert_eq!(PLUGIN_ID, "com.nexus.acp");
    }

    #[test]
    fn on_init_succeeds_with_empty_registry() {
        let dir = tempdir().unwrap();
        let mut p = make_plugin(dir.path());
        assert!(p.on_init().is_ok());
        // Registry stays empty until a contribution arrives.
        assert!(p.config.read().unwrap().adapters.is_empty());
    }

    #[test]
    fn list_agents_is_empty_after_init() {
        let dir = tempdir().unwrap();
        let mut p = make_plugin(dir.path());
        p.on_init().unwrap();
        let out = p.dispatch(HANDLER_LIST_AGENTS, &Value::Null).unwrap();
        assert_eq!(out, Value::Array(vec![]));
    }

    #[test]
    fn register_server_round_trip_through_dispatch() {
        let dir = tempdir().unwrap();
        let mut p = make_plugin(dir.path());
        p.on_init().unwrap();
        let args = json!({
            "name": "hermes",
            "command": "hermes-agent",
            "args": ["--stdio"],
            "capabilities": ["delegate", "tools"],
            "disabled": false,
            "env": {"HERMES_LOG": "info"},
            "metadata": { "plugin_id": "community.hermes", "display_name": "Hermes" },
            "plugin_id": "community.hermes",
        });
        let reply = p.dispatch(HANDLER_REGISTER_SERVER, &args).unwrap();
        assert_eq!(reply["ok"], true);
        assert_eq!(reply["status"], "ok");
        let list = p.dispatch(HANDLER_LIST_AGENTS, &Value::Null).unwrap();
        let arr = list.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["name"], "hermes");
        assert_eq!(arr[0]["capabilities"], json!(["delegate", "tools"]));
        assert_eq!(arr[0]["metadata"]["plugin_id"], "community.hermes");
    }

    #[test]
    fn register_server_invalid_name_surfaces_status() {
        let dir = tempdir().unwrap();
        let mut p = make_plugin(dir.path());
        p.on_init().unwrap();
        let reply = p
            .dispatch(
                HANDLER_REGISTER_SERVER,
                &json!({
                    "name": "  ",
                    "command": "x",
                    "plugin_id": "p",
                }),
            )
            .unwrap();
        assert_eq!(reply["ok"], false);
        assert_eq!(reply["status"], "invalid_name");
    }

    #[test]
    fn register_server_duplicate_returns_already_registered() {
        let dir = tempdir().unwrap();
        let mut p = make_plugin(dir.path());
        p.on_init().unwrap();
        let args = json!({
            "name": "a",
            "command": "x",
            "plugin_id": "p1",
        });
        assert_eq!(
            p.dispatch(HANDLER_REGISTER_SERVER, &args).unwrap()["status"],
            "ok",
        );
        let args2 = json!({
            "name": "a",
            "command": "y",
            "plugin_id": "p2",
        });
        assert_eq!(
            p.dispatch(HANDLER_REGISTER_SERVER, &args2).unwrap()["status"],
            "already_registered",
        );
    }

    #[test]
    fn unregister_server_refuses_intruder() {
        let dir = tempdir().unwrap();
        let mut p = make_plugin(dir.path());
        p.on_init().unwrap();
        p.dispatch(
            HANDLER_REGISTER_SERVER,
            &json!({"name": "a", "command": "x", "plugin_id": "owner"}),
        )
        .unwrap();
        let reply = p
            .dispatch(
                HANDLER_UNREGISTER_SERVER,
                &json!({"name": "a", "plugin_id": "intruder"}),
            )
            .unwrap();
        assert_eq!(reply["status"], "not_owned_by_plugin");
        assert_eq!(reply["actual_owner"], "owner");
    }

    #[test]
    fn unregister_server_round_trip() {
        let dir = tempdir().unwrap();
        let mut p = make_plugin(dir.path());
        p.on_init().unwrap();
        p.dispatch(
            HANDLER_REGISTER_SERVER,
            &json!({"name": "a", "command": "x", "plugin_id": "p"}),
        )
        .unwrap();
        let reply = p
            .dispatch(
                HANDLER_UNREGISTER_SERVER,
                &json!({"name": "a", "plugin_id": "p"}),
            )
            .unwrap();
        assert_eq!(reply["ok"], true);
        assert_eq!(reply["status"], "ok");
        let list = p.dispatch(HANDLER_LIST_AGENTS, &Value::Null).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 0);
    }

    #[test]
    fn dispatch_unknown_handler_errors() {
        let dir = tempdir().unwrap();
        let mut p = make_plugin(dir.path());
        let err = p.dispatch(9999, &Value::Null).unwrap_err();
        assert!(matches!(err, PluginError::ExecutionFailed { .. }));
    }

    #[test]
    fn async_handlers_routed_through_dispatch_async() {
        let dir = tempdir().unwrap();
        let mut p = make_plugin(dir.path());
        // Sync `dispatch` must reject them with a clear error.
        for &h in &[
            HANDLER_INITIALIZE,
            HANDLER_PROPOSE,
            HANDLER_ACCEPT,
            HANDLER_REJECT,
            HANDLER_DISCONNECT,
        ] {
            let err = p.dispatch(h, &Value::Null).unwrap_err();
            let PluginError::ExecutionFailed { reason, .. } = err else {
                panic!("expected ExecutionFailed");
            };
            assert!(
                reason.contains("dispatch_async"),
                "handler {h} reason was {reason:?}",
            );
        }
    }
}
