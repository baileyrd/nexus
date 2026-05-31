//! MCP server configuration (`mcp.toml`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// MCP server configuration loaded from `.forge/mcp.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    /// Whether the MCP server is enabled.
    pub enabled: bool,
    /// Transport type (`"stdio"` or `"http"`).
    pub transport: String,
    /// Tools that may be exposed to MCP clients.
    pub allowed_tools: Vec<String>,
    /// Named MCP server entries from `[mcp.<name>]` table.
    #[serde(default)]
    pub mcp: BTreeMap<String, McpServerEntry>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            transport: "stdio".into(),
            allowed_tools: Vec::new(),
            mcp: BTreeMap::new(),
        }
    }
}

/// A single MCP server entry (e.g. `[mcp.local-database]`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    /// Server type: `"local"`, `"stdio"`, or `"http"`.
    #[serde(rename = "type")]
    pub server_type: String,
    /// Command to spawn (for stdio / local transports).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Command arguments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// URL for http transport.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// API key for authenticated endpoints.
    #[serde(rename = "apiKey", skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Request timeout in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    /// Environment variables to pass to the subprocess.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}
