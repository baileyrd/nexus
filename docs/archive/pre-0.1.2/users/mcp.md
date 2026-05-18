# MCP Server

Nexus ships an [MCP (Model Context Protocol)](https://modelcontextprotocol.io)
server that exposes 15 tools over stdio. Point any MCP-speaking client
(Claude Code, Claude Desktop, Cursor) at it to give that client read /
write / search / RAG access to your forge.

## Starting the server

```sh
nexus mcp
```

The process serves the protocol on stdio and runs until the client
disconnects. There's no daemon mode — the client launches `nexus mcp`
as a subprocess.

## Tools

Authoritative source: `crates/nexus-mcp/src/server.rs`. The 15 tools:

| Tool | Description |
|---|---|
| `nexus_read_note` | Read a note's content by vault-relative path |
| `nexus_create_note` | Create a new note with the given path and markdown content |
| `nexus_update_note` | Update an existing note's content (creates if it does not exist) |
| `nexus_delete_note` | Delete a note by vault-relative path |
| `nexus_list_notes` | List notes in the forge, optionally filtered by a path prefix |
| `nexus_search` | Full-text search across notes (rebuilds the search index before querying) |
| `nexus_backlinks` | Find all notes that link to the specified note |
| `nexus_outgoing_links` | Find all outgoing links from the specified note |
| `nexus_graph_status` | Knowledge graph statistics: node count, edge count, unresolved links |
| `nexus_list_tags` | List all occurrences of a tag by name across the forge |
| `nexus_list_tasks` | List tasks (checkboxes) across notes with optional filters |
| `nexus_toggle_task` | Toggle a task's completed/incomplete state by its database ID |
| `nexus_ask` | Ask a question via RAG over your notes |
| `nexus_list_skills` | List skills declared in the forge's `.forge/skills` directory |
| `nexus_render_skill` | Render a skill template to its expanded prompt body |

## Client setup

### Claude Code (`~/.config/claude/mcp.json` or per-project)

```json
{
  "mcpServers": {
    "nexus": {
      "command": "nexus",
      "args": ["mcp"],
      "env": {
        "NEXUS_FORGE_PATH": "/home/you/notes"
      }
    }
  }
}
```

### Claude Desktop (`~/Library/Application Support/Claude/claude_desktop_config.json`)

```json
{
  "mcpServers": {
    "nexus": {
      "command": "nexus",
      "args": ["mcp"],
      "env": {
        "NEXUS_FORGE_PATH": "/Users/you/notes"
      }
    }
  }
}
```

### Cursor

Configure the same `command` / `args` / `env` in Cursor's MCP server settings.

## Tool routing internals

MCP tools route *to* IPC handlers in the kernel. `nexus_read_note` calls
`com.nexus.storage::read_file`; `nexus_ask` calls `com.nexus.ai::stream_ask`;
etc. The MCP tool surface is its own contract for external AI clients —
it is **not** the same surface as the IPC handlers tracked under
[`../architecture/ipc-schemas.md`](../architecture/ipc-schemas.md).

## Capability and trust

The MCP server runs with the same forge-root permissions as the user
running it. It does not add a separate authentication layer — anyone
with shell access to the machine can launch `nexus mcp` against your
forge. For multi-user setups, run separate forges per user.

## See also

- [`cli.md`](cli.md) — full CLI command reference.
- [`../adr/0014-ribbon-vs-activity-bar-api-alignment.md`](../adr/0014-ribbon-vs-activity-bar-api-alignment.md) — API naming notes.
- `crates/nexus-mcp/src/server.rs` — authoritative tool source.
