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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde_json::json;
use tokio::sync::{Mutex, RwLock};

use crate::{McpClient, McpClientError, McpHostConfig};

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

type ClientMap = Arc<RwLock<HashMap<String, Arc<Mutex<McpClient>>>>>;

/// Core plugin that manages connections to external MCP servers.
pub struct McpHostPlugin {
    forge_root: PathBuf,
    event_bus: Option<Arc<EventBus>>,
    config: Option<Arc<McpHostConfig>>,
    clients: ClientMap,
}

impl McpHostPlugin {
    /// Create a new (unstarted) MCP host plugin for the given forge root.
    #[must_use]
    pub fn new(forge_root: PathBuf, event_bus: Option<Arc<EventBus>>) -> Self {
        Self {
            forge_root,
            event_bus,
            config: None,
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl CorePlugin for McpHostPlugin {
    fn on_init(&mut self) -> Result<(), PluginError> {
        let mcp_toml = self.forge_root.join(".forge").join("mcp.toml");
        match McpHostConfig::read_from(&mcp_toml) {
            Ok(cfg) => {
                tracing::info!(
                    plugin_id = PLUGIN_ID,
                    servers = cfg.servers.len(),
                    "loaded mcp.toml"
                );
                self.config = Some(Arc::new(cfg));
            }
            Err(crate::McpConfigError::Io { .. }) => {
                tracing::debug!(
                    plugin_id = PLUGIN_ID,
                    "no mcp.toml found — MCP host has no external servers"
                );
            }
            Err(e) => {
                tracing::warn!(plugin_id = PLUGIN_ID, error = %e, "failed to parse mcp.toml");
            }
        }
        Ok(())
    }

    fn on_start(&mut self) -> Result<(), PluginError> {
        let server_count = self
            .config
            .as_ref()
            .map_or(0, |c| c.servers.len());

        if let Some(bus) = &self.event_bus {
            let _ = bus.publish_plugin(
                PLUGIN_ID,
                "com.nexus.mcp.host.started",
                json!({ "configured_servers": server_count }),
            );
        }
        tracing::info!(
            plugin_id = PLUGIN_ID,
            configured_servers = server_count,
            "MCP host started (connections are lazy)"
        );
        Ok(())
    }

    fn on_stop(&mut self) {
        // Best-effort: drop client map — McpClient's Drop sends graceful close.
        let clients = Arc::clone(&self.clients);
        let _ = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            if let Ok(rt) = rt {
                rt.block_on(async move {
                    let mut map = clients.write().await;
                    map.clear();
                });
            }
        })
        .join();
        tracing::info!(plugin_id = PLUGIN_ID, "MCP host stopped");
    }

    fn dispatch(
        &mut self,
        handler_id: u32,
        _args: &serde_json::Value,
    ) -> Result<serde_json::Value, PluginError> {
        match handler_id {
            HANDLER_LIST_SERVERS => {
                let arr = self
                    .config
                    .as_ref()
                    .map(|cfg| {
                        cfg.servers
                            .iter()
                            .map(|(name, spec)| {
                                json!({
                                    "name": name,
                                    "command": spec.command,
                                    "args": spec.args,
                                    "disabled": spec.disabled,
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                Ok(serde_json::Value::Array(arr))
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
        let clients = Arc::clone(&self.clients);
        let config = self.config.clone();

        match handler_id {
            HANDLER_CONNECT => {
                let server = str_arg(args, "server")?;
                Some(Box::pin(async move {
                    let cfg = config_or_err(config.as_ref())?;
                    connect_server(&server, &cfg, &clients).await?;
                    Ok(json!({"ok": true, "server": server}))
                }))
            }

            HANDLER_DISCONNECT => {
                let server = str_arg(args, "server")?;
                Some(Box::pin(async move {
                    let mut map = clients.write().await;
                    if map.remove(&server).is_some() {
                        Ok(json!({"ok": true, "server": server}))
                    } else {
                        Ok(json!({"ok": false, "server": server, "reason": "not connected"}))
                    }
                }))
            }

            HANDLER_LIST_TOOLS => {
                let server = str_arg(args, "server")?;
                Some(Box::pin(async move {
                    let cfg = config_or_err(config.as_ref())?;
                    let client = get_or_connect(&server, &cfg, &clients).await?;
                    let lock = client.lock().await;
                    let tools = lock.list_tools().await.map_err(map_client_err)?;
                    let arr = tools
                        .iter()
                        .map(|t| {
                            json!({
                                "name": t.name,
                                "description": t.description,
                            })
                        })
                        .collect::<Vec<_>>();
                    Ok(serde_json::Value::Array(arr))
                }))
            }

            HANDLER_LIST_RESOURCES => {
                let server = str_arg(args, "server")?;
                Some(Box::pin(async move {
                    let cfg = config_or_err(config.as_ref())?;
                    let client = get_or_connect(&server, &cfg, &clients).await?;
                    let lock = client.lock().await;
                    let resources = lock
                        .list_resources()
                        .await
                        .map_err(map_client_err)?;
                    let arr = resources
                        .iter()
                        .map(|r| {
                            json!({
                                "uri": r.uri,
                                "name": r.name,
                                "description": r.description,
                                "mime_type": r.mime_type,
                            })
                        })
                        .collect::<Vec<_>>();
                    Ok(serde_json::Value::Array(arr))
                }))
            }

            HANDLER_LIST_PROMPTS => {
                let server = str_arg(args, "server")?;
                Some(Box::pin(async move {
                    let cfg = config_or_err(config.as_ref())?;
                    let client = get_or_connect(&server, &cfg, &clients).await?;
                    let lock = client.lock().await;
                    let prompts = lock.list_prompts().await.map_err(map_client_err)?;
                    let arr = prompts
                        .iter()
                        .map(|p| {
                            json!({
                                "name": p.name,
                                "description": p.description,
                            })
                        })
                        .collect::<Vec<_>>();
                    Ok(serde_json::Value::Array(arr))
                }))
            }

            HANDLER_CALL_TOOL => {
                let server = str_arg(args, "server")?;
                let tool = str_arg(args, "tool")?;
                let tool_args = args
                    .get("arguments")
                    .and_then(|v| v.as_object())
                    .cloned();
                Some(Box::pin(async move {
                    let cfg = config_or_err(config.as_ref())?;
                    let client = get_or_connect(&server, &cfg, &clients).await?;
                    let lock = client.lock().await;
                    let result = lock
                        .call_tool(tool, tool_args)
                        .await
                        .map_err(map_client_err)?;
                    let content: Vec<_> = result
                        .content
                        .iter()
                        .filter_map(|c| serde_json::to_value(c).ok())
                        .collect();
                    Ok(json!({
                        "content": content,
                        "is_error": result.is_error,
                    }))
                }))
            }

            _ => None,
        }
    }
}

fn str_arg(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
}

fn config_or_err(
    config: Option<&Arc<McpHostConfig>>,
) -> Result<Arc<McpHostConfig>, PluginError> {
    config
        .cloned()
        .ok_or_else(|| PluginError::ExecutionFailed {
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

async fn connect_server(
    name: &str,
    config: &McpHostConfig,
    clients: &RwLock<HashMap<String, Arc<Mutex<McpClient>>>>,
) -> Result<(), PluginError> {
    let spec = config.servers.get(name).ok_or_else(|| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: format!("server '{name}' not found in mcp.toml"),
    })?;
    if spec.disabled {
        return Err(PluginError::ExecutionFailed {
            plugin_id: PLUGIN_ID.to_string(),
            reason: format!("server '{name}' is disabled in mcp.toml"),
        });
    }
    let client = McpClient::connect(name, spec)
        .await
        .map_err(map_client_err)?;
    clients
        .write()
        .await
        .insert(name.to_string(), Arc::new(Mutex::new(client)));
    tracing::info!(plugin_id = PLUGIN_ID, server = name, "connected to MCP server");
    Ok(())
}

async fn get_or_connect(
    name: &str,
    config: &McpHostConfig,
    clients: &RwLock<HashMap<String, Arc<Mutex<McpClient>>>>,
) -> Result<Arc<Mutex<McpClient>>, PluginError> {
    {
        let map = clients.read().await;
        if let Some(c) = map.get(name) {
            return Ok(Arc::clone(c));
        }
    }
    connect_server(name, config, clients).await?;
    let map = clients.read().await;
    map.get(name).cloned().ok_or_else(|| PluginError::ExecutionFailed {
        plugin_id: PLUGIN_ID.to_string(),
        reason: format!("failed to store client for '{name}' after connect"),
    })
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
        // read_from returns Ok(empty) for a missing file — config is Some but empty.
        let cfg = plugin.config.as_ref().unwrap();
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
        let cfg = plugin.config.as_ref().unwrap();
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
        let names: Vec<&str> = arr
            .iter()
            .map(|v| v["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"fs"));
        assert!(names.contains(&"gh"));
    }

    #[test]
    fn async_handler_without_server_arg_returns_none() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".forge")).unwrap();
        let mut plugin = make_plugin(dir.path());
        plugin.on_init().unwrap();
        // Missing "server" arg → str_arg returns None → dispatch_async returns None
        let result = plugin.dispatch_async(HANDLER_LIST_TOOLS, &json!({}));
        assert!(result.is_none());
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
}
