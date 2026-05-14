# Thoth → Nexus Capability Assessment

> **Status:** Research / non-binding. No code changes proposed in this doc — it
> only inventories Thoth's surface area and judges what could be ported into
> the Nexus architecture against what already exists.
>
> **Source:** [siddsachar/Thoth](https://github.com/siddsachar/Thoth)
> (Apache 2.0, Python 97.8%, NiceGUI + LangGraph + Ollama).
> **Date:** 2026-05-14.

---

## 1. Executive Summary

Thoth is a **local-first desktop AI assistant** built in Python (NiceGUI UI,
LangGraph agent core, FAISS vector store, Ollama local inference). Its stated
goal is "personal AI sovereignty" — all data on-device, no account required,
no telemetry. The feature surface is wide: a personal entity knowledge graph,
33 agent tools (browser automation, vision, voice, Gmail, Calendar, web search,
charting, image/video generation, health tracking, and more), multi-channel
messaging (Telegram/WhatsApp/Discord/Slack/SMS), a Designer Studio, a Developer
Studio with Docker sandbox, and a plugin/MCP marketplace.

Nexus already covers the *AI plumbing* shared with Thoth: multi-provider LLM,
streaming RAG, agent planning, MCP client/server, scheduled workflows, git
integration, plugin sandboxing, and capability-gated IPC. The architectural
philosophy of the two projects is also very different — Nexus is a Rust
microkernel with file-as-truth and formal capability checks; Thoth is a
monolithic Python app with SQLite as truth.

The highest-value capabilities to adopt from Thoth are therefore **product
patterns and new capability classes** that Nexus does not yet expose:

1. **Typed personal entity knowledge graph** — entities (person/place/event/
   project/concept/etc.) with 40+ directed relationship types, confidence
   scoring, and FAISS semantic recall. Nexus's petgraph is a document-link
   graph, not a personal entity graph.
2. **Dream Cycle** — periodic background refinement: duplicate merging,
   confidence decay, description enrichment, and relationship inference.
3. **Prompt injection detection** — pattern-based scanning of role overrides,
   data-exfiltration patterns, invisible Unicode, and hidden HTML directives
   in agent input/output pipelines.
4. **Sophisticated context-window management** — 85%-trim rule, compression of
   stale tool outputs, deduplication of identical tool call results, stripping
   base64 URIs before LLM invocation.
5. **Runtime approval gates in the agent loop** — LangGraph `interrupt()`
   analog: pausing agent execution to surface destructive-operation approval
   to the user across all frontends (chat, terminal, external channels).
6. **Multi-channel messaging output** — routing agent / workflow results to
   Telegram, Discord, Slack, WhatsApp, or SMS; critical for background-running
   workflows without requiring the shell to be open.
7. **Vision capabilities** — screen capture, webcam, and file-based image
   analysis via a lightweight local vision model.
8. **Voice I/O** — faster-whisper (STT) and Kokoro TTS, both running locally
   without cloud dependencies.
9. **Web search as a first-class agent tool** — Tavily + DuckDuckGo in the
   agent tool registry (today Nexus agents can reach search via MCP but have
   no built-in web search handler).
10. **Browser automation** — Playwright-based navigation, click, type, scroll,
    and DOM snapshot tools for agent-driven web research.
11. **Chart generation** — 10 chart types (bar, line, scatter, pie, heatmap,
    box, area, histogram, donut, horizontal_bar) via Plotly, driven by the
    agent from tabular data.
12. **Docker sandbox for agent code execution** — isolated shadow workspace,
    file-diff detection, patch-apply workflow; makes agent-driven code
    execution meaningfully safer than Nexus's current terminal-in-process
    model.
13. **Health / habit tracker** — structured boolean/numeric/duration/categorical
    trackers with streaks, statistics, and co-occurrence analysis. A natural
    fit as a Nexus plugin on top of the Bases engine.
14. **Custom tool builder UX** — guided workflow to turn a local folder or
    GitHub repo into a reusable agent tool, with approval before promotion.

Items that are **not worth porting** because Nexus already does them at least
as well, or because they conflict with Nexus's architecture:
- Provider abstraction and RAG pipeline (Nexus is stronger here)
- Workflow scheduling / cron (Nexus is more powerful)
- MCP client/host (Nexus has connection pooling, reconnect, in-flight cap)
- Plugin sandboxing (Nexus's wasmtime WASM is stronger than Thoth's Python
  subprocess sandbox)
- Git integration (Nexus's git2-based 26-handler service is more complete)
- File indexing / FTS (Tantivy > Thoth's keyword search)
- Thoth's SQLite-as-truth pattern (conflicts with Nexus's file-as-truth invariant)
- Thoth's monolithic Python architecture (conflicts with Nexus's microkernel IPC model)

---

## 2. Thoth Capability Inventory

### 2.1 Tech Stack

| Layer | Technology |
|---|---|
| UI | NiceGUI (Python-native web UI, served at localhost:8080) |
| Agent runtime | LangGraph `create_react_agent` (ReAct loop) |
| LLM access | Ollama local + OpenAI / Anthropic / Google AI / xAI / MiniMax / OpenRouter / custom OpenAI-compatible endpoints |
| Embeddings | HuggingFace sentence-transformers (local) |
| Vector store | FAISS (L2-normalised, incremental upsert) |
| Graph | NetworkX MultiDiGraph (in-memory, rebuilt from SQLite on boot) |
| Storage | SQLite (WAL mode) — `~/.thoth/*.db` |
| Language | Python 3.11+ (97.8% of codebase) |
| Desktop packaging | One-click `.exe` (Windows), `.dmg` (macOS), one-line bash installer (Linux) |

### 2.2 LLM Provider Support

- **Local (default):** Ollama with 39 curated models; default brain = `qwen3:14b`
- **Cloud:** OpenAI, Anthropic Claude, Google AI (Gemini), xAI (Grok), MiniMax, OpenRouter
- **Custom:** Any OpenAI-compatible endpoint (LM Studio, vLLM, LocalAI, llama.cpp)
- **Subscription:** ChatGPT / Codex subscription-backed access
- Provider routing is abstracted through `providers/routing.py` with OAuth and
  OS-keyring credential storage

### 2.3 Personal Knowledge Graph

The core memory system is an **entity-relationship graph** (not a document
graph). Key properties:

**Entity types (11 canonical):** `person`, `preference`, `fact`, `event`,
`place`, `project`, `organisation`, `concept`, `skill`, `media`,
`self_knowledge`.

**Relation types (40+ controlled vocabulary):**
- Family / social: `knows`, `friend_of`, `mother_of`, `married_to`
- Location: `lives_in`, `works_at`, `born_in`
- Work: `works_on`, `manages`, `employed_by`, `leads`
- Knowledge: `proficient_in`, `certified_in`, `studies`
- Media: `reading`, `watching`, `authored`
- (Plus ~25 more covering temporal, ownership, membership, causality, etc.)

**Storage layers:**
- SQLite: `entities` table (id, type, subject, description, aliases, tags,
  properties, source, timestamps) + `relations` table (source/target IDs,
  type, confidence, metadata)
- NetworkX MultiDiGraph: in-memory mirror rebuilt at startup; RLock for
  thread safety; supports multiple edge types between the same node pair
- FAISS: L2-normalised HuggingFace embeddings; incremental `_upsert_index`
  avoids full rebuilds; thread-safe via dedicated `_faiss_lock`

**API surface:** `save_entity`, `get_entity`, `update_entity`,
`delete_entity`, `save_relation`, `get_relations`, `find_by_subject` (fuzzy
match), `rebuild_index`, `semantic_search`, `find_duplicate`,
`consolidate_duplicates`.

**Backward compat:** `memory.py` wrapper maps legacy column names (`category`,
`content`) to modern graph equivalents; existing code needs no changes.

### 2.4 Dream Cycle (Knowledge Refinement Engine)

A nightly background daemon that automatically maintains knowledge quality:
- **Duplicate merging** — semantic similarity comparison with threshold; merges
  near-duplicate entities
- **Description enrichment** — uses the LLM to expand sparse entity descriptions
- **Confidence decay** — reduces confidence scores on relationships not
  reinforced over time
- **Relationship inference** — asks the LLM to propose new relationships from
  entity clusters

### 2.5 Agent Architecture

Built on LangGraph's `create_react_agent` prebuilt:
- **Recursion limits:** 50 steps (chat), 100 (background tasks), 120
  (developer mode)
- **Context optimisation (`_pre_model_trim`):** Trims message history to 85%
  of context window before every LLM invocation; compresses old browser
  snapshots to stubs; deduplicates identical tool outputs; strips base64 data
  URIs; injects current datetime
- **Graph-enhanced recall:** Before each query, relevant entity graph facts are
  fetched and prepended to the context
- **Prompt injection detection:** Pattern-based scanning for role overrides,
  instruction-hijacking strings, data-exfiltration patterns, invisible Unicode
  (U+200B, U+FEFF etc.), and hidden HTML directives (`<!--`, `<script>`, etc.)
- **Wind-down warnings:** Issues an alert as the recursion limit approaches,
  allowing the agent to checkpoint before termination
- **Untrusted tool isolation:** Web search, email, and URL-read results are
  wrapped in XML boundary tags before being presented to the LLM, preventing
  injection from external content

### 2.6 Tool Ecosystem (33 tools)

| Tool | Capability |
|---|---|
| `web_search_tool` | Tavily (primary) + DuckDuckGo (fallback); up to 8 results; disabled by default |
| `browser_tool` | Playwright automation (headless=False, persistent profile); shared-browser multi-tab model; 7 sub-tools: navigate, click, type, scroll, snapshot (accessibility tree), back, tab management; numbered-reference element model reduces token waste |
| `url_reader_tool` | Fetch and extract content from arbitrary URLs |
| `shell_tool` | Persistent-directory shell; 3-tier safety (safe/needs-approval/blocked); `LangGraph interrupt()` for approval; per-thread `ShellSession`; history to `~/.thoth/shell_history.json`; pipes/redirects auto-escalate |
| `vision_tool` | Screen capture, webcam (live frame), or file path; dispatched to local Ollama vision model (e.g. `gemma3:4b`) |
| `filesystem_tool` | Read/write/list files in sandboxed workspace |
| `documents_tool` | FAISS semantic search over user-uploaded documents (PDF, DOCX, etc.) |
| `memory_tool` | CRUD + semantic search over the personal entity knowledge graph |
| `gmail_tool` | OAuth Gmail: search, read, draft, send (with attachments); send disabled by default |
| `calendar_tool` | Google Calendar CRUD: search events, create, update, delete; delete disabled by default |
| `chart_tool` | 10 chart types (bar, horizontal_bar, line, scatter, pie, donut, histogram, box, area, heatmap) via Plotly; reads CSV/TSV/Excel/JSON/JSONL; auto column detection; wide-to-long conversion; PNG export |
| `image_gen_tool` | Generate + edit images; providers: OpenAI (GPT Image 1/1.5/Mini), Google Imagen 4 (standard/fast/ultra), xAI Grok Imagine; sizes up to 1536×1024; quality tiers |
| `video_gen_tool` | Text-to-video + image-to-video; Google Veo (720p–4K, 4–8s, native audio) and xAI (480p–720p, 1–15s, 6 aspect ratios) |
| `task_tool` | Scheduled tasks: `daily:HH:MM`, `weekly:day:HH:MM`, one-shot `delay_minutes`; template variables (`{{date}}`, `{{time}}`, etc.); delivery via Telegram/email/desktop notification; safety modes |
| `tracker_tool` | Health/habit tracking; boolean/numeric/duration/categorical types; streaks, adherence%, mean/median/stddev, day-of-week distribution, co-occurrence detection, CSV export |
| `developer_tool` | Git operations, file editing, test management in Developer Studio context |
| `custom_tool_builder_tool` | Convert GitHub repo or local folder into a reusable tool; guided review + approval before promotion |
| `arxiv_tool` | Academic paper search |
| `wikipedia_tool` | Wikipedia article lookup |
| `youtube_tool` | YouTube search / transcript access |
| `wolfram_tool` | WolframAlpha computation queries |
| `weather_tool` | Real-time weather data |
| `x_tool` | X (Twitter) integration |
| `calculator_tool` | Mathematical calculations |
| `system_info_tool` | OS and hardware info |
| `mcp_tool` | Invoke tools from connected external MCP servers |
| `conversation_search_tool` | Search across past conversation threads |
| `thoth_status_tool` | Agent self-status and diagnostics |
| `updater_tool` | In-app update management |

### 2.7 Multi-Channel Messaging

Five messaging backends with a shared channel abstraction:

| Channel | Transport | Notes |
|---|---|---|
| Telegram | Long-polling bot | Text/voice/photo/document input; streaming response edits; approval buttons via inline keyboard; message splitting at 4096-char limit; HTML formatting |
| WhatsApp | Bridge server | Via `whatsapp_bridge/` subdirectory |
| Discord | Bot | `discord_channel.py` |
| Slack | Bot | `slack.py` |
| SMS | SMS gateway | `sms.py` |

Common infrastructure: `auth.py`/`auth_store.py`, `approval.py` for cross-channel approval routing, `commands.py` for slash commands, `media.py` for media forwarding.

### 2.8 Designer Studio (46 modules)

A full-featured content-creation environment:
- **Artifacts:** Decks, documents, landing pages, app mockups, storyboards
- **AI content generation:** `ai_content.py` — AI-driven text/image generation
  integrated into the editing surface
- **Critique-repair loops:** `critique.py` + `review.py` — AI reviews a draft
  and proposes improvements; user iterates
- **Brand system:** `brand.py`, `brand_lint.py` — brand guidelines definition
  and lint checks to keep generated content on-brand
- **Template gallery:** Pre-built templates for common document types
- **Interactive prototypes:** Hotspot recorder for clickable mockups
- **History / diffing:** Version history and mutation diff tracking
- **Import / export / publish / share:** Multiple output formats
- **Image generation integration:** OpenAI, Google, xAI models

### 2.9 Developer Studio (20 modules)

A git-integrated coding workspace:
- **Git operations:** Status, branch creation/switching, commits, fast-forward
  merge, worktree management
- **Code review:** `review.py` — AI-driven code analysis with approval
  workflows
- **File editing:** `edits.py` + `change_ledger.py` — tracked edits with a
  change ledger for audit
- **Sandbox execution:** Docker/Podman container per workspace; shadow workspace
  isolation (changes don't touch host files until approved); network isolation
  option; 120s timeout; file-diff detection; patch-apply via git
- **PR workflow:** `github.py` — PR preparation (branch, commit, push)
- **Test management:** Execute tests in sandbox with output capture
- **Tool capsules:** `tool_capsules.py` — package workspace-level tools for
  agent reuse

### 2.10 Workflow Engine

TOML-free; workflow definitions live in Python task objects stored to SQLite:
- **Trigger types:** Scheduled (`daily:HH:MM`, `weekly:day:HH:MM`), one-shot
  timer, notification-only
- **Approval gates:** `safety_mode` per task — `block` (refuse non-safe tools
  in background), `approve` (LangGraph interrupt), `allow_all`
- **Delivery channels:** Task outputs routed via Telegram, email, or desktop
  notification
- **Variable interpolation:** `{{date}}`, `{{day}}`, `{{time}}`, `{{month}}`,
  `{{year}}` in prompt templates
- **Pipeline tasks:** Conditional branching and subtasks for advanced flows

(Note: Nexus's workflow engine is substantially more powerful — TOML-defined,
multi-trigger types including file-event/webhook/git-event, parallel steps,
per-step retry with jitter, POSIX cron syntax with `next_after` calculation.)

### 2.11 Voice I/O

- **STT:** faster-whisper (local Whisper model) — no cloud dependency
- **TTS:** Kokoro TTS — neural TTS running locally
- Both integrate with the main NiceGUI chat surface and the Telegram channel
  (voice messages in → transcribed → agent → audio back out)

### 2.12 Plugin System

| Component | Description |
|---|---|
| `manifest.py` | Plugin metadata: name, version, capabilities |
| `loader.py` | Dynamic Python module loading |
| `installer.py` | Download and install from marketplace |
| `sandbox.py` | Isolated execution environment (subprocess-based) |
| `marketplace.py` | Discovery and distribution |
| `registry.py` | Runtime plugin registry |
| `ui_marketplace.py` | In-app marketplace browser UI |

Community plugins are Python modules sandboxed via subprocess isolation (weaker
than Nexus's wasmtime WASM sandbox). The MCP client (`mcp_client/`) adds a
`marketplace.py` for discovering MCP servers with per-server and per-tool
approval toggles.

### 2.13 MCP Client

- `mcp_client/runtime.py` manages server lifecycle
- `mcp_client/marketplace.py` provides discovery and recommended servers
  (`recommended_servers.json` ships curated defaults)
- `mcp_client/safety.py` applies destructive-action approval gates to MCP
  tool calls
- `mcp_client/conflicts.py` handles tool-name conflicts across servers
- Per-server and per-tool enable/disable toggles exposed in UI

### 2.14 Privacy & Security Model

- All data at `~/.thoth/` — no cloud upload of user content
- API keys stored in OS keyring (Windows Credential Manager, macOS Keychain,
  Linux Secret Service)
- Filesystem sandbox: agent file access limited to a configured workspace root
- Shell command classification: safe (auto-run) / needs-approval / blocked
  (catastrophic patterns like `rm -rf /`, fork bombs)
- Prompt injection detection: role-override patterns, instruction-hijacking
  strings, data exfiltration (`base64`, `curl`, `wget` in suspicious contexts),
  invisible Unicode (U+200B, zero-width chars), HTML comment/script directives
- Untrusted data wrapped in XML boundary tags before LLM presentation
- No account system, no server, no telemetry pipeline

### 2.15 Obsidian-Compatible Wiki Export

The knowledge graph can export to an Obsidian vault format, generating
`[[wikilink]]`-style `.md` files per entity. This makes Thoth's in-memory
entity graph portable to any Markdown-based PKM tool.

---

## 3. Nexus ↔ Thoth Feature Matrix

| Capability | Nexus | Thoth |
|---|---|---|
| **Document knowledge graph** | ✅ petgraph of file links, backlinks, wikilink resolution | ❌ |
| **Personal entity knowledge graph** | ❌ (document links only) | ✅ 11 entity types, 40+ relation types, confidence scores |
| **Dream Cycle refinement** | ❌ | ✅ nightly dedup/decay/enrich/infer |
| **Full-text search** | ✅ Tantivy with scoping operators | ⚠️ keyword-only; FAISS handles semantic |
| **Semantic / vector search** | ⚠️ planned; block-level RAG chunking present | ✅ FAISS, HuggingFace embeddings |
| **RAG with citations** | ✅ `RagSource[]`, streaming, token budgeting, privacy redaction | ⚠️ present but less structured |
| **Multi-provider LLM** | ✅ Anthropic, OpenAI, Ollama, llama.cpp | ✅ Ollama + 6 cloud providers + custom endpoints |
| **Streaming chat** | ✅ kernel-bus `stream_{start,chunk,done}` | ✅ LangGraph streaming |
| **Agent / planner** | ✅ LLM planner + plan executor, step approval, history | ✅ LangGraph ReAct, richer tool set |
| **Context window management** | ⚠️ token budgeting for RAG; conversation trimming not explicit | ✅ 85% trim, snapshot compression, dedup, base64 strip |
| **Prompt injection detection** | ❌ | ✅ pattern-based + invisible Unicode + HTML directives |
| **Runtime approval gates (agent)** | ❌ (capability checks are compile-time) | ✅ `interrupt()` gates, per-channel delivery |
| **MCP client** | ✅ connection pool, reconnect, in-flight cap | ✅ marketplace, per-server approval |
| **MCP server** | ✅ stdio, 13 nexus_* tools, notes as resources | ❌ |
| **Workflow engine** | ✅ TOML, multi-trigger, parallel, retry, cron | ⚠️ simpler; task-oriented, SQLite-stored |
| **Skills / prompt presets** | ✅ .skill.md, composition, parameter substitution | ❌ (bundled skills are static) |
| **Git integration** | ✅ 26 IPC handlers, git2, push/pull/stash/tag | ⚠️ basic git subprocess wrapper |
| **Terminal / PTY** | ✅ portable-pty, 50-session cap, saved commands, AI suggestions | ⚠️ terminal bridge (PTY), no saved commands |
| **Plugin system** | ✅ wasmtime WASM sandbox, capability-gated, hot-reload | ⚠️ Python subprocess sandbox (weaker) |
| **Bases / structured database** | ✅ .bases TOML, table/gallery/kanban/calendar views, formula | ❌ |
| **Rich markdown editor** | ✅ CodeMirror 6, block transactions, MDX components | ❌ (Designer Studio is for design, not PKM editing) |
| **File-as-truth architecture** | ✅ invariant | ❌ SQLite-as-truth |
| **Capability security model** | ✅ hierarchical caps, WASM sandbox, audit log | ⚠️ filesystem sandbox + safety classification |
| **Desktop shell** | ✅ Tauri 2 + React/Vite, plugin-first | ✅ NiceGUI (Python web UI, not native Tauri) |
| **TUI** | ✅ Ratatui | ❌ |
| **CLI** | ✅ 14+ command groups | ❌ (launcher.py only) |
| **Themes** | ✅ 11 themes, 547 CSS vars, hot-reload | ⚠️ NiceGUI styling |
| **Browser automation** | ❌ | ✅ Playwright, accessibility tree snapshots |
| **Vision (screen / camera / file)** | ❌ | ✅ local Ollama vision model |
| **Voice STT / TTS** | ❌ | ✅ faster-whisper + Kokoro (local) |
| **Multi-channel messaging** | ❌ | ✅ Telegram, WhatsApp, Discord, Slack, SMS |
| **Web search (built-in)** | ❌ (reachable via MCP, not built-in) | ✅ Tavily + DuckDuckGo |
| **Chart generation** | ❌ | ✅ 10 Plotly chart types |
| **Image generation** | ❌ | ✅ OpenAI, Google Imagen 4, xAI |
| **Video generation** | ❌ | ✅ Google Veo, xAI |
| **Health / habit tracking** | ❌ | ✅ 4 tracker types, streaks, statistics |
| **Email integration (Gmail)** | ❌ | ✅ OAuth, read/draft/send, attachments |
| **Calendar integration** | ❌ | ✅ Google Calendar CRUD |
| **Document upload & semantic search** | ⚠️ Tantivy FTS; FAISS planned | ✅ FAISS over user-uploaded docs |
| **Docker / container sandbox** | ❌ (terminal runs in-process) | ✅ Docker/Podman shadow workspace, patch-apply |
| **Designer / media studio** | ⚠️ canvas (JSON nodes) | ✅ full design studio (decks/mockups/storyboards) |
| **Custom tool builder UX** | ⚠️ `nexus plugin scaffold --type wasm` (CLI only) | ✅ guided approval UI workflow |
| **Obsidian vault export** | ⚠️ files are already Obsidian-compatible markdown | ✅ explicit entity→wikilink export |
| **Arxiv / Wikipedia / YouTube / Wolfram / Weather** | ❌ (reachable via MCP) | ✅ built-in tools |
| **X (Twitter) integration** | ❌ | ✅ x_tool.py |
| **One-click installers** | ❌ (from-source) | ✅ .exe / .dmg / one-line bash |
| **CRDT collaborative editing** | ✅ Phase 4, kernel-bus conflict tracking | ❌ |
| **Knowledge graph visual explorer** | ✅ graph plugin in shell | ✅ visual exploration UI |

---

## 4. Adoption Recommendations

Grouped by effort vs. value, all recommendations expressed as new capability
directions — **no Thoth code would be ported directly** since the stack is
incompatible. Each item maps to the Nexus service crate or shell plugin where
it would land.

### 4.1 High Value — Strongly Recommended

#### 4.1.1 Typed Personal Entity Knowledge Graph

**Gap:** Nexus's knowledge graph is a document-link graph (wikilinks, backlinks,
petgraph of file relationships). It does not model personal entities — people,
events, preferences, projects, concepts — as first-class nodes with typed
directional relationships.

**What to adopt:** An entity/relationship layer on top of the existing
`nexus-storage` indexing pipeline. Entity definitions could live as YAML
frontmatter in Markdown files (preserving file-as-truth), with `nexus-storage`
indexing entity-type, subject, description, and relations. The in-memory graph
could reuse or extend the existing `petgraph` structure with typed edge labels
and confidence scores.

**Where it lands:** `nexus-storage` (indexing + graph) + new `graph_entity_*`
IPC handlers, a new shell plugin `nexus.entityGraph`, and CLI subcommands
under `nexus graph`.

**Reference in Thoth:** `knowledge_graph.py`, `memory.py`

---

#### 4.1.2 Dream Cycle / Knowledge Refinement Engine

**Gap:** Nexus has no background maintenance of stored knowledge — no duplicate
detection, no confidence decay, no AI-driven enrichment of sparse entries.

**What to adopt:** A scheduled workflow (reusing `nexus-workflow`'s cron
engine) that periodically:
1. Runs semantic similarity checks over entity descriptions to surface duplicates
2. Proposes merges via the AI service
3. Decays confidence scores on infrequently-accessed relationships
4. Prompts the AI to infer new relationships from entity clusters

**Where it lands:** A new step type in `nexus-workflow` plus a dedicated
`nexus-ai` handler for semantic deduplication and enrichment.

**Reference in Thoth:** `app.py` startup sequence (dream cycle daemon), `knowledge_graph.py` dedup/consolidation methods.

---

#### 4.1.3 Prompt Injection Detection

**Gap:** Nexus's `PrivacyPolicy` redactor (in `nexus-ai`) handles outbound PII
but there is no inbound prompt injection scanning — no detection of role-
override strings, exfiltration patterns, invisible Unicode characters, or hidden
HTML directives in content that flows into the agent context.

**What to adopt:** A pre-LLM sanitisation pass in `nexus-ai`'s context assembly
path that:
- Scans for role-override patterns (`"ignore previous instructions"`, `"you are now"`, etc.)
- Detects invisible Unicode (zero-width space U+200B, BOM U+FEFF, etc.)
- Flags HTML comment / script directive injections (`<!--`, `<script>`)
- Applies to RAG-retrieved chunks, tool call results, and channel-sourced messages

**Where it lands:** `crates/nexus-ai/src/sanitize.rs` (new module) called
before prompt assembly in `stream_ask` and `stream_chat` handlers.

**Reference in Thoth:** `agent.py` `_check_prompt_injection()` function.

---

#### 4.1.4 Sophisticated Context Window Management

**Gap:** Nexus's token budgeting (`BudgetWarning`, lowest-scoring chunks
dropped first) applies to RAG context assembly. The conversation message
history itself has no explicit trim policy — long agent runs can inflate context
past provider limits.

**What to adopt:** A pre-invocation trim pass in the agent execution loop:
- Hard limit at 85% of provider context window
- Compress stale tool call results to stub summaries (preserving key outputs)
- Deduplicate identical consecutive tool outputs
- Strip base64-encoded data URIs from message history
- Always preserve the most recent N turns regardless of budget

**Where it lands:** `crates/nexus-agent/src/context.rs` (new module) called
before each `stream_chat` invocation in plan execution.

**Reference in Thoth:** `agent.py` `_pre_model_trim()` function.

---

#### 4.1.5 Runtime Approval Gates in the Agent Loop

**Gap:** Nexus's capability system enforces access at the IPC dispatch layer —
a plugin either has `exec.spawn` or it doesn't. There is no runtime "pause and
surface this action to the user for confirmation" mechanism in the agent
execution path. Destructive agent steps (file deletion, git force-push, shell
commands) proceed without a user checkpoint.

**What to adopt:** An `approval_required` flag on IPC handlers that, when set,
emits a `com.nexus.agent.approval_required` kernel-bus event before executing
the step. Frontends subscribe and display an approve/cancel UI (Tauri shell
panel, TUI modal, or Telegram inline keyboard). The agent plan executor
`await`s the response with a configurable timeout.

**Where it lands:** `crates/nexus-kernel/src/` (event type), `crates/nexus-agent/src/executor.rs` (await-gate), shell plugin `nexus.agentApproval`.

**Reference in Thoth:** `tools/shell_tool.py` `LangGraph interrupt()` pattern, `channels/approval.py`.

---

#### 4.1.6 Multi-Channel Messaging Output

**Gap:** Nexus agent and workflow outputs are only surfaced in the active
frontend session. Background workflows that complete while the shell is closed
have no delivery mechanism.

**What to adopt:** A new `nexus-notifications` service plugin (or extension to
`nexus-workflow`) that routes completion events and agent outputs to external
channels. Minimum viable: Telegram bot; extensible to Discord webhooks, Slack
incoming webhooks, email (SMTP), and desktop OS notifications. Credentials
stored in the existing `nexus-security` keyring.

**Where it lands:** New `crates/nexus-notifications/` core plugin; shell plugin
for notification preferences UI; workflow TOML gains an optional `notify:` key.

**Reference in Thoth:** `channels/` directory, `tools/task_tool.py` delivery
channel routing.

---

### 4.2 Medium Value — Worth Evaluating

#### 4.2.1 Vision Capabilities

**Gap:** No agent tool for image analysis in Nexus. Screen capture and file
image analysis would meaningfully extend agent research loops.

**What to adopt:** A `nexus-vision` IPC handler (in `nexus-ai` or as a separate
plugin) wrapping an Ollama vision model (e.g. `llava`, `gemma3:4b`) for:
- File-based image analysis (`analyze_image(path, question)`)
- Screenshot capture (platform-specific; `scap` crate on Linux/macOS)

**Where it lands:** New handler in `crates/nexus-ai/` or `crates/nexus-vision/`.
Agent tool list auto-discovery (existing mechanism in `nexus-agent`).

**Reference in Thoth:** `tools/vision_tool.py`, `VisionService`.

---

#### 4.2.2 Voice I/O (STT + TTS)

**Gap:** Nexus has no speech-to-text or text-to-speech capability. The TUI and
Tauri shell have no voice input surface.

**What to adopt:** Local STT via `whisper-rs` (Rust bindings to whisper.cpp)
and TTS via a local model (Kokoro ONNX or Piper). Exposed as two IPC handlers
(`transcribe_audio(path)` and `synthesize_speech(text)`) in a new
`nexus-voice` plugin. Shell plugin contribution to the command palette and
Tauri microphone permissions.

**Where it lands:** New `crates/nexus-voice/` plugin.

**Reference in Thoth:** `app.py` voice service, `channels/telegram.py` voice
message handling.

---

#### 4.2.3 Web Search as a Built-In Agent Tool

**Gap:** Nexus agents can reach web search via MCP servers (e.g., Brave Search
MCP) but there is no built-in web search IPC handler. Users who have not
configured an MCP server have no search capability.

**What to adopt:** A `web_search(query, provider)` IPC handler in `nexus-ai`
(or a dedicated `nexus-web` plugin) supporting Tavily and DuckDuckGo with API
key stored in the security keyring. Requires `net.fetch` capability. The agent
planner auto-discovers it via the existing tool-registry mechanism.

**Where it lands:** New handler(s) in `crates/nexus-ai/` or new plugin.

**Reference in Thoth:** `tools/web_search_tool.py`, `tools/duckduckgo_tool.py`.

---

#### 4.2.4 Browser Automation

**Gap:** No browser automation in Nexus's agent tool set. Research tasks that
require web interaction currently require manual user steps.

**What to adopt:** A Playwright-based browser tool exposed via IPC. The
Thoth architecture of a single shared browser with per-thread tab allocation
and an accessibility-tree snapshot model is worth adopting directly (it avoids
profile-lock conflicts and reduces token waste). Safety: all navigation actions
require the `net.fetch` capability; form-submission and click actions require
`net.interact` (new cap string).

**Where it lands:** New `crates/nexus-browser/` plugin; Tauri shell can reuse
the existing WebView for a "managed browser" tab.

**Reference in Thoth:** `tools/browser_tool.py`, `ShellSessionManager`.

---

#### 4.2.5 Chart Generation

**Gap:** No charting capability in Nexus. Agent analyses that produce tabular
data (from Bases queries, CSV files, tracker exports) have no visualisation
path.

**What to adopt:** A `generate_chart(data, chart_type, x_col, y_col)` IPC
handler backed by a Rust charting library (`plotters`) or a thin Python
sidecar. The 10 chart types in Thoth (bar, line, scatter, pie, donut, histogram,
box, area, heatmap, horizontal_bar) cover the most common agent output scenarios.
Output: PNG file path (Tauri can render inline) or SVG string.

**Where it lands:** New handler in `crates/nexus-ai/` or `crates/nexus-formats/`.

**Reference in Thoth:** `tools/chart_tool.py`.

---

#### 4.2.6 Health / Habit Tracker

**Gap:** Nexus's Bases engine (`.bases` TOML + table/kanban/gallery/calendar
views) is close to what a tracker needs but has no built-in notion of daily
adherence, streaks, or statistical analysis.

**What to adopt:** A `nexus-tracker` plugin (or Bases extension) that adds
first-class tracker semantics: auto-create trackers on first log, 4 tracker
types, streak computation, adherence%, and co-occurrence detection. The
underlying storage could use the existing `nexus-kv` or a new `.tracker` TOML
file per tracker (preserving file-as-truth).

**Where it lands:** New `crates/nexus-tracker/` plugin or extended Bases type
in `crates/nexus-database/`.

**Reference in Thoth:** `tools/tracker_tool.py`, `tracker.db` schema.

---

#### 4.2.7 Docker Sandbox for Agent Code Execution

**Gap:** Nexus's terminal runs `portable-pty` directly on the host OS. Agent-
driven code execution (e.g., "run this script") has no container isolation.

**What to adopt:** An optional Docker/Podman execution mode in `nexus-terminal`:
when a `docker_sandbox: true` flag is set on a terminal session, commands
execute in an ephemeral container with a shadow copy of the workspace. File
changes are diffed against the shadow and surfaced to the user for apply/reject
before the host workspace is touched.

**Where it lands:** `crates/nexus-terminal/src/sandbox.rs` (new module);
terminal IPC gains `create_sandbox_session` and `apply_sandbox_diff` handlers.

**Reference in Thoth:** `developer/sandbox_runtime.py`, Docker/Podman container lifecycle, shadow workspace + unified diff.

---

#### 4.2.8 Custom Tool Builder UX

**Gap:** Nexus has `nexus plugin scaffold --type wasm` (CLI) but no guided UI
workflow for converting an existing repository or folder into a usable agent
tool.

**What to adopt:** A shell plugin (`nexus.toolBuilder`) wrapping the existing
`nexus plugin scaffold` flow with a wizard-style UI: select source (local folder
or GitHub URL), review proposed tool interface, test in sandbox, approve for
promotion. The backend is the existing WASM plugin pipeline; the builder only
adds UX.

**Where it lands:** Shell plugin `shell/src/plugins/nexus/tool-builder/`.

**Reference in Thoth:** `tools/custom_tool_builder_tool.py`, approval workflow.

---

### 4.3 Lower Priority — Community Plugin Territory

The following capabilities are real but either out of Nexus's core scope or
better delivered as community plugins or MCP server connections:

| Capability | Rationale |
|---|---|
| **Gmail / email integration** | High credential-management complexity; better as a community plugin or via a Gmail MCP server |
| **Google Calendar** | Same rationale; a CalDAV-compatible approach would be broader |
| **Image generation** | Cloud API cost concerns; fits as a community plugin using `net.fetch` cap |
| **Video generation** | Same; very cloud-dependent |
| **Arxiv / Wikipedia / YouTube / Wolfram / Weather** | All reachable via existing MCP servers; no need for built-in handlers |
| **X (Twitter) integration** | Narrow use case; community plugin |
| **Obsidian vault export** | Nexus files are already Obsidian-compatible `.md`; a dedicated export is a minor convenience wrapper |
| **One-click installers** | Infrastructure / packaging concern; orthogonal to capabilities |

---

## 5. What Nexus Does Better (Do Not Regress)

These are areas where Nexus's architecture is materially stronger than Thoth's
and should remain as-is:

| Area | Why Nexus Wins |
|---|---|
| **File-as-truth** | Thoth's SQLite-as-truth creates lock-in; Nexus's markdown files are portable and recoverable |
| **Microkernel isolation** | Nexus's `dep_invariants.rs` enforces IPC-only coupling; Thoth's monolithic `app.py` has implicit module dependencies |
| **WASM plugin sandboxing** | wasmtime with fuel metering and capability gates is fundamentally more secure than Thoth's Python subprocess sandbox |
| **Full-text search** | Tantivy with scoping operators (`tag:`, `path:`, `prop:`, `type:`) far exceeds Thoth's keyword search |
| **Workflow engine** | TOML-defined, multi-trigger (file-event/webhook/git-event/cron), parallel steps, per-step jitter retry; Thoth's task system is simpler |
| **Multi-frontend parity** | Nexus CLI, TUI, MCP server, and Tauri shell all reach capabilities via the same IPC path; Thoth is shell-only |
| **Bases / structured database** | `.bases` TOML with 4 view engines and formula evaluation has no equivalent in Thoth |
| **Rich Markdown editor** | CodeMirror 6 + block transaction model + MDX components; Thoth has no PKM editor |
| **Git integration** | 27 git2 methods, 26 IPC handlers, SSH passphrase caching; Thoth's git.py is a thin subprocess wrapper |
| **Collaborative CRDT** | Thoth has no collaboration story |
| **Theming** | 11 bundled themes + 547 CSS variables + hot-reload; Thoth relies on NiceGUI's basic styling |
| **MCP server (exposing Nexus)** | Nexus exposes itself as an MCP server; Thoth only consumes MCP servers |

---

## 6. Summary Table

| Priority | Capability | Target Crate / Plugin |
|---|---|---|
| 🔴 High | Typed entity knowledge graph | `nexus-storage` + new `nexus.entityGraph` shell plugin |
| 🔴 High | Dream Cycle refinement engine | `nexus-workflow` step type + `nexus-ai` handler |
| 🔴 High | Prompt injection detection | `nexus-ai` pre-prompt sanitiser |
| 🔴 High | Context window management (trim/dedup) | `nexus-agent` context module |
| 🔴 High | Runtime approval gates (agent loop) | `nexus-kernel` event + `nexus-agent` executor |
| 🔴 High | Multi-channel notification output | New `nexus-notifications` plugin |
| 🟡 Medium | Vision (screen / file image analysis) | New `nexus-vision` plugin |
| 🟡 Medium | Voice STT + TTS (local) | New `nexus-voice` plugin |
| 🟡 Medium | Web search (built-in agent tool) | New handlers in `nexus-ai` or `nexus-web` plugin |
| 🟡 Medium | Browser automation (Playwright) | New `nexus-browser` plugin |
| 🟡 Medium | Chart generation | New handler in `nexus-ai` or `nexus-formats` |
| 🟡 Medium | Health / habit tracker | New `nexus-tracker` plugin or Bases extension |
| 🟡 Medium | Docker sandbox for code execution | `nexus-terminal` sandbox module |
| 🟡 Medium | Custom tool builder UX | Shell plugin `nexus.toolBuilder` |
| 🟢 Low | Gmail / Calendar integration | Community plugin |
| 🟢 Low | Image / video generation | Community plugin (MCP) |
| 🟢 Low | Arxiv / Wikipedia / Wolfram / Weather | Community plugin (MCP) |
| 🟢 Low | Obsidian vault export | Minor wrapper; files already compatible |
| 🟢 Low | One-click installers | Packaging / infra, not capability |
