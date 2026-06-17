# The native memory engine (`com.nexus.memory`)

`nexus-memory` is Nexus's first-party persistent memory store — an AI-first, model-agnostic memory layer at **full parity with the `remind_me` MCP server**, but native to the kernel (reachable over `ipc_call` from CLI, TUI, MCP, and the desktop shell rather than as an external MCP process). Promoted from a staging library to a wired service plugin under #188.

It is registered by `nexus-bootstrap` right after `storage` (it owns its own store and has no inter-plugin dependencies) and owns:

```
<forge>/.forge/memory/memory.db    # SQLite — memories, FTS5 index, vectors, SPO facts, entities
```

Unlike the forge's markdown (file-as-truth), the memory store is **authoritative state** in SQLite — it is a derived/operational store, not rebuildable from files. (The LLM-`wiki` surface is the exception: it writes file-as-truth markdown pages.)

## Data model

A `Memory` row carries: `content`, `category`, `memory_type` (episodic / semantic / procedural), `source`, `tags`, optional SPO triple (`subject` / `predicate` / `object`), ACT-R `vitality` fields (access count + timestamps), `embedding` (for vector recall), `capture_id` / `source_capture_id` (capture lineage), and `status` + `superseded_by` (lifecycle / consolidation).

## IPC surface — 21 handlers

Source of truth: `IPC_HANDLERS` in `crates/nexus-memory/src/core_plugin.rs`. Handlers 13–19 are **async** (`dispatch_async`): recall/vector_sync/wiki_*/auto_capture make nested `ipc_call`s to `com.nexus.ai` (embeddings / generation) and `com.nexus.storage` (wiki files); `sync` makes outbound HTTP. The rest are synchronous.

| Group | Handlers |
|-------|----------|
| **CRUD + list** | `add`, `get`, `list`, `update`, `delete` |
| **Search / recall** | `search` (FTS5), `recall` (hybrid FTS + vector, fused with Reciprocal Rank Fusion), `vector_sync` (back-fill embeddings) |
| **Knowledge graph** | `facts` (SPO triples), `entities` (entity index from subjects/objects) |
| **Tags / vitality / stats** | `tags`, `vitality_report` (ACT-R decay ranking), `stats` (aggregate counts by category/type/source) |
| **Capture pipeline** | `auto_capture` (store a turn; optional LLM decompose into atomic facts), `get_capture` (a capture's lineage), `consolidate` (dedupe exact normalized duplicates → supersede) |
| **Knowledge synthesis** | `wiki_compile` (LLM-synthesize a wiki page from related memories), `wiki_read`, `wiki_list` (file-as-truth markdown under the forge) |
| **Portability / sharing** | `export` (full records), `sync` (push/pull against a `nexus-memory-hub`) |

Capability classification for every handler is in [`reference/audit-flags.md`](reference/audit-flags.md) / `crates/nexus-bootstrap/cap_matrix.toml`; the recall/capture/wiki handlers reach the AI provider through the plugin's *own* gated context (so a plain caller needs no `ai.chat`).

## Cross-instance sync — `nexus-memory-hub`

`sync` replicates memories against a **standalone HTTP hub** (`crates/nexus-memory-hub`, a deployable binary — not a bootstrap plugin). Multiple Nexus instances pointed at one hub converge their stores (last-writer-wins on `updated_at`), so memory follows the user across machines.

## Passive bus capture

Beyond explicit `add` / `auto_capture`, the plugin subscribes to the kernel event bus and turns salient events into episodic memories (`crate::capture`), so the store accrues context without every caller remembering to write to it.

## Surfaces

- **CLI** — `nexus` memory subcommands, including **import** (read a `remind_me` SQLite DB or chat-log export and replay it through `add`; import is a CLI-side adapter, not an IPC handler).
- **TUI** — memory browse/search panes.
- **MCP** — `nexus_memory_*` tools (search/recall/add/facts/entities/stats/export/capture/consolidate/…) in `nexus-mcp`.
- **Shell** — the *Memory Dashboard* plugin (`shell/src/plugins/nexus/memoryDashboard/`): Search, Recall, Recent, Facts, Entities, Tags, Vitality, Stats, Sync, Capture, Consolidate, Wiki — from the command palette.

## Relationship to other crates

- `nexus-context` (staging) is designed to consume `nexus-memory` for budget-bounded context assembly (awaiting its `nexus-ai-runtime` consumer, #188).
- Embeddings + generation are not in `nexus-memory`; it calls `com.nexus.ai` over IPC, keeping the memory engine model-agnostic.
