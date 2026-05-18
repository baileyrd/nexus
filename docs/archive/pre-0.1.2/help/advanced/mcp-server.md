# MCP server

Nexus exposes your forge over the
[Model Context Protocol](https://modelcontextprotocol.io) so that any
MCP-aware client — Claude Code, Cursor, Continue, Zed AI, custom
agents — can read, search, and edit your notes as a set of tools.

## Start the server

```bash
nexus mcp                # stdio transport, ready for an MCP client
```

The server runs in the foreground and speaks MCP over stdin/stdout.
It does not open a network port.

## Wire it to Claude Code

Add to `~/.claude.json` (or your platform's equivalent):

```json
{
  "mcpServers": {
    "nexus": {
      "command": "nexus",
      "args": ["mcp"],
      "env": { "NEXUS_FORGE_PATH": "/home/you/notes" }
    }
  }
}
```

Restart Claude Code; the `nexus_*` tools become available to the
model.

## Wire it to Cursor

Cursor's settings → MCP → add server:

- Command: `nexus`
- Args: `["mcp"]`
- Env: `NEXUS_FORGE_PATH=/home/you/notes`

## Tools exposed

| Tool | What it does |
|---|---|
| `nexus_read_note` | Read the full text of a note |
| `nexus_create_note` | Create a new note |
| `nexus_update_note` | Replace the contents of a note |
| `nexus_delete_note` | Delete a note |
| `nexus_list_notes` | List notes (with optional path prefix filter) |
| `nexus_search` | Full-text search |
| `nexus_backlinks` | All notes linking to a given note |
| `nexus_outgoing_links` | All wikilinks in a note |
| `nexus_graph_status` | Node/edge counts and unresolved-link totals |
| `nexus_list_tags` | All tags in the forge with counts |
| `nexus_list_tasks` | All `- [ ]` and `- [x]` tasks across the forge |
| `nexus_toggle_task` | Toggle a task by file + line |
| `nexus_ask` | RAG-augmented Q&A (calls `com.nexus.ai`) |
| `nexus_list_skills` | List skill templates |
| `nexus_render_skill` | Render a skill with parameters |

Full schemas: [`docs/users/mcp.md`](../../users/mcp.md).

## Resources

Each note is also exposed as an MCP **Resource**:

```
mcp://nexus/notes/path/to/note.md
```

Clients that support resources can subscribe to changes (the resource
list updates when the file watcher fires).

## Scope and isolation

The MCP server only sees the forge it's started against. Set
`NEXUS_FORGE_PATH` in the server's environment to scope it. To expose
multiple forges, run multiple `nexus mcp` instances with different
paths.

## Security model

Tools call into the same kernel IPC as everything else. Capability
checks apply — the MCP frontend declares the capability set it grants
the connected client. By default that's the read-and-write set above;
you can restrict it via `.forge/mcp.toml`:

```toml
[server.tools]
deny = ["nexus_delete_note", "nexus_update_note"]
```

Useful when you want a model to read but not modify your forge.

## Debugging

Run with `RUST_LOG=debug nexus mcp 2>/tmp/nexus-mcp.log` and tail the
log to see every JSON-RPC frame. The MCP framing is verbose but
diffable when you need to figure out what a client is asking for.
