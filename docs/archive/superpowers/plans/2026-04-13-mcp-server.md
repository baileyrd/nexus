# MCP Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build an MCP server exposing 13 Nexus forge tools over stdio transport, usable by Claude Code, Cursor, and other MCP clients.

**Architecture:** New `nexus-mcp` crate using the official `rmcp` SDK. A `NexusMcpServer` struct holds an `Arc<StorageEngine>` and uses `#[tool_router]`/`#[tool]` macros to define tools. The CLI command `nexus mcp` starts the server on stdio.

**Tech Stack:** rmcp 1.3 (official Rust MCP SDK), schemars (JSON Schema for tool inputs), tokio (async runtime)

---

## File Structure

| File | Role |
|------|------|
| `Cargo.toml` (workspace) | Add rmcp + schemars, nexus-mcp to members |
| `crates/nexus-mcp/Cargo.toml` | **NEW** — crate manifest |
| `crates/nexus-mcp/src/lib.rs` | **NEW** — public start_server function |
| `crates/nexus-mcp/src/server.rs` | **NEW** — NexusMcpServer struct + all tool implementations |
| `crates/nexus-cli/Cargo.toml` | Add nexus-mcp dep |
| `crates/nexus-cli/src/main.rs` | Replace Mcp stub |
| `crates/nexus-cli/src/commands/mcp.rs` | **NEW** — MCP command handler |
| `crates/nexus-cli/src/commands/mod.rs` | Register mcp module |

---

### Task 1: Crate Scaffolding

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `crates/nexus-mcp/Cargo.toml`
- Create: `crates/nexus-mcp/src/lib.rs`

- [ ] **Step 1: Add workspace deps and members**

In workspace root `Cargo.toml`, add to `[workspace.dependencies]`:

```toml
# MCP server
rmcp = { version = "1.3", features = ["server", "transport-io"] }
schemars = "0.8"
```

Add `"crates/nexus-mcp"` to the `members` list.

- [ ] **Step 2: Create nexus-mcp Cargo.toml**

Create `crates/nexus-mcp/Cargo.toml`:

```toml
[package]
name = "nexus-mcp"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
description = "Nexus MCP server: expose forge operations to AI clients"

[dependencies]
nexus-storage = { path = "../nexus-storage" }
nexus-ai = { path = "../nexus-ai" }
rmcp = { workspace = true }
schemars = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
```

- [ ] **Step 3: Create lib.rs stub**

Create `crates/nexus-mcp/src/lib.rs`:

```rust
//! Nexus MCP server: exposes forge operations as MCP tools.

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod server;

pub use server::NexusMcpServer;
```

- [ ] **Step 4: Create server.rs stub**

Create `crates/nexus-mcp/src/server.rs`:

```rust
//! MCP server implementation using rmcp.

use std::sync::Arc;
use nexus_storage::StorageEngine;

/// MCP server exposing Nexus forge operations as tools.
pub struct NexusMcpServer {
    storage: Arc<StorageEngine>,
}

impl NexusMcpServer {
    /// Create a new MCP server backed by the given storage engine.
    pub fn new(storage: Arc<StorageEngine>) -> Self {
        Self { storage }
    }
}
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p nexus-mcp`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/nexus-mcp/
git commit -m "chore(mcp): scaffold nexus-mcp crate with rmcp dependency"
```

---

### Task 2: Tool Implementations

**Files:**
- Modify: `crates/nexus-mcp/src/server.rs`

- [ ] **Step 1: Implement all 13 tools**

Replace `crates/nexus-mcp/src/server.rs` with the full implementation. The server struct uses rmcp's `#[tool_router]` and `#[tool]` attribute macros to register tool handlers.

Each tool handler:
1. Receives a `Parameters<InputType>` with the deserialized JSON input
2. Calls the appropriate `StorageEngine` method
3. Returns `Result<String, String>` where the String is JSON-serialized output (rmcp renders this as tool result text)

Key implementation details:

**Struct and imports:**
```rust
use std::sync::Arc;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use serde::{Deserialize, Serialize};

use nexus_storage::StorageEngine;
```

**Input/output types** — each tool gets a pair of input/output structs deriving `Deserialize, schemars::JsonSchema` (input) and `Serialize` (output). Examples:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
pub struct ReadNoteInput {
    /// Path to the note (e.g. "notes/hello.md")
    pub path: String,
}

#[derive(Serialize)]
struct ReadNoteOutput {
    content: String,
    size_bytes: usize,
}
```

**Tool implementations** using the `#[tool_router]` impl block:

```rust
#[tool_router]
impl NexusMcpServer {
    #[tool(name = "nexus_read_note", description = "Read a note's content and metadata")]
    fn read_note(&self, Parameters(input): Parameters<ReadNoteInput>) -> Result<String, String> {
        let bytes = self.storage.read_file(&input.path).map_err(|e| e.to_string())?;
        let content = String::from_utf8_lossy(&bytes).to_string();
        let output = ReadNoteOutput { content, size_bytes: bytes.len() };
        serde_json::to_string_pretty(&output).map_err(|e| e.to_string())
    }
    // ... remaining tools follow same pattern
}
```

**Complete tool list with input types:**

1. `nexus_read_note` — input: `{ path }`, calls `storage.read_file`
2. `nexus_create_note` — input: `{ path, content }`, calls `storage.write_file`
3. `nexus_update_note` — input: `{ path, content }`, calls `storage.write_file` (same as create — upsert semantics)
4. `nexus_delete_note` — input: `{ path }`, calls `storage.delete_file`
5. `nexus_list_notes` — input: `{ prefix? }`, calls `storage.list_files`
6. `nexus_search` — input: `{ query, limit? }`, calls `storage.search` (rebuild search index first)
7. `nexus_backlinks` — input: `{ path }`, calls `storage.backlinks`
8. `nexus_outgoing_links` — input: `{ path }`, calls `storage.outgoing_links`
9. `nexus_graph_status` — input: `{}` (empty), calls `storage.graph_stats`
10. `nexus_list_tags` — input: `{ name }`, calls `storage.query_tags`
11. `nexus_list_tasks` — input: `{ completed?, file? }`, calls `storage.query_tasks`
12. `nexus_toggle_task` — input: `{ task_id }`, calls `storage.toggle_task`
13. `nexus_ask` — input: `{ question }`, needs AI providers; if none configured, return error string. Otherwise, get pool connection, block_on `rag_query`. Use `tokio::task::block_in_place` for the async call since we're inside a tokio runtime.

**ServerHandler trait** — rmcp requires implementing `ServerHandler` to provide server info. The `#[tool_router]` macro generates the tool listing, but you need to wire it:

```rust
#[rmcp::async_trait]
impl rmcp::ServerHandler for NexusMcpServer {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo {
            name: "nexus".into(),
            version: "0.1.0".into(),
            ..Default::default()
        }
    }
}
```

The `#[tool_router]` macro should auto-generate the `list_tools` and `call_tool` methods on the `ServerHandler` impl. If it doesn't (depends on rmcp version), manually delegate:

```rust
async fn list_tools(&self, _request: rmcp::model::PaginatedRequest) -> Result<rmcp::model::ListToolsResult, rmcp::ServiceError> {
    Ok(self.tool_router.list_tools())
}

async fn call_tool(&self, request: rmcp::model::CallToolRequest) -> Result<rmcp::model::CallToolResult, rmcp::ServiceError> {
    self.tool_router.call_tool(self, request).await
}
```

**Important notes:**
- The `schemars::JsonSchema` derive is needed on all input types for rmcp to generate JSON schemas for tool discovery.
- Optional fields use `Option<T>` with `#[serde(default)]`.
- `search` tool should call `storage.rebuild_search_index()` before searching (matching CLI behavior).
- All tool handlers return `Result<String, String>` — rmcp converts Ok into tool result content and Err into error content.

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p nexus-mcp`
Expected: PASS (may need adjustments to match rmcp's exact API)

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-mcp/src/server.rs
git commit -m "feat(mcp): implement 13 MCP tools for note CRUD, search, graph, tasks, and RAG"
```

---

### Task 3: CLI Wiring + Start Server

**Files:**
- Modify: `crates/nexus-cli/Cargo.toml`
- Create: `crates/nexus-cli/src/commands/mcp.rs`
- Modify: `crates/nexus-cli/src/commands/mod.rs`
- Modify: `crates/nexus-cli/src/main.rs`
- Modify: `crates/nexus-mcp/src/lib.rs`

- [ ] **Step 1: Add start_server function to lib.rs**

Update `crates/nexus-mcp/src/lib.rs`:

```rust
//! Nexus MCP server: exposes forge operations as MCP tools.

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod server;

pub use server::NexusMcpServer;

use std::sync::Arc;
use nexus_storage::StorageEngine;

/// Start the MCP server on stdio transport.
///
/// This function blocks until the client disconnects (stdin closes).
///
/// # Errors
///
/// Returns an error if the server fails to start or encounters a transport error.
pub async fn start_stdio_server(storage: Arc<StorageEngine>) -> Result<(), Box<dyn std::error::Error>> {
    let server = NexusMcpServer::new(storage);
    let transport = rmcp::transport::io::stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}
```

- [ ] **Step 2: Add nexus-mcp dependency to CLI**

In `crates/nexus-cli/Cargo.toml`, add:

```toml
nexus-mcp = { path = "../nexus-mcp" }
```

(`tokio` should already be a dependency from the AI engine task)

- [ ] **Step 3: Create MCP command handler**

Create `crates/nexus-cli/src/commands/mcp.rs`:

```rust
use std::sync::Arc;
use anyhow::Result;
use crate::app::App;

/// Start the MCP server on stdio.
pub fn serve(app: &mut App) -> Result<()> {
    let storage = app.storage_mut()?;

    // We need to move storage into an Arc for the server.
    // Since App owns StorageEngine, we need to get a reference that outlives this call.
    // The simplest approach: create a new tokio runtime and run the server.
    let rt = tokio::runtime::Runtime::new()?;
    
    // Get the forge root and re-open storage in an Arc
    let forge_root = storage.forge().root().to_path_buf();
    drop(storage); // Release the App borrow
    
    let storage = Arc::new(
        nexus_storage::StorageEngine::open(&forge_root, &nexus_storage::StorageConfig::default())
            .map_err(|e| anyhow::anyhow!("failed to open storage: {e}"))?
    );
    
    rt.block_on(nexus_mcp::start_stdio_server(storage))
        .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;

    Ok(())
}
```

- [ ] **Step 4: Register MCP module and wire CLI**

In `crates/nexus-cli/src/commands/mod.rs`, add:
```rust
pub mod mcp;
```

In `crates/nexus-cli/src/main.rs`, replace the `Mcp(StubArgs)` variant in the `Commands` enum with:
```rust
/// Start MCP server (stdio mode)
Mcp,
```

Replace the `Commands::Mcp(_) => stubs::not_implemented("mcp")` dispatch with:
```rust
Commands::Mcp => commands::mcp::serve(&mut app),
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/nexus-mcp/src/lib.rs crates/nexus-cli/
git commit -m "feat(cli): add nexus mcp command to start MCP server on stdio"
```

---

### Task 4: Verification

- [ ] **Step 1: Run all workspace tests**

Run: `cargo test --workspace`
Expected: All PASS (except known flaky credential vault test)

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace`
Expected: No new warnings

- [ ] **Step 3: Verify CLI help**

Run: `cargo run -p nexus-cli -- --help`
Expected: Shows `mcp` as a subcommand (no longer marked "coming soon")

- [ ] **Step 4: Verify MCP server starts**

Run: `echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}' | cargo run -p nexus-cli -- --forge-path /tmp/mcp-test mcp 2>/dev/null`

Expected: JSON response with server info and capabilities (may need to init the forge first)

- [ ] **Step 5: Fix any issues and commit**

If any issues found, fix and commit.
