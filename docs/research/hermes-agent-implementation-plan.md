# Hermes Agent — Native Rust Implementation Plan (Option B)

> **Source:** [NousResearch/hermes-agent](https://github.com/nousresearch/hermes-agent) (MIT) · Assessed 2026-05-12  
> **Approach:** Port Hermes's highest-value capabilities natively into the Nexus Rust workspace, following the microkernel IPC pattern. No Python runtime dependency.

---

## Background

Hermes Agent is an open-source Python orchestration layer that wraps any LLM with a persistent agent loop, tool execution engine, memory system, skills library, context compression, multi-agent delegation, and scheduling. Its core differentiators over Nexus's current agent infrastructure are:

| Hermes Feature | Nexus Status |
|---|---|
| Multi-round agent loop | ✅ Shipped (`session.rs`, `MAX_AGENT_ROUNDS=8`) |
| Tool registry + function calling | ✅ Shipped (`tools/registry.rs`) |
| Anthropic / OpenAI / Ollama providers | ✅ Shipped (`nexus-ai` provider abstraction) |
| MCP client (stdio / HTTP / SSE) | ✅ Shipped (`nexus-mcp`, `mcp_bridge.rs`) |
| RAG / semantic search | ✅ Shipped (`rag.rs`, Tantivy + vector store) |
| Agent archetypes (writer/coder/researcher) | ✅ Shipped (`archetypes.rs`) |
| Interactive approval gate | ⚠️ Phase 2b — not yet shipped |
| **Memory persistence** | ❌ Missing |
| **Skills system** | ❌ Missing |
| **Context compression** | ❌ Missing |
| **Multi-agent delegation** | ❌ Missing |
| **Session search (FTS)** | ❌ Missing |
| **Configurable iteration budget (up to 90)** | ❌ Hardcoded at 8 |
| **ACP protocol adapter** | ❌ Missing |

---

## Cross-Cutting Architectural Notes

Five facts shape every decision in this plan:

1. **`MAX_AGENT_ROUNDS` is duplicated.** Defined independently as `MAX_TOOL_ROUNDS` in `nexus-ai/src/core_plugin.rs` and `MAX_AGENT_ROUNDS` in `nexus-agent/src/session.rs`. Both must be made configurable separately.

2. **`run_session` needs a `SessionConfig` struct.** Current positional `goal`/`system`/`archetype` args cannot cleanly carry new fields (iteration budget, context limit, delegation depth). `SessionConfig` is introduced in Feature 1 as the foundation all others depend on.

3. **`write_vault_file` is the correct write path** for `memory.md` and skill files — keeps them out of FTS, the knowledge graph, and wikilink resolution.

4. **`nexus-bootstrap/src/lib.rs` touches every feature** — all new IPC handler registrations land there.

5. **Schema migrations are append-only.** `CURRENT_VERSION = 7` in `nexus-storage/src/schema.rs`. The new FTS table for session search lands in migration 8.

---

## Merge Order and Dependency Graph

```
Feature 1 (IterationBudget + SessionConfig)       ← merge first
    ├── Feature 2 (Memory)                         ┐ parallel
    ├── Feature 3 (Skills)                         ┘
    ├── Feature 4 (Context Compression)            ┐ parallel
    ├── Feature 5 (Session Search)                 ┘
    └── Feature 6 (Multi-Agent Delegation)         ← after Feature 1; largest
Feature 7 (ACP Adapter)                           ← last; wraps everything
```

---

## Feature 1 — Raise Iteration Limit + `IterationBudget`

**Complexity: S** | **Blocks: all other features**

### What changes

**`crates/nexus-agent/src/session.rs`**

Replace the single `MAX_AGENT_ROUNDS` constant with:

```rust
pub const DEFAULT_MAX_AGENT_ROUNDS: u32 = 8;
pub const HARD_MAX_AGENT_ROUNDS: u32 = 90;

pub struct IterationBudget {
    pub remaining: u32,
    pub depth: u32,
    pub max_depth: u32,
}

pub struct SessionConfig {
    pub goal: String,
    pub system: String,
    pub archetype: Option<String>,
    pub max_rounds: u32,                    // clamped to HARD_MAX_AGENT_ROUNDS
    pub budget: Option<IterationBudget>,    // None for top-level sessions
    pub context_limit_tokens: Option<u32>,  // populated by Feature 4
}
```

Change `run_session` / `run_session_with_id` to accept `SessionConfig`. Loop bound becomes `config.max_rounds.min(HARD_MAX_AGENT_ROUNDS)`.

Re-export from `crates/nexus-agent/src/lib.rs`:
```rust
pub use session::{DEFAULT_MAX_AGENT_ROUNDS, HARD_MAX_AGENT_ROUNDS, IterationBudget, SessionConfig};
```

**`crates/nexus-ai/src/ipc.rs`**

Add `max_tool_rounds: Option<usize>` to `AiProposeArgs`. Defaults to `MAX_TOOL_ROUNDS=8` when absent, clamped to 90 in `run_tool_dispatch_loop`.

**`crates/nexus-agent/src/core_plugin.rs`**

Add `max_rounds: Option<u32>` to `SessionRunArgs`. Handler clamps it:
```rust
parsed.max_rounds.unwrap_or(DEFAULT_MAX_AGENT_ROUNDS).clamp(1, HARD_MAX_AGENT_ROUNDS)
```
Update the two `run_session` / `run_session_with_id` call-sites to pass a `SessionConfig`.

### PR boundary
`feat(agent): configurable iteration budget up to 90 rounds`

---

## Feature 2 — Memory Persistence

**Complexity: M** | **Depends on: Feature 1**

Memory file lives at `.forge/.forge/memory.md` (vault directory — invisible to user search, FTS, and wikilinks).

### New IPC handlers on `com.nexus.agent`

| Handler ID | Name | Args | Returns |
|---|---|---|---|
| 18 | `memory_get` | `{}` | `{ content: String }` |
| 19 | `memory_set` | `{ content: String }` | `{}` |

Both read/write via `write_vault_file` / `ctx.read_file`. Returns `content: ""` if the file doesn't exist yet.

### New tool executors — `crates/nexus-ai/src/tools/functions.rs`

- **`MemoryReadTool`** — dispatches to `com.nexus.agent::memory_get`
- **`MemoryUpdateTool`** — schema: `{ content: String, mode: "overwrite"|"append" }`. Append mode calls `memory_get` first, then `memory_set` with the combined content.

Register both in `wire_context` alongside the existing `register_storage_builtins` call. `memory_read` is read-only (`READ_ONLY_TOOL_NAMES`); `memory_update` is write-class.

### System prompt injection — `crates/nexus-agent/src/core_plugin.rs`

In `handle_session_run`, call `load_memory_block(&ctx)` before constructing the final `system` string. When the memory file exists, prepend:

```xml
<memory>
{memory_content}
</memory>
```

**Capability note:** `memory_set` writes vault files, already covered by `FsWrite` in `agent_capabilities()`. No new capability grant needed.

### Files to modify
- `crates/nexus-agent/src/core_plugin.rs`
- `crates/nexus-ai/src/tools/functions.rs`
- `crates/nexus-bootstrap/src/lib.rs`

### PR boundary
`feat(agent): memory persistence via .forge/memory.md with memory_read/memory_update tools`

---

## Feature 3 — Skills System (RAG Retrieval at Session Start)

**Complexity: M** | **Depends on: Feature 1, Feature 2 (system prompt injection pattern)**

`nexus-skills` already has `triggered_by`, `compose`, `render`, and full CRUD. What is missing: semantic retrieval at session start, and agent-initiated CRUD tools.

### New IPC handlers on `com.nexus.skills`

| Handler ID | Name | Args | Returns |
|---|---|---|---|
| 9 | `semantic_skill_match` | `{ query: String, limit?: u32 }` | `Vec<{ id, name, score }>` |
| 10 | `create` | `{ id: String, content: String }` | `{}` |
| 11 | `delete` | `{ id: String }` | `{}` |

**`semantic_skill_match`** calls `com.nexus.ai::semantic_search` with the goal text and post-filters results by `.forge/skills/` path prefix, mapping file paths back to skill IDs. This avoids relaxing the vault-file exclusion in the indexing daemon.

**`create`** validates YAML frontmatter (`name`, `description`), writes to `.forge/.forge/skills/<id>.skill.md` via `write_vault_file`, calls `reload`.

**`delete`** deletes the vault file and reloads the in-memory registry.

### New tool executor — `crates/nexus-ai/src/tools/functions.rs`

**`SkillManageTool`** with schema:
```json
{
  "properties": {
    "action":  { "type": "string", "enum": ["create", "delete", "get"] },
    "id":      { "type": "string" },
    "content": { "type": "string" }
  },
  "required": ["action", "id"]
}
```
Routes `create` → handler 10, `delete` → handler 11, `get` → existing handler 2.

### System prompt injection — `crates/nexus-agent/src/core_plugin.rs`

In `handle_session_run`, call both `triggered_by` (keyword) and `semantic_skill_match` (embedding), deduplicate, then inject:

```xml
<available_skills>
{skill_body}
</available_skills>
```

### Architecture decision
Skills are not indexed in the shared vector store (that would require relaxing vault-file exclusion). `semantic_skill_match` filters `semantic_search` results by path prefix. This under-retrieves skills with low surface vocabulary overlap with the goal. A future ADR can revisit dedicated skill embeddings.

### Files to modify
- `crates/nexus-skills/src/core_plugin.rs`
- `crates/nexus-ai/src/tools/functions.rs`
- `crates/nexus-agent/src/core_plugin.rs`
- `crates/nexus-bootstrap/src/lib.rs`

### PR boundary
`feat(skills): semantic skill retrieval + agent skill_manage tool + skill CRUD IPC`

---

## Feature 4 — Context Compression

**Complexity: L** | **Depends on: Feature 1**

Triggered when estimated token count of conversation history exceeds ~70% of the provider's context window. Summarizes the middle of the history using a cheap one-shot LLM call; preserves the first 2 and last 10 turns unconditionally.

### New type — `crates/nexus-agent/src/session.rs`

```rust
pub enum SessionTurn {
    User(String),
    Assistant { text: String, tool_calls: Vec<ProposedToolCall> },
    ToolResult { id: String, content: String, is_error: bool },
}
```

### Extended `ChatDriver` trait

```rust
async fn propose_with_history(
    &self, system: &str, turns: &[SessionTurn]
) -> Result<Proposal, String>;

async fn compress(
    &self, system: &str, turns: &[SessionTurn]
) -> Result<String, String>;
```

Default `propose_with_history` concatenates history into a single `user_message` string (backward-compatible). `AiChatBridge` in `nexus-bootstrap/src/agent.rs` overrides it to pass full turns to `propose_tool_calls` via the existing `messages: Vec<AiStreamAskMessage>` field in `AiProposeArgs`.

### Compression trigger in the session loop

Token estimation is inlined in `session.rs` as `(len + 3) / 4` (no new dependency on `nexus-ai`). When `estimated > context_limit * 0.70`:

1. Call `driver.compress(system, &turns)` — issues a `mode=complete` (no tools, one shot) prompt:
   ```
   Summarize this agent session history as:
   ## Active Task
   ## Completed Actions
   ## Pending Questions
   ## Remaining Work
   Keep it under 600 tokens.
   ```
2. Replace middle turns with a single synthetic `SessionTurn::User(summary)`.

`context_limit_tokens` in `SessionConfig` is populated from `com.nexus.ai::config` when building `SessionConfig` in `handle_session_run`.

### Files to modify
- `crates/nexus-agent/src/session.rs`
- `crates/nexus-bootstrap/src/agent.rs`

### PR boundary
`feat(agent): context compression with configurable token threshold`

---

## Feature 5 — Session Search (FTS5 over Transcripts)

**Complexity: M** | **Depends on: Feature 1 (shares session_run path)**

### Schema migration 8 — `crates/nexus-storage/src/schema.rs`

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS agent_session_fts USING fts5(
    session_id  UNINDEXED,
    goal,
    outcome     UNINDEXED,
    transcript,
    started_at  UNINDEXED
);
```

Bump `CURRENT_VERSION` to 8.

### New IPC handlers on `com.nexus.storage`

| Handler ID | Name | Args | Returns |
|---|---|---|---|
| 50 | `session_fts_insert` | `{ session_id, goal, outcome, transcript, started_at }` | `{}` |
| 51 | `session_fts_search` | `{ query: String, limit?: u32 }` | `Vec<{ session_id, goal, started_at, rank }>` |
| 52 | `session_fts_delete` | `{ session_id: String }` | `{}` |

### Integration in `crates/nexus-agent/src/core_plugin.rs`

After persisting a session JSON to disk in `handle_session_run`, call `session_fts_insert` with a synopsis (not full JSON):
```
goal + " " + rounds.map(|r| r.text + " " + tool_call_names).join(" ")
```
Keeps FTS rows under ~2 KB.

After deleting a session file in `handle_session_delete`, call `session_fts_delete`.

### New handler on `com.nexus.agent`

| Handler ID | Name | Args | Returns |
|---|---|---|---|
| 20 | `session_search` | `{ query: String, limit?: u32 }` | `Vec<{ id, goal, started_at, score }>` |

Calls `com.nexus.storage::session_fts_search` and returns the results.

### Files to modify
- `crates/nexus-storage/src/schema.rs`
- `crates/nexus-storage/src/core_plugin.rs`
- `crates/nexus-agent/src/core_plugin.rs`
- `crates/nexus-bootstrap/src/lib.rs`

### PR boundary
`feat(storage,agent): FTS5 over agent session transcripts via session_search IPC`

---

## Feature 6 — Multi-Agent Delegation

**Complexity: XL** | **Depends on: Feature 1**

### Architecture

The `delegate` tool executor dispatches `com.nexus.agent::session_run` from within the parent session's tool dispatch loop. The parent blocks on the child completing (600s timeout). A depth guard in `handle_session_run` prevents infinite recursion.

**Deadlock safety:** `SharedPluginLoader::call_async` releases its read-lock before `.await`-ing the handler future, so the child's `propose_tool_calls` calls can proceed while the parent handler is suspended. Verify this in `nexus-kernel/src/plugin_loader.rs` before merging.

### `DelegateTool` — `crates/nexus-ai/src/tools/functions.rs`

```rust
pub struct DelegateTool { ctx: Arc<KernelPluginContext> }
```

Schema:
```json
{
  "properties": {
    "goal":        { "type": "string" },
    "archetype":   { "type": "string", "enum": ["writer", "coder", "researcher"] },
    "tool_subset": { "type": "array", "items": { "type": "string" } },
    "max_rounds":  { "type": "integer", "minimum": 1, "maximum": 20 }
  },
  "required": ["goal"]
}
```

The executor builds `SessionRunArgs` with:
- `auto_approve: true`
- `_delegation_depth: parent_depth + 1` (private field, parsed by `handle_session_run` for depth enforcement)
- `max_rounds: min(model_input, budget.remaining)`
- 600s IPC timeout (overrides the default 60s tool timeout)

Returns a summary string to the parent model:
```
Delegated session {id} completed: {outcome}
Summary: {last_round_text}
Tool calls made: {tool_names}
```

### Depth enforcement — `crates/nexus-agent/src/core_plugin.rs`

Add to `SessionRunArgs`:
```rust
#[serde(default)]
_delegation_depth: Option<u32>,
```

At the top of `handle_session_run`, reject calls where `_delegation_depth >= max_depth` (default 2) before building the driver.

### Budget propagation

Parent passes `budget.remaining` to child as `max_rounds`. After child returns, parent deducts the child's actual round count from its own `IterationBudget.remaining`.

### Integration test — `crates/nexus-bootstrap/tests/`

Add an integration test that runs two-level delegation (parent delegates to child, child completes, parent summarizes) using the existing `ScriptedDriver` + `CountingDispatcher` test helpers.

### Files to modify
- `crates/nexus-ai/src/tools/functions.rs`
- `crates/nexus-ai/src/core_plugin.rs`
- `crates/nexus-agent/src/core_plugin.rs`
- `crates/nexus-bootstrap/tests/` (new integration test)

### PR boundary
`feat(ai,agent): multi-agent delegation via delegate tool with depth-limited IterationBudget`

---

## Feature 7 — ACP Adapter (`nexus-acp` crate)

**Complexity: M** | **Depends on: all other features**

A new thin crate that exposes Nexus's agent surface via JSON-RPC 2.0 over line-delimited stdio, making it accessible to Hermes-compatible external clients (Hermes's `hermes-acp` protocol).

### New crate: `crates/nexus-acp/`

**`Cargo.toml` dependencies:**
```toml
[dependencies]
nexus-bootstrap = { path = "../nexus-bootstrap" }
nexus-kernel     = { path = "../nexus-kernel" }
tokio            = { workspace = true, features = ["io-std", "macros", "rt-multi-thread"] }
serde            = { workspace = true }
serde_json       = { workspace = true }
thiserror        = { workspace = true }
```

No direct dependency on `nexus-agent` or `nexus-ai` — all agent calls route through `runtime.context.ipc_call(...)`.

### Wire format

JSON-RPC 2.0 over line-delimited stdio (one object per line):
```
stdin  → { "jsonrpc": "2.0", "id": ..., "method": "agent/run",  "params": { ... } }
stdout ← { "jsonrpc": "2.0", "id": ..., "result": { ... } }
         { "jsonrpc": "2.0", "id": ..., "error":  { "code": ..., "message": ... } }
```

### Methods exposed

| Method | Maps to |
|---|---|
| `agent/run` | `com.nexus.agent::session_run` |
| `agent/list` | `com.nexus.agent::session_list` |
| `agent/get` | `com.nexus.agent::session_get` |

### Public API

```rust
pub struct AcpServer { runtime: Runtime }

impl AcpServer {
    pub fn new(forge_root: PathBuf) -> Result<Self>;
    pub async fn serve_stdio(&self) -> Result<()>;
}
```

**Streaming note:** A future `StreamingAcpServer` can subscribe to `com.nexus.agent.round_proposed` bus events and forward them as JSON-RPC notifications.

### Workspace registration

Add `"crates/nexus-acp"` to `[workspace.members]` in the root `Cargo.toml`.

### PR boundary
`feat: nexus-acp — JSON-RPC 2.0 stdio adapter for Hermes-compatible clients`

---

## ADRs to Record

| Decision | Rationale | Trade-off |
|---|---|---|
| `SessionConfig` struct replaces positional `run_session` args | Avoids proliferating function overloads as new fields are added across features | Breaking API change; all callers in `nexus-bootstrap/src/agent.rs` must be updated in the same PR as Feature 1 |
| Skill embedding via `semantic_search` path-filter, not dedicated skill vector store | Avoids relaxing the vault-file exclusion invariant in the indexing daemon | Under-retrieves semantically relevant skills with low surface vocabulary overlap with the goal; revisit in a future ADR |
| `memory.md` stored as a vault file | Keeps agent metadata out of user-visible search, FTS, and knowledge graph | User cannot wiki-link to their own memory file from notes — correct trade-off |
| Delegation deadlock safety relies on `call_async` lock discipline | Existing `SharedPluginLoader` design releases the read-lock before `.await`-ing handlers | Must be verified in `nexus-kernel/src/plugin_loader.rs` before merging Feature 6; if the lock is held across await points, `DelegateTool` must use `tokio::spawn` instead |

---

## Critical Files Reference

| File | Features |
|---|---|
| `crates/nexus-agent/src/session.rs` | 1, 4, 6 — `SessionConfig`, `IterationBudget`, `ChatDriver::propose_with_history`, compression logic, main loop |
| `crates/nexus-agent/src/core_plugin.rs` | 2, 5, 6 — `memory_get/set`, `session_search`, delegation depth enforcement, memory/skill system prompt injection |
| `crates/nexus-ai/src/tools/functions.rs` | 2, 3, 6 — `MemoryReadTool`, `MemoryUpdateTool`, `SkillManageTool`, `DelegateTool` |
| `crates/nexus-skills/src/core_plugin.rs` | 3 — `semantic_skill_match`, `create`, `delete` handlers |
| `crates/nexus-storage/src/schema.rs` | 5 — migration 8, `agent_session_fts` FTS5 table |
| `crates/nexus-bootstrap/src/lib.rs` | 1–7 — registration of all new handler IDs across every plugin |
| `crates/nexus-bootstrap/src/agent.rs` | 4, 6 — `AiChatBridge::propose_with_history`, `compress` implementations |
| `crates/nexus-acp/` | 7 — new crate |
