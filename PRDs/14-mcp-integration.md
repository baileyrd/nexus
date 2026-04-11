# PRD: MCP Integration Subsystem — Nexus v1.0

**Document:** Product Requirements Document (PRD)  
**Subsystem:** MCP Integration (nexus-mcp crate)  
**Version:** 1.0  
**Date:** April 2026  
**Status:** Implementation Ready  
**Target Audience:** Core engineering team, MCP server developers, external integration partners

---

## 1. Executive Summary

Nexus fully embraces the **Model Context Protocol (MCP)** as a first-class integration layer, positioning itself as both an MCP **Host (Client)** and **Server (Provider)**. This dual role enables a unified ecosystem where:

- **Nexus as MCP Host:** Connects to external MCP servers (databases, services, custom tools) to extend the AI Engine and agent capabilities with third-party integrations.
- **Nexus as MCP Server:** Exposes forge capabilities (file I/O, search, terminal, process management, databases, plugins) to external MCP-compatible AI clients (Claude Code, Cursor, standalone agents).

This PRD specifies the complete MCP integration architecture: protocol implementation, server/host lifecycle management, tool/resource definitions, authentication, configuration, and operational patterns. The subsystem is designed for extensibility, security, and seamless AI-native integration.

---

## 2. Goals and Success Criteria

### Goals
- Enable bidirectional MCP integration: Nexus consumes external MCP servers and exposes itself as an MCP server.
- Provide a frictionless discovery and connection experience for MCP servers via UI and configuration.
- Expose all Nexus forge capabilities as standard MCP tools and resources accessible to external AI clients.
- Support multiple transport layers (stdio, HTTP+SSE, WebSocket) with automatic failover.
- Deliver sampling support (servers requesting host LLM text generation) via AI Engine integration.
- Maintain security boundaries: authenticate external clients, gate access to sensitive operations, audit usage.

### Success Criteria (v1.0)
- [ ] MCP protocol v1.0+ fully implemented with support for stdio, HTTP+SSE, and WebSocket transports.
- [ ] MCP Host: Successfully connect to ≥5 external reference servers (Postgres, SQLite, GitHub API, Slack, Linear).
- [ ] MCP Server: Expose ≥12 core tools (forge_read, forge_write, forge_search, db_query, db_create_record, terminal_exec, process_start, plugin_command, etc.).
- [ ] Automatic tool registration: When plugins register commands, those commands appear as MCP tools within 100ms.
- [ ] Resource enumeration: Notes, databases, records, and config accessible via MCP resource URIs with template matching.
- [ ] Authentication: API key and OAuth flows functional for external clients; Nexus host securely stores MCP server credentials in keychain.
- [ ] Configuration: mcp.toml schema complete with server definitions, transport config, capability restrictions, and auto-connect settings.
- [ ] Sampling support: External MCP servers can request text generation from Nexus AI Engine via sampling interface.
- [ ] Error resilience: Graceful degradation when MCP servers crash, timeout, or become unavailable.
- [ ] UI operational: MCP server browser, tool explorer, and server configuration panels fully functional.
- [ ] Headless server: `nexus mcp serve` CLI command starts Nexus MCP server without GUI.

---

## 3. Architecture Overview

### 3.1 Core Components

```
nexus-mcp/
├── protocol/
│   ├── message.rs            # MCP message types, JSON marshalling
│   ├── version.rs            # Protocol version handling, negotiation
│   └── codec.rs              # Message framing for all transports
├── transport/
│   ├── trait.rs              # Transport abstraction
│   ├── stdio.rs              # Stdio transport (child process communication)
│   ├── http_sse.rs           # HTTP+SSE bidirectional transport
│   ├── websocket.rs          # WebSocket transport with reconnection
│   ├── pool.rs               # Connection pooling, lifecycle management
│   └── reconnect.rs          # Automatic reconnection with exponential backoff
├── host/
│   ├── client.rs             # MCP Client: connects to external servers
│   ├── discovery.rs          # Server discovery (local registry, remote manifest)
│   ├── capability_negotiation.rs # Handshake, feature detection
│   ├── tool_registry.rs      # Tool enumeration and caching from external servers
│   ├── resource_retrieval.rs # Resource enumeration and fetching
│   └── request_queue.rs      # Request queuing, deduplication, priority
├── server/
│   ├── handler.rs            # MCP Server: request dispatcher
│   ├── tool_executor.rs      # Execute tool requests against Nexus capabilities
│   ├── resource_provider.rs  # Serve resources (notes, databases, config)
│   ├── prompt_provider.rs    # MCP prompts: system prompts for external agents
│   └── sampling_handler.rs   # Respond to sampling requests via AI Engine
├── tools/
│   ├── schema.rs             # JSON Schema builders for all tools
│   ├── forge_tools.rs        # forge_read, forge_write, forge_search, forge_list
│   ├── database_tools.rs     # db_query, db_create_record, db_mutate, db_schema
│   ├── terminal_tools.rs     # terminal_exec, terminal_read, terminal_kill
│   ├── process_tools.rs      # process_start, process_stop, process_status, process_list
│   ├── plugin_tools.rs       # plugin_command (dynamic registration)
│   └── execution_engine.rs   # Tool invocation, result marshalling, error handling
├── resources/
│   ├── resolver.rs           # Resource URI resolution (mcp://forge/notes/*, mcp://db/*, etc.)
│   ├── note_resources.rs     # Note listing and content retrieval
│   ├── database_resources.rs # Database and record resources
│   └── config_resources.rs   # Forge configuration and index resources
├── auth/
│   ├── authenticator.rs      # Client authentication (API key, OAuth)
│   ├── keychain.rs           # Secure credential storage for MCP servers
│   └── capability_gate.rs    # Access control: which operations are allowed per client
├── config/
│   ├── schema.rs             # mcp.toml schema and validation
│   ├── loader.rs             # Load and parse mcp.toml
│   └── watcher.rs            # File system watcher for config hot-reload
├── sampling/
│   ├── request_handler.rs    # Incoming sampling requests
│   └── ai_integration.rs     # Bridge to nexus-ai Engine
├── logging/
│   ├── audit.rs              # Audit log for all tool calls and resource access
│   └── metrics.rs            # Latency, throughput, error rates per server
├── error.rs                  # MCP-specific error types
├── lib.rs                    # Public API facade
└── cli.rs                    # CLI: `nexus mcp serve`, `nexus mcp connect`, etc.
```

### 3.2 Integration Diagram

```
┌────────────────────────────────────────────────────────────────┐
│                         Nexus Core                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐          │
│  │AI Engine     │  │Plugin System │  │Terminal Mgr  │          │
│  │(sampling)    │  │(tool reg)    │  │(exec)        │          │
│  └──────────────┘  └──────────────┘  └──────────────┘          │
│          ↑                ↑                  ↑                   │
└──────────┼────────────────┼──────────────────┼───────────────────┘
           │                │                  │
┌──────────┴────────────────┴──────────────────┴──────────────────┐
│                    nexus-mcp Subsystem                           │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │ Server (MCP Provider)                                      │ │
│  │  ├─ tool_executor: forge_read, db_query, terminal_exec   │ │
│  │  ├─ resource_provider: mcp://forge/notes/*, mcp://db/*   │ │
│  │  ├─ sampling_handler: delegate to AI Engine              │ │
│  │  └─ request_dispatcher: route incoming requests          │ │
│  └────────────────────────────────────────────────────────────┘ │
│           ↑                                        ↓              │
│    External MCP Clients                    [transport: stdio,    │
│    (Claude Code, Cursor,                    HTTP+SSE, ws]       │
│     standalone agents)                                          │
│           ↑                                        ↓              │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │ Host (MCP Client)                                          │ │
│  │  ├─ client: connect to external servers                   │ │
│  │  ├─ tool_registry: enumerate remote tools                │ │
│  │  ├─ resource_retrieval: fetch remote resources           │ │
│  │  ├─ sampling_forwarder: forward requests to servers      │ │
│  │  └─ reconnect_manager: auto-recover from failures        │ │
│  └────────────────────────────────────────────────────────────┘ │
│                      ↓                                            │
└──────────────────────┼────────────────────────────────────────────┘
                       │
    External MCP Servers
    (Postgres, Slack, GitHub API, Linear, custom)
```

---

## 4. MCP Protocol Implementation

### 4.1 Specification Compliance

- **Protocol Version:** MCP 1.0+ (Anthropic's published specification).
- **Implementation Library:** `mcp` Rust crate (if available) or custom implementation.
- **Message Format:** JSON-RPC 2.0 over multiple transports.
- **Tool & Resource Schemas:** JSON Schema Draft 2020-12.

### 4.2 Transport Layers

#### 4.2.1 Stdio Transport (`nexus-mcp/transport/stdio.rs`)

Used for spawning MCP servers as child processes.

```rust
pub struct StdioTransport {
    child: Child,
    reader: BufReader<ChildStdout>,
    writer: BufWriter<ChildStdin>,
}

impl Transport for StdioTransport {
    async fn send(&mut self, msg: ManagedMessage) -> Result<()> { ... }
    async fn recv(&mut self) -> Result<ManagedMessage> { ... }
}
```

**When Used:**
- Connecting to local MCP servers distributed as executables.
- Default for plugin-provided MCP tools (plugins can export MCP servers).

#### 4.2.2 HTTP+SSE Transport (`nexus-mcp/transport/http_sse.rs`)

Bidirectional communication using HTTP POST (requests) and Server-Sent Events (responses).

```rust
pub struct HttpSseTransport {
    client: reqwest::Client,
    base_url: String,  // http://mcp-server:8080
    session_id: String,
}

impl Transport for HttpSseTransport {
    async fn send(&mut self, msg: ManagedMessage) -> Result<()> {
        let response = self.client.post(&format!("{}/messages", self.base_url))
            .json(&msg).send().await?;
        // Handle response; SSE stream provides async messages
    }
    async fn recv(&mut self) -> Result<ManagedMessage> { ... }
}
```

**When Used:**
- Connecting to remote MCP servers exposed via HTTP.
- SaaS MCP providers (e.g., hosted Postgres, Slack, GitHub APIs).

#### 4.2.3 WebSocket Transport (`nexus-mcp/transport/websocket.rs`)

Full-duplex WebSocket for low-latency bidirectional streaming.

```rust
pub struct WebSocketTransport {
    socket: WebSocketStream<ConnectStream>,
}

impl Transport for WebSocketTransport {
    async fn send(&mut self, msg: ManagedMessage) -> Result<()> {
        self.socket.send(Message::Text(serde_json::to_string(&msg)?)).await?;
        Ok(())
    }
    async fn recv(&mut self) -> Result<ManagedMessage> { ... }
}
```

**When Used:**
- Real-time data services (Slack real-time events, live collaboration servers).
- Preferred for high-throughput integrations.

#### 4.2.4 Connection Pooling (`nexus-mcp/transport/pool.rs`)

```rust
pub struct ConnectionPool {
    connections: Arc<Mutex<HashMap<ServerId, Connection>>>,
    config: PoolConfig,
}

impl ConnectionPool {
    pub async fn get_or_create(&self, server_id: &str) -> Result<PooledConnection> { ... }
    pub async fn reconnect(&self, server_id: &str) -> Result<()> { ... }
}
```

- **Max connections per server:** Configurable (default 5).
- **Connection timeout:** 30s (configurable).
- **Idle timeout:** 5min; automatic cleanup.
- **Reconnection:** Exponential backoff (100ms → 30s).

---

## 5. MCP Host (Client) Architecture

### 5.1 Server Discovery (`nexus-mcp/host/discovery.rs`)

#### Local Registry

Servers defined in `mcp.toml` (see Section 6).

```toml
[[mcp.servers]]
id = "postgres-prod"
type = "stdio"
command = "/usr/local/bin/mcp-postgres"
args = ["--host", "prod-db.example.com", "--port", "5432"]
auto_connect = true
```

#### Remote Manifest

Optionally, fetch server definitions from a remote manifest:

```rust
pub async fn discover_remote(url: &str) -> Result<Vec<ServerDefinition>> {
    let manifest: ServerManifest = reqwest::get(url).json().await?;
    Ok(manifest.servers)
}
```

### 5.2 Client Implementation (`nexus-mcp/host/client.rs`)

```rust
pub struct McpClient {
    transport: Box<dyn Transport>,
    capabilities_remote: ServerCapabilities,  // What the remote server offers
    tool_registry: HashMap<String, ToolDefinition>,
    resource_registry: HashMap<String, ResourceDefinition>,
}

impl McpClient {
    pub async fn connect(server_def: &ServerDefinition) -> Result<Self> {
        // 1. Create transport (stdio / HTTP+SSE / WebSocket)
        let transport = create_transport(&server_def).await?;
        
        // 2. Send initialize request
        let init_response = transport.send(
            initialize_request(&ServerCapabilities {
                supports_tool_use: true,
                supports_resources: true,
                supports_sampling: true,
                ..Default::default()
            })
        ).await?;
        
        // 3. Negotiate capabilities
        let capabilities = negotiate_capabilities(&init_response)?;
        
        // 4. List available tools and resources
        let tools = list_tools(&mut transport).await?;
        let resources = list_resources(&mut transport).await?;
        
        Ok(McpClient { 
            transport, 
            capabilities_remote: capabilities,
            tool_registry: tools,
            resource_registry: resources,
        })
    }
    
    pub async fn call_tool(
        &mut self, 
        tool_name: &str, 
        arguments: JsonValue
    ) -> Result<ToolResult> {
        let request = call_tool_request(tool_name, arguments);
        let response = self.transport.send(request).await?;
        Ok(parse_tool_result(response)?)
    }
}
```

### 5.3 Tool Integration

When an external MCP server exposes tools, those tools become available to:
1. **Nexus AI Engine:** Tools registered in the completion context for agent usage.
2. **UI Tool Explorer:** Browse and test tools interactively.

```rust
// nexus-ai integration
pub fn register_mcp_tools_with_ai_engine(
    ai_engine: &mut AiEngine,
    mcp_client: &McpClient,
) {
    for (tool_name, definition) in &mcp_client.tool_registry {
        let tool = ToolDefinition {
            name: tool_name.clone(),
            description: definition.description.clone(),
            input_schema: definition.input_schema.clone(),
            // Tool executor delegates to mcp_client.call_tool()
        };
        ai_engine.register_tool(tool);
    }
}
```

### 5.4 Sampling Support (`nexus-mcp/sampling/request_handler.rs`)

When an external MCP server issues a sampling request (asking for text generation), Nexus delegates to the AI Engine:

```rust
pub async fn handle_sampling_request(
    sampling_req: SamplingRequest,
    ai_engine: &AiEngine,
) -> Result<TextResponse> {
    let chat_request = ChatRequest {
        model: sampling_req.model,
        messages: sampling_req.messages,
        system_prompt: sampling_req.system_prompt,
        max_tokens: sampling_req.max_tokens,
        ..Default::default()
    };
    
    let response = ai_engine.chat(chat_request).await?;
    Ok(TextResponse {
        content: response.text,
    })
}
```

---

## 6. MCP Server (Provider) Architecture

### 6.1 Request Dispatcher (`nexus-mcp/server/handler.rs`)

```rust
pub struct McpServer {
    transport: Box<dyn Transport>,
    capabilities_local: ServerCapabilities,
    request_handlers: HashMap<String, Box<dyn RequestHandler>>,
}

impl McpServer {
    pub async fn initialize(req: InitializeRequest) -> Result<InitializeResponse> {
        Ok(InitializeResponse {
            server_info: ServerInfo {
                name: "Nexus".to_string(),
                version: "1.0.0".to_string(),
            },
            capabilities: ServerCapabilities {
                tools: ToolCapability { list_changed: true },
                resources: ResourceCapability { 
                    subscribe: false, 
                    list_changed: true 
                },
                prompts: PromptCapability { list_changed: true },
                sampling: SamplingCapability {},
            },
            protocol_version: "1.0".to_string(),
        })
    }

    pub async fn dispatch(&mut self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        match request.method.as_str() {
            "tools/list" => self.list_tools().await,
            "tools/call" => self.call_tool(request.params).await,
            "resources/list" => self.list_resources().await,
            "resources/read" => self.read_resource(request.params).await,
            "prompts/list" => self.list_prompts().await,
            "prompts/get" => self.get_prompt(request.params).await,
            "sampling/request" => self.handle_sampling(request.params).await,
            _ => Err(McpError::MethodNotFound),
        }
    }
}
```

### 6.2 Tool Executor (`nexus-mcp/server/tool_executor.rs`)

All Nexus capabilities exposed as MCP tools with consistent schemas.

#### Tool: `forge_read`

Read file content from the forge.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "path": {
      "type": "string",
      "description": "Relative or absolute path within the forge"
    },
    "encoding": {
      "type": "string",
      "enum": ["utf8", "base64"],
      "default": "utf8"
    }
  },
  "required": ["path"]
}
```

**Output Schema:**
```json
{
  "type": "object",
  "properties": {
    "content": { "type": "string" },
    "size_bytes": { "type": "integer" },
    "charset": { "type": "string" }
  }
}
```

**Implementation:**
```rust
pub async fn tool_forge_read(input: JsonValue) -> Result<JsonValue> {
    let path = input["path"].as_str().ok_or("Missing path")?;
    let content = tokio::fs::read_to_string(path).await?;
    Ok(json!({
        "content": content,
        "size_bytes": content.len(),
        "charset": "utf-8"
    }))
}
```

#### Tool: `forge_write`

Write or append to a file.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "path": { "type": "string" },
    "content": { "type": "string" },
    "mode": { 
      "type": "string", 
      "enum": ["write", "append"], 
      "default": "write" 
    }
  },
  "required": ["path", "content"]
}
```

**Output:**
```json
{
  "type": "object",
  "properties": {
    "success": { "type": "boolean" },
    "bytes_written": { "type": "integer" }
  }
}
```

#### Tool: `forge_search`

Full-text search across notes.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "query": { "type": "string" },
    "limit": { "type": "integer", "default": 20 },
    "offset": { "type": "integer", "default": 0 }
  },
  "required": ["query"]
}
```

**Output:**
```json
{
  "type": "object",
  "properties": {
    "results": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "path": { "type": "string" },
          "title": { "type": "string" },
          "snippet": { "type": "string" },
          "score": { "type": "number" }
        }
      }
    },
    "total": { "type": "integer" }
  }
}
```

#### Tool: `forge_list`

List files and directories.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "path": { "type": "string", "default": "." },
    "recursive": { "type": "boolean", "default": false }
  }
}
```

**Output:**
```json
{
  "type": "object",
  "properties": {
    "entries": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "path": { "type": "string" },
          "type": { "type": "string", "enum": ["file", "directory"] },
          "size_bytes": { "type": "integer" }
        }
      }
    }
  }
}
```

#### Tool: `db_query`

Execute a read query against a .bases database.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "database": { "type": "string", "description": "Database ID or path" },
    "query": { "type": "string" },
    "limit": { "type": "integer", "default": 100 }
  },
  "required": ["database", "query"]
}
```

**Output:**
```json
{
  "type": "object",
  "properties": {
    "rows": {
      "type": "array",
      "items": { "type": "object" }
    },
    "column_names": { "type": "array", "items": { "type": "string" } },
    "row_count": { "type": "integer" }
  }
}
```

#### Tool: `db_create_record`

Insert a record into a database.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "database": { "type": "string" },
    "table": { "type": "string" },
    "data": { "type": "object" }
  },
  "required": ["database", "table", "data"]
}
```

**Output:**
```json
{
  "type": "object",
  "properties": {
    "id": { "type": "string" },
    "created_at": { "type": "string", "format": "date-time" }
  }
}
```

#### Tool: `terminal_exec`

Execute a command in the terminal.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "command": { "type": "string" },
    "cwd": { "type": "string" },
    "timeout_ms": { "type": "integer", "default": 30000 }
  },
  "required": ["command"]
}
```

**Output:**
```json
{
  "type": "object",
  "properties": {
    "stdout": { "type": "string" },
    "stderr": { "type": "string" },
    "exit_code": { "type": "integer" }
  }
}
```

#### Tool: `process_start`

Start a long-running process (returns process ID).

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "command": { "type": "string" },
    "cwd": { "type": "string" },
    "background": { "type": "boolean", "default": true }
  },
  "required": ["command"]
}
```

**Output:**
```json
{
  "type": "object",
  "properties": {
    "process_id": { "type": "string" },
    "started_at": { "type": "string", "format": "date-time" }
  }
}
```

#### Tool: `process_stop`

Terminate a process by ID.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "process_id": { "type": "string" },
    "force": { "type": "boolean", "default": false }
  },
  "required": ["process_id"]
}
```

**Output:**
```json
{
  "type": "object",
  "properties": {
    "success": { "type": "boolean" }
  }
}
```

#### Tool: `process_status`

Get status of a running process.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "process_id": { "type": "string" }
  },
  "required": ["process_id"]
}
```

**Output:**
```json
{
  "type": "object",
  "properties": {
    "state": { "type": "string", "enum": ["running", "stopped", "exited"] },
    "exit_code": { "type": "integer", "nullable": true },
    "cpu_percent": { "type": "number" },
    "memory_mb": { "type": "number" }
  }
}
```

#### Tool: `plugin_command`

Invoke a command registered by a plugin.

**Input Schema:**
```json
{
  "type": "object",
  "properties": {
    "plugin_id": { "type": "string" },
    "command_id": { "type": "string" },
    "arguments": { "type": "object" }
  },
  "required": ["plugin_id", "command_id"]
}
```

**Output:**
```json
{
  "type": "object",
  "properties": {
    "success": { "type": "boolean" },
    "result": {}
  }
}
```

---

## 7. Resource Definitions

### 7.1 Resource URI Scheme (`nexus-mcp/resources/resolver.rs`)

```
mcp://nexus/{resource_type}/{path}
```

- **Notes:** `mcp://nexus/notes/path/to/note.md`
- **Databases:** `mcp://nexus/db/database_id`
- **Records:** `mcp://nexus/db/database_id/table_name/{record_id}`
- **Config:** `mcp://nexus/config/forge`
- **Search Index:** `mcp://nexus/index/search`

### 7.2 Resource: Notes

List and retrieve note content.

**Listing URI:** `mcp://nexus/notes` (with wildcard: `mcp://nexus/notes/**`)

**Properties:**
```json
{
  "uri": "mcp://nexus/notes/path/to/note.md",
  "name": "note.md",
  "description": "Markdown note file",
  "mime_type": "text/markdown",
  "size_bytes": 1024
}
```

**Retrieval:**
```json
{
  "uri": "mcp://nexus/notes/path/to/note.md",
  "contents": [
    {
      "mime_type": "text/markdown",
      "data": "# My Note\n\nContent here..."
    }
  ]
}
```

### 7.3 Resource: Databases

List and retrieve database metadata.

**Listing:** `mcp://nexus/db` and `mcp://nexus/db/{database_id}`

**Properties:**
```json
{
  "uri": "mcp://nexus/db/my_database",
  "name": "my_database",
  "tables": [
    {
      "name": "users",
      "record_count": 150,
      "columns": [
        { "name": "id", "type": "text" },
        { "name": "email", "type": "text" }
      ]
    }
  ]
}
```

---

## 8. Authentication & Authorization

### 8.1 MCP Client Authentication (`nexus-mcp/auth/authenticator.rs`)

External MCP clients authenticate to Nexus's MCP server via:

#### API Key

```toml
# mcp.toml under [server.auth]
auth_method = "api_key"
api_keys = ["nexus-sk-abc123xyz"]  # Hashed in storage
```

External clients include key in request header:
```
Authorization: Bearer nexus-sk-abc123xyz
```

#### OAuth 2.0 (Future)

Delegate to external OAuth provider (e.g., Okta, Auth0).

### 8.2 MCP Server Authentication (`nexus-mcp/auth/keychain.rs`)

When Nexus connects to external MCP servers, credentials are stored securely:

```toml
[[mcp.servers]]
id = "postgres-prod"
type = "stdio"
command = "/usr/local/bin/mcp-postgres"
auth = "keychain"  # Reference to OS keychain
credential_key = "mcp/postgres-prod"  # Entry in keychain
```

**Keychain Integration:**
- **macOS:** Keychain Services
- **Linux:** `pass` or `secret-tool`
- **Windows:** Windows Credential Manager

```rust
pub async fn fetch_credentials(key: &str) -> Result<String> {
    #[cfg(target_os = "macos")]
    {
        // Use Keychain Services
    }
    #[cfg(target_os = "linux")]
    {
        // Use secret-tool
    }
}
```

### 8.3 Capability Gating (`nexus-mcp/auth/capability_gate.rs`)

Restrict which tools/resources external clients can access:

```toml
[[mcp.auth.policies]]
client_id = "claude-code"
allowed_tools = ["forge_read", "forge_search", "terminal_exec"]
denied_tools = ["forge_write", "db_create_record"]
resource_patterns = ["mcp://nexus/notes/**"]
```

**Gate Implementation:**
```rust
pub fn check_capability(
    client_id: &str,
    action: &str,  // e.g., "tools/call:forge_write"
) -> Result<()> {
    let policy = load_policy(client_id)?;
    if policy.is_allowed(action) {
        Ok(())
    } else {
        Err(McpError::PermissionDenied)
    }
}
```

---

## 9. Configuration Model

### 9.1 mcp.toml Format (`nexus-mcp/config/schema.rs`)

```toml
[mcp]
version = "1.0"
# Global MCP settings

[mcp.server]
# Nexus as MCP server config
enabled = true
transport = "stdio"  # or "http_sse" or "websocket"
port = 8765  # For HTTP+SSE/WebSocket
# TLS config (optional for HTTP+SSE/WebSocket)
tls_enabled = false
tls_cert = "/etc/nexus/server.crt"
tls_key = "/etc/nexus/server.key"

[mcp.server.auth]
enabled = true
auth_method = "api_key"  # or "oauth2"
api_keys = ["hashed:xxxx"]  # Stored hashed
require_authentication = true

[mcp.server.capabilities]
# What the Nexus server exposes
tools = true
resources = true
prompts = true
sampling = true

[mcp.server.tooling]
# Tool-level restrictions
allowed_tools = []  # Empty = all
denied_tools = []
max_tool_timeout_ms = 30000

# Host: External MCP servers
[[mcp.servers]]
id = "postgres-prod"
name = "Production PostgreSQL"
description = "Connect to prod database via MCP"
type = "stdio"  # stdio | http_sse | websocket
command = "/usr/local/bin/mcp-postgres"
args = ["--host", "prod-db.example.com"]
cwd = "/home/nexus"
auto_connect = true
env = { PG_PASSWORD_KEYCHAIN = "mcp/postgres-prod" }

[mcp.servers.transport]
timeout_ms = 30000
reconnect_backoff_ms = [100, 500, 2000, 10000, 30000]  # Exponential
max_retries = 10

[mcp.servers.auth]
method = "keychain"
credential_key = "mcp/postgres-prod"

[mcp.servers.capabilities]
# Restrict what tools this server can access from Nexus
allowed_tools = []  # Empty = all
denied_tools = ["forge_write"]

[[mcp.servers]]
id = "slack"
name = "Slack Workspace"
type = "http_sse"
url = "https://mcp.slack.example.com"
auto_connect = false

[[mcp.servers]]
id = "github"
name = "GitHub API"
type = "websocket"
url = "wss://mcp.github.com"
auth = "oauth"
oauth_provider = "github"
```

### 9.2 Config Loader (`nexus-mcp/config/loader.rs`)

```rust
pub struct MpcConfig {
    pub version: String,
    pub server: ServerConfig,
    pub servers: Vec<ServerDefinition>,
}

pub async fn load_config(path: &Path) -> Result<MpcConfig> {
    let content = tokio::fs::read_to_string(path).await?;
    let config: MpcConfig = toml::from_str(&content)?;
    validate_config(&config)?;
    Ok(config)
}

pub fn validate_config(config: &MpcConfig) -> Result<()> {
    // Validate server definitions, auth methods, transport configs, etc.
}
```

### 9.3 Hot Reload (`nexus-mcp/config/watcher.rs`)

```rust
pub async fn watch_config(path: PathBuf) -> Result<broadcast::Receiver<ConfigReload>> {
    let (tx, rx) = broadcast::channel(10);
    
    let mut watcher = notify::watcher(move |event: notify::Result<_>| {
        if let Ok(notify::Event { kind: notify::event::EventKind::Modify(_), .. }) = event {
            if let Ok(new_config) = load_config(&path) {
                let _ = tx.send(ConfigReload { config: new_config });
            }
        }
    })?;
    
    watcher.watch(&path, notify::RecursiveMode::NonRecursive)?;
    Ok(rx)
}
```

---

## 10. Dynamic Tool Registration

### 10.1 Plugin Command → MCP Tool Flow

When a plugin registers a command via the Plugin System:

```rust
// In plugin code:
pub async fn register_command(registry: &mut CommandRegistry) {
    registry.register(Command {
        id: "my_plugin:analyze",
        label: "Analyze Code",
        capability: "cli.subcommand",
        handler: analyze_handler,
    }).await;
}
```

The MCP Server automatically exposes this as a tool:

```rust
// In nexus-mcp/server/tool_executor.rs
pub async fn on_plugin_command_registered(cmd: &Command) {
    let tool_def = ToolDefinition {
        name: format!("{}:{}", cmd.plugin_id, cmd.id),
        description: cmd.label.clone(),
        input_schema: infer_schema_from_handler(&cmd.handler),
    };
    
    // Notify connected MCP clients (via list_changed event)
    broadcast_tool_list_changed();
}
```

**Latency:** < 100ms from registration to availability to external clients.

---

## 11. Error Handling & Resilience

### 11.1 Connection Failures (`nexus-mcp/transport/reconnect.rs`)

```rust
pub async fn call_with_reconnect<F, T>(
    mcp_client: &mut McpClient,
    f: F,
) -> Result<T>
where
    F: Fn(&mut McpClient) -> BoxFuture<'static, Result<T>>,
{
    match f(mcp_client).await {
        Ok(result) => Ok(result),
        Err(e) if e.is_transient() => {
            // Exponential backoff: 100ms, 500ms, 2s, 10s, 30s
            for backoff in [100, 500, 2000, 10000, 30000] {
                tokio::time::sleep(Duration::from_millis(backoff)).await;
                mcp_client.reconnect().await.ok();
                if let Ok(result) = f(mcp_client).await {
                    return Ok(result);
                }
            }
            Err(e)
        }
        Err(e) => Err(e),
    }
}
```

### 11.2 Tool Execution Timeout

```rust
pub async fn execute_tool_with_timeout(
    tool_name: &str,
    input: JsonValue,
    timeout_ms: u64,
) -> Result<JsonValue> {
    tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        execute_tool_impl(tool_name, input)
    )
    .await
    .map_err(|_| McpError::Timeout)?
}
```

### 11.3 Graceful Degradation

When an external MCP server is unavailable:
- Tools from that server are removed from the AI Engine's context.
- UI tool explorer shows "offline" status.
- Requests to that server fail fast with clear error message.

---

## 12. Security Model

### 12.1 Access Control Matrix

| Capability | Local Users | External Clients (API Key) | Notes |
|------------|-------------|---------------------------|-------|
| `forge_read` | ✅ | ✅ (configurable) | Default allowed; can restrict to certain paths |
| `forge_write` | ✅ | ❌ | Disabled by default for external clients |
| `forge_search` | ✅ | ✅ | Safe read-only operation |
| `db_query` | ✅ | ✅ (configurable) | Per-database access control |
| `db_create_record` | ✅ | ❌ | Disabled by default for external clients |
| `terminal_exec` | ✅ | ❌ | Dangerous; local users only |
| `process_*` | ✅ | ❌| Dangerous; local users only |
| `plugin_command` | ✅ | ✅ (per-plugin) | Plugin's own capability controls |

### 12.2 Audit Logging (`nexus-mcp/logging/audit.rs`)

Every tool call and resource access is logged:

```json
{
  "timestamp": "2026-04-11T14:30:22Z",
  "client_id": "claude-code",
  "action": "tools/call",
  "tool": "forge_read",
  "input": { "path": "src/main.rs" },
  "result": "success",
  "duration_ms": 12,
  "user": "local"
}
```

Logs stored in:
- **File:** `~/.nexus/logs/mcp-audit.jsonl`
- **Rotation:** Daily, 30-day retention.

### 12.3 Rate Limiting

```rust
pub struct RateLimiter {
    limits: HashMap<ClientId, RateLimit>,
}

pub struct RateLimit {
    requests_per_minute: u32,
    requests_per_hour: u32,
}

pub async fn check_rate_limit(client_id: &ClientId) -> Result<()> {
    let limiter = get_limiter();
    if limiter.is_over_limit(client_id) {
        Err(McpError::RateLimited)
    } else {
        Ok(())
    }
}
```

Default: 100 requests/minute per client.

---

## 13. UX Components

### 13.1 MCP Server Browser

Location: `Settings → Integrations → MCP Servers`

**Features:**
- List connected servers with status (✅ Connected, ⚠️ Offline, ❌ Error)
- Add new server: form-based or paste TOML snippet
- Test connection: "Ping" button sends test message
- View server capabilities: expandable list of tools and resources
- Edit auth credentials: keychain integration for secure updates
- Auto-connect toggle per server

### 13.2 Tool Explorer

Location: `Tools → MCP Tools` (global tools panel)

**Features:**
- Search/filter tools by name or server
- Tool detail panel: description, input schema, example calls
- "Test Tool" button: form to provide arguments and invoke
- View recent results: last 10 calls with input/output/latency
- Copy tool schema as JSON

### 13.3 Server Configuration Panel

Location: `Settings → Integrations → Add MCP Server`

**Form Fields:**
- Server ID (auto-slug from name)
- Display name
- Transport type (dropdown: stdio, HTTP+SSE, WebSocket)
- Conditional fields based on transport:
  - **Stdio:** Command, arguments, working directory
  - **HTTP+SSE / WebSocket:** URL, port, TLS enabled
- Authentication method (dropdown: API key, OAuth, keychain)
- Auto-connect toggle
- Advanced: timeout, reconnection backoff, capability restrictions

---

## 14. Headless MCP Server

### 14.1 CLI Command: `nexus mcp serve`

Start Nexus as an MCP server without GUI:

```bash
nexus mcp serve \
  --config /path/to/mcp.toml \
  --port 8765 \
  --bind 127.0.0.1 \
  --log-level debug
```

**Output:**
```
2026-04-11T14:30:00Z INFO nexus-mcp: MCP server listening on http://127.0.0.1:8765
2026-04-11T14:30:00Z INFO nexus-mcp: Connected to 3 external MCP servers
2026-04-11T14:30:00Z INFO nexus-mcp: Exposing 14 tools, 8 resources
```

**Systemd Unit (example):**

```ini
[Unit]
Description=Nexus MCP Server
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/nexus mcp serve --config /etc/nexus/mcp.toml
Restart=always
RestartSec=10
User=nexus
Group=nexus

[Install]
WantedBy=multi-user.target
```

---

## 15. Testing Strategy

### 15.1 Mock MCP Servers

For testing without external dependencies:

```rust
// tests/mock_servers.rs
pub struct MockMcpServer {
    tools: HashMap<String, ToolDefinition>,
}

impl MockMcpServer {
    pub fn new() -> Self { ... }
    
    pub fn add_tool(mut self, tool: ToolDefinition) -> Self { ... }
    
    pub async fn serve_on(self, port: u16) { ... }
}

#[tokio::test]
async fn test_mcp_host_connects_and_calls_tool() {
    let mock_server = MockMcpServer::new()
        .add_tool(ToolDefinition {
            name: "echo".to_string(),
            description: "Echo tool".to_string(),
            input_schema: json!({}),
        })
        .serve_on(9999).await;
    
    let mut client = McpClient::connect(&ServerDefinition {
        id: "mock".into(),
        url: "http://127.0.0.1:9999".into(),
        ..Default::default()
    }).await.unwrap();
    
    let result = client.call_tool("echo", json!({})).await.unwrap();
    assert_eq!(result["message"], "Hello from echo");
}
```

### 15.2 Protocol Compliance

Validate MCP protocol implementation against spec:

```rust
#[tokio::test]
async fn test_protocol_version_negotiation() {
    // Test server accepts and negotiates protocol version
}

#[tokio::test]
async fn test_json_rpc_message_format() {
    // Verify messages conform to JSON-RPC 2.0
}

#[tokio::test]
async fn test_tool_schema_validation() {
    // Ensure all tool schemas are valid JSON Schema
}
```

### 15.3 Integration Tests

End-to-end flows:

```rust
#[tokio::test]
async fn test_plugin_command_becomes_mcp_tool() {
    // 1. Register plugin command
    // 2. Verify it appears in tool list
    // 3. Call via MCP
    // 4. Verify result
}

#[tokio::test]
async fn test_external_client_calls_forge_read() {
    // 1. Start Nexus MCP server
    // 2. Connect external client
    // 3. Call forge_read
    // 4. Verify file content returned
}

#[tokio::test]
async fn test_mcp_host_connection_failure_and_recovery() {
    // 1. Connect to external server
    // 2. Simulate network failure
    // 3. Verify exponential backoff retry
    // 4. Verify reconnection
}
```

---

## 16. Performance Targets

| Metric | Target | Notes |
|--------|--------|-------|
| MCP tool call latency (p50) | < 50ms | For local forge ops; longer for remote servers |
| MCP tool call latency (p99) | < 500ms | Includes network + processing |
| External server connection setup | < 5s | Stdio spawn + protocol handshake |
| Tool list enumeration | < 200ms | Cached after initial handshake |
| Concurrent MCP connections | ≥10 | Per server; pool size configurable |
| Reconnection time (after failure) | 1-30s | Exponential backoff |
| Audit log size (per month) | < 100MB | With daily rotation and 30-day retention |

---

## 17. Acceptance Criteria

- [ ] MCP protocol v1.0 fully implemented with all three transports (stdio, HTTP+SSE, WebSocket).
- [ ] Nexus successfully connects to and consumes tools from ≥5 reference external MCP servers.
- [ ] ≥12 core Nexus tools exposed as MCP tools with complete JSON Schema definitions.
- [ ] Plugin commands automatically registered as MCP tools within 100ms of registration.
- [ ] External MCP clients (Claude Code, Cursor) successfully authenticate, enumerate tools, and invoke forge_read and forge_search.
- [ ] Sampling requests from external servers handled via AI Engine integration.
- [ ] mcp.toml configuration schema complete and validated; hot-reload functional.
- [ ] Authentication: API key auth implemented; keychain integration for credentials.
- [ ] Error resilience: Connection failures handled with exponential backoff; graceful degradation when servers unavailable.
- [ ] Audit logging: All tool calls and resource access logged to `~/.nexus/logs/mcp-audit.jsonl`.
- [ ] UI: MCP Server Browser, Tool Explorer, and Server Configuration panels fully functional.
- [ ] Headless server: `nexus mcp serve` CLI command operational.
- [ ] All unit, integration, and compliance tests passing.
- [ ] Documentation: API docs, configuration guide, and integration examples complete.

---

## 18. Success Metrics (Post-Launch)

- **Adoption:** ≥50% of Nexus users connect ≥1 external MCP server within first month.
- **Tool Coverage:** ≥20 third-party MCP servers tested and publicly documented as compatible.
- **Stability:** < 0.1% of MCP tool calls result in errors (post-reconnection); < 1% timeout.
- **Developer Experience:** Average time to set up new MCP server integration ≤ 5 minutes.
- **Community Plugins:** ≥10 community-built MCP server plugins published within 3 months.

---

## 19. Dependencies & Constraints

### External Crates
- `mcp` (official Anthropic Rust implementation, if available; else custom JSON-RPC)
- `tokio` (async runtime)
- `serde` / `serde_json` (JSON serialization)
- `reqwest` (HTTP client)
- `tokio-tungstenite` (WebSocket)
- `notify` (file system watcher)
- `keyring` (OS keychain integration)
- `tracing` / `tracing-subscriber` (structured logging)

### Nexus Internal Dependencies
- `nexus-ipc` (for command dispatch)
- `nexus-ai` (for sampling, AI provider access)
- `nexus-plugin` (for command registry hooks)
- `nexus-terminal` (for terminal_exec, process management)
- `nexus-database` (for db_query, db_create_record)
- `nexus-storage` (for forge_read, forge_write, forge_search)

### Constraints
- MCP server connections limited to 10 concurrent connections per server.
- Tool execution timeout: 30s (configurable per server).
- Audit log retention: 30 days; daily rotation.
- Memory per MCP client: ~50MB (connection buffers + tool registry cache).

---

## 20. Deployment & Operations

### 20.1 Configuration Distribution

MCP server definitions can be shared as TOML snippets or .mcp files:

```bash
# Copy server definition to clipboard
nexus config export-server postgres-prod | pbcopy

# Import from clipboard or file
nexus config import-server < postgres-prod.mcp
```

### 20.2 Monitoring & Observability

**Prometheus Metrics:**
- `mcp_tool_calls_total` (counter by tool, server, result)
- `mcp_tool_call_duration_seconds` (histogram)
- `mcp_server_connections_active` (gauge per server)
- `mcp_server_reconnections_total` (counter)

**Health Checks:**
```bash
# Check MCP server status
nexus mcp status

# Output:
# postgres-prod: ✅ Connected (5s ago)
# slack: ⚠️ Offline (reconnecting...)
# github: ✅ Connected (2m ago)
```

---

## Appendix A: Reference Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                         Nexus Monolith                               │
├──────────┬──────────────┬────────────┬──────────────┬────────────────┤
│ AI Engine│ Plugin System│ Terminal   │ Database     │ Storage Engine │
│          │              │ Manager    │ Engine       │ (forge)        │
└──────────┴──────────────┴────────────┴──────────────┴────────────────┘
                           ↑                  ↑
                           │                  │
                  ┌────────┴──────────────────┴────────┐
                  │    nexus-mcp Subsystem              │
                  │                                    │
     ┌────────────┴────────────────────────────────────┴────────────┐
     │                                                               │
  ┌──┴──┐  ┌─────────────────────────────────────────────────────┐  │
  │ MCP │  │ MCP Server (Provider)                               │  │
  │Host │  │                                                     │  │
  │     │  │  ┌──────────────┐  ┌──────────────┐               │  │
  │     │  │  │Tool Executor │  │Resource      │               │  │
  │     │  │  │forge_read    │  │Provider      │               │  │
  │     │  │  │db_query      │  │notes/db/cfg  │               │  │
  │     │  │  │terminal_exec │  │              │               │  │
  │     │  │  └──────────────┘  └──────────────┘               │  │
  │     │  │                                                     │  │
  │     │  │  ┌──────────────────────────────────────────────┐ │  │
  │     │  │  │ [stdio] [HTTP+SSE] [WebSocket]              │ │  │
  │     │  │  │ Transport abstraction + connection pooling  │ │  │
  │     │  │  └──────────────────────────────────────────────┘ │  │
  └──────┘  └─────────────────────────────────────────────────────┘
     │                           ↑
     │ External MCP Servers      │ External MCP Clients
     │ (Postgres, Slack,         │ (Claude Code, Cursor,
     │  GitHub, Linear, etc.)    │  Standalone agents)
     │
  ┌──┴────────────────────────────────────────────────────────────┐
  │                                                                │
  ├─ Tool Registry (enumerate external tools)                    │
  ├─ Resource Retrieval (fetch external resources)               │
  ├─ Sampling Handler (forward LLM requests)                     │
  └────────────────────────────────────────────────────────────────┘
```

---

## Appendix B: mcp.toml Complete Example

```toml
[mcp]
version = "1.0"

[mcp.server]
enabled = true
transport = "stdio"
tls_enabled = false

[mcp.server.auth]
enabled = true
auth_method = "api_key"
api_keys = ["hashed:$2b$12$..."]
require_authentication = true

[mcp.server.capabilities]
tools = true
resources = true
sampling = true

[mcp.server.rate_limit]
requests_per_minute = 100
requests_per_hour = 5000

# Production PostgreSQL
[[mcp.servers]]
id = "postgres-prod"
name = "Production PostgreSQL"
type = "stdio"
command = "/usr/local/bin/mcp-postgres"
args = ["--tls=true"]
auto_connect = true

[mcp.servers[0].transport]
timeout_ms = 30000
reconnect_backoff_ms = [100, 500, 2000, 10000, 30000]

[mcp.servers[0].auth]
method = "keychain"
credential_key = "mcp/postgres-prod"

[mcp.servers[0].capabilities]
denied_tools = ["forge_write"]

# Slack Workspace
[[mcp.servers]]
id = "slack-workspace"
name = "Slack Workspace"
type = "http_sse"
url = "https://mcp-slack.example.com"
auto_connect = false

[mcp.servers[1].auth]
method = "oauth"
oauth_provider = "slack"

# GitHub API (WebSocket)
[[mcp.servers]]
id = "github"
name = "GitHub API"
type = "websocket"
url = "wss://mcp.github.example.com"
auto_connect = true

[mcp.servers[2].auth]
method = "keychain"
credential_key = "mcp/github-token"
```

---

**Document Version:** 1.0  
**Last Updated:** April 2026  
**Next Review:** May 2026
