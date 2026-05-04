# AI Interaction Surface Audit

**Date:** 2026-05-04
**Scope:** What can AI actually interact with in the Nexus application today?
**Method:** Code-level audit of `crates/nexus-ai/`, `crates/nexus-agent/`, `crates/nexus-mcp/`, frontends (`crates/nexus-cli/`, `crates/nexus-tui/`, `shell/src/plugins/`), with file:line citations. Docs (e.g. `IMPLEMENTATION_STATUS.md`) were treated as claims to verify, not authority.

---

## TL;DR

In chat, the model can **read and write markdown files in the forge** — that is the entire AI tool-use surface. Everything else listed as "AI 🟢" in status docs is provider plumbing (Anthropic / OpenAI / Ollama), RAG, streaming, and session persistence — not additional tool reach. Terminal, database, git, knowledge-graph, and MCP are **not** exposed as model-callable tools. To extend AI's reach, register more tools in `crates/nexus-ai/src/tools/functions.rs` that proxy IPC calls to the appropriate service plugins.

---

## 1. AI Engine Surface (`crates/nexus-ai/`)

The AI plugin registers as `com.nexus.ai` with **19 IPC handlers** (`crates/nexus-ai/src/core_plugin.rs:53–137`):

| # | Command | Sync/Async | Purpose |
|---|---|---|---|
| 1 | `ask` | async | RAG: embed question → vector search → chat with results |
| 2 | `index_file` | async | Chunk & embed one file, upsert to vectorstore |
| 3 | `vectorstore_count` | async | Count indexed chunks (proxy to storage) |
| 4 | `status` | async | Provider + indexed-chunk summary |
| 5 | `config` | sync | Detected provider snapshot (no I/O) |
| 6 | `stream_chat` | async | Direct chat with per-token bus events; `mode=chat` (tool dispatch) or `complete` (single round-trip) |
| 7 | `stream_ask` | async | RAG retrieve + streaming chat with source citations |
| 8–11 | `session_load/save/list/delete` | async | Multi-session JSON storage under `.forge/chat/sessions/` |
| 12 | `set_config` | sync | Hot-swap provider credentials at runtime |
| 13 | `semantic_search` | async | Direct vector search bypassing chat (stubbed) |
| 14 | `index_status` | sync | Background indexing daemon counters (BL-041) |
| 15–16 | `enrich_file` / `apply` | async | Frontmatter enrichment proposal + upsert (BL-045) |
| 17 | `index_trigger` | sync | Wake background daemon (BL-041) |
| 18–19 | `activity_list` / `clear` | sync | Chat timeline recording (BL-037) |

### Providers (all wired & functional)

- **Anthropic (Claude)** — `crates/nexus-ai/src/anthropic.rs:71–180`. Default model `claude-sonnet-4-20250514` (line 13). Tool-calling fully functional via `tool_use` / `tool_result` blocks (lines 131–180, parsing 359–380).
- **OpenAI** — `crates/nexus-ai/src/openai.rs:111+`. `chat()` + `chat_turn_with_tools()` implemented; `function_calls[]` adapter at line 177+.
- **Ollama** — `crates/nexus-ai/src/ollama.rs:122+`. Chat + NDJSON streaming (lines 184–259) + tool-calling (line 260+); synthesizes tool-call IDs since Ollama doesn't provide them.

### Embeddings

- Remote (Anthropic / OpenAI / Ollama): implemented.
- Local: `crates/nexus-ai/src/local_embedding.rs` is scaffolded but inert (deferred per `IMPLEMENTATION_STATUS.md:135`).

---

## 2. What AI Can Call OUT To

### A. Built-in tools registered to the model — exactly two

`register_storage_builtins()` (`crates/nexus-ai/src/tools/functions.rs:219–230`) registers:

1. **`read_file`** (schema lines 41–61)
   - Input: `{"path": "forge-relative-path"}`
   - Dispatches to `com.nexus.storage::read_file` (line 139)
   - Returns UTF-8 content or "not found" error

2. **`write_file`** (schema lines 63–90)
   - Input: `{"path": "...", "content": "..."}`
   - Dispatches to `com.nexus.storage::write_file` (line 199)
   - Returns `"Wrote N bytes to <path>"`

Both run through `KernelPluginContext::ipc_call` with a 30 s timeout. **Functional and tested** (`core_plugin.rs:1854–2350` contains 15+ unit tests exercising the dispatch loop).

The comments at `tools/functions.rs:14–18` list `terminal_exec`, `database_query`, `search_knowledge_graph` as deferred — **no implementations exist**.

### B. Tool-aware dispatch loop

`run_tool_dispatch_loop()` (`core_plugin.rs:1265–1350`):

- Up to **8 rounds** per `stream_chat` (`MAX_TOOL_ROUNDS = 8`, line 47).
- Each round: provider call → check for tool calls → execute via registry → feed results back as `ToolResult` turns.
- Provider integration:
  - Anthropic — `tool_use` blocks mapped via `tool_use_id` (`anthropic.rs:359–380`).
  - OpenAI — `function_calls[]` adapter (`openai.rs:177+`).
  - Ollama — synthesizes missing IDs (`ollama.rs:260+`).
- Hitting max rounds returns an error string; no partial-answer loss (lines 1345–1347).

End-to-end example: chat → `stream_chat` mode `chat` (lines 64–66) → provider gets `read_file`+`write_file` schemas (line 1273) → model calls `read_file("notes/agenda.md")` → registry executes via IPC → result fed back → model can compose summary and `write_file` it back (lines 1313–1339).

### C. Agent system — separate path, NOT integrated with AI tool registry

`com.nexus.agent` (`crates/nexus-agent/`):

- `LlmAgent<D>` (`llm.rs`) calls a `ChatDriver` to generate a JSON plan (lines 64–139).
- Production driver wraps `com.nexus.ai::stream_chat`.
- Planner prompt (lines 41–49) lists available plugins: `com.nexus.storage`, `com.nexus.database`, `com.nexus.git`, `com.nexus.terminal`, `com.nexus.ai`.
- Each `ToolCall` carries `target_plugin_id` (`crates/nexus-agent/src/lib.rs:143–150`); the executor dispatches over kernel IPC.
- **Important:** agents do not reuse the AI engine's tool registry. The tool-loop in `stream_chat` and the agent planner are two separate mechanisms.

### D. MCP integration — NOT wired to AI

`com.nexus.mcp.host` (`crates/nexus-mcp/src/core_plugin.rs:38–51`):

- `list_servers` (handler 1), `call_tool` (handler 3) — read `.forge/mcp.toml`, invoke external MCP tools.
- Agent planner can auto-discover MCP tools (`IMPLEMENTATION_STATUS.md:155`).
- **AI engine's `stream_chat` has zero MCP wiring.** The model cannot call MCP tools through chat today.

---

## 3. What Invokes AI

### CLI — `crates/nexus-cli/src/commands/ai.rs`

| Command | Handler | Lines |
|---|---|---|
| `nexus ai chat` | `stream_chat` | 21–24 |
| `nexus ai ask <q>` | `stream_ask` | 32–54 |
| `nexus ai embed [file]` | `index_file` | 56–82 |
| `nexus ai status` | `status` | 94–137 |
| `nexus ai config` | `config` | 139+ |

All route through `context.ipc_call("com.nexus.ai", handler, args, 120s)`.

### Shell — `shell/src/plugins/nexus/ai/`

- **RAG chat:** `submitQuestion()` (`aiRuntime.ts:402–471`) → `kernel.invoke("com.nexus.ai", "stream_ask", {messages, session_id, limit: 5})`. Subscribes to `com.nexus.ai.stream_chunk` and `stream_done` (lines 51, 52). ChatView.tsx renders tokens live with file-path citation chips.
- **Tool-aware chat:** `submitCmdI()` (`cmdIRuntime.ts:926–982`) → `stream_chat` with optional `mode`/`tools`.
- **Inline completion (BL-035):** `marginSuggest.ts:13–20` calls `stream_chat` with `mode: "complete", tools: "none"` (no side effects).
- **Sessions:** `session_load/save/list/delete` exposed through aiRuntime.

### TUI — `crates/nexus-tui/`

**No direct AI integration found.** AI is shell-only on the GUI side.

---

## 4. Capability Gating — Weak

- AI plugin holds `Capability::IpcCall` by default (`core_plugin.rs:2386` test wiring).
- Storage tools propagate: `read_file`/`write_file` → `ipc_call` → kernel validates caller's `ipc.call` capability → storage's own checks.
- **No `ai.chat` capability exists.** Any caller with `ipc.call` can invoke any AI handler — no per-handler granularity (e.g. allow `chat`, deny `index_file`).
- The `tools` request argument (`stream_chat` lines 225, 181) is **client-controlled, not server-enforced**.

---

## 5. Honest Status Table

| Surface | Status | Evidence |
|---|---|---|
| Anthropic chat + tool-calling | ✅ Fully wired | `anthropic.rs:131–180`, parsing 359–380 |
| OpenAI chat + tool-calling | ✅ Fully wired | `openai.rs:177+` |
| Ollama chat + tool-calling | ✅ Fully wired | `ollama.rs:260+` |
| Tool dispatch loop (8 rounds) | ✅ Fully wired | `core_plugin.rs:1265–1350` |
| `read_file` / `write_file` tools | ✅ Fully wired | `tools/functions.rs:219–230` |
| Streaming (per-token events) | ✅ Fully wired | `ipc.rs:141–163`, `core_plugin.rs:776–870` |
| RAG + vector search | ✅ Fully wired | `core_plugin.rs:1+`, `rag.rs` |
| Session persistence | ✅ Fully wired | `core_plugin.rs:73–85` |
| Terminal / database / git tools | 🔴 Not implemented | `tools/functions.rs:14–18` (deferred comment) |
| Knowledge-graph search tool | 🔴 Not implemented | No code; only Tantivy used internally for RAG |
| Local embeddings | 🟡 Scaffold only | `local_embedding.rs:46–48`; deferred per status doc |
| `semantic_search` handler | 🟡 Stub | Handler registered, no real impl |
| Agent ↔ AI tool registry | 🟡 Not unified | Agents go via kernel IPC, not AI's tool loop |
| MCP ↔ AI | 🟡 Agent-only | `stream_chat` cannot call MCP |

---

## 6. Discrepancies vs `IMPLEMENTATION_STATUS.md`

The status doc marks "AI Engine" 🟢. Self-acknowledged gaps (line 135):

> No embedding backend beyond remote providers (local embeddings deferred). No tool registration for agents. Token budget is library-only — not yet wired into stream_chat / stream_ask provider request paths.

Confirmed by code:

- Local embeddings — scaffolded only.
- Token-budget privacy redactor — wired into RAG prompt assembly, NOT into streaming handler request paths.
- "No tool registration for agents" — accurate; agents emit raw IPC plans.

---

## 7. Practical Upshot

A user running `nexus ai chat` or hitting Chat in the shell gets a Claude / GPT / Ollama instance that can:

1. Read any forge markdown file via `read_file`.
2. Write any forge markdown file via `write_file`.
3. See indexed chunks injected as RAG context (when using `ask` / `stream_ask`).
4. Continue across sessions.

Everything beyond file I/O — terminal commands, DB queries, git history, external APIs via MCP — requires the **agent system**, which is a separate planning/execution layer not invoked by the chat tool-loop.

---

## 8. Recommended Next Steps to Broaden the Surface

1. **Register more tools in `crates/nexus-ai/src/tools/functions.rs`** that proxy to existing service plugins (e.g. `terminal_exec` → `com.nexus.terminal`, `kg_search` → `com.nexus.storage` knowledge-graph handlers, `git_log` → `com.nexus.git`).
2. **Bridge MCP into the AI tool registry** — at `stream_chat` time, query `com.nexus.mcp.host::list_servers` and dynamically synthesize tool schemas so the model can invoke MCP tools alongside built-ins.
3. **Introduce per-handler capabilities** (`ai.chat`, `ai.index`, `ai.session.write`) for proper gating.
4. **Unify agent and AI tool-calling** so `LlmAgent` can reuse the registered tool registry instead of free-form JSON plans.
5. **Wire token-budget redactor into streaming handler request paths**, not only RAG assembly.
