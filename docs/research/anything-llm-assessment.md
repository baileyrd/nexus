# Anything-LLM → Nexus Capability Assessment

> **Status:** Research / non-binding. No code changes proposed in this doc — it
> only inventories Anything-LLM's surface area and judges what could be ported
> into the Nexus architecture, against what already exists.
>
> **Source:** [Mintplex-Labs/anything-llm](https://github.com/Mintplex-Labs/anything-llm)
> (MIT, JS/TS, ~60k★ at time of writing).
> **Date:** 2026-05-07.

---

## 1. Executive summary

Anything-LLM (ALL) is a self-hosted "chat with your docs + agents" web app
shipped as a Vite/React frontend, a Node/Express server, a separate
Node/Express **collector** for document ingestion, plus an embeddable web
widget and a Chrome extension. It is built around three first-class concepts:
**workspaces** (chat-scoped doc collections), **threads** (chats inside a
workspace), and a no-code **agent** runtime ("AIbitat") with a fixed bag of
plugins (web browse, web scrape, SQL, summarize, memory, file ops, calendar,
email, etc.) plus full **MCP** client compatibility.

Nexus already overlaps ALL on most of the *AI plumbing* axis (provider
abstraction, RAG, vector store, MCP client, agent loop, scheduled workflows),
so the highest-value ports are not "engine" features — they are **product
patterns** that close UX gaps Nexus does not yet have:

1. **Workspaces / threads as a first-class chat surface** with document
   scoping and citation rendering (Nexus has agent sessions but no
   "scoped chat-with-these-files" workspace concept).
2. **Embeddable chat widget + JS API** (Nexus has no public chat-as-iframe
   story).
3. **Agent flows ("no-code")** — visual/serializable multi-step agent
   recipes distinct from Nexus workflows (which are file/event-driven, not
   conversational).
4. **Document Sync queues** for periodic refresh of remote-sourced
   documents (URLs, GitHub, Confluence, YouTube transcripts).
5. **Audio I/O surface** (Whisper STT, ElevenLabs/Piper/OpenAI TTS) — Nexus
   has none.
6. **Embed/browser-extension keys + scoped API tokens** — a clean
   per-integration auth model Nexus could adopt for shell plugins and
   external tools.
7. **System-prompt variables** and **slash-command presets** (Nexus has
   skills, but not user-level prompt-variable substitution in chat input).

Items that are **not worth porting** because Nexus already does them
better-or-equally: provider abstraction, embeddings, vector storage, RAG,
MCP, scheduled jobs (workflow cron), file watcher / index rebuild,
capability-gated plugin model, multi-frontend architecture.

---

## 2. Anything-LLM capability inventory

### 2.1 Tech stack & top-level layout

| Sub-project | Stack | Purpose |
|---|---|---|
| `frontend/` | Vite + React | SPA admin/chat UI |
| `server/` | Node + Express + Prisma (SQLite default, Postgres optional) | Core API, LLM/vector orchestration, agent runtime |
| `collector/` | Node + Express | Out-of-process document parsing/normalisation |
| `embed/` (submodule) | Vanilla JS bundle | Embeddable chat widget for third-party sites |
| `browser-extension/` (submodule) | Chrome MV3 | "Send page to workspace" |
| `docker/`, deploy templates | Dockerfile, CFN, Terraform-lite | One-click cloud deploys |

### 2.2 LLM providers (40+)

OpenAI, Azure OpenAI, AWS Bedrock, Anthropic, Google Gemini, HuggingFace,
Ollama, LM Studio, LocalAI, Together AI, Fireworks, Perplexity, OpenRouter,
DeepSeek, Mistral, Groq, Cohere, KoboldCPP, LiteLLM, xAI, NVIDIA NIM, Apipie,
Z.AI, Novita, PPIO, Gitee, Moonshot, Microsoft Foundry Local, CometAPI,
Docker Model Runner, PrivateModeAI, SambaNova, Lemonade (AMD), llama.cpp, Text
Generation Web UI.

### 2.3 Embedders

ALL native embedder (default), OpenAI, Azure OpenAI, LocalAI, Ollama, LM
Studio, Cohere.

### 2.4 Vector stores (9)

LanceDB (default), PGVector, Astra DB, Pinecone, Chroma / ChromaCloud,
Weaviate, Qdrant, Milvus, Zilliz.

### 2.5 Audio

- **STT:** native browser, OpenAI Whisper, ALL built-in.
- **TTS:** native browser, PiperTTS (local), OpenAI TTS, ElevenLabs,
  OpenAI-compatible.

### 2.6 Document & data ingestion (collector)

PDF, DOCX, TXT, MD, HTML, ePub, MBOX, audio/video transcripts (Whisper),
GitHub repo crawler, GitLab repo crawler, Confluence, YouTube transcript,
generic URL/sitemap crawl, Drupal Wiki, Obsidian vaults, plain folders,
Drag-and-drop. Sync via **document_sync_queues** + **document_sync_executions**
with a cron interval per source.

### 2.7 Agent runtime ("AIbitat")

A single-file orchestrator (`server/utils/agents/aibitat/index.js`) with a
plugin registry. Built-in plugins under
`server/utils/agents/aibitat/plugins/`:

| Plugin | Capability |
|---|---|
| `web-browsing.js` | Headless browse + extract |
| `web-scraping.js` | Scrape page → text |
| `summarize.js` | Long-doc summarisation |
| `memory.js` | Long-term agent memory |
| `chat-history.js` | Inject prior turns |
| `file-history.js` | Track file context |
| `rechart.js` | Generate charts |
| `cli.js` | Shell exec (gated) |
| `http-socket.js`, `websocket.js` | Transport |
| `create-files/` | Write files into workspace |
| `filesystem/` | Read/list workspace files |
| `gmail/`, `outlook/`, `google-calendar/` | OAuth'd email/calendar tools |
| `sql-agent/` | Connect & query SQL warehouses |
| **MCP** | Full MCP client; servers configured per workspace |

Two layered concepts above plugins:
- **Agent Skills** — single-purpose prompt+tool bundles, hub-installable.
- **Agent Flows** — multi-step no-code recipes ("scrape → summarise → save").
- **Tool Reranker** — reduces tool-list size per turn (the "80% token cut"
  claim).

### 2.8 Scheduling

`scheduled_jobs` + `scheduled_job_runs` tables: cron-style triggers that
invoke an agent flow / chat with stored arguments.

### 2.9 Workspaces / threads / users

Prisma schema (key tables):

- `workspaces` (chat settings, model override, system prompt, similarity
  threshold, top-k, history len, …)
- `workspace_threads` (per-user sub-conversations)
- `workspace_chats` (message log + thumbs-up/down feedback)
- `workspace_documents` / `document_vectors` (pinning, citation map)
- `workspace_users` (membership; ALL is multi-tenant in Docker mode)
- `workspace_suggested_messages` (starter prompts in UI)
- `workspace_agent_invocations` (audit of agent tool calls)
- `workspace_parsed_files` (collector output cache)
- `system_prompt_variables` (`{{var}}` substitution in prompts)
- `slash_command_presets` (`/summarize`, `/translate`, …)
- `prompt_history` (audit of system-prompt edits)

### 2.10 AuthN/AuthZ & API surface

- Local users (bcrypt) + invite codes.
- 2FA with `recovery_codes`.
- `password_reset_tokens`, `temporary_auth_tokens`.
- `api_keys` for the developer API (Swagger doc'd).
- `embed_configs` + `embed_chats` for public widget tokens (per-site,
  rate-limited, optional auth).
- `browser_extension_api_keys` for Chrome ext.
- `desktop_mobile_devices` for native client pairing.
- Roles: admin / manager / default user.

### 2.11 Telemetry / observability

PostHog anonymous analytics, opt-out via `DISABLE_TELEMETRY`. `event_logs`
for in-app audit trail.

### 2.12 Deploy

Docker, AWS CFN, GCP, DigitalOcean, Render, Railway, RepoCloud, Elestio,
Northflank, bare metal. Desktop apps (Mac/Win/Linux) built on the same
server bundled in Electron-ish shell.

---

## 3. Nexus capability baseline (what already exists)

Pulled from `Cargo.toml` workspace, `crates/**/src/**`, `docs/PRDs/**`, and
`docs/PRDs/IMPLEMENTATION_STATUS.md`.

| Concern | Where it lives in Nexus | State |
|---|---|---|
| Provider abstraction | `nexus-ai::provider::AiProvider` (OpenAI, Anthropic, Ollama) | Shipped |
| Embeddings | `nexus-ai::local_embedding`, `embedding.rs` | Shipped (local + remote) |
| Vector store | `nexus-storage::vectorstore` (SQLite-backed); `nexus-ai::vectorstore` IPC client | Shipped |
| Chunker / RAG | `nexus-ai::{chunker,rag,enrichment}` | Shipped |
| MCP **client** | `nexus-mcp` (auth, pool, ipc, server) | Shipped |
| MCP **server** | `crates/nexus-mcp/src/server.rs` | Shipped |
| Agent loop & tool registry | `nexus-agent` (sessions, archetypes, ADRs 0023/0024) | Shipped |
| Scheduled triggers | `nexus-workflow::{cron,executor}` | Shipped |
| File-as-truth + watcher | `nexus-storage::{watcher,reconcile,index}` (ADR 0003) | Shipped |
| Markdown / Obsidian / Notion formats | `nexus-formats`, `nexus-storage::obsidian_base` | Shipped |
| Capability system | `nexus-kernel` + ADR 0002 hierarchical strings | Shipped |
| Skills (prompt+tool bundles) | `nexus-skills` (`builtins`, `compose`, `registry`, `substitute`) | Shipped |
| Frontends | CLI, TUI, MCP, Tauri shell | Shipped |
| Auth / credential vault | `nexus-security` + keyring (ADR 0009) | Shipped |
| Multi-user web UI | — | **Absent** (Nexus is a single-user desktop tool) |
| Embeddable web widget | — | **Absent** |
| Audio (STT/TTS) | — | **Absent** |
| Workspace/thread chat surface | Partial (agent sessions exist; no "scoped chat with these docs" UI) | **Gap** |
| Document sync (URL/GitHub/etc.) | — | **Absent** |
| Slash-command presets in chat | — | **Absent** (skills cover programmatic composition, not user-typed shortcuts) |
| Prompt variables (`{{var}}`) in user input | — | **Absent** |

---

## 4. Portability matrix

Each item is rated:

- **Direct fit** — already aligned with Nexus's invariants; mainly a UX
  layer.
- **Adapt** — concept transfers but must be re-shaped to fit Nexus's
  microkernel + IPC + capability model.
- **Skip** — Nexus already does this, or it conflicts with invariants.

| ALL feature | Verdict | Where it'd land in Nexus | Notes / friction |
|---|---|---|---|
| Workspace = scoped chat over a doc set | **Adapt** | New `nexus-workspace` crate or extend `nexus-agent` with a `Workspace` aggregate; persist as a forge-side TOML/MD descriptor (file-as-truth) + SQLite index row | Don't model as a DB-only entity — make it a `.workspace.toml` so invariant 1 holds. Threads = JSON entries under `<forge>/.forge/workspaces/<id>/threads/`. |
| Threads inside a workspace | **Adapt** | Same crate; one IPC handler set | Reuse existing agent `session` machinery; threads ≈ sessions with a parent workspace id. |
| System prompt + per-workspace overrides (model, top-k, similarity, history len) | **Adapt** | Extend `nexus-ai` config + workspace descriptor | Existing `nexus-ai::config` already has the dials; route via per-workspace override. |
| Suggested-messages / starter prompts | **Direct fit** | Field on workspace descriptor | Pure UI. |
| Slash-command presets (`/summarize foo`) | **Adapt** | Reuse `nexus-skills` registry; add a "user-typed alias" projection in `nexus-skills::substitute` | Skills already exist; we need a chat-input parser + alias table. Cheap win. |
| System-prompt variables (`{{user.name}}`, `{{date}}`) | **Adapt** | Extend `nexus-skills::substitute` to run on any chat send, not just skill bodies | Use existing `interpolate.rs` patterns from `nexus-workflow`. |
| Document sync queues (URL/GitHub/Confluence/YouTube cron-refresh) | **Adapt** | New "connectors" subsystem inside `nexus-storage` *or* a `nexus-connectors` crate; reuse `nexus-workflow::cron`; outputs land as plain MD files (file-as-truth) | Big-but-clean: each connector = a CorePlugin exposing `sync_now` + a `cron` trigger. **Highest leverage port.** |
| GitHub repo connector | **Adapt** | Connector plugin; uses existing `git2` dep | Code reuse is high. |
| YouTube transcript connector | **Adapt** | Connector plugin; needs a transcript backend | Could use existing `nexus-ai` or call a Whisper backend. |
| URL / sitemap crawler | **Adapt** | Connector plugin | Capability-gated `net.http`. |
| PDF/DOCX/EPUB/MBOX collectors | **Adapt** | Add to `nexus-formats`; collector runs in-process (no separate Node service needed) | We don't need ALL's "two Node servers" split — Rust parsers (e.g., `lopdf`, `docx-rs`, `epub`) plug straight into `nexus-formats`. |
| Citation rendering on chat answers | **Direct fit** | UI-only on top of existing RAG output (`ChunkMatch` already returns provenance) | Easy shell plugin. |
| Workspace agent-invocation log | **Direct fit** | Already have `nexus-ai::activity_log`; surface per-workspace | Pure plumbing. |
| AIbitat agent runtime | **Skip (engine)** / **Adapt (recipes)** | Engine = redundant with `nexus-agent` (ADR 0024). The *recipes* (web-browse → scrape → summarise) port as **agent flows** built on `nexus-agent` + `nexus-skills`. | Don't reimplement orchestration. |
| Web-browsing tool | **Adapt** | Tool plugin under `nexus-ai::tools` | Capability `net.http` + headless browser dep. |
| Web-scraping tool | **Adapt** | Tool plugin | Same. |
| SQL-agent tool | **Adapt** | Already have `nexus-database`; expose query as agent tool | Trivial. |
| Filesystem tool | **Skip** | Storage IPC is already the agent's filesystem | Use existing handlers. |
| Create-files tool | **Skip** | Same — `com.nexus.storage::write` exists | — |
| Memory tool | **Adapt** | Could land in `nexus-kv` + `nexus-ai::tools`; see also `AI-MEMORY-LAYER-PLAN.md` | Existing plan covers most of it. |
| Summarize tool | **Adapt** | Trivial wrapper over `nexus-ai::chat` | — |
| Rechart tool | **Skip / low priority** | Niche; defer | — |
| Gmail / Outlook / Google Calendar | **Adapt (eventual)** | OAuth + connector plugins; not core | Heavy auth surface; only do if user demand exists. |
| MCP client | **Skip** | `nexus-mcp` already does this, including pooled servers | — |
| MCP server | **Skip** | Already exposed | — |
| Agent flows (no-code multi-step) | **Adapt** | Hybrid: serialise as TOML under `<forge>/flows/*.flow.toml` (consistent with `nexus-workflow`); UI builder is a shell plugin | Conceptually parallel to `nexus-workflow` but conversational/agent-driven; could share `nexus-workflow::interpolate` + `condition`. |
| Agent skills marketplace ("hub") | **Adapt** | Mirrors plugin marketplace already on roadmap (`WI-44`) | Defer until plugin marketplace lands. |
| Multi-user / RBAC | **Skip** | Out of scope per personal-tool positioning (`docs/PRDs/IMPLEMENTATION_STATUS.md`) | Only revisit if a hosted variant emerges. |
| Embed configs + public widget | **Adapt** | New `nexus-embed` crate exposing a *read-only* HTTP/WS surface gated by per-site tokens; would need a "serve mode" for the kernel (currently desktop-only) | Big new attack surface. Treat as opt-in service plugin behind capabilities `net.serve`, `embed.token`. **Not for personal-tool MVP.** |
| Browser extension | **Adapt** | Would consume the same embed surface; nice symmetry | Depends on embed work. |
| Developer API (Swagger) | **Adapt** | Exposed today only via MCP / IPC; an HTTP wrapper would mirror MCP server semantics | Possibly wrap MCP server as an OpenAPI gateway. |
| Per-user API keys | **Adapt** | `nexus-security` + keyring already store secrets; add a "scoped token" issuer | Smaller version of multi-user. |
| Audio: Whisper STT | **Adapt** | New `nexus-audio` crate; backends OpenAI Whisper / local whisper.cpp | Greenfield. |
| Audio: TTS (OpenAI/ElevenLabs/Piper) | **Adapt** | Same crate; provider trait analogous to `AiProvider` | Greenfield. |
| Native browser STT/TTS | **Adapt** | Pure shell plugin — Web Speech API in the Tauri webview | Cheap. |
| 2FA / recovery codes / pw reset | **Skip** | Multi-user concern | — |
| PostHog telemetry | **Skip** | Conflicts with personal-tool privacy posture (`nexus-ai::privacy`) | — |
| Event log audit | **Direct fit (already)** | `nexus-storage` + `panic-log`; extend to AI/agent | Mostly there. |
| Scheduled jobs | **Skip (engine)** | `nexus-workflow::cron` is the same primitive | Reuse. |
| Cloud one-click deploys | **Skip** | Not applicable to a desktop tool | — |
| Desktop app | **Skip** | Tauri shell is the equivalent (ADR 0011) | — |

---

## 5. Recommended port ordering (if pursued)

These are independent enough to land in any order, but ranked by
leverage/cost.

1. **Workspaces + threads UI** over existing `nexus-agent` sessions —
   biggest UX gap closed for the smallest engine work; everything below
   composes onto it.
2. **Slash-command presets + prompt variables** — a few hundred lines on
   top of `nexus-skills::substitute`; instant productivity win.
3. **Document connectors with cron sync** (URL → MD, GitHub → MD, YouTube
   transcript → MD) — turns Nexus from "chat with notes you already have"
   into "chat with the world", and remains file-as-truth.
4. **Citation rendering** in the chat surface — small UI work, RAG already
   carries provenance.
5. **Web-browse / web-scrape / summarise agent tools** — promotes the
   agent from forge-only to web-capable.
6. **Audio I/O (Whisper + Piper)** — net-new `nexus-audio` crate; nice for
   journaling/dictation use cases.
7. **Embed surface + scoped tokens** — only if/when a hosted/multi-user
   variant becomes a goal; large blast radius (network listener,
   tenancy), so gate behind explicit ADR.

Items 1–5 fit cleanly under existing PRDs (12 AI engine, 13 Skills, 15
Agent system, 16 Workflow). Items 6–7 would need new PRDs.

---

## 6. Architectural friction notes

- **No second Node service.** ALL's split into a Node "server" and a Node
  "collector" exists because Node is single-threaded and PDF/DOCX parsing
  blocks the event loop. Nexus has Tokio + a real plugin model — every
  collector capability should land as an in-process IPC handler, not a
  sidecar.
- **No Prisma-shaped DB.** ALL stores workspaces/threads/chats in a single
  Prisma SQLite. Nexus's invariant 1 says **files are truth**; ports of
  workspaces/threads/flows must serialise to disk (TOML/MD/JSON in
  `<forge>/.forge/...`) with the SQLite index rebuildable.
- **No bespoke Tauri commands for new features.** ALL has a JS API
  endpoint per feature; in Nexus the same capability must be reachable
  from CLI, TUI, MCP, and shell uniformly via `ipc_call`. New shell-only
  Tauri commands are a `CLAUDE.md` smell.
- **Capabilities, not roles.** ALL gates with admin/manager/user roles.
  Nexus gates with hierarchical capability strings (ADR 0002). Every
  ported feature must declare the new capability strings it requires
  (e.g. `net.http`, `audio.tts`, `connector.github`).
- **Telemetry.** ALL ships with PostHog; Nexus must not. Any ported
  module that wants metrics goes through `nexus-panic-log` /
  `tracing`, never an external sink without explicit opt-in.

---

## 7. Out of scope (won't port)

- Multi-tenant user management, invites, RBAC.
- 2FA, password reset.
- PostHog / hosted analytics.
- Cloud deployment templates (CFN, Render, Railway, …).
- Bundled Electron / Docker images — Nexus's distribution model is the
  Tauri shell (ADR 0011) plus the `nexus` CLI binary.
- Recharts / chart-image generation as an agent tool.

---

## 8. Sources

- Repo: https://github.com/Mintplex-Labs/anything-llm
- Server tree: https://github.com/Mintplex-Labs/anything-llm/tree/master/server
- Agent plugins:
  https://github.com/Mintplex-Labs/anything-llm/tree/master/server/utils/agents/aibitat/plugins
- Prisma schema:
  https://github.com/Mintplex-Labs/anything-llm/blob/master/server/prisma/schema.prisma
- Nexus PRDs: `docs/PRDs/12-ai-engine.md`, `13-skills.md`, `14-mcp-integration.md`,
  `15-agent-system.md`, `16-workflow-system.md`.
- Nexus invariants: `CLAUDE.md`, `docs/architecture/invariants.md`,
  ADRs 0002, 0003, 0004, 0011, 0016, 0023, 0024.
