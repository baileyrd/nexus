//! `nexus mcp` — start the MCP server on stdio transport + drive the
//! MCP host (connect to external servers, list tools, invoke tools)
//! through `com.nexus.mcp.host` over `ipc_call`.

use std::sync::Arc;

use anyhow::{Context, Result};
use nexus_bootstrap::{build_cli_runtime, Runtime};
use nexus_types::constants::IPC_TIMEOUT_NORMAL as IPC_TIMEOUT;
use nexus_types::plugin_ids;
use serde_json::Value;

use crate::app::App;

const MCP_HOST_PLUGIN: &str = plugin_ids::MCP;

/// Start the MCP stdio server, blocking until the client disconnects.
///
/// Builds a Nexus runtime and hands the resulting plugin context to
/// [`nexus_mcp::NexusMcpServer`], which dispatches every tool call through
/// `ipc_call` rather than holding a storage engine directly.
///
/// # Errors
///
/// Returns an error if the forge cannot be opened or the server fails to start.
pub fn serve(app: &App) -> Result<()> {
    let forge_root = app.forge_root().to_path_buf();
    let runtime = build_cli_runtime(forge_root.clone())
        .with_context(|| format!("failed to build runtime at {}", forge_root.display()))?;

    // Destructure Runtime so we can move `context` into an Arc while keeping
    // the kernel and loader alive for the server's lifetime.
    let Runtime { kernel: _kernel, context, loader: _loader } = runtime;
    let context = Arc::new(context);

    let server = nexus_mcp::NexusMcpServer::new(Arc::clone(&context));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .max_blocking_threads(nexus_types::constants::KERNEL_BLOCKING_POOL_SIZE)
        .enable_all()
        .build()?;
    rt.block_on(server.serve_stdio())
        .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;

    Ok(())
}

/// `nexus mcp servers` — enumerate external MCP servers declared in
/// `.forge/mcp.toml` via the host plugin.
pub fn host_servers(app: &mut App) -> Result<()> {
    let response = call(app, "list_servers", serde_json::json!({}))?;
    let servers = response.as_array().cloned().unwrap_or_default();
    if servers.is_empty() {
        println!("(no servers declared in .forge/mcp.toml)");
        return Ok(());
    }
    let name_w = servers
        .iter()
        .filter_map(|s| s.get("name").and_then(Value::as_str))
        .map(str::len)
        .max()
        .unwrap_or(4)
        .max(4);
    println!(
        "{:<name_w$}  {:<8}  COMMAND",
        "NAME", "STATE",
        name_w = name_w
    );
    for server in servers {
        let name = server.get("name").and_then(Value::as_str).unwrap_or("?");
        let cmd = server.get("command").and_then(Value::as_str).unwrap_or("?");
        let args = server
            .get("args")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let state = if server
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            "disabled"
        } else {
            "enabled"
        };
        println!(
            "{:<name_w$}  {:<8}  {} {}",
            name,
            state,
            cmd,
            args,
            name_w = name_w
        );
    }
    Ok(())
}

/// `nexus mcp tools <server>` — list tools exposed by one MCP server.
/// Connects lazily through the host plugin on first use.
pub fn host_tools(app: &mut App, server: &str) -> Result<()> {
    let response = call(app, "list_tools", serde_json::json!({ "server": server }))?;
    let tools = response.as_array().cloned().unwrap_or_default();
    if tools.is_empty() {
        println!("(server '{server}' exposes no tools)");
        return Ok(());
    }
    let name_w = tools
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .map(str::len)
        .max()
        .unwrap_or(4)
        .max(4);
    println!("{:<name_w$}  DESCRIPTION", "NAME", name_w = name_w);
    for tool in tools {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or("?");
        let desc = tool.get("description").and_then(Value::as_str).unwrap_or("");
        println!("{:<name_w$}  {}", name, desc, name_w = name_w);
    }
    Ok(())
}

/// `nexus mcp call <server> <tool> --arguments '{...}'` — invoke a
/// tool on an external MCP server.
pub fn host_call(app: &mut App, server: &str, tool: &str, arguments: &str) -> Result<()> {
    let args: Value = serde_json::from_str(arguments)
        .with_context(|| format!("--arguments is not valid JSON: {arguments}"))?;
    let response = call(
        app,
        "call_tool",
        serde_json::json!({
            "server": server,
            "tool": tool,
            "arguments": args,
        }),
    )?;
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

fn call(app: &mut App, command: &str, args: Value) -> Result<Value> {
    let (invoker, rt) = app.invoker()?;
    rt.block_on(invoker.ipc_call(MCP_HOST_PLUGIN, command, args, IPC_TIMEOUT))
        .with_context(|| format!("mcp host ipc call '{command}' failed"))
}
