# MCP Server Design

**Date:** 2026-04-13
**Status:** Approved
**Scope:** stdio MCP server exposing Nexus forge operations as tools
**Source:** PRD 14 (server-only slice), Growth Plan Phase 4

---

## Overview

New `nexus-mcp` crate implementing a Model Context Protocol server using the official `rmcp` SDK. Exposes 13 tools covering note CRUD, search, graph queries, tags, tasks, and RAG. Runs over stdio transport for use with Claude Code, Cursor, and other MCP-compatible AI clients.

Host-side MCP (connecting to external servers), HTTP transport, auth, sampling, and UI are deferred to a future pass.

---

## 1. Crate Structure

```
crates/nexus-mcp/
├── Cargo.toml      (depends on rmcp, nexus-storage, nexus-ai, tokio, serde_json)
├── src/
│   ├── lib.rs      — public API: start_server function
│   ├── server.rs   — MCP server setup, tool registration
│   └── tools.rs    — Tool handler implementations
```

Added to workspace members.

New workspace dependency: `rmcp = { version = "1.3", features = ["server", "transport-io"] }`

## 2. Tools

All tools receive JSON input and return JSON output. Each tool delegates to `StorageEngine` or `nexus-ai` methods.

| Tool | Input | Output | Backing Method |
|---|---|---|---|
| `nexus_read_note` | `{ path }` | `{ content, size_bytes, content_hash }` | `read_file` |
| `nexus_create_note` | `{ path, content, tags? }` | `{ path, size_bytes }` | `write_file` |
| `nexus_update_note` | `{ path, content }` | `{ path, size_bytes }` | `write_file` |
| `nexus_delete_note` | `{ path }` | `{ deleted: true }` | `delete_file` |
| `nexus_list_notes` | `{ prefix? }` | `{ files: [{path, size_bytes}] }` | `list_files` |
| `nexus_search` | `{ query, limit? }` | `{ results: [{file_path, score, block_type}] }` | `search` |
| `nexus_backlinks` | `{ path }` | `{ backlinks: [{source_path, link_text, link_type}] }` | `backlinks` |
| `nexus_outgoing_links` | `{ path }` | `{ links: [{target_path, link_text, is_resolved}] }` | `outgoing_links` |
| `nexus_graph_status` | `{}` | `{ nodes, edges, unresolved }` | `graph_stats` |
| `nexus_list_tags` | `{ name }` | `{ tags: [{name, file_path, source}] }` | `query_tags` |
| `nexus_list_tasks` | `{ completed?, file? }` | `{ tasks: [{id, status, content, file_path}] }` | `query_tasks` |
| `nexus_toggle_task` | `{ task_id }` | `{ id, completed, content, file_path }` | `toggle_task` |
| `nexus_ask` | `{ question }` | `{ answer, sources: [{file_path, score}] }` | `rag_query` |

## 3. Server Setup

The MCP server struct holds an `Arc<StorageEngine>` and implements rmcp's server trait. Each tool is registered with its JSON Schema for input validation.

For `nexus_ask`, the server also needs AI providers. It detects them from env vars (same as CLI) and holds them as `Option<Arc<dyn AiProvider>>` and `Option<Arc<dyn EmbeddingProvider>>`. If no AI provider is configured, the `nexus_ask` tool returns an error message rather than failing server startup.

## 4. CLI Integration

Replace the existing `mcp` stub with:

```
nexus mcp    — start MCP server (stdio mode)
```

No subcommands. The command initializes `StorageEngine`, constructs the MCP server, and runs the stdio transport loop. It blocks until stdin closes (client disconnects).

## 5. Testing

- Unit tests: verify tool input/output JSON serialization
- Integration test: programmatically create server, send JSON-RPC initialize + tool call, verify response
- Manual test: configure in Claude Code's MCP settings, verify tools appear

## 6. Files

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Add nexus-mcp to members, add rmcp |
| `crates/nexus-mcp/Cargo.toml` | **NEW** |
| `crates/nexus-mcp/src/lib.rs` | **NEW** |
| `crates/nexus-mcp/src/server.rs` | **NEW** |
| `crates/nexus-mcp/src/tools.rs` | **NEW** |
| `crates/nexus-cli/Cargo.toml` | Add nexus-mcp dep |
| `crates/nexus-cli/src/main.rs` | Replace Mcp stub with real command |
| `crates/nexus-cli/src/commands/mcp.rs` | **NEW** |
| `crates/nexus-cli/src/commands/mod.rs` | Register mcp module |

## Out of Scope

- MCP Host (connecting to external servers)
- HTTP/SSE transport
- WebSocket transport
- Authentication / API keys for clients
- Sampling (server requesting LLM from host)
- Resource/prompt primitives (tools only)
- MCP server browser UI
