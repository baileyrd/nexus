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

### A. Built-in tools registered to the model — five

Two registration helpers in `crates/nexus-ai/src/tools/functions.rs` populate the registry, both called from `core_plugin.rs::wire_context`:

**`register_storage_builtins()`** — read/write file pair:

1. **`read_file`** — dispatches to `com.nexus.storage::read_file`. Returns UTF-8 content or "not found" error.
2. **`write_file`** — dispatches to `com.nexus.storage::write_file`. Returns `"Wrote N bytes to <path>"`.

**`register_extended_builtins()`** (G4) — read-only KG/VCS lookups, hard-capped at 25 results per call:

3. **`search_forge`** — dispatches to `com.nexus.storage::search`. Tantivy full-text query, default limit 10.
4. **`list_backlinks`** — dispatches to `com.nexus.storage::backlinks`. Incoming wikilinks for a path.
5. **`git_log`** — dispatches to `com.nexus.git::log`. Recent commits; the git plugin returns a clear error when the forge isn't a repo, surfaced verbatim to the model.

All five run through `KernelPluginContext::ipc_call` with a 30 s timeout. Schema/arg-decoding tests live alongside in `tools/functions.rs`; the dispatch loop integration is exercised by `core_plugin.rs` tests (`semantic_search_dispatch_tests` and friends).

`terminal_exec` and `database_query` from PRD-12 §8.2 remain deferred — they need their own capability surface (`process.spawn`, etc.) that doesn't exist yet.

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

### D. MCP integration — opt-in via `tools=auto_with_mcp` (G5b)

`com.nexus.mcp.host` (`crates/nexus-mcp/src/core_plugin.rs:38–51`):

- `list_servers` (handler 1), `list_tools` (handler 2), `call_tool` (handler 3) — read `.forge/mcp.toml`, invoke external MCP tools.
- Agent planner can auto-discover MCP tools (`IMPLEMENTATION_STATUS.md:155`).
- **AI bridge** (`crates/nexus-ai/src/tools/mcp_bridge.rs`): when `stream_chat` is called with `tools=auto_with_mcp`, the engine fans out `list_servers` + per-server `list_tools` (5s discovery timeout, parallel) and merges every advertised tool into the per-call registry under `mcp__<server>__<tool>`. `call_tool` invocations carry a 60s timeout. Failures degrade gracefully — a slow or broken server is logged and skipped. No cache: discovery re-runs each chat call, which is the right v1 default for correctness over latency.

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

## 4. Capability Gating

- AI plugin holds `Capability::IpcCall` by default (`core_plugin.rs` test wiring).
- Storage tools propagate: `read_file`/`write_file` → `ipc_call` → kernel validates caller's `ipc.call` capability → storage's own checks.
- **Per-handler `ai.*` caps wired (G6, ADR 0022).** Bootstrap registers
  `ai.chat`, `ai.index`, `ai.session.{read,write}`, `ai.config.write`,
  `ai.activity.write` via `add_cap_requirement` for every gated AI handler.
  Workflow + agent contexts now hold `ai.chat` only — `set_config` and
  `activity_clear` return `CapabilityDenied` when reached from those
  contexts. `ai.config.write` is HIGH-risk (persisted-grant prompt).
- **Open follow-up (Phase 2):** the `tools` request argument
  (`AiToolPolicy::Auto` / `None` / `AutoWithMcp`) is still
  client-controlled. ADR 0022 §"Server-side `tools` policy enforcement"
  defers `ai.tools.write` / `ai.tools.mcp` to a separate ADR.

---

## 5. Honest Status Table

| Surface | Status | Evidence |
|---|---|---|
| Anthropic chat + tool-calling | ✅ Fully wired | `anthropic.rs:131–180`, parsing 359–380 |
| OpenAI chat + tool-calling | ✅ Fully wired | `openai.rs:177+` |
| Ollama chat + tool-calling | ✅ Fully wired | `ollama.rs:260+` |
| Tool dispatch loop (8 rounds) | ✅ Fully wired | `core_plugin.rs:1265–1350` |
| `read_file` / `write_file` tools | ✅ Fully wired | `tools/functions.rs` `register_storage_builtins` |
| `search_forge` / `list_backlinks` / `git_log` (G4) | ✅ Fully wired | `tools/functions.rs` `register_extended_builtins` |
| Streaming (per-token events) | ✅ Fully wired | `ipc.rs:141–163`, `core_plugin.rs:776–870` |
| RAG + vector search | ✅ Fully wired | `core_plugin.rs:1+`, `rag.rs` |
| Session persistence | ✅ Fully wired | `core_plugin.rs:73–85` |
| Terminal / database tools | 🔴 Not implemented | `tools/functions.rs` module doc — needs new capability surface |
| Git tool | 🟢 `git_log` shipped | Read-only; mutation tools (`commit`, `stage_*`) still deferred |
| Knowledge-graph search tool | 🟢 `search_forge` + `list_backlinks` shipped | Read-only Tantivy + backlink lookup |
| Local embeddings | 🟡 Scaffold only | `local_embedding.rs:46–48`; deferred per status doc |
| `semantic_search` handler | 🟡 Stub | Handler registered, no real impl |
| Agent ↔ AI tool registry | 🟡 Not unified | Agents go via kernel IPC, not AI's tool loop |
| MCP ↔ AI | 🟢 Opt-in via `tools=auto_with_mcp` (G5b) | `tools/mcp_bridge.rs` discovers + bridges per call |

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

---

## 9. Skills Plugin (`com.nexus.skills`)

Registered handlers (`crates/nexus-skills/src/core_plugin.rs:9–20`):

| # | Command | Sync/Async | Purpose |
|---|---|---|---|
| 1 | `list` | sync | All loaded skills |
| 2 | `get` | sync | One skill by id |
| 3 | `list_by_context` | sync | Skills matching applicable contexts |
| 4 | `triggered_by` | sync | Skills whose triggers match input text |
| 5 | `reload` | sync | Re-scan skills directory |
| 6 | `render` | sync | Render skill with parameter substitution |
| 7 | `compose` | sync | Resolve `depends_on` closure (BL-021) |

**Wired into AI?** Only via the **agent** path, not chat. `crates/nexus-agent/src/core_plugin.rs:651, 712–713, 820–825` — `system_prompt_with_skills`, `compose_skill_body`, `render_skill_body` pull skills into the planner system prompt. `stream_chat` does **not** consult the skills registry.

---

## 10. Agent Plugin Handlers (`com.nexus.agent`)

Full registration (`crates/nexus-agent/src/core_plugin.rs:18–31`) — supersedes the partial description in §2C:

| # | Command | Sync/Async | Purpose |
|---|---|---|---|
| 1 | `plan` | async | Produce a Plan from a goal |
| 2 | `run` | async | Plan + execute in one shot |
| 3 | `run_plan` | async | Execute a preset Plan |
| 4 | `execute_step` | async | Execute a single step |
| 5 | `history_list` | async | List persisted histories |
| 6 | `history_get` | async | Load one history |
| 7 | `history_delete` | async | Delete one history |
| 8 | `list_archetypes` | sync | Return archetype catalogue |
| 9 | `delegate` | async | One archetype → goal → observation (BL-027) |
| 10 | `parallel` | async | Fan-out `(archetype, goal)` jobs (BL-027) |
| 11 | `pipeline` | async | Sequential stages (BL-027) |
| 12 | `trace_get` | async | Orchestrator trace log (BL-027) |

---

## 11. MCP Host Handlers (`com.nexus.mcp.host`)

Full registration (`crates/nexus-mcp/src/core_plugin.rs:13–23`) — supersedes the partial list in §2D:

| # | Command | Sync/Async | Purpose |
|---|---|---|---|
| 1 | `list_servers` | sync | List configured servers |
| 2 | `list_tools` | async | List tools for one server (returns `name`, `description`, `input_schema` per entry — G5a) |
| 3 | `call_tool` | async | Invoke a tool on a server |
| 4 | `list_resources` | async | List resources (BL-026) |
| 5 | `list_prompts` | async | List prompts (BL-026) |
| 6 | `connect` | async | Explicitly establish connection |
| 7 | `disconnect` | async | Gracefully close connection |

---

## 12. Bus Events Emitted by AI

Published by the AI plugin (`crates/nexus-ai/src/core_plugin.rs`):

| Event | Sites | Payload |
|---|---|---|
| `com.nexus.ai.stream_start` | 1057–1060, 1482–1488 | session id; optional RAG source list |
| `com.nexus.ai.stream_chunk` | 1072–1076, 1497–1504 | per-token text + index |
| `com.nexus.ai.stream_done` | 1084–1088, 1521–1529 | final text + citations |
| `com.nexus.ai.activity_appended` | `activity_log.rs` (`ActivityRecorder`) | timeline entry |

Shell consumers subscribe at `shell/src/plugins/nexus/ai/aiRuntime.ts:51–52`.

---

## 13. Shell AI Plugin Contributions

`shell/src/plugins/nexus/ai/index.ts`:

- **Commands** (line 246): `nexus.ai.focus`, `nexus.ai.clear`, `nexus.ai.openSettings`, `nexus.ai.cmdI.open`, `nexus.ai.cmdI.close`, `nexus.ai.reindexForge`.
- **Keybindings** (line 255): focus binding for the chat view.
- **Settings schema** (line 93): `ai.provider`, `ai.model`, `ai.apiKey`, `ai.baseUrl`, `ai.embedProvider`, `ai.embedModel`, `ai.embedApiKey`, plus inline-completion toggles.
- **Views** (lines 310–339): `nexus.ai.view` (chat panel) and `nexus.ai.cmdI.overlay` (command palette overlay).

---

## 14. Prompt Assembly — Where the System Prompt Comes From

- **`stream_chat`** (`crates/nexus-ai/src/core_plugin.rs`) — for `mode=chat`, prepends a host-owned floor (`HOST_SYSTEM_PROMPT_FLOOR`) to the caller-supplied `system` via `compose_chat_system()` (G3). The floor identifies the forge environment, enforces forge-relative paths, and nudges toward tool use; ~80 tokens. `mode=complete` skips the floor (ghost-completion contract is "raw text, no host scaffolding"). Caller's `system`, when present, is appended verbatim after a blank line.
- **`stream_ask`** (line 1480) — calls `build_rag_prompt` to stitch retrieved chunks into a RAG system message.
- **Agent planner** (`crates/nexus-agent/src/core_plugin.rs:642–701`, `system_prompt_with_skills`) — layers `DEFAULT_SYSTEM_PROMPT` + skill guidance + MCP server hints. This is the only path that injects skills/MCP into the prompt.

Implication: the chat tool-loop has **no host-controlled system prompt floor**. A shell that forgets to pass `system` gets whatever the provider defaults to.

---

## 15. Token-Budget Redactor — Code Path

- **Defined:** `crates/nexus-ai/src/privacy.rs:1–238`. `Redactor::with_default_patterns()` (line 106) ships 6 patterns (AWS keys, generic API tokens, GitHub PATs, private keys, …).
- **Called from:** `build_rag_prompt_budgeted` (`crates/nexus-ai/src/rag.rs:443–510`, redaction applied at line 471 before prompt stitching) **and** `query()` (`rag.rs:127–138`, post-fix) — both paths now pass `Some(&Redactor::with_default_patterns())`.
- **Intentionally NOT called from `stream_chat`** — see `privacy.rs:9–17`: *"silently mutating user input would be surprising and the user already chose to send what they pasted."* The caller-supplied `system` and message content flow through unredacted by design. RAG-injected file content is the boundary the redactor is meant to cover, and §15's previous wording was wrong to frame `stream_chat` as a gap.
- **Closed gap:** `stream_ask`'s non-budgeted RAG path (`rag.rs:127`) previously injected retrieved chunk text raw. `query()` now routes through `build_rag_prompt_budgeted` with the default redactor; covered by `query_redacts_secrets_in_retrieved_chunks` in `rag.rs` tests.
