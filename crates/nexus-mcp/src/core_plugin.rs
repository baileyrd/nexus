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

use std::path::PathBuf;
use std::sync::Arc;

use nexus_kernel::EventBus;
use nexus_plugins::{CorePlugin, CorePluginFuture, PluginError};
use serde_json::json;

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

/// Core plugin that manages connections to external MCP servers.
pub struct McpHostPlugin {
    forge_root: PathBuf,
    event_bus: Option<Arc<EventBus>>,
    config: Option<Arc<McpHostConfig>>,
    pool: Arc<ConnectionPool>,
}

impl McpHostPlugin {
    /// Create a new (unstarted) MCP host plugin for the given forge root.
    #[must_use]
    pub fn new(forge_root: PathBuf, event_bus: Option<Arc<EventBus>>) -> Self {
        Self {
            forge_root,
            event_bus,
            config: None,
            pool: Arc::new(ConnectionPool::new(PoolConfig::default())),
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
        let server_count = self.config.as_ref().map_or(0, |c| c.servers.len());

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
        let pool = Arc::clone(&self.pool);
        let config = self.config.clone();

        match handler_id {
            HANDLER_CONNECT => {
                let server = str_arg(args, "server")?;
                Some(Box::pin(async move {
                    let cfg = config_or_err(config.as_ref())?;
                    pool.get_or_connect(&server, &cfg)
                        .await
                        .map_err(map_client_err)?;
                    Ok(json!({"ok": true, "server": server}))
                }))
            }

            HANDLER_DISCONNECT => {
                let server = str_arg(args, "server")?;
                Some(Box::pin(async move {
                    if pool.disconnect(&server).await {
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
                    let client = pool
                        .get_or_connect(&server, &cfg)
                        .await
                        .map_err(map_client_err)?;
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
                    let client = pool
                        .get_or_connect(&server, &cfg)
                        .await
                        .map_err(map_client_err)?;
                    let lock = client.lock().await;
                    let resources = lock.list_resources().await.map_err(map_client_err)?;
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
                    let client = pool
                        .get_or_connect(&server, &cfg)
                        .await
                        .map_err(map_client_err)?;
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
                let tool_args = args.get("arguments").and_then(|v| v.as_object()).cloned();
                Some(Box::pin(async move {
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
                    Ok(json!({
                        "content": content,
                        "is_error": result.is_error,
                        "truncated": truncated,
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
        let names: Vec<&str> = arr.iter().map(|v| v["name"].as_str().unwrap()).collect();
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
