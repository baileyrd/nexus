//! Map a registry tool name to the IPC `(target_plugin_id,
//! command_id, args)` triple needed to invoke it directly via
//! [`KernelPluginContext::ipc_call`].
//!
//! Used by the upcoming agent migration (ADR 0023): the agent's
//! `LlmAgent` produces a [`Plan`] of [`ToolCall`]s by mapping the
//! model's provider-native tool-use blocks through this function,
//! and the existing `PlanExecutor` then dispatches each call.
//!
//! Today the mapping is a hand-written table — every new built-in
//! tool added to [`crate::tools::functions`] also needs an entry
//! here. Phase 1b of ADR 0023 will fold this metadata back into the
//! registry so `register_*_builtins` is the only place to update.
//!
//! ## Argument reshaping
//!
//! Most tools accept the same JSON shape as their downstream IPC
//! handler. The exception so far is `write_file`: the model thinks
//! in `content: string` but `com.nexus.storage::write_file` expects
//! `bytes: number[]`. The mapping function performs the reshape
//! eagerly so the agent's executor never sees the asymmetry.
//!
//! ## MCP tools
//!
//! Tools advertised via the MCP bridge are registered as
//! `mcp__<server>__<tool>` (see [`crate::tools::mcp_bridge`]). The
//! mapper parses the `mcp__` prefix and the `__` separator after the
//! server name; the model's `input` is wrapped under
//! `{server, tool, arguments}` for `com.nexus.mcp.host::call_tool`.
//!
//! Names truncated to 64 chars by the bridge's sanitiser may resolve
//! to a tool name the MCP server doesn't recognise — the call will
//! fail at dispatch time. Same failure mode that exists today; not a
//! regression.
//!
//! [`Plan`]: nexus_agent::Plan
//! [`ToolCall`]: nexus_agent::ToolCall
//! [`KernelPluginContext::ipc_call`]: nexus_kernel::PluginContext::ipc_call

use serde_json::{json, Value};

/// Plugin id of the storage core plugin.
const STORAGE_PLUGIN: &str = "com.nexus.storage";
/// Plugin id of the git core plugin.
const GIT_PLUGIN: &str = "com.nexus.git";
/// Plugin id of the MCP host plugin.
const MCP_PLUGIN: &str = "com.nexus.mcp.host";
/// Plugin id of the terminal core plugin (BL-055).
const TERMINAL_PLUGIN: &str = "com.nexus.terminal";

/// Why a tool name couldn't be mapped to an IPC dispatch triple.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DispatchTargetError {
    /// The tool name doesn't match any built-in or the MCP-bridge prefix.
    #[error("unknown tool name '{0}'")]
    UnknownTool(String),
    /// An MCP-prefixed name was malformed (missing the `__` separator
    /// between server and tool).
    #[error("malformed mcp tool name '{0}': expected mcp__<server>__<tool>")]
    MalformedMcp(String),
    /// A built-in's input was missing a required field needed for the
    /// reshape (e.g. `write_file` without `content`).
    #[error("invalid input for tool '{tool}': {reason}")]
    InvalidInput {
        /// Tool name as advertised in the registry.
        tool: String,
        /// Human-readable reason.
        reason: String,
    },
}

/// Resolved dispatch triple: the target plugin id, command, and args
/// to hand to `ipc_call`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchTarget {
    /// Reverse-DNS plugin id (e.g. `"com.nexus.storage"`).
    pub target_plugin_id: String,
    /// Command id within the target plugin (e.g. `"read_file"`).
    pub command_id: String,
    /// JSON arguments — possibly reshaped from the model's input.
    pub args: Value,
}

/// Resolve a registry tool name to a `(target, command, args)`
/// triple. `input` is the JSON the model emitted for the tool call;
/// some tools require it to be reshaped before dispatch.
///
/// # Errors
/// - [`DispatchTargetError::UnknownTool`] if `name` doesn't match a
///   built-in and isn't `mcp__`-prefixed.
/// - [`DispatchTargetError::MalformedMcp`] if an `mcp__` name lacks
///   the `__` separator between server and tool.
/// - [`DispatchTargetError::InvalidInput`] if a built-in needed a
///   field for reshape that wasn't present.
pub fn dispatch_target(name: &str, input: Value) -> Result<DispatchTarget, DispatchTargetError> {
    match name {
        "read_file" => Ok(DispatchTarget {
            target_plugin_id: STORAGE_PLUGIN.into(),
            command_id: "read_file".into(),
            args: input,
        }),
        "write_file" => {
            // Reshape `content: string` → `bytes: number[]` so the
            // storage handler accepts the call. WriteFileTool does
            // the same reshape at execution time; doing it here
            // means the agent's executor can hand args straight to
            // ipc_call without per-tool special cases.
            let path = input
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| DispatchTargetError::InvalidInput {
                    tool: "write_file".into(),
                    reason: "missing 'path' string".into(),
                })?
                .to_string();
            let content = input
                .get("content")
                .and_then(Value::as_str)
                .ok_or_else(|| DispatchTargetError::InvalidInput {
                    tool: "write_file".into(),
                    reason: "missing 'content' string".into(),
                })?;
            let bytes: Vec<u8> = content.as_bytes().to_vec();
            Ok(DispatchTarget {
                target_plugin_id: STORAGE_PLUGIN.into(),
                command_id: "write_file".into(),
                args: json!({ "path": path, "bytes": bytes }),
            })
        }
        "search_forge" => Ok(DispatchTarget {
            target_plugin_id: STORAGE_PLUGIN.into(),
            command_id: "search".into(),
            args: input,
        }),
        "list_backlinks" => Ok(DispatchTarget {
            target_plugin_id: STORAGE_PLUGIN.into(),
            command_id: "backlinks".into(),
            args: input,
        }),
        "git_log" => Ok(DispatchTarget {
            target_plugin_id: GIT_PLUGIN.into(),
            command_id: "log".into(),
            args: input,
        }),
        "terminal_run_saved" => Ok(DispatchTarget {
            target_plugin_id: TERMINAL_PLUGIN.into(),
            command_id: "run_saved".into(),
            args: input,
        }),
        "terminal_get_status" => {
            // Tool surface uses `id`; the underlying handler also takes
            // `id`, so the input is a straight passthrough.
            Ok(DispatchTarget {
                target_plugin_id: TERMINAL_PLUGIN.into(),
                command_id: "get_session_info".into(),
                args: input,
            })
        }
        "terminal_send_signal" => {
            // Reshape `{ id, signal }` → `{ id, data: [<byte>] }` so
            // the agent's executor can hand args straight to ipc_call
            // without per-tool special cases — same pattern as
            // `write_file`'s content→bytes reshape above.
            let id = input
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| DispatchTargetError::InvalidInput {
                    tool: "terminal_send_signal".into(),
                    reason: "missing 'id' string".into(),
                })?
                .to_string();
            let signal = input.get("signal").and_then(Value::as_str).ok_or_else(|| {
                DispatchTargetError::InvalidInput {
                    tool: "terminal_send_signal".into(),
                    reason: "missing 'signal' string".into(),
                }
            })?;
            let byte = match signal {
                "SIGINT" => 0x03_u8,
                "SIGQUIT" => 0x1c,
                "SIGTSTP" => 0x1a,
                "EOF" => 0x04,
                other => {
                    return Err(DispatchTargetError::InvalidInput {
                        tool: "terminal_send_signal".into(),
                        reason: format!(
                            "unsupported signal '{other}'; expected SIGINT|SIGQUIT|SIGTSTP|EOF"
                        ),
                    });
                }
            };
            Ok(DispatchTarget {
                target_plugin_id: TERMINAL_PLUGIN.into(),
                command_id: "send_raw_input".into(),
                args: json!({ "id": id, "data": [byte] }),
            })
        }
        other if other.starts_with("mcp__") => {
            // Strip prefix, split on the first `__` to recover
            // (server, tool). The bridge's sanitiser may have
            // truncated the tool tail at 64 chars — that fails at
            // the MCP server's resolver, not here.
            let rest = &other["mcp__".len()..];
            let sep = rest
                .find("__")
                .ok_or_else(|| DispatchTargetError::MalformedMcp(other.to_string()))?;
            let server = &rest[..sep];
            let tool = &rest[sep + 2..];
            if server.is_empty() || tool.is_empty() {
                return Err(DispatchTargetError::MalformedMcp(other.to_string()));
            }
            Ok(DispatchTarget {
                target_plugin_id: MCP_PLUGIN.into(),
                command_id: "call_tool".into(),
                args: json!({
                    "server": server,
                    "tool": tool,
                    "arguments": input,
                }),
            })
        }
        other => Err(DispatchTargetError::UnknownTool(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_file_passes_input_through() {
        let t = dispatch_target("read_file", json!({"path": "notes/a.md"})).unwrap();
        assert_eq!(t.target_plugin_id, "com.nexus.storage");
        assert_eq!(t.command_id, "read_file");
        assert_eq!(t.args, json!({"path": "notes/a.md"}));
    }

    #[test]
    fn write_file_reshapes_content_to_bytes() {
        let t = dispatch_target("write_file", json!({"path": "x.md", "content": "hi"})).unwrap();
        assert_eq!(t.target_plugin_id, "com.nexus.storage");
        assert_eq!(t.command_id, "write_file");
        let bytes = t.args["bytes"].as_array().expect("bytes array");
        let decoded: Vec<u8> = bytes
            .iter()
            .map(|v| u8::try_from(v.as_u64().unwrap()).unwrap())
            .collect();
        assert_eq!(decoded, b"hi");
        assert_eq!(t.args["path"], "x.md");
    }

    #[test]
    fn write_file_rejects_missing_content() {
        let err = dispatch_target("write_file", json!({"path": "x.md"})).unwrap_err();
        assert!(matches!(err, DispatchTargetError::InvalidInput { .. }));
    }

    #[test]
    fn search_forge_targets_storage_search() {
        let t = dispatch_target("search_forge", json!({"query": "rust"})).unwrap();
        assert_eq!(t.target_plugin_id, "com.nexus.storage");
        assert_eq!(t.command_id, "search");
    }

    #[test]
    fn list_backlinks_targets_storage_backlinks() {
        let t = dispatch_target("list_backlinks", json!({"path": "x.md"})).unwrap();
        assert_eq!(t.command_id, "backlinks");
    }

    #[test]
    fn git_log_targets_git_log() {
        let t = dispatch_target("git_log", json!({})).unwrap();
        assert_eq!(t.target_plugin_id, "com.nexus.git");
        assert_eq!(t.command_id, "log");
    }

    #[test]
    fn mcp_name_routes_to_call_tool_with_wrapped_args() {
        let t = dispatch_target("mcp__notes__fetch", json!({"url": "https://example"})).unwrap();
        assert_eq!(t.target_plugin_id, "com.nexus.mcp.host");
        assert_eq!(t.command_id, "call_tool");
        assert_eq!(t.args["server"], "notes");
        assert_eq!(t.args["tool"], "fetch");
        assert_eq!(t.args["arguments"]["url"], "https://example");
    }

    #[test]
    fn mcp_malformed_without_separator_errors() {
        let err = dispatch_target("mcp__only_one_part", json!({})).unwrap_err();
        assert!(matches!(err, DispatchTargetError::MalformedMcp(_)));
    }

    #[test]
    fn mcp_empty_server_or_tool_errors() {
        let err = dispatch_target("mcp____tool", json!({})).unwrap_err();
        assert!(matches!(err, DispatchTargetError::MalformedMcp(_)));
        let err = dispatch_target("mcp__server__", json!({})).unwrap_err();
        assert!(matches!(err, DispatchTargetError::MalformedMcp(_)));
    }

    #[test]
    fn terminal_run_saved_targets_run_saved() {
        let t = dispatch_target(
            "terminal_run_saved",
            json!({"slug": "dev", "working_dir": "/tmp"}),
        )
        .unwrap();
        assert_eq!(t.target_plugin_id, "com.nexus.terminal");
        assert_eq!(t.command_id, "run_saved");
        assert_eq!(t.args["slug"], "dev");
        assert_eq!(t.args["working_dir"], "/tmp");
    }

    #[test]
    fn terminal_get_status_targets_get_session_info() {
        let t = dispatch_target("terminal_get_status", json!({"id": "sess-1"})).unwrap();
        assert_eq!(t.target_plugin_id, "com.nexus.terminal");
        assert_eq!(t.command_id, "get_session_info");
        assert_eq!(t.args["id"], "sess-1");
    }

    #[test]
    fn terminal_send_signal_reshapes_signal_to_byte() {
        for (signal, expected) in [
            ("SIGINT", 0x03_u64),
            ("SIGQUIT", 0x1c),
            ("SIGTSTP", 0x1a),
            ("EOF", 0x04),
        ] {
            let t = dispatch_target("terminal_send_signal", json!({"id": "x", "signal": signal}))
                .unwrap_or_else(|e| panic!("{signal}: {e:?}"));
            assert_eq!(t.target_plugin_id, "com.nexus.terminal");
            assert_eq!(t.command_id, "send_raw_input");
            assert_eq!(t.args["id"], "x");
            let data = t.args["data"].as_array().expect("data array");
            assert_eq!(data.len(), 1);
            assert_eq!(data[0].as_u64().unwrap(), expected);
        }
    }

    #[test]
    fn terminal_send_signal_rejects_unknown_signal() {
        let err = dispatch_target(
            "terminal_send_signal",
            json!({"id": "x", "signal": "SIGKILL"}),
        )
        .unwrap_err();
        match err {
            DispatchTargetError::InvalidInput { tool, reason } => {
                assert_eq!(tool, "terminal_send_signal");
                assert!(reason.contains("SIGKILL"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn terminal_send_signal_rejects_missing_fields() {
        // Missing id.
        let err = dispatch_target("terminal_send_signal", json!({"signal": "SIGINT"})).unwrap_err();
        assert!(matches!(err, DispatchTargetError::InvalidInput { .. }));
        // Missing signal.
        let err = dispatch_target("terminal_send_signal", json!({"id": "x"})).unwrap_err();
        assert!(matches!(err, DispatchTargetError::InvalidInput { .. }));
    }

    #[test]
    fn unknown_tool_errors_loudly() {
        let err = dispatch_target("definitely_not_a_tool", json!({})).unwrap_err();
        match err {
            DispatchTargetError::UnknownTool(n) => assert_eq!(n, "definitely_not_a_tool"),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
